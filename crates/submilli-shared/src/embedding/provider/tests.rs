//! Provider-core tests, against hand-rolled dispatch fakes. **Zero live API
//! calls.** Texts are decimal ids ("7", or "7aaaa" when padded); the fake turns
//! each id into the direction `[1, id, 0, 0]`, so a normalized row identifies the
//! input it answers.

use std::sync::Mutex;
use std::time::Duration;

use interpreter::runtime::{EmbeddingLimitKind, EmbeddingLimits, SharedTokenBudget};
use submilli_blueprint::parse;

use super::*;
use crate::embedding::{
    DispatchFailure, DispatchResponse, DispatchRow, NotSentReason, Rejection, SentFailure,
};

const DIMS: usize = 4;

struct Answer {
    delay: Duration,
    result: Result<DispatchResponse, DispatchFailure>,
}

impl Answer {
    fn now(result: Result<DispatchResponse, DispatchFailure>) -> Self {
        Self {
            delay: Duration::ZERO,
            result,
        }
    }
}

#[derive(Debug, Clone)]
struct Seen {
    ids: Vec<u64>,
    purpose: Purpose,
    send_dimensions: bool,
}

type Handler = dyn Fn(&Seen) -> Answer + Send + Sync;

struct Fake {
    handler: Box<Handler>,
    calls: Mutex<Vec<Seen>>,
}

impl Fake {
    fn new(handler: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            handler: Box::new(handler),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<Seen> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl EmbeddingDispatch for Fake {
    fn dispatch<'a>(
        &'a self,
        request: EmbeddingRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResponse, DispatchFailure>> + Send + 'a>> {
        let seen = Seen {
            ids: request.texts.iter().map(|t| id_of(t)).collect(),
            purpose: request.purpose,
            send_dimensions: request.send_dimensions,
        };
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(seen.clone());
        }
        let answer = (self.handler)(&seen);
        Box::pin(async move {
            tokio::time::sleep(answer.delay).await;
            answer.result
        })
    }
}

fn id_of(text: &str) -> u64 {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(u64::MAX)
}

fn texts(count: u64) -> Vec<String> {
    (0..count).map(|i| i.to_string()).collect()
}

fn row_for(id: u64) -> Vec<f32> {
    vec![1.0, id as f32, 0.0, 0.0]
}

fn rows_for(ids: &[u64]) -> Vec<DispatchRow> {
    ids.iter()
        .map(|&id| DispatchRow {
            index: None,
            values: row_for(id),
        })
        .collect()
}

fn response(
    rows: Vec<DispatchRow>,
    usage: Option<u64>,
) -> Result<DispatchResponse, DispatchFailure> {
    Ok(DispatchResponse { rows, usage })
}

/// A dispatch that answers every request with correct rows and `usage`.
fn echo(usage: Option<u64>) -> Arc<Fake> {
    Fake::new(move |seen| Answer::now(response(rows_for(&seen.ids), usage)))
}

fn yaml(kind: &str, model: &str, dims: u64, provider_extra: &str, model_extra: &str) -> String {
    format!(
        "name: t\nsecrets:\n  K: {{ store: K }}\nembedding:\n  providers:\n    p:\n      \
         type: {kind}\n      api_key: \"${{secrets.K}}\"\n{provider_extra}  models:\n    m:\n      \
         provider: p\n      model: {model}\n      dimensions: {dims}\n{model_extra}"
    )
}

fn openai_blueprint() -> Blueprint {
    blueprint(&yaml("openai", "text-embedding-3-small", 4, "", ""))
}

fn blueprint(source: &str) -> Blueprint {
    parse(source).expect("the test blueprint is valid")
}

fn provider(fake: &Arc<Fake>) -> BlueprintEmbeddingProvider {
    provider_with(&openai_blueprint(), fake, 4)
}

fn provider_with(
    blueprint: &Blueprint,
    fake: &Arc<Fake>,
    concurrency: usize,
) -> BlueprintEmbeddingProvider {
    BlueprintEmbeddingProvider::new(blueprint, fake.clone(), concurrency)
}

fn identity_of(source: &str) -> String {
    let provider = provider(&echo(None));
    let provider = BlueprintEmbeddingProvider::new(&blueprint(source), provider.dispatch, 1);
    provider
        .identity("m")
        .expect("alias m resolves")
        .to_string()
}

fn budget() -> EmbeddingTokenBudget {
    EmbeddingTokenBudget::unmetered()
}

async fn embed(
    provider: &BlueprintEmbeddingProvider,
    inputs: &[String],
) -> Result<EmbeddingBatch, EmbeddingError> {
    let budget = budget();
    let reserved = provider.estimate_tokens("m", inputs);
    budget
        .reserve("m", reserved)
        .expect("unmetered budget accepts");
    provider
        .embed("m", inputs, Purpose::Document, &budget)
        .await
}

fn assert_in_order(batch: &EmbeddingBatch, count: u64) {
    assert_eq!(batch.count() as u64, count);
    for id in 0..count {
        let row = batch.row(id as usize).expect("row exists");
        let norm = (1.0 + (id * id) as f32).sqrt();
        assert!(
            (row[0] - 1.0 / norm).abs() < 1e-5,
            "row {id} is not input {id}"
        );
        assert!(
            (row[1] - id as f32 / norm).abs() < 1e-5,
            "row {id} is not input {id}"
        );
    }
}

fn malformed_reason(error: &EmbeddingError) -> Option<EmbeddingMalformedReason> {
    match error {
        EmbeddingError::Malformed { reason, .. } => Some(*reason),
        _ => None,
    }
}

// --- AE7 --------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn ae7_250_texts_capped_at_96_return_in_order_when_batches_finish_out_of_order() {
    // Earlier sub-batches answer later, so completion order is reversed.
    let fake = Fake::new(|seen| Answer {
        delay: Duration::from_millis(300 - 100 * (seen.ids[0] / 96)),
        result: response(rows_for(&seen.ids), Some(seen.ids.len() as u64)),
    });
    let provider = provider(&fake).with_max_inputs_per_request(96);
    let budget = budget();
    let inputs = texts(250);
    budget
        .reserve("m", provider.estimate_tokens("m", &inputs))
        .expect("reserve");

    let batch = provider
        .embed("m", &inputs, Purpose::Document, &budget)
        .await
        .expect("embeds");

    assert_in_order(&batch, 250);
    let sizes: Vec<usize> = fake.calls().iter().map(|c| c.ids.len()).collect();
    assert_eq!(sizes.iter().sum::<usize>(), 250);
    assert!(sizes.iter().all(|&n| n <= 96));
    assert_eq!(sizes.len(), 3);
    // Every sent sub-batch is counted exactly once, and usage sums.
    assert_eq!(budget.requests(), 3);
    assert_eq!(batch.input_tokens(), Some(250));
    assert_eq!(batch.settlements().len(), 3);
}

#[tokio::test]
async fn sub_batches_stay_under_half_the_openai_request_cap() {
    let fake = echo(None);
    let provider = provider(&fake);
    // 24,576 bytes estimates to the 8,192-token input limit.
    let inputs: Vec<String> = (0..128)
        .map(|i| format!("{i}{}", "a".repeat(24_576 - i.to_string().len())))
        .collect();

    embed(&provider, &inputs).await.expect("embeds");

    let calls = fake.calls();
    assert!(calls.len() > 1);
    for call in &calls {
        let estimate = call.ids.len() as u64 * 8_192;
        assert!(estimate <= 150_000, "sub-batch estimates {estimate} tokens");
    }
    assert_eq!(calls.iter().map(|c| c.ids.len()).sum::<usize>(), 128);
}

#[test]
fn google_sub_batches_close_on_the_hundred_input_cap_alone() {
    // Confirmed live (2026-10-05): Google refuses more than 100 requests per
    // batch and imposes no byte cap, so inputs at the 2,032-byte bound pack
    // 100 to a sub-batch.
    let source = yaml("google", "gemini-embedding-001", 4, "", "");
    let provider = BlueprintEmbeddingProvider::new(&blueprint(&source), echo(None), 1);
    let resolved = provider.aliases.get("m").expect("alias resolves");
    let caps = resolved.split_caps(Purpose::Document);
    let sizes = |count: usize| -> Vec<usize> {
        let inputs: Vec<String> = (0..count).map(|_| "a".repeat(2_032)).collect();
        plan_sub_batches(&inputs, &caps)
            .iter()
            .map(|s| s.range.len())
            .collect()
    };
    assert_eq!(sizes(100), vec![100]);
    assert_eq!(sizes(101), vec![100, 1]);
}

#[test]
fn a_templated_google_model_adds_its_template_bytes_to_the_usage_ceiling() {
    use submilli_blueprint::embedding_limits::GOOGLE_DOCUMENT_TEMPLATE_PREFIX;
    let source = yaml("google", "gemini-embedding-2", 4, "", "");
    let provider = BlueprintEmbeddingProvider::new(&blueprint(&source), echo(None), 1);
    let resolved = provider.aliases.get("m").expect("alias resolves");
    let caps = resolved.split_caps(Purpose::Document);
    assert_eq!(
        caps.text_overhead_bytes,
        GOOGLE_DOCUMENT_TEMPLATE_PREFIX.len() as u64
    );
    let inputs = vec!["a".repeat(10)];
    let plan = plan_sub_batches(&inputs, &caps);
    assert_eq!(
        plan[0].max_usage,
        usage_ceiling(&inputs[0], caps.text_overhead_bytes)
    );
    // A model without a template adds nothing.
    let plain = yaml("google", "gemini-embedding-001", 4, "", "");
    let plain = BlueprintEmbeddingProvider::new(&blueprint(&plain), echo(None), 1);
    let resolved = plain.aliases.get("m").expect("alias resolves");
    assert_eq!(resolved.split_caps(Purpose::Query).text_overhead_bytes, 0);
}

// --- AE5 and validation -----------------------------------------------------

#[tokio::test]
async fn ae5_unusable_responses_fail_the_call_as_malformed() {
    let cases: Vec<(&str, Vec<DispatchRow>, EmbeddingMalformedReason)> = vec![
        (
            "two vectors for three inputs",
            rows_for(&[0, 1]),
            EmbeddingMalformedReason::CountMismatch,
        ),
        (
            "a NaN component",
            vec![
                row(vec![1.0, 0.0, 0.0, 0.0]),
                row(vec![1.0, f32::NAN, 0.0, 0.0]),
                row(vec![1.0, 0.0, 0.0, 0.0]),
            ],
            EmbeddingMalformedReason::NonFiniteValue,
        ),
        (
            "an infinite component",
            vec![
                row(vec![1.0, 0.0, 0.0, 0.0]),
                row(vec![f32::INFINITY, 0.0, 0.0, 0.0]),
                row(vec![1.0, 0.0, 0.0, 0.0]),
            ],
            EmbeddingMalformedReason::NonFiniteValue,
        ),
        (
            "wrong dimensions",
            vec![
                row(vec![1.0, 0.0, 0.0, 0.0]),
                row(vec![1.0, 0.0, 0.0]),
                row(vec![1.0, 0.0, 0.0, 0.0]),
            ],
            EmbeddingMalformedReason::DimensionMismatch,
        ),
        (
            "a zero vector",
            vec![
                row(vec![1.0, 0.0, 0.0, 0.0]),
                row(vec![0.0; DIMS]),
                row(vec![1.0, 0.0, 0.0, 0.0]),
            ],
            EmbeddingMalformedReason::InvalidBody,
        ),
    ];
    for (name, rows, expected) in cases {
        let fake = Fake::new(move |_| Answer::now(response(rows.clone(), Some(9))));
        let error = embed(&provider(&fake), &texts(3)).await.expect_err(name);
        assert_eq!(malformed_reason(&error), Some(expected), "{name}");
        // A failed validation of a sent sub-batch settles at the reported usage.
        assert_eq!(
            error.settlements(),
            [SubBatchSettlement {
                estimate: 3,
                reported: 9,
                indeterminate: 0
            }],
            "{name}"
        );
    }
}

#[tokio::test]
async fn ae5_a_token_level_3d_response_fails_as_malformed() {
    // The wire layer reports a 3-D shape through the failure type.
    let fake = Fake::new(|_| {
        Answer::now(Err(DispatchFailure::Malformed {
            reason: EmbeddingMalformedReason::InvalidBody,
            usage: None,
        }))
    });
    let error = embed(&provider(&fake), &texts(3)).await.expect_err("fails");
    assert_eq!(
        malformed_reason(&error),
        Some(EmbeddingMalformedReason::InvalidBody)
    );
    assert_eq!(error.settlements(), [sent_unknown(3)]);
}

fn row(values: Vec<f32>) -> DispatchRow {
    DispatchRow {
        index: None,
        values,
    }
}

fn indexed(index: usize, id: u64) -> DispatchRow {
    DispatchRow {
        index: Some(index),
        values: row_for(id),
    }
}

#[tokio::test]
async fn permuted_index_fields_are_reordered() {
    // Rows arrive as inputs 2, 0, 1 and carry their own index.
    let fake = Fake::new(|_| {
        Answer::now(response(
            vec![indexed(2, 2), indexed(0, 0), indexed(1, 1)],
            None,
        ))
    });
    let batch = embed(&provider(&fake), &texts(3)).await.expect("embeds");
    assert_in_order(&batch, 3);
}

#[tokio::test]
async fn duplicate_missing_or_mixed_indices_fail() {
    let cases = vec![
        (
            "duplicate",
            vec![indexed(0, 0), indexed(0, 1), indexed(2, 2)],
        ),
        (
            "out of range",
            vec![indexed(0, 0), indexed(1, 1), indexed(3, 2)],
        ),
        ("mixed", vec![indexed(0, 0), row(row_for(1)), indexed(2, 2)]),
    ];
    for (name, rows) in cases {
        let fake = Fake::new(move |_| Answer::now(response(rows.clone(), None)));
        let error = embed(&provider(&fake), &texts(3)).await.expect_err(name);
        assert!(malformed_reason(&error).is_some(), "{name}");
    }
}

#[tokio::test]
async fn vectors_from_a_non_normalizing_provider_come_back_at_unit_length() {
    let fake = Fake::new(|_| {
        Answer::now(response(
            vec![
                row(vec![3.0, 4.0, 0.0, 0.0]),
                row(vec![0.0, 0.0, 10.0, 0.0]),
            ],
            None,
        ))
    });
    let batch = embed(&provider(&fake), &texts(2)).await.expect("embeds");
    let first = batch.row(0).expect("row 0");
    assert!((first[0] - 0.6).abs() < 1e-6 && (first[1] - 0.8).abs() < 1e-6);
    for index in 0..2 {
        let norm: f32 = batch.row(index).expect("row").iter().map(|v| v * v).sum();
        assert!((norm - 1.0).abs() < 1e-5);
    }
}

#[tokio::test]
async fn usage_is_none_when_any_sub_batch_lacks_it() {
    let fake = Fake::new(|seen| {
        let usage = (seen.ids[0] == 0).then_some(5);
        Answer::now(response(rows_for(&seen.ids), usage))
    });
    let provider = provider(&fake).with_max_inputs_per_request(1);
    let batch = embed(&provider, &texts(2)).await.expect("embeds");
    assert_eq!(batch.input_tokens(), None);
    assert_eq!(
        batch.settlements(),
        [
            SubBatchSettlement {
                estimate: 1,
                reported: 5,
                indeterminate: 0
            },
            sent_unknown(1)
        ]
    );
}

// --- AE4 and AE9: identity --------------------------------------------------

/// The digest is part of the contract: vectors labeled with an identity are only
/// comparable while it is unchanged, so a change to the canonical record is a
/// deliberate break and shows up here.
#[test]
fn identity_digest_is_pinned() {
    assert_eq!(
        identity_of(&yaml("voyage", "voyage-3.5", 1024, "", "")),
        "emb1:voyage:voyage-3.5:1024:cc25df8731a6a176dd70220bb0abae43"
    );
}

#[test]
fn ae4_identity_follows_configuration_that_changes_the_vector_space() {
    let base = identity_of(&yaml("openai", "text-embedding-3-small", 4, "", ""));
    assert!(base.starts_with("emb1:openai:text-embedding-3-small:4:"));

    let changed = [
        yaml("openai", "text-embedding-3-small", 8, "", ""),
        yaml("openai", "text-embedding-3-large", 4, "", ""),
        yaml("voyage", "text-embedding-3-small", 4, "", ""),
    ];
    for source in changed {
        assert_ne!(identity_of(&source), base, "{source}");
    }

    let described = yaml(
        "openai",
        "text-embedding-3-small",
        4,
        "",
        "      description: \"Anything at all.\"\n",
    );
    assert_eq!(identity_of(&described), base);
}

#[test]
fn ae4_hugging_face_route_host_and_prompt_names_change_identity() {
    let shared = identity_of(&yaml("huggingface", "bge", 4, "", ""));
    let dedicated = |host: &str| {
        yaml(
            "huggingface",
            "bge",
            4,
            &format!("      base_url: https://{host}\n"),
            "",
        )
    };
    let dedicated_a = identity_of(&dedicated("a.example.com"));
    assert_ne!(dedicated_a, shared);
    assert_ne!(identity_of(&dedicated("b.example.com")), dedicated_a);

    // Prompt names are accepted only on a dedicated endpoint.
    let endpoint = "      base_url: https://a.example.com\n";
    let prompted = yaml(
        "huggingface",
        "bge",
        4,
        endpoint,
        "      query_prompt_name: query\n",
    );
    assert_ne!(identity_of(&prompted), dedicated_a);
    let other_prompt = yaml(
        "huggingface",
        "bge",
        4,
        endpoint,
        "      query_prompt_name: search_query\n",
    );
    assert_ne!(identity_of(&other_prompt), identity_of(&prompted));
}

#[test]
fn a_non_hugging_face_base_url_does_not_change_identity() {
    let direct = identity_of(&yaml("openai", "text-embedding-3-small", 4, "", ""));
    let proxied = yaml(
        "openai",
        "text-embedding-3-small",
        4,
        "      base_url: https://proxy.example.com\n",
        "",
    );
    assert_eq!(identity_of(&proxied), direct);
}

#[test]
fn google_identity_distinguishes_the_purpose_mechanism() {
    let v1 = identity_of(&yaml("google", "gemini-embedding-001", 4, "", ""));
    let v2 = identity_of(&yaml("google", "gemini-embedding-2", 4, "", ""));
    assert_ne!(v1, v2);
}

#[tokio::test]
async fn ae9_query_and_document_calls_share_the_identity_models_reports() {
    let fake = echo(None);
    let provider = provider(&fake);
    let budget = budget();
    let inputs = texts(2);

    let query = provider
        .embed("m", &inputs, Purpose::Query, &budget)
        .await
        .expect("query");
    let document = provider
        .embed("m", &inputs, Purpose::Document, &budget)
        .await
        .expect("document");
    let models = provider.models().await.expect("models");

    assert_eq!(query.identity(), document.identity());
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].identity, query.identity());
    assert_eq!(models[0].dimensions, 4);
    assert_eq!(models[0].max_input_tokens, Some(8_192));
    assert_eq!(models[0].max_input_bytes, 24_576);
    let purposes: Vec<Purpose> = fake.calls().iter().map(|c| c.purpose).collect();
    assert_eq!(purposes, vec![Purpose::Query, Purpose::Document]);
}

#[tokio::test]
async fn dimensions_are_sent_only_for_models_that_accept_them() {
    let fake = echo(None);
    embed(&provider(&fake), &texts(1)).await.expect("embeds");
    assert!(fake.calls()[0].send_dimensions);

    let ada = blueprint(&yaml("openai", "text-embedding-ada-002", 1_536, "", ""));
    let fake = Fake::new(|seen| {
        let rows = seen.ids.iter().map(|_| row(vec![1.0; 1_536])).collect();
        Answer::now(response(rows, None))
    });
    embed(&provider_with(&ada, &fake, 1), &texts(1))
        .await
        .expect("embeds");
    assert!(!fake.calls()[0].send_dimensions);
}

// --- settlement and failure -------------------------------------------------

fn settlement(estimate: u64, reported: u64, indeterminate: u64) -> SubBatchSettlement {
    SubBatchSettlement {
        estimate,
        reported,
        indeterminate,
    }
}

/// Three one-input sub-batches run one at a time: input 0 succeeds with usage 7,
/// input 1 fails with `failure`, input 2 is never sent.
async fn run_failing_second(failure: DispatchFailure) -> (EmbeddingError, Vec<Seen>) {
    let fake = Fake::new(move |seen| match seen.ids[0] {
        1 => Answer::now(Err(failure.clone())),
        id => Answer::now(response(rows_for(&[id]), Some(7))),
    });
    let provider = provider_with(&openai_blueprint(), &fake, 1).with_max_inputs_per_request(1);
    let error = embed(&provider, &texts(3))
        .await
        .expect_err("one sub-batch fails");
    (error, fake.calls())
}

#[tokio::test]
async fn one_failing_sub_batch_fails_the_call_with_settlements_per_outcome() {
    let free = settlement(1, 0, 0);
    let held = settlement(1, 0, 1);
    let cases = [
        (
            "429",
            DispatchFailure::Rejected(Rejection::RateLimited { retry_after: None }),
            free,
        ),
        (
            "other 4xx",
            DispatchFailure::Rejected(Rejection::Other),
            free,
        ),
        (
            "not sent: credential",
            DispatchFailure::NotSent(NotSentReason::CredentialUnresolved),
            free,
        ),
        (
            "not sent: blocked",
            DispatchFailure::NotSent(NotSentReason::Blocked),
            free,
        ),
        (
            "not sent: unreachable",
            DispatchFailure::NotSent(NotSentReason::Unreachable),
            free,
        ),
        (
            "5xx",
            DispatchFailure::Failed {
                kind: SentFailure::ProviderUnavailable,
                usage: None,
            },
            held,
        ),
        (
            "timeout",
            DispatchFailure::Failed {
                kind: SentFailure::Timeout,
                usage: Some(0),
            },
            held,
        ),
        (
            "transport with usage",
            DispatchFailure::Failed {
                kind: SentFailure::Transport,
                usage: Some(4),
            },
            settlement(1, 4, 0),
        ),
    ];
    for (name, failure, expected) in cases {
        let (error, calls) = run_failing_second(failure).await;
        // Sub-batch 0 succeeded, 1 failed, 2 was never sent: no entry for it.
        assert_eq!(
            error.settlements(),
            [settlement(1, 7, 0), expected],
            "{name}"
        );
        assert_eq!(calls.len(), 2, "{name}");
    }
}

#[tokio::test]
async fn failures_map_to_fixed_reasons() {
    let cases = [
        (
            DispatchFailure::Rejected(Rejection::RateLimited { retry_after: None }),
            EmbeddingFailureReason::RateLimited,
        ),
        (
            DispatchFailure::Rejected(Rejection::Other),
            EmbeddingFailureReason::RequestRejected,
        ),
        (
            DispatchFailure::Failed {
                kind: SentFailure::ProviderUnavailable,
                usage: None,
            },
            EmbeddingFailureReason::ProviderUnavailable,
        ),
        (
            DispatchFailure::Failed {
                kind: SentFailure::Transport,
                usage: None,
            },
            EmbeddingFailureReason::Transport,
        ),
        (
            DispatchFailure::Failed {
                kind: SentFailure::Timeout,
                usage: None,
            },
            EmbeddingFailureReason::Timeout,
        ),
    ];
    let mut cases = cases.to_vec();
    cases.push((
        DispatchFailure::NotSent(NotSentReason::Blocked),
        EmbeddingFailureReason::BlockedByNetworkPolicy,
    ));
    cases.push((
        DispatchFailure::NotSent(NotSentReason::Unreachable),
        EmbeddingFailureReason::Transport,
    ));
    for (failure, expected) in cases {
        let (error, _) = run_failing_second(failure).await;
        assert!(
            matches!(&error, EmbeddingError::Provider { reason, .. } if *reason == expected),
            "{error:?}"
        );
    }
    let (error, _) = run_failing_second(DispatchFailure::Rejected(Rejection::Unauthorized)).await;
    assert!(matches!(error, EmbeddingError::Unauthorized { .. }));
}

#[tokio::test]
async fn a_provider_length_rejection_names_the_original_input_index() {
    let (error, _) = run_failing_second(DispatchFailure::Rejected(Rejection::InputTooLong {
        index: Some(0),
    }))
    .await;
    // Sub-batch 1 holds only input 1, so its local index 0 is original index 1.
    assert!(matches!(
        error,
        EmbeddingError::InputTooLong {
            index: Some(1),
            limit: None,
            ..
        }
    ));
    // AE2: the processed sub-batch stays charged and the rejected one is free.
    assert_eq!(
        error.settlements(),
        [settlement(1, 7, 0), settlement(1, 0, 0)]
    );
}

#[tokio::test(start_paused = true)]
async fn a_sub_batch_in_flight_when_a_sibling_fails_settles_as_indeterminate() {
    let fake = Fake::new(|seen| match seen.ids[0] {
        0 => Answer {
            delay: Duration::from_millis(1_000),
            result: response(rows_for(&[0]), Some(3)),
        },
        _ => Answer::now(Err(DispatchFailure::Rejected(Rejection::Other))),
    });
    let provider = provider_with(&openai_blueprint(), &fake, 2).with_max_inputs_per_request(1);
    let error = embed(&provider, &texts(2)).await.expect_err("fails");
    assert_eq!(
        error.settlements(),
        [settlement(1, 0, 1), settlement(1, 0, 0)]
    );
}

#[tokio::test]
async fn the_request_cap_stops_sending_and_carries_what_was_sent() {
    let fake = echo(Some(2));
    let provider = provider_with(&openai_blueprint(), &fake, 1).with_max_inputs_per_request(1);
    let budget = EmbeddingTokenBudget::new(
        EmbeddingLimits {
            max_requests: 2,
            ..EmbeddingLimits::default()
        },
        SharedTokenBudget::new(u64::MAX),
    );
    let inputs = texts(4);
    budget
        .reserve("m", provider.estimate_tokens("m", &inputs))
        .expect("reserve");

    let error = provider
        .embed("m", &inputs, Purpose::Document, &budget)
        .await
        .expect_err("third request is refused");

    assert!(matches!(
        &error,
        EmbeddingError::BudgetExceeded {
            limit_kind: EmbeddingLimitKind::Requests { limit: 2 },
            ..
        }
    ));
    assert_eq!(
        error.settlements(),
        [settlement(1, 2, 0), settlement(1, 2, 0)]
    );
    assert_eq!(fake.calls().len(), 2);
    assert_eq!(budget.requests(), 2);
}

// --- pre-send checks --------------------------------------------------------

#[tokio::test]
async fn an_unknown_alias_lists_the_declared_ones() {
    let fake = echo(None);
    let budget = budget();
    let error = provider(&fake)
        .embed("nope", &texts(1), Purpose::Query, &budget)
        .await
        .expect_err("unknown");
    assert!(matches!(
        error,
        EmbeddingError::UnknownModel { available, .. } if available == ["m"]
    ));
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn an_over_long_input_is_refused_before_dispatch_with_its_index() {
    let fake = echo(None);
    let mut inputs = texts(3);
    inputs[2] = "a".repeat(24_577);
    let error = embed(&provider(&fake), &inputs)
        .await
        .expect_err("too long");
    assert!(matches!(
        error,
        EmbeddingError::InputTooLong {
            index: Some(2),
            limit: Some(24_576),
            ..
        }
    ));
    assert!(fake.calls().is_empty());
}

/// The shared Hugging Face router silently truncates (live, 2026-10-05), so its
/// byte bound is the token limit less the 16 special tokens: 496 by default.
#[tokio::test]
async fn a_shared_hugging_face_input_over_496_bytes_is_refused_before_dispatch() {
    let fake = echo(None);
    let blueprint = blueprint(&yaml("huggingface", "bge", 4, "", ""));
    let provider = provider_with(&blueprint, &fake, 4);
    assert_eq!(provider.max_input_bytes("m"), Some(496));

    let error = embed(&provider, &["a".repeat(497)])
        .await
        .expect_err("one byte over");
    assert!(matches!(
        error,
        EmbeddingError::InputTooLong {
            index: Some(0),
            limit: Some(496),
            ..
        }
    ));
    assert!(fake.calls().is_empty());

    embed(&provider, &["a".repeat(496)])
        .await
        .expect("exactly the bound is sent");
    assert_eq!(fake.calls().len(), 1);
}

/// Byte bounds are not part of the identity: the same alias keeps its identity
/// whatever `max_input_tokens` it declares.
#[test]
fn the_byte_bound_does_not_change_the_identity() {
    let default = identity_of(&yaml("huggingface", "bge", 4, "", ""));
    let custom = identity_of(&yaml(
        "huggingface",
        "bge",
        4,
        "",
        "      max_input_tokens: 256\n",
    ));
    assert_eq!(default, custom);
}

/// `Embeddings.model` is the alias the program called, not the provider's own
/// model id.
#[tokio::test]
async fn the_batch_model_is_the_alias_not_the_provider_model_id() {
    let fake = echo(None);
    let batch = embed(&provider(&fake), &texts(2)).await.expect("embeds");
    assert_eq!(batch.model(), "m");
    assert_ne!(batch.model(), "text-embedding-3-small");
}

/// A closed pool is the provider's own bookkeeping failing: it is the internal
/// variant (which ends the run), not a transport or malformed-response error a
/// program could catch.
#[tokio::test]
async fn a_closed_pool_is_an_internal_error() {
    let fake = echo(None);
    let provider = provider(&fake);
    let resolved = provider.aliases.get("m").expect("alias resolves");
    let inputs = texts(2);
    let budget = budget();
    let call = Call {
        alias: "m",
        resolved,
        texts: &inputs,
        purpose: Purpose::Document,
        budget: &budget,
        dispatch: provider.dispatch.as_ref(),
        limit: Semaphore::new(1),
        slots: Mutex::new(vec![None]),
    };
    call.limit.close();
    let plan = plan_sub_batches(&inputs, &resolved.split_caps(Purpose::Document));
    let Err(error) = call.run(&plan).await else {
        panic!("a closed pool cannot dispatch");
    };
    assert!(
        matches!(error, EmbeddingError::Internal { .. }),
        "{error:?}"
    );
    assert!(fake.calls().is_empty(), "nothing was sent");
}

/// A shape the provider itself built wrongly is internal too, and keeps the
/// settlements of what was already sent.
#[test]
fn a_shape_mismatch_building_the_batch_is_internal_and_keeps_settlements() {
    let provider = provider(&echo(None));
    let resolved = provider.aliases.get("m").expect("alias resolves");
    let error = build_batch(
        "m",
        resolved,
        vec![1.0; 3],
        2,
        4,
        None,
        vec![settlement(1, 0, 1)],
    )
    .expect_err("3 values are not 2 x 4");
    assert!(
        matches!(error, EmbeddingError::Internal { .. }),
        "{error:?}"
    );
    assert_eq!(error.settlements(), [settlement(1, 0, 1)]);
}

#[tokio::test]
async fn an_empty_call_returns_an_empty_batch_without_dispatch() {
    let fake = echo(None);
    let batch = embed(&provider(&fake), &[]).await.expect("embeds");
    assert_eq!(batch.count(), 0);
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn a_failed_preflight_is_unauthorized_and_counts_nothing() {
    struct NoKey;
    impl EmbeddingDispatch for NoKey {
        fn dispatch<'a>(
            &'a self,
            _request: EmbeddingRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<DispatchResponse, DispatchFailure>> + Send + 'a>>
        {
            Box::pin(async {
                Err(DispatchFailure::NotSent(
                    NotSentReason::CredentialUnresolved,
                ))
            })
        }
        fn preflight<'a>(
            &'a self,
            _provider: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), DispatchFailure>> + Send + 'a>> {
            Box::pin(async {
                Err(DispatchFailure::NotSent(
                    NotSentReason::CredentialUnresolved,
                ))
            })
        }
    }
    let provider = BlueprintEmbeddingProvider::new(&openai_blueprint(), Arc::new(NoKey), 1);
    let budget = budget();
    let error = provider
        .embed("m", &texts(1), Purpose::Query, &budget)
        .await
        .expect_err("no key");
    assert!(matches!(&error, EmbeddingError::Unauthorized { .. }));
    assert_eq!(budget.requests(), 0);
    assert!(error.settlements().is_empty());
}

// --- usage clamp ------------------------------------------------------------

#[tokio::test]
async fn reported_usage_is_clamped_to_what_the_sent_bytes_can_cost() {
    for absurd in [u64::MAX, 1_000_000_000_000_000] {
        let fake = echo(Some(absurd));
        let provider = provider(&fake);
        let inputs = texts(3);
        // One byte per text plus the 16 special tokens each.
        let ceiling: u64 = inputs.iter().map(|t| t.len() as u64 + 16).sum();

        let batch = embed(&provider, &inputs).await.expect("the call succeeds");

        assert_eq!(batch.input_tokens(), Some(ceiling));
        let [only] = batch.settlements() else {
            panic!("one sub-batch, one settlement");
        };
        assert_eq!(only.reported, ceiling);
        assert_eq!(only.indeterminate, 0);
    }
}

#[tokio::test]
async fn a_huge_reported_usage_does_not_starve_other_runs_on_the_shared_aggregate() {
    let aggregate = SharedTokenBudget::new(1_000_000);
    let mine = EmbeddingTokenBudget::new(EmbeddingLimits::default(), aggregate.clone());
    let other = EmbeddingTokenBudget::new(EmbeddingLimits::default(), aggregate.clone());
    let fake = echo(Some(u64::MAX));
    let provider = provider(&fake);
    let inputs = texts(3);
    let reserved = provider.estimate_tokens("m", &inputs);

    other.reserve("m", 500_000).expect("the other run reserves");
    mine.reserve("m", reserved).expect("reserve");
    let batch = provider
        .embed("m", &inputs, Purpose::Document, &mine)
        .await
        .expect("the call succeeds");
    mine.settle(reserved, batch.settlements());

    assert!(aggregate.used() <= 500_000 + 3 * 17);
    assert!(mine.used() <= 3 * 17);
    other
        .reserve("m", 400_000)
        .expect("the other run's headroom is intact");
}
