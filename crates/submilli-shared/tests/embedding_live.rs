//! Live embedding-provider verification. **These tests call real provider
//! APIs, are opt-in, and can cost money.** They are not part of the default test
//! command or CI.
//!
//! A case calls its provider only when all of these hold:
//!
//! 1. `SUBMILLI_EMBEDDING_LIVE=1` is set (the opt-in; without it every case
//!    prints a skip note and returns, even when keys are present).
//! 2. `SUBMILLI_SKIP_HTTP_TESTS` is not `1` (the case is `ignore`d under it).
//! 3. The provider's key is in the environment:
//!
//! | Provider | Key variable | Optional |
//! |---|---|---|
//! | Voyage | `VOYAGE_API_KEY` | |
//! | OpenAI | `OPENAI_API_KEY` | |
//! | Google | `GEMINI_API_KEY` | |
//! | Jina | `JINA_API_KEY` | |
//! | Hugging Face (shared router) | `HF_TOKEN` | `HF_MODEL`, `HF_DIMENSIONS` |
//! | Hugging Face (dedicated) | `HF_TOKEN` and `HF_ENDPOINT_URL` | `HF_DIMENSIONS` |
//!
//! Run:
//!
//! ```text
//! SUBMILLI_EMBEDDING_LIVE=1 SUBMILLI_SKIP_HTTP_TESTS=0 \
//!   cargo test -p submilli-shared --test embedding_live -- --nocapture
//! ```
//!
//! Expected cost: each provider run sends a few short texts plus one
//! over-length text that the provider refuses, well under US$0.01 per provider
//! per run. Free tiers suffice. The Voyage over-length check uses `voyage-2`
//! (4,000-token context), whose limit a 5,000-token text exceeds, so it needs
//! no paid rate limits; the other Voyage checks use `voyage-3.5`.
//!
//! The key reaches the dispatch as a harness secret binding, exactly as in
//! production; it is never printed.

use std::sync::Arc;

use interpreter::runtime::{EmbeddingError, EmbeddingProvider, EmbeddingTokenBudget, Purpose};
use interpreter::stdlib::http::NetworkPolicy;
use submilli_blueprint::{HarnessSecretBindings, parse};
use submilli_shared::embedding::{BlueprintEmbeddingProvider, HttpEmbeddingDispatch};

const SECRET: &str = "LIVE_EMBEDDING_KEY";
const UNIT_TOLERANCE: f32 = 1e-3;

/// One provider under test.
struct Case {
    label: &'static str,
    key: String,
    /// The `embedding.providers.p` body after `type:`, indented.
    provider_yaml: String,
    model: String,
    /// Native vector length.
    dimensions: u64,
    /// A smaller length the model accepts as an override, when it does.
    override_dimensions: Option<u64>,
    /// Token ceiling for the over-length alias, so the provider (not the
    /// pre-send byte check) sees the too-long input.
    long_max_input_tokens: u64,
    /// A different model for the over-length alias only, when the default
    /// model's limit is too costly to exceed.
    long_model: Option<&'static str>,
    /// Over-length text: past the provider's own limit, within the alias's bytes.
    long_text: String,
}

/// The key, or a printed note and `None` so the caller skips. Also `None`
/// unless `SUBMILLI_EMBEDDING_LIVE=1`, whatever keys are present.
fn key_or_skip(label: &str, variable: &str) -> Option<String> {
    if std::env::var("SUBMILLI_EMBEDDING_LIVE").as_deref() != Ok("1") {
        println!("skipping {label}: SUBMILLI_EMBEDDING_LIVE=1 is not set (live tests are opt-in)");
        return None;
    }
    match std::env::var(variable) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => {
            println!("skipping {label}: {variable} is not set");
            None
        }
    }
}

fn env_u64(variable: &str, default: u64) -> u64 {
    std::env::var(variable)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// `bytes` bytes of text at about one token per two bytes.
fn filler(bytes: usize) -> String {
    "a ".repeat(bytes / 2)
}

fn blueprint_yaml(case: &Case) -> String {
    let mut yaml = format!(
        "name: live\nsecrets:\n  {SECRET}:\n    harness: {{}}\nembedding:\n  providers:\n    p:\n{}\
         \n  models:\n    default:\n      provider: p\n      model: \"{}\"\n      dimensions: {}\n",
        case.provider_yaml, case.model, case.dimensions
    );
    if let Some(dimensions) = case.override_dimensions {
        yaml.push_str(&format!(
            "    small:\n      provider: p\n      model: \"{}\"\n      dimensions: {dimensions}\n",
            case.model
        ));
    }
    yaml.push_str(&format!(
        "    long:\n      provider: p\n      model: \"{}\"\n      dimensions: {}\n      max_input_tokens: {}\n",
        case.long_model.unwrap_or(&case.model),
        case.dimensions,
        case.long_max_input_tokens
    ));
    yaml
}

fn provider_for(case: &Case) -> BlueprintEmbeddingProvider {
    let blueprint = match parse(&blueprint_yaml(case)) {
        Ok(blueprint) => blueprint,
        Err(err) => panic!("{}: the live blueprint parses: {err:?}", case.label),
    };
    let dispatch = HttpEmbeddingDispatch::new(
        Arc::new(blueprint.clone()),
        None,
        Arc::new(NetworkPolicy::allow_all()),
    )
    .expect("dispatch client")
    .with_harness_secrets(Arc::new(HarnessSecretBindings::from([(
        SECRET.to_string(),
        case.key.clone(),
    )])));
    BlueprintEmbeddingProvider::new(&blueprint, Arc::new(dispatch), 2)
}

fn two_texts() -> Vec<String> {
    vec![
        "How do I reset my password?".to_string(),
        "Passwords can be reset from the account settings page.".to_string(),
    ]
}

fn assert_unit(label: &str, row: &[f32], dimensions: u64) {
    assert_eq!(row.len() as u64, dimensions, "{label}: vector length");
    let norm = row.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() <= UNIT_TOLERANCE,
        "{label}: norm {norm} is not unit length"
    );
}

async fn embed(
    provider: &BlueprintEmbeddingProvider,
    alias: &str,
    texts: &[String],
    purpose: Purpose,
) -> Result<interpreter::runtime::EmbeddingBatch, EmbeddingError> {
    let budget = EmbeddingTokenBudget::unmetered();
    provider.embed(alias, texts, purpose, &budget).await
}

async fn check_query_and_document(case: &Case) {
    let provider = provider_for(case);
    let texts = two_texts();
    let mut identities = Vec::new();
    for purpose in [Purpose::Query, Purpose::Document] {
        let batch = match embed(&provider, "default", &texts, purpose).await {
            Ok(batch) => batch,
            Err(err) => panic!("{}: {purpose:?} embed failed: {err:?}", case.label),
        };
        assert_eq!(batch.count(), texts.len(), "{}: count", case.label);
        assert_eq!(batch.dimensions() as u64, case.dimensions);
        for index in 0..batch.count() {
            match batch.row(index) {
                Ok(row) => assert_unit(case.label, row, case.dimensions),
                Err(err) => panic!("{}: row {index}: {err:?}", case.label),
            }
        }
        identities.push(batch.identity().to_string());
    }
    assert_eq!(
        identities[0], identities[1],
        "{}: identity is stable across calls",
        case.label
    );
    assert_eq!(
        Some(identities[0].as_str()),
        provider.identity("default"),
        "{}: result identity matches discovery",
        case.label
    );
    println!("{}: query/document embed ok", case.label);
}

async fn check_dimensions_override(case: &Case) {
    let Some(dimensions) = case.override_dimensions else {
        println!(
            "{}: no dimensions override for this route; skipped",
            case.label
        );
        return;
    };
    let provider = provider_for(case);
    let batch = match embed(&provider, "small", &two_texts(), Purpose::Document).await {
        Ok(batch) => batch,
        Err(err) => panic!("{}: override embed failed: {err:?}", case.label),
    };
    assert_eq!(batch.dimensions() as u64, dimensions);
    for index in 0..batch.count() {
        match batch.row(index) {
            Ok(row) => assert_unit(case.label, row, dimensions),
            Err(err) => panic!("{}: row {index}: {err:?}", case.label),
        }
    }
    println!("{}: dimensions override {dimensions} ok", case.label);
}

async fn check_over_length_rejected(case: &Case) {
    let provider = provider_for(case);
    let texts = vec![case.long_text.clone()];
    // The alias is generous, so the pre-send byte check must pass and the
    // provider itself must refuse.
    assert!(
        provider
            .max_input_bytes("long")
            .is_some_and(|bytes| bytes >= case.long_text.len() as u64),
        "{}: the long alias admits the over-length text before sending",
        case.label
    );
    match embed(&provider, "long", &texts, Purpose::Document).await {
        Err(EmbeddingError::InputTooLong { .. }) => {
            println!("{}: over-length input rejected by the provider", case.label);
        }
        other => panic!(
            "{}: expected InputTooLong from the provider, got {:?}",
            case.label,
            other.map(|batch| batch.count())
        ),
    }
}

/// Probe a pre-send byte bound on the `default` alias: one byte over is refused
/// before sending with the bound as its limit, and exactly the bound is
/// accepted by the provider and comes back unit length.
async fn check_byte_bound(case: &Case, bound: usize) {
    let provider = provider_for(case);
    assert_eq!(
        provider.max_input_bytes("default"),
        Some(bound as u64),
        "{}: the alias's byte bound",
        case.label
    );
    let over = filler(bound + 2)[..=bound].to_string();
    match embed(&provider, "default", &[over], Purpose::Document).await {
        Err(EmbeddingError::InputTooLong { limit, .. }) if limit == Some(bound as u64) => {}
        other => panic!(
            "{}: {} bytes must be refused before sending, got {:?}",
            case.label,
            bound + 1,
            other.map(|batch| batch.count())
        ),
    }
    let at_bound = filler(bound);
    assert_eq!(at_bound.len(), bound);
    match embed(&provider, "default", &[at_bound], Purpose::Document).await {
        Ok(batch) => assert_unit(case.label, batch.row(0).unwrap_or(&[]), case.dimensions),
        Err(err) => panic!(
            "{}: exactly {bound} bytes must succeed: {err:?}",
            case.label
        ),
    }
    println!(
        "{}: byte-bound probe ok ({} refused, {bound} accepted)",
        case.label,
        bound + 1
    );
}

async fn run_all(case: Case) {
    check_query_and_document(&case).await;
    check_dimensions_override(&case).await;
    check_over_length_rejected(&case).await;
}

fn plain_provider(kind: &str) -> String {
    format!("      type: {kind}\n      api_key: \"${{secrets.{SECRET}}}\"")
}

// --- Voyage, OpenAI, Jina ---------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn voyage_live() {
    let Some(key) = key_or_skip("voyage", "VOYAGE_API_KEY") else {
        return;
    };
    run_all(Case {
        label: "voyage",
        key,
        provider_yaml: plain_provider("voyage"),
        model: "voyage-3.5".into(),
        dimensions: 1024,
        override_dimensions: Some(256),
        // `voyage-2` has a 4,000-token context, so a ~5,000-token text is
        // refused on a free account; the current models' limits are not
        // reachable within free-tier rate limits. 20 KB at ~2 bytes per token
        // is ~10k tokens.
        long_max_input_tokens: 60_000,
        long_model: Some("voyage-2"),
        long_text: filler(20_000),
    })
    .await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn openai_live() {
    let Some(key) = key_or_skip("openai", "OPENAI_API_KEY") else {
        return;
    };
    run_all(Case {
        label: "openai",
        key,
        provider_yaml: plain_provider("openai"),
        model: "text-embedding-3-small".into(),
        dimensions: 1536,
        override_dimensions: Some(256),
        // 100 KB is ~50k tokens, past the 8,192 limit.
        long_max_input_tokens: 100_000,
        long_model: None,
        long_text: filler(100_000),
    })
    .await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn jina_live() {
    let Some(key) = key_or_skip("jina", "JINA_API_KEY") else {
        return;
    };
    run_all(Case {
        label: "jina",
        key,
        provider_yaml: plain_provider("jina"),
        model: "jina-embeddings-v3".into(),
        dimensions: 1024,
        override_dimensions: Some(256),
        // ~50k tokens against an 8,192 limit.
        long_max_input_tokens: 100_000,
        long_model: None,
        long_text: filler(100_000),
    })
    .await;
}

// --- Hugging Face -----------------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn huggingface_shared_router_live() {
    let Some(key) = key_or_skip("huggingface (shared router)", "HF_TOKEN") else {
        return;
    };
    let model = std::env::var("HF_MODEL").unwrap_or_else(|_| "BAAI/bge-small-en-v1.5".into());
    // The shared router cannot reject over-length input: it ignores
    // `truncate: false` and silently cuts to the model's maximum length with
    // HTTP 200 (confirmed live, 2026-10-05). So the runtime bounds each text to
    // `max_input_tokens` less the 16 special tokens (496 bytes by default) and
    // the bound is probed instead of a provider rejection.
    let case = Case {
        label: "huggingface (shared router)",
        key,
        provider_yaml: plain_provider("huggingface"),
        model,
        dimensions: env_u64("HF_DIMENSIONS", 384),
        // The shared route does not take a dimensions parameter.
        override_dimensions: None,
        long_max_input_tokens: 512,
        long_model: None,
        long_text: String::new(),
    };
    check_query_and_document(&case).await;
    check_dimensions_override(&case).await;
    check_byte_bound(&case, 496).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn huggingface_dedicated_endpoint_live() {
    let Some(key) = key_or_skip("huggingface (dedicated)", "HF_TOKEN") else {
        return;
    };
    let Some(endpoint) = key_or_skip("huggingface (dedicated)", "HF_ENDPOINT_URL") else {
        return;
    };
    run_all(Case {
        label: "huggingface (dedicated)",
        key,
        provider_yaml: format!(
            "      type: huggingface\n      base_url: \"{endpoint}\"\n      api_key: \"${{secrets.{SECRET}}}\""
        ),
        model: "endpoint".into(),
        dimensions: env_u64("HF_DIMENSIONS", 384),
        // The override depends on the operator's server; not assumed.
        override_dimensions: None,
        long_max_input_tokens: 100_000,
        long_model: None,
        long_text: filler(20_000),
    })
    .await;
}

// --- Google -----------------------------------------------------------------

/// `gemini-embedding-001` silently truncates over-length input and succeeds, even
/// with `autoTruncate` off (confirmed live, 2026-10-05), so the runtime bounds it
/// by bytes. Probe the bound: 2,033 bytes is refused before sending,
/// and exactly 2,032 bytes is accepted by the provider.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn google_live() {
    let Some(key) = key_or_skip("google", "GEMINI_API_KEY") else {
        return;
    };
    let case = Case {
        label: "google",
        key,
        provider_yaml: format!("      type: google\n      api_key: \"${{secrets.{SECRET}}}\""),
        model: "gemini-embedding-001".into(),
        dimensions: 3072,
        override_dimensions: Some(768),
        long_max_input_tokens: 2_048,
        long_model: None,
        long_text: String::new(),
    };
    check_query_and_document(&case).await;
    check_dimensions_override(&case).await;

    check_byte_bound(&case, 2_032).await;
}
