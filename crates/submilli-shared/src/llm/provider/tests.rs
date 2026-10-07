//! Provider tests, against hand-rolled dispatch fakes. **Zero live API calls.**

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use interpreter::runtime::{FailureReason, LlmCallError, LlmOutcome, LlmProvider};
use submilli_blueprint::{Blueprint, parse};

use super::*;

const MODEL: &str = "claude-sonnet-5";

/// A blueprint declaring one provider and two models, one of which describes
/// itself and sizes itself and one of which does neither.
fn blueprint() -> Arc<Blueprint> {
    Arc::new(
        parse(
            "\
name: triage
llm:
  providers:
    anthropic:
      type: anthropic
  models:
    claude-sonnet-5:
      provider: anthropic
      context_window: 200000
      output_reserve: 64000
      description: \"Strong reasoning; use for synthesis.\"
    bare:
      provider: anthropic
",
        )
        .expect("the test blueprint is valid"),
    )
}

/// A dispatch that answers every element the same way.
struct Always(Result<ProviderResponse, ProviderFailure>);

impl ModelDispatch for Always {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        let answer = self.0.clone();
        Box::pin(async move { answer })
    }
}

/// A dispatch that answers per prompt, looked up by the prompt text.
struct PerPrompt(BTreeMap<String, Result<ProviderResponse, ProviderFailure>>);

impl ModelDispatch for PerPrompt {
    fn dispatch<'a>(
        &'a self,
        request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        let answer = self
            .0
            .get(request.prompt)
            .cloned()
            .unwrap_or_else(|| Ok(stopped("unmapped")));
        Box::pin(async move { answer })
    }
}

fn stopped(text: &str) -> ProviderResponse {
    ProviderResponse {
        stop_reason: StopReason::Stop,
        text: Some(text.to_string()),
        usage: ProviderUsage::default(),
    }
}

fn provider(dispatch: impl ModelDispatch + 'static) -> BlueprintLlmProvider {
    BlueprintLlmProvider::new(blueprint(), Arc::new(dispatch))
}

/// One element through the classifier, via the real `call` path.
async fn one(answer: Result<ProviderResponse, ProviderFailure>) -> LlmOutcome {
    let prompts = vec!["p".to_string()];
    let mut outcomes = provider(Always(answer))
        .call(MODEL, &prompts, None)
        .await
        .expect("dispatch reached the model");
    assert_eq!(outcomes.len(), 1);
    outcomes.remove(0)
}

fn reason_of(outcome: &LlmOutcome) -> FailureReason {
    outcome
        .failure
        .as_ref()
        .unwrap_or_else(|| panic!("expected a failure arm, got {outcome:?}"))
        .reason
}

/// A local refusal reads to the guest as the transport failure it always did, and is told
/// apart only by the flag the call log records it by.
#[tokio::test]
async fn a_local_refusal_reads_as_a_transport_failure_and_is_flagged_local() {
    let local = one(Err(ProviderFailure::Local {
        detail: "provider is not declared".into(),
    }))
    .await;
    let wire = one(Err(ProviderFailure::Transport {
        detail: "connection reset".into(),
    }))
    .await;
    let (local, wire) = (
        local.failure.expect("failure"),
        wire.failure.expect("failure"),
    );
    assert!(local.local && !wire.local);
    assert_eq!(
        LlmFailure {
            local: false,
            ..local
        },
        wire
    );
}

/// A credential refused for one element is this host's own state: `request-rejected` for the
/// guest, flagged local for the call log.
#[tokio::test]
async fn a_per_element_unauthorized_is_flagged_local() {
    let outcome = one(Err(ProviderFailure::Unauthorized)).await;
    assert_eq!(reason_of(&outcome), FailureReason::RequestRejected);
    assert!(outcome.failure.expect("failure").local);
}

/// KTD1: `ok` keys off a natural stop, not off "nothing threw". Every one of the
/// nine per-element reasons is reachable, and each maps to exactly one outcome.
#[tokio::test]
async fn each_of_the_nine_outcomes_maps_to_the_right_reason() {
    let cases: Vec<(Result<ProviderResponse, ProviderFailure>, FailureReason)> = vec![
        (
            Ok(ProviderResponse {
                stop_reason: StopReason::Length,
                text: Some("partial".into()),
                usage: ProviderUsage::default(),
            }),
            FailureReason::Truncated,
        ),
        (
            Ok(ProviderResponse {
                stop_reason: StopReason::ContentFilter,
                text: None,
                usage: ProviderUsage::default(),
            }),
            FailureReason::ContentFiltered,
        ),
        (
            Err(ProviderFailure::NoObjectGenerated {
                text: None,
                usage: ProviderUsage::default(),
                stop_reason: None,
            }),
            FailureReason::InvalidOutput,
        ),
        (
            Err(ProviderFailure::ApiCall {
                status: Some(429),
                message: "rate limited".into(),
                response_body: None,
                retry_after_secs: Some(3),
                retry_after_present: true,
            }),
            FailureReason::RateLimited,
        ),
        (
            Err(ProviderFailure::ApiCall {
                status: Some(400),
                message: "bad request".into(),
                response_body: None,
                retry_after_secs: None,
                retry_after_present: false,
            }),
            FailureReason::RequestRejected,
        ),
        (
            Err(ProviderFailure::ApiCall {
                status: Some(503),
                message: "overloaded".into(),
                response_body: None,
                retry_after_secs: None,
                retry_after_present: false,
            }),
            FailureReason::ProviderUnavailable,
        ),
        (
            Err(ProviderFailure::Transport {
                detail: "connection reset".into(),
            }),
            FailureReason::Transport,
        ),
        (Err(ProviderFailure::Abort), FailureReason::Cancelled),
        (
            Ok(ProviderResponse {
                stop_reason: StopReason::Error,
                text: Some("half an answer".into()),
                usage: ProviderUsage::default(),
            }),
            FailureReason::Incomplete,
        ),
    ];

    assert_eq!(
        cases.len(),
        FailureReason::ALL.len(),
        "every per-element reason needs a case"
    );

    let mut covered = Vec::new();
    for (answer, expected) in cases {
        let outcome = one(answer.clone()).await;
        assert!(!outcome.ok, "{answer:?} must not be ok");
        assert_eq!(reason_of(&outcome), expected, "{answer:?}");
        covered.push(expected);
    }
    for reason in FailureReason::ALL {
        assert!(covered.contains(&reason), "{reason} is not covered");
    }
}

/// The honest cost of KTD1: a truncated completion is `ok: false` *and* still
/// carries the partial text, so a guest that wants it need not reach through an
/// error arm. Discarding it here is the data-loss bug the plan accepts a
/// mitigation for.
#[tokio::test]
async fn a_truncated_completion_keeps_its_usable_partial_text() {
    let outcome = one(Ok(ProviderResponse {
        stop_reason: StopReason::Length,
        text: Some("the first half of the ans".into()),
        usage: ProviderUsage::reported(10.0, 64.0),
    }))
    .await;

    assert!(!outcome.ok);
    assert_eq!(reason_of(&outcome), FailureReason::Truncated);
    assert_eq!(outcome.text.as_deref(), Some("the first half of the ans"));
    assert_eq!(outcome.input_tokens, Some(10));
    assert_eq!(outcome.output_tokens, Some(64));
}

/// Content filtering reaches us from both directions and must normalize: the
/// text path resolves with a filtered result, sometimes with text and sometimes
/// without, and neither may degrade into a different reason.
#[tokio::test]
async fn content_filtering_classifies_the_same_with_and_without_text() {
    for text in [Some("redact".to_string()), None] {
        let outcome = one(Ok(ProviderResponse {
            stop_reason: StopReason::ContentFilter,
            text: text.clone(),
            usage: ProviderUsage::default(),
        }))
        .await;
        assert!(!outcome.ok, "{text:?}");
        assert_eq!(
            reason_of(&outcome),
            FailureReason::ContentFiltered,
            "{text:?}"
        );
        // The two must not collapse: `None` is "produced none at all",
        // `Some("")` would be "produced an empty one".
        assert_eq!(outcome.text, text);
    }
}

/// The one truthful ambiguity KTD1 leaves: a model that genuinely said nothing
/// reports a natural stop, and is a success. Telling this from "empty because
/// filtered" is the whole reason `ok` keys off the stop reason.
#[tokio::test]
async fn a_natural_stop_with_empty_text_is_ok() {
    let outcome = one(Ok(ProviderResponse {
        stop_reason: StopReason::Stop,
        text: Some(String::new()),
        usage: ProviderUsage::default(),
    }))
    .await;

    assert!(outcome.ok);
    assert_eq!(outcome.text.as_deref(), Some(""));
    assert!(outcome.failure.is_none());

    // Byte-identical payload, different stop reason, opposite verdict.
    let filtered = one(Ok(ProviderResponse {
        stop_reason: StopReason::ContentFilter,
        text: Some(String::new()),
        usage: ProviderUsage::default(),
    }))
    .await;
    assert!(!filtered.ok);
    assert_eq!(filtered.text, outcome.text);
}

/// S8: the object path never consults the stop reason, so valid JSON with a
/// `content-filter` stop resolves there as a clean success. It must not be
/// reported as one — and when the object path *throws* carrying that stop
/// reason, it must not flatten into `invalid-output` either.
#[tokio::test]
async fn a_structured_success_with_a_content_filter_stop_is_not_reported_as_success() {
    // The resolved half: the object path handed back valid JSON and a
    // content-filter stop.
    let resolved = one(Ok(ProviderResponse {
        stop_reason: StopReason::ContentFilter,
        text: Some(r#"{"level":"low"}"#.into()),
        usage: ProviderUsage::reported(12.0, 4.0),
    }))
    .await;
    assert!(
        !resolved.ok,
        "valid JSON does not make a filtered completion a success"
    );
    assert_eq!(reason_of(&resolved), FailureReason::ContentFiltered);

    // The thrown half: the stop reason travels on the error and still wins over
    // the generic `invalid-output` the arm would otherwise pick.
    let thrown = one(Err(ProviderFailure::NoObjectGenerated {
        text: Some(r#"{"level":"low"}"#.into()),
        usage: ProviderUsage::default(),
        stop_reason: Some(StopReason::ContentFilter),
    }))
    .await;
    assert_eq!(reason_of(&thrown), FailureReason::ContentFiltered);

    // With no stop reason at all it is genuinely unusable output.
    let bare = one(Err(ProviderFailure::NoObjectGenerated {
        text: None,
        usage: ProviderUsage::default(),
        stop_reason: None,
    }))
    .await;
    assert_eq!(reason_of(&bare), FailureReason::InvalidOutput);
}

/// S5: at the default retry count the wrapper is the normal shape of a
/// persistent 429. A classifier that reads the outer error sees no status and
/// calls it a transport failure.
#[tokio::test]
async fn a_persistent_429_under_retries_classifies_as_rate_limited() {
    let wrapped = ProviderFailure::Retry {
        last_error: Box::new(ProviderFailure::ApiCall {
            status: Some(429),
            message: "rate limit exceeded".into(),
            response_body: None,
            retry_after_secs: Some(30),
            retry_after_present: true,
        }),
    };
    let outcome = one(Err(wrapped)).await;

    assert_eq!(reason_of(&outcome), FailureReason::RateLimited);
    let failure = outcome.failure.expect("failure arm");
    assert_eq!(failure.status, Some(429));
    assert!(
        failure.retryable,
        "a retry-after means it is worth retrying"
    );

    // Nested wrappers unwrap to the innermost, since a wrapper's last error may
    // itself be one.
    let nested = ProviderFailure::Retry {
        last_error: Box::new(ProviderFailure::Retry {
            last_error: Box::new(ProviderFailure::ApiCall {
                status: Some(503),
                message: "overloaded".into(),
                response_body: None,
                retry_after_secs: None,
                retry_after_present: false,
            }),
        }),
    };
    assert_eq!(
        reason_of(&one(Err(nested)).await),
        FailureReason::ProviderUnavailable
    );
}

/// An abort is a bare exception matching no SDK error class, and it
/// short-circuits retry — so it never arrives wrapped, and left to the
/// structural step it would read as a transport death. The cause is on this
/// side, not the wire.
#[tokio::test]
async fn an_abort_classifies_as_cancelled_not_transport() {
    let outcome = one(Err(ProviderFailure::Abort)).await;
    assert_eq!(reason_of(&outcome), FailureReason::Cancelled);

    let transport = one(Err(ProviderFailure::Transport {
        detail: "connection reset".into(),
    }))
    .await;
    assert_eq!(reason_of(&transport), FailureReason::Transport);
    assert_ne!(reason_of(&outcome), reason_of(&transport));
}

/// A missing status is what distinguishes a connection that died from a
/// provider that answered with an error.
#[tokio::test]
async fn an_api_call_with_no_status_classifies_as_transport() {
    let outcome = one(Err(ProviderFailure::ApiCall {
        status: None,
        message: "fetch failed".into(),
        response_body: None,
        retry_after_secs: None,
        retry_after_present: false,
    }))
    .await;
    assert_eq!(reason_of(&outcome), FailureReason::Transport);
    assert_eq!(outcome.failure.expect("failure arm").status, None);
}

/// S6: context-length-exceeded is the one class with no structural signal. The
/// body's error code is checked first; prose is the documented fallback for the
/// provider that emits no code at all.
#[tokio::test]
async fn context_length_exceeded_is_classified_from_the_body_code_then_prose() {
    let by_code = one(Err(ProviderFailure::ApiCall {
        status: Some(400),
        message: "Bad Request".into(),
        response_body: Some(
            r#"{"error":{"code":"context_length_exceeded","message":"too long"}}"#.into(),
        ),
        retry_after_secs: None,
        retry_after_present: false,
    }))
    .await;
    assert_eq!(reason_of(&by_code), FailureReason::RequestRejected);

    let by_prose = one(Err(ProviderFailure::ApiCall {
        status: Some(400),
        message: "prompt is too long: 250000 tokens > 200000 maximum".into(),
        response_body: None,
        retry_after_secs: None,
        retry_after_present: false,
    }))
    .await;
    assert_eq!(reason_of(&by_prose), FailureReason::RequestRejected);

    // Both are `request-rejected`: the request as sent will not start
    // succeeding, so neither is retryable. The closed set a guest branches on
    // stays closed — but step 5's reading must be *observable*, or it is not a
    // step: a context-length refusal names the fix that a generic one cannot.
    let generic = one(Err(ProviderFailure::ApiCall {
        status: Some(400),
        message: "messages[0].role is invalid".into(),
        response_body: None,
        retry_after_secs: None,
        retry_after_present: false,
    }))
    .await;
    assert_eq!(reason_of(&generic), FailureReason::RequestRejected);

    for outcome in [&by_code, &by_prose] {
        let failure = outcome.failure.as_ref().expect("failure arm");
        assert!(!failure.retryable);
        assert_eq!(failure.status, Some(400));
        assert!(
            failure.message.contains("context window"),
            "step 5 must be observable: {}",
            failure.message
        );
    }
    let generic_message = generic.failure.expect("failure arm").message;
    assert_eq!(
        generic_message,
        FailureReason::RequestRejected.default_message(),
        "a refusal step 5 did not classify keeps the generic string"
    );
    assert_ne!(
        by_code.failure.expect("failure arm").message,
        generic_message
    );
}

/// S9/KTD3: `null` means indeterminate, not free. A throttled call may still
/// have been billed, so the reconciler must be able to tell "no usage reported"
/// from "zero tokens".
#[tokio::test]
async fn absent_and_non_finite_usage_both_report_null_never_zero() {
    let absent = one(Ok(ProviderResponse {
        stop_reason: StopReason::Stop,
        text: Some("ok".into()),
        usage: ProviderUsage::default(),
    }))
    .await;
    assert_eq!(absent.input_tokens, None);
    assert_eq!(absent.output_tokens, None);

    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let outcome = one(Ok(ProviderResponse {
            stop_reason: StopReason::Stop,
            text: Some("ok".into()),
            usage: ProviderUsage::reported(bad, bad),
        }))
        .await;
        assert_eq!(outcome.input_tokens, None, "{bad}");
        assert_eq!(outcome.output_tokens, None, "{bad}");
    }

    // A genuine zero is still a zero — the guard must not swallow a real count.
    let zero = one(Ok(ProviderResponse {
        stop_reason: StopReason::Stop,
        text: Some("ok".into()),
        usage: ProviderUsage::reported(0.0, 0.0),
    }))
    .await;
    assert_eq!(zero.input_tokens, Some(0));
    assert_eq!(zero.output_tokens, Some(0));

    // One side reported and the other not — nullability is per field.
    let half = one(Ok(ProviderResponse {
        stop_reason: StopReason::Stop,
        text: Some("ok".into()),
        usage: ProviderUsage {
            input_tokens: Some(42.0),
            output_tokens: None,
        },
    }))
    .await;
    assert_eq!(half.input_tokens, Some(42));
    assert_eq!(half.output_tokens, None);
}

/// A dispatch whose per-element latency is deliberately inverted: the last
/// prompt answers first. Completion order is not input order, so a provider that
/// pushed results as they arrived would return them shuffled.
struct InvertedLatency {
    count: usize,
}

impl ModelDispatch for InvertedLatency {
    fn dispatch<'a>(
        &'a self,
        request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        let index: usize = request.prompt.parse().expect("prompt is its index");
        let delay = Duration::from_millis(((self.count - index) * 10) as u64);
        let text = format!("answer-{index}");
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(stopped(&text))
        })
    }
}

/// Element `i` is the outcome of `prompts[i]`, contractually — including when
/// the elements finish in the opposite order.
#[tokio::test]
async fn batch_results_are_positional_under_varied_latency() {
    const COUNT: usize = 8;
    let prompts: Vec<String> = (0..COUNT).map(|i| i.to_string()).collect();

    let outcomes = provider(InvertedLatency { count: COUNT })
        .call(MODEL, &prompts, None)
        .await
        .expect("dispatch");

    assert_eq!(outcomes.len(), COUNT);
    for (index, outcome) in outcomes.iter().enumerate() {
        assert!(outcome.ok, "{index}: {outcome:?}");
        assert_eq!(
            outcome.text.as_deref(),
            Some(format!("answer-{index}").as_str()),
            "element {index} is not the outcome of prompt {index}"
        );
    }
}

/// A per-element failure is never an `Err`: that would discard the siblings that
/// did succeed and were already billed, and leave budget reconciliation with
/// nothing to reconcile against.
#[tokio::test]
async fn one_failing_element_leaves_the_other_results_intact() {
    let mut answers = BTreeMap::new();
    answers.insert("a".to_string(), Ok(stopped("first")));
    answers.insert(
        "b".to_string(),
        Err(ProviderFailure::ApiCall {
            status: Some(429),
            message: "slow down".into(),
            response_body: None,
            retry_after_secs: Some(1),
            retry_after_present: true,
        }),
    );
    answers.insert("c".to_string(), Ok(stopped("third")));

    let prompts = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let outcomes = provider(PerPrompt(answers))
        .call(MODEL, &prompts, None)
        .await
        .expect("a failing element is not a dispatch failure");

    assert_eq!(outcomes.len(), 3);
    assert!(outcomes[0].ok);
    assert_eq!(outcomes[0].text.as_deref(), Some("first"));
    assert!(!outcomes[1].ok);
    assert_eq!(reason_of(&outcomes[1]), FailureReason::RateLimited);
    assert!(outcomes[2].ok);
    assert_eq!(outcomes[2].text.as_deref(), Some("third"));
}

/// Records the high-water mark of concurrent in-flight dispatches.
struct ConcurrencyProbe {
    in_flight: AtomicUsize,
    peak: AtomicUsize,
    /// Every element parks until this many have arrived, so the bound is
    /// actually contested rather than accidentally serialized by a fast fake.
    gate: Arc<tokio::sync::Barrier>,
}

impl ModelDispatch for ConcurrencyProbe {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        Box::pin(async move {
            let live = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(live, Ordering::SeqCst);
            // Hold the permit until a full bound's worth of elements is in
            // flight. If the implementation admitted fewer than the bound this
            // would deadlock rather than pass — which is the point.
            self.gate.wait().await;
            // Stay in flight a moment longer, so a bound-plus-one admission has
            // a window to be observed.
            tokio::task::yield_now().await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(stopped("ok"))
        })
    }
}

/// KTD4: unbounded fan-out manufactures the one error (429) that is
/// structurally detectable, and retry wrapping means N prompts unbounded is up
/// to 3N calls in flight.
///
/// The barrier is what makes this non-vacuous: it parks each element until a
/// full bound's worth have arrived, so the elements genuinely overlap. A fake
/// that returned immediately would peak at 1 and the assertion would hold
/// against *any* bound, including none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_never_exceeds_the_configured_bound() {
    for bound in [1usize, 2, 4] {
        let probe = Arc::new(ConcurrencyProbe {
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            gate: Arc::new(tokio::sync::Barrier::new(bound)),
        });
        let provider =
            BlueprintLlmProvider::new(blueprint(), Arc::clone(&probe) as Arc<dyn ModelDispatch>)
                .with_max_concurrency(bound);

        // A multiple of the bound, so every wave fills the barrier exactly.
        let prompts: Vec<String> = (0..bound * 5).map(|i| i.to_string()).collect();
        let outcomes = provider
            .call(MODEL, &prompts, None)
            .await
            .expect("dispatch");

        assert_eq!(outcomes.len(), prompts.len());
        let peak = probe.peak.load(Ordering::SeqCst);
        assert!(
            peak <= bound,
            "bound {bound} exceeded: {peak} elements were in flight at once"
        );
        assert_eq!(
            peak, bound,
            "bound {bound}: the elements never actually contested the bound, so this proves nothing"
        );
    }
}

/// KTD9: declaration is authoritative. A model the blueprint does not declare is
/// refused, and the refusal names the block that declares one.
#[tokio::test]
async fn calling_an_undeclared_model_is_rejected_naming_the_models_block() {
    let prompts = vec!["p".to_string()];
    let err = provider(Always(Ok(stopped("never reached"))))
        .call("gpt-5", &prompts, None)
        .await
        .expect_err("an undeclared model cannot dispatch");

    assert!(
        matches!(&err, LlmCallError::UnknownModel { model, .. } if model == "gpt-5"),
        "{err:?}"
    );
    let LlmCallError::UnknownModel { available, .. } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(available, &["bare".to_string(), MODEL.to_string()]);
    let rendered = err.to_string();
    assert!(rendered.contains("gpt-5"), "{rendered}");
    assert!(rendered.contains(MODEL), "the alternatives: {rendered}");
}

/// `models()` is a map over the blueprint: no I/O, so it cannot fail transiently
/// and a guest never sees a flaky catalog.
#[tokio::test]
async fn models_reports_the_declared_catalog_with_absent_fields_as_null() {
    let models = provider(Always(Ok(stopped("unused"))))
        .models()
        .await
        .expect("models() does no I/O and cannot fail");

    assert_eq!(models.len(), 2);

    let described = models
        .iter()
        .find(|m| m.name == MODEL)
        .expect("the described model");
    assert_eq!(
        described.description.as_deref(),
        Some("Strong reasoning; use for synthesis.")
    );
    assert_eq!(described.context_window, Some(200_000));

    // A model that declared neither reports `null` for both — never an invented
    // description, and never a zero window a program would size chunks against.
    let bare = models
        .iter()
        .find(|m| m.name == "bare")
        .expect("the bare model");
    assert_eq!(bare.description, None);
    assert_eq!(bare.context_window, None);
}

/// The declared `output_reserve` must reach the reservation, not just the
/// request.
///
/// KTD3b reserves `input + (output_cap × prompt_count)` *before* dispatch and
/// sends the same cap as the request's output limit — that pairing is what makes
/// the reservation an upper bound rather than an estimate. If the interpreter
/// reserved the configured default while the request carried the model's own
/// larger reserve, a provider could legitimately bill past what was reserved and
/// the ceiling would stop being preventive. A model that declared nothing
/// answers `None`, and the caller applies the default.
#[tokio::test]
async fn the_declared_output_reserve_is_answerable_for_the_reservation() {
    let provider = provider(Always(Ok(stopped("unused"))));

    assert_eq!(provider.output_reserve(MODEL), Some(64_000));
    assert_eq!(provider.output_reserve("bare"), None);
    // An undeclared model has no reserve to report; the call is refused
    // elsewhere, and this must not invent a cap for it.
    assert_eq!(provider.output_reserve("never-declared"), None);
}

/// The blueprint's declared `output_reserve` is what reaches the request as its
/// output cap, which is what makes the budget reservation an actual upper bound
/// rather than an estimate (KTD3b).
#[tokio::test]
async fn the_declared_output_reserve_reaches_the_request() {
    /// What one element actually asked the provider for.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Recorded {
        model: String,
        provider: String,
        output_cap: Option<u64>,
        schema_json: Option<String>,
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Recorded>>);

    impl ModelDispatch for Recorder {
        fn dispatch<'a>(
            &'a self,
            request: ModelRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>>
        {
            self.0.lock().expect("lock").push(Recorded {
                model: request.model.to_string(),
                provider: request.provider.to_string(),
                output_cap: request.output_cap,
                schema_json: request.schema_json.map(str::to_string),
            });
            Box::pin(async move { Ok(stopped("ok")) })
        }
    }

    let recorder = Arc::new(Recorder::default());
    let provider =
        BlueprintLlmProvider::new(blueprint(), Arc::clone(&recorder) as Arc<dyn ModelDispatch>);
    let prompts = vec!["p".to_string()];
    provider
        .call(MODEL, &prompts, Some(r#"{"type":"object"}"#))
        .await
        .expect("dispatch");

    let recorded = recorder.0.lock().expect("lock").clone();
    assert_eq!(
        recorded,
        vec![Recorded {
            model: MODEL.to_string(),
            provider: "anthropic".to_string(),
            output_cap: Some(64_000),
            schema_json: Some(r#"{"type":"object"}"#.to_string()),
        }]
    );

    // A model declaring no reserve leaves the cap to the operator default
    // rather than inventing one here.
    let prompts = vec!["p".to_string()];
    provider
        .call("bare", &prompts, None)
        .await
        .expect("dispatch");
    let recorded = recorder.0.lock().expect("lock").clone();
    assert_eq!(recorded[1].output_cap, None);
}

/// R13/KTD7, the half U1 flagged for this unit: step 5 *reads* a response body
/// to classify, so the body is in hand exactly where the error is built — and
/// the transport this file mirrors interpolates one. Nothing here may.
#[tokio::test]
async fn no_failure_carries_a_response_body_or_completion_text() {
    const ECHOED_PROMPT: &str = "patient SSN is 000-00-0000";
    const ACCOUNT_ID: &str = "org-9f3c1a2b4d";
    const COMPLETION: &str = "the diagnosis is confidential";

    // A 400 whose body carries a request echo and an account identifier — the
    // shape a provider actually returns.
    let body = format!(
        r#"{{"error":{{"code":"context_length_exceeded","message":"prompt is too long","param":"messages"}},"echo":"{ECHOED_PROMPT}","account":"{ACCOUNT_ID}"}}"#
    );
    let echoing_400 = one(Err(ProviderFailure::ApiCall {
        status: Some(400),
        message: format!("Bad Request: {ECHOED_PROMPT}"),
        response_body: Some(body),
        retry_after_secs: None,
        retry_after_present: false,
    }))
    .await;

    // A structured-output error whose `text` holds raw model output. That field
    // is completion content by definition.
    let no_object = one(Err(ProviderFailure::NoObjectGenerated {
        text: Some(COMPLETION.to_string()),
        usage: ProviderUsage::default(),
        stop_reason: None,
    }))
    .await;

    for (label, outcome) in [("400 echo", echoing_400), ("no-object", no_object)] {
        let failure = outcome
            .failure
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: expected a failure arm"));

        // Everything a guest can read off the failure arm, and the Debug
        // rendering that a log line would carry.
        let surfaces = [
            failure.message.clone(),
            failure.finish_reason.clone().unwrap_or_default(),
            format!("{failure:?}"),
            format!("{outcome:?}"),
        ];
        for surface in surfaces {
            for secret in [
                ECHOED_PROMPT,
                ACCOUNT_ID,
                COMPLETION,
                "context_length_exceeded",
            ] {
                assert!(
                    !surface.contains(secret),
                    "{label}: '{secret}' leaked into: {surface}"
                );
            }
        }

        // The classification survived — the assertions above are not passing
        // merely because the failure is empty.
        assert!(!failure.message.is_empty(), "{label}");
        // Every message is one of the fixed constants, never a rendering of
        // anything the provider sent. Membership in the closed vocabulary is
        // the property; which member it is, is step 5's business.
        let vocabulary: Vec<&str> = FailureReason::ALL
            .iter()
            .map(FailureReason::default_message)
            .chain(std::iter::once(super::CONTEXT_LENGTH_MESSAGE))
            .collect();
        assert!(
            vocabulary.contains(&failure.message.as_str()),
            "{label}: '{}' is not in the closed vocabulary",
            failure.message
        );
    }
}

/// The failure arm's `text` is for *partial completions the guest asked for*,
/// not for output the taxonomy boundary dropped. A structured-output error's
/// raw text is the latter.
#[tokio::test]
async fn a_no_object_error_drops_its_raw_text_rather_than_carrying_it() {
    let outcome = one(Err(ProviderFailure::NoObjectGenerated {
        text: Some("prose, not your shape".into()),
        usage: ProviderUsage::reported(7.0, 3.0),
        stop_reason: None,
    }))
    .await;

    assert_eq!(outcome.text, None);
    // Usage is still recovered — it is a number, not content, and it is the
    // only place a failed structured call reports one.
    assert_eq!(outcome.input_tokens, Some(7));
    assert_eq!(outcome.output_tokens, Some(3));
}

/// The raw stop reason travels for diagnosis while `reason` stays the closed set
/// a guest branches on, so an unclassified stop is diagnosable without widening
/// the enum.
#[tokio::test]
async fn an_unclassified_stop_reason_is_incomplete_and_keeps_its_raw_spelling() {
    let outcome = one(Ok(ProviderResponse {
        stop_reason: StopReason::Other("tool-calls".into()),
        text: Some("".into()),
        usage: ProviderUsage::default(),
    }))
    .await;

    assert_eq!(reason_of(&outcome), FailureReason::Incomplete);
    assert_eq!(
        outcome
            .failure
            .expect("failure arm")
            .finish_reason
            .as_deref(),
        Some("tool-calls")
    );
}

/// An empty batch is a no-op, not a deadlock or a panic on the placement array.
#[tokio::test]
async fn an_empty_batch_returns_no_outcomes() {
    let outcomes = provider(Always(Ok(stopped("unused"))))
        .call(MODEL, &[], None)
        .await
        .expect("dispatch");
    assert!(outcomes.is_empty());
}

/// A rate limit with no retry-after is the provider saying the same request will
/// keep failing; one with a retry-after is worth honoring and retrying.
#[tokio::test]
async fn retryability_of_a_rate_limit_follows_the_retry_after() {
    let with = one(Err(ProviderFailure::ApiCall {
        status: Some(429),
        message: "slow down".into(),
        response_body: None,
        retry_after_secs: Some(12),
        retry_after_present: true,
    }))
    .await;
    assert!(with.failure.expect("failure arm").retryable);

    let without = one(Err(ProviderFailure::ApiCall {
        status: Some(429),
        message: "quota exhausted".into(),
        response_body: None,
        retry_after_secs: None,
        retry_after_present: false,
    }))
    .await;
    assert!(!without.failure.expect("failure arm").retryable);

    // The HTTP-date form carries no seconds but says "come back later" just as
    // plainly. Keying retryability off the parsed seconds would report this as
    // the provider saying the same request will keep failing, and a guest retry
    // loop would abandon a request that was going to succeed.
    let dated = one(Err(ProviderFailure::ApiCall {
        status: Some(429),
        message: "slow down".into(),
        response_body: None,
        retry_after_secs: None,
        retry_after_present: true,
    }))
    .await;
    assert!(
        dated.failure.expect("failure arm").retryable,
        "a retry-after in the HTTP-date form is still a retry-after"
    );
}

/// The whole 5xx range maps to `provider-unavailable`, not just the one status a
/// test happened to use.
///
/// The boundaries are what a regression moves. Misrouting a bare 500 to
/// `request-rejected` flips two guest-visible fields — `reason`, and `retryable`
/// from true to false — so a guest retry loop keyed on `retryable` would stop
/// retrying a transient outage. 500 is the most common 5xx and was the one left
/// unpinned: narrowing the arm to `503..=503` kept the whole suite green.
#[tokio::test]
async fn the_whole_5xx_range_is_provider_unavailable_and_its_neighbours_are_not() {
    for (status, expected) in [
        (499, FailureReason::RequestRejected),
        (500, FailureReason::ProviderUnavailable),
        (503, FailureReason::ProviderUnavailable),
        (599, FailureReason::ProviderUnavailable),
        (600, FailureReason::RequestRejected),
    ] {
        let outcome = one(Err(ProviderFailure::ApiCall {
            status: Some(status),
            message: "upstream".into(),
            response_body: None,
            retry_after_secs: None,
            retry_after_present: false,
        }))
        .await;
        assert_eq!(
            outcome.failure.expect("failure arm").reason,
            expected,
            "status {status}",
        );
    }
}

#[tokio::test]
async fn empty_single_and_extreme_concurrency_batches_preserve_results() {
    for bound in [0, 1, usize::MAX] {
        let provider = provider(Always(Ok(stopped("ok")))).with_max_concurrency(bound);
        for count in [0, 1, 7] {
            let prompts = vec!["prompt".to_string(); count];
            let outcomes = provider.call(MODEL, &prompts, None).await.unwrap();
            assert_eq!(outcomes.len(), count);
            assert!(
                outcomes
                    .iter()
                    .all(|outcome| outcome.text.as_deref() == Some("ok"))
            );
        }
    }
}

struct CancellableDispatch {
    active: AtomicUsize,
    started: AtomicUsize,
    park: std::sync::atomic::AtomicBool,
}

struct ActiveDispatch<'a>(&'a AtomicUsize);

impl Drop for ActiveDispatch<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ModelDispatch for CancellableDispatch {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        Box::pin(async move {
            self.started.fetch_add(1, Ordering::SeqCst);
            self.active.fetch_add(1, Ordering::SeqCst);
            let _active = ActiveDispatch(&self.active);
            if self.park.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            Ok(stopped("ok"))
        })
    }
}

#[tokio::test]
async fn cancelled_batch_drops_active_dispatches_and_admits_no_queued_work() {
    let dispatch = Arc::new(CancellableDispatch {
        active: AtomicUsize::new(0),
        started: AtomicUsize::new(0),
        park: std::sync::atomic::AtomicBool::new(true),
    });
    let provider = BlueprintLlmProvider::new(blueprint(), dispatch.clone()).with_max_concurrency(2);
    let prompts = vec!["prompt".to_string(); 7];
    let mut call = provider.call(MODEL, &prompts, None);
    assert!(futures::poll!(call.as_mut()).is_pending());
    assert_eq!(dispatch.active.load(Ordering::SeqCst), 2);
    drop(call);
    assert_eq!(dispatch.active.load(Ordering::SeqCst), 0);
    assert_eq!(dispatch.started.load(Ordering::SeqCst), 2);
    dispatch.park.store(false, Ordering::SeqCst);
    assert_eq!(provider.call(MODEL, &prompts, None).await.unwrap().len(), 7);
}

#[tokio::test]
async fn empty_batches_still_validate_credentials() {
    struct Unauthorized;
    impl ModelDispatch for Unauthorized {
        fn dispatch<'a>(
            &'a self,
            _: ModelRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>>
        {
            panic!("preflight must stop dispatch");
        }
        fn preflight<'a>(
            &'a self,
            _: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), ProviderFailure>> + Send + 'a>> {
            Box::pin(async { Err(ProviderFailure::Unauthorized) })
        }
    }
    assert!(matches!(
        provider(Unauthorized).call(MODEL, &[], None).await,
        Err(LlmCallError::Unauthorized { .. })
    ));
}
