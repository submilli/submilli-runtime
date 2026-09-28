//! Dispatch tests against a local mock HTTP server. **Zero live API calls** —
//! every request in this file terminates at a `httpmock` listener on loopback.
//!
//! These cover what the wire tests cannot: that the request actually reaches the
//! socket in the shape the codec built, that a non-2xx becomes the
//! [`ProviderFailure`] the ladder expects, and — the reason this file is long —
//! that **nothing secret survives into an error**. The leak assertions run the
//! real `LlmProvider` ladder over the real dispatch and search the guest-visible
//! outcome for the key, the prompt, the completion, and the body, because an
//! assertion on the dispatch's own return value would pass while the ladder
//! leaked.
//!
//! **How a request is asserted on.** `httpmock` 0.7 exposes no request-capture
//! API, so the expectation is written as the mock's own *routing condition*: the
//! path, the header, and a `json_body_partial` of the fields that must be
//! present. A request that does not match is not routed, the endpoint answers
//! 404, and `assert_hits(1)` fails — so the assertion cannot pass vacuously by
//! inspecting something the server never actually required.

use std::sync::Arc;

use httpmock::prelude::*;
use interpreter::runtime::{FailureReason, LlmCallError, LlmOutcome, LlmProvider};
use interpreter::stdlib::http::NetworkPolicy;
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, parse};

use super::super::provider::{BlueprintLlmProvider, ProviderUsage, StopReason};
use super::*;

const MODEL: &str = "m-1";
const PROMPT: &str = "the-prompt-text-that-must-not-leak";
const KEY: &str = "sk-live-secret-key-that-must-not-leak";
const SECRET_ENV: &str = "SUBMILLI_TEST_LLM_KEY";

/// A secret name nothing in this process ever sets.
///
/// The unresolvable-credential test needs a placeholder that cannot resolve.
/// Relying on `SECRET_ENV` merely being *unset* would be flaky: environment
/// variables are process-global and `with_key` runs concurrently in other tests
/// here, so that test would resolve their key and pass for the wrong reason.
const NEVER_SET: &str = "SUBMILLI_TEST_LLM_KEY_NEVER_SET";

/// A blueprint with one provider of the given kind and one model on it.
///
/// `base_url` is rewritten *after* parsing, the way `mcp/transport.rs`'s own
/// `stdio` test rewrites `transport`. Blueprint validation deliberately refuses
/// a plaintext-`http` loopback endpoint — that refusal is a security control
/// this file must not weaken to make itself testable, so the parse runs against
/// a valid https URL and the field is pointed at the mock server afterwards.
fn blueprint(kind: &str, base_url: &str, with_key: bool) -> Arc<Blueprint> {
    let key_line = if with_key {
        format!("      api_key: \"${{secrets.{SECRET_ENV}}}\"\n")
    } else {
        String::new()
    };
    let yaml = format!(
        "\
name: t
secrets:
  {SECRET_ENV}:
    env: {SECRET_ENV}
llm:
  providers:
    p:
      type: {kind}
      base_url: https://placeholder.invalid
{key_line}  models:
    {MODEL}:
      provider: p
",
    );
    let mut bp = parse(&yaml).expect("the test blueprint parses");
    bp.llm.providers.get_mut("p").unwrap().base_url = Some(base_url.to_string());
    Arc::new(bp)
}

fn dispatcher(blueprint: Arc<Blueprint>) -> HttpModelDispatch {
    HttpModelDispatch::new(blueprint, None, Arc::new(NetworkPolicy::allow_all()))
}

fn request<'a>(schema_json: Option<&'a str>) -> ModelRequest<'a> {
    ModelRequest {
        model: MODEL,
        provider: "p",
        prompt: PROMPT,
        schema_json,
        output_cap: Some(256),
    }
}

/// Serializes the environment-variable window below. Without it these tests
/// race: `cargo test` runs them on a thread pool, the variable is
/// process-global, and one test's `remove_var` lands inside another's call.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The credential resolves through the blueprint's `${secrets.X}` placeholder
/// from the environment, so a test asserting on it sets the variable for exactly
/// the duration of the call and clears it after.
fn with_key<T>(body: impl FnOnce() -> T) -> T {
    let guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: `ENV_LOCK` is held for the whole set/read/remove window, and the
    // variable is a test-only name no other code in this process reads.
    unsafe { std::env::set_var(SECRET_ENV, KEY) };
    let out = body();
    unsafe { std::env::remove_var(SECRET_ENV) };
    drop(guard);
    out
}

fn ok_body(kind: &str) -> Value {
    match kind {
        "anthropic" => json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "the-completion-text" }],
            "usage": { "input_tokens": 7, "output_tokens": 3 },
        }),
        "google" => json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": { "parts": [{ "text": "the-completion-text" }] },
            }],
            "usageMetadata": { "promptTokenCount": 7, "candidatesTokenCount": 3 },
        }),
        _ => json!({
            "choices": [{ "finish_reason": "stop", "message": { "content": "the-completion-text" } }],
            "usage": { "prompt_tokens": 7, "completion_tokens": 3 },
        }),
    }
}

// --- per-kind round trips --------------------------------------------------

/// Every kind reaches its own path with its own auth header and a body the
/// server can match on. The mock asserts the method, the path, the header, and
/// the prompt's position — so a kind whose request is built wrong never gets a
/// 200 and the test fails on the assertion *and* on the hit count.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn each_kind_round_trips_against_its_own_endpoint_and_header() {
    let cases = [
        (
            "anthropic",
            "/v1/messages",
            "x-api-key",
            KEY.to_string(),
            json!({ "model": MODEL, "messages": [{ "role": "user", "content": PROMPT }] }),
        ),
        (
            "google",
            "/v1beta/models/m-1:generateContent",
            "x-goog-api-key",
            KEY.to_string(),
            json!({ "contents": [{ "role": "user", "parts": [{ "text": PROMPT }] }] }),
        ),
        (
            "openai",
            "/v1/chat/completions",
            "authorization",
            format!("Bearer {KEY}"),
            json!({ "model": MODEL, "messages": [{ "role": "user", "content": PROMPT }] }),
        ),
        (
            "openai-compatible",
            "/v1/chat/completions",
            "authorization",
            format!("Bearer {KEY}"),
            json!({ "model": MODEL, "messages": [{ "role": "user", "content": PROMPT }] }),
        ),
    ];

    for (kind, path, header, header_value, expected_body) in cases {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path(path)
                .header(header, &header_value)
                .json_body_partial(expected_body.to_string());
            then.status(200)
                .header("content-type", "application/json")
                .json_body(ok_body(kind));
        });

        let response = with_key(|| {
            block_on(async {
                dispatcher(blueprint(kind, &server.base_url(), true))
                    .dispatch(request(None))
                    .await
            })
        });

        // The path, the auth header, and the prompt's position in the body were
        // all required to route this call — a miss on any one of them is a 404
        // and a hit count of zero.
        mock.assert_hits(1);
        let response = response.unwrap_or_else(|e| panic!("{kind} failed: {e:?}"));
        assert_eq!(response.stop_reason, StopReason::Stop, "{kind}");
        assert_eq!(
            response.text.as_deref(),
            Some("the-completion-text"),
            "{kind}",
        );
        assert_eq!(response.usage, ProviderUsage::reported(7.0, 3.0), "{kind}");
    }
}

/// The control for the test above: a request that does *not* match is not
/// routed, so the mock answers 404 and the hit count stays zero.
///
/// Without this, `json_body_partial` could be silently permissive — matching
/// everything — and the per-kind body assertions would all pass vacuously.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_mismatched_body_is_not_routed_which_is_what_makes_the_matchers_real() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .json_body_partial(json!({ "model": "a-different-model" }).to_string());
        then.status(200).json_body(ok_body("openai"));
    });

    let outcome = with_key(|| {
        block_on(async {
            dispatcher(blueprint("openai", &server.base_url(), true))
                .dispatch(request(None))
                .await
        })
    });

    mock.assert_hits(0);
    assert!(
        matches!(
            outcome,
            Err(ProviderFailure::ApiCall {
                status: Some(404),
                ..
            }),
        ),
        "an unmatched request should 404, got {outcome:?}",
    );
}

/// A redirect must not be followed, because following one hands the API key to
/// whatever host the response names.
///
/// `reqwest`'s default policy follows up to ten hops and strips only
/// `Authorization` when the host changes. Two of the four kinds authenticate
/// with a *custom* header — Anthropic's `x-api-key` and Google's
/// `x-goog-api-key` — which survive the hop, so the exposure is inverted from
/// intuition: the first-party kinds are the vulnerable ones and
/// `openai`/`openai-compatible` are only incidentally safe.
///
/// The attacker mock routes **only** when the secret header is present, so a
/// non-zero hit count is proof the credential arrived rather than merely that a
/// request did.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_redirect_is_refused_rather_than_carrying_the_key_to_another_host() {
    for (kind, path, header) in [
        ("anthropic", "/v1/messages", "x-api-key"),
        (
            "google",
            "/v1beta/models/m-1:generateContent",
            "x-goog-api-key",
        ),
    ] {
        let attacker = MockServer::start();
        let collected = attacker.mock(|when, then| {
            when.method(POST).header(header, KEY);
            then.status(200).json_body(ok_body(kind));
        });

        let upstream = MockServer::start();
        let redirect = upstream.mock(|when, then| {
            when.method(POST).path(path);
            then.status(307)
                .header("location", format!("{}/collected", attacker.base_url()));
        });

        let outcome = with_key(|| {
            block_on(async {
                dispatcher(blueprint(kind, &upstream.base_url(), true))
                    .dispatch(request(None))
                    .await
            })
        });

        redirect.assert_hits(1);
        collected.assert_hits(0);
        assert!(
            outcome.is_err(),
            "{kind}: a 307 must not resolve to a completion, got {outcome:?}",
        );
    }
}

/// The schema reaches the wire in each kind's own envelope, asserted on the
/// bytes the server actually received rather than on the codec's return value.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_typed_call_puts_the_schema_on_the_wire_per_kind() {
    let schema = r#"{"type":"object","properties":{"s":{"type":"string"}}}"#;
    let inner: Value = serde_json::from_str(schema).unwrap();
    let cases = [
        (
            "anthropic",
            json!({ "output_config": { "format": { "type": "json_schema", "schema": inner } } }),
        ),
        (
            "google",
            json!({
                "generationConfig": {
                    "responseMimeType": "application/json",
                    "responseSchema": inner,
                }
            }),
        ),
        (
            "openai",
            json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "submilli_response", "schema": inner },
                }
            }),
        ),
        (
            "openai-compatible",
            json!({
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "submilli_response", "schema": inner },
                }
            }),
        ),
    ];

    for (kind, expected_envelope) in cases {
        let server = MockServer::start();
        // The envelope is the routing condition: a kind that drops the schema,
        // nests it wrong, or downgrades it to `json_object` does not match and
        // never gets a 200.
        let mock = server.mock(|when, then| {
            when.method(POST)
                .json_body_partial(expected_envelope.to_string());
            then.status(200).json_body(ok_body(kind));
        });

        with_key(|| {
            block_on(async {
                dispatcher(blueprint(kind, &server.base_url(), true))
                    .dispatch(request(Some(schema)))
                    .await
            })
        })
        .unwrap_or_else(|e| panic!("{kind} did not send the schema envelope: {e:?}"));

        mock.assert_hits(1);
    }
}

/// The `json_object` form is asserted absent on the two kinds where it is the
/// available mistake — as a *negative* route, so a request carrying it matches
/// this mock and the hit count proves it was never sent.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn the_openai_kinds_never_send_the_shapeless_json_object_form() {
    for kind in ["openai", "openai-compatible"] {
        let server = MockServer::start();
        let broken = server.mock(|when, then| {
            when.method(POST).body_contains("json_object");
            then.status(200).json_body(ok_body(kind));
        });
        let good = server.mock(|when, then| {
            when.method(POST).body_contains("json_schema");
            then.status(200).json_body(ok_body(kind));
        });

        with_key(|| {
            block_on(async {
                dispatcher(blueprint(kind, &server.base_url(), true))
                    .dispatch(request(Some(r#"{"type":"object"}"#)))
                    .await
            })
        })
        .unwrap_or_else(|e| panic!("{kind} failed: {e:?}"));

        broken.assert_hits(0);
        good.assert_hits(1);
    }
}

/// A structured answer's JSON comes back as [`ProviderResponse::text`] so the
/// typed path can parse it — asserted end to end through a real socket.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_structured_response_returns_parseable_text() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(json!({
            "choices": [{
                "finish_reason": "stop",
                "message": { "content": "{\"severity\":\"high\"}" },
            }],
        }));
    });

    let response = with_key(|| {
        block_on(async {
            dispatcher(blueprint("openai", &server.base_url(), true))
                .dispatch(request(Some(r#"{"type":"object"}"#)))
                .await
        })
    })
    .expect("the call resolved");

    let text = response.text.expect("a structured answer carries text");
    let parsed: Value = serde_json::from_str(&text).expect("the text is JSON");
    assert_eq!(parsed, json!({ "severity": "high" }));
}

/// The `supports_structured_outputs: false` opt-out drops the schema entirely
/// rather than downgrading it to a shapeless `json_object` — an explicit
/// downgrade, not a silent one.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn the_structured_output_opt_out_sends_no_schema_at_all() {
    let server = MockServer::start();
    // Any request mentioning a response format at all — the schema envelope or
    // the shapeless downgrade — routes here, so a hit means the opt-out was not
    // honored.
    let any_format = server.mock(|when, then| {
        when.method(POST).body_contains("response_format");
        then.status(200).json_body(ok_body("openai-compatible"));
    });
    let plain = server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(ok_body("openai-compatible"));
    });

    let mut bp = blueprint("openai-compatible", &server.base_url(), true);
    Arc::get_mut(&mut bp)
        .expect("uniquely held")
        .llm
        .providers
        .get_mut("p")
        .unwrap()
        .supports_structured_outputs = false;

    with_key(|| {
        block_on(async {
            dispatcher(bp)
                .dispatch(request(Some(r#"{"type":"object"}"#)))
                .await
        })
    })
    .expect("the call resolved");

    any_format.assert_hits(0);
    plain.assert_hits(1);
}

// --- usage (KTD3) ----------------------------------------------------------

/// A provider can resolve successfully reporting no usage at all. `None`, never
/// `Some(0)`: a zero would tell the reconciler the call was free.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_success_with_no_usage_reports_none_not_zero() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(json!({
            "choices": [{ "finish_reason": "stop", "message": { "content": "hi" } }],
        }));
    });

    let outcome = ladder_outcome(&server, "openai", 200, None);
    assert!(outcome.ok, "a stop with no usage is still a success");
    assert_eq!(outcome.input_tokens, None);
    assert_eq!(outcome.output_tokens, None);
}

// --- failures the ladder has to classify ------------------------------------

/// 429, 5xx, and a dead connection each produce the failure the ladder turns
/// into the right guest-visible reason.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn rate_limited_provider_unavailable_and_transport_each_classify() {
    // A 429 carrying retry-after is the retryable arm.
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(429)
            .header("retry-after", "30")
            .json_body(json!({ "error": { "message": "slow down" } }));
    });
    let outcome = ladder_outcome(&server, "openai", 429, None);
    let failure = outcome.failure.as_ref().expect("a 429 is a failure");
    assert_eq!(failure.reason, FailureReason::RateLimited);
    assert_eq!(failure.status, Some(429));
    assert!(failure.retryable, "a retry-after makes it retryable");

    // A 429 without one is the provider saying the same request keeps failing.
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(429)
            .json_body(json!({ "error": { "message": "quota" } }));
    });
    let outcome = ladder_outcome(&server, "openai", 429, None);
    let failure = outcome.failure.as_ref().expect("a 429 is a failure");
    assert_eq!(failure.reason, FailureReason::RateLimited);
    assert!(!failure.retryable, "no retry-after means not retryable");

    // 5xx is provider-unavailable.
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(503).body("upstream down");
    });
    let outcome = ladder_outcome(&server, "openai", 503, None);
    let failure = outcome.failure.as_ref().expect("a 503 is a failure");
    assert_eq!(failure.reason, FailureReason::ProviderUnavailable);
    assert_eq!(failure.status, Some(503));
}

/// A connection that never answers is `transport`, and it carries **no status**
/// — that absence is exactly what distinguishes it from a provider that replied
/// with an error, and it is why the ladder cannot classify it structurally.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_connection_failure_is_transport_with_no_status() {
    // A port with nothing listening. Bind one to learn a free number, then drop
    // the listener before calling: `MockServer::start()` hands out a *pooled*
    // server that outlives the binding, so a mock's own URL would still answer
    // 404 and this would assert `request-rejected` instead.
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = listener.local_addr().expect("a bound address").port();
        drop(listener);
        format!("http://127.0.0.1:{port}")
    };

    let outcome = with_key(|| {
        block_on(async {
            let bp = blueprint("openai", &dead, true);
            BlueprintLlmProvider::new(
                Arc::clone(&bp),
                Arc::new(HttpModelDispatch::new(
                    bp,
                    None,
                    Arc::new(NetworkPolicy::allow_all()),
                )),
            )
            .call(MODEL, &[PROMPT.to_string()], None)
            .await
        })
    })
    .expect("the provider resolved")
    .remove(0);

    let failure = outcome
        .failure
        .as_ref()
        .expect("a dead socket is a failure");
    assert_eq!(failure.reason, FailureReason::Transport);
    assert_eq!(
        failure.status, None,
        "a connection that never answered must carry no status",
    );
    // reqwest renders the URL into its own `Display`, and an
    // `openai-compatible` URL is operator-supplied — so the detail has to be a
    // fixed classification string rather than the error's message.
    assert!(
        !failure.message.contains("127.0.0.1") && !failure.message.contains(&dead),
        "the endpoint reached the error: {}",
        failure.message,
    );
}

/// Context-length-exceeded is classified **from the body** — the one class with
/// no structural signal — and the body does not survive into the error.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn context_length_is_classified_from_the_body_which_never_reaches_the_error() {
    let body = json!({
        "error": {
            "message": "This model's maximum context length is 4097 tokens. \
                        However, you requested 4927 tokens.",
            "type": "invalid_request_error",
            "code": "context_length_exceeded",
        }
    });
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(400).json_body(body.clone());
    });

    let outcome = ladder_outcome(&server, "openai", 400, None);
    let failure = outcome.failure.as_ref().expect("a 400 is a failure");

    // Classified: the message is the context-length one, not the generic
    // request-rejected string.
    assert_eq!(failure.reason, FailureReason::RequestRejected);
    assert!(
        failure.message.contains("does not fit"),
        "not classified from the body: {}",
        failure.message,
    );

    // And nothing from the body came with it.
    for leaked in [
        "4097",
        "4927",
        "maximum context length",
        "invalid_request_error",
    ] {
        assert!(
            !failure.message.contains(leaked),
            "the body's '{leaked}' reached the error: {}",
            failure.message,
        );
    }
}

// --- R13 / KTD7: nothing secret survives ------------------------------------

/// The negative assertion the whole unit turns on.
///
/// A 400 whose body carries a **request echo** — the provider repeating the
/// prompt back, alongside an account identifier — is the realistic shape of a
/// leak: classification has to *read* that body, so it is in hand at exactly the
/// point the error is constructed. The assertion runs over the guest-visible
/// outcome after the full ladder, because an assertion on the dispatch's own
/// return value would pass while the ladder leaked.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn no_error_carries_the_key_the_prompt_the_completion_or_the_body() {
    let echo = json!({
        "error": {
            "message": format!("invalid request for prompt: {PROMPT}"),
            "type": "invalid_request_error",
            "param": "messages",
        },
        "request_echo": { "messages": [{ "role": "user", "content": PROMPT }] },
        "account_id": "acct_1234567890",
        "organization": "org-private-name",
    });

    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(400).json_body(echo.clone());
    });

    let outcome = ladder_outcome(&server, "openai", 400, None);
    let failure = outcome.failure.as_ref().expect("a 400 is a failure");

    // Everything a guest can see from this outcome, in one string.
    let visible = format!(
        "{} {:?} {:?} {:?}",
        failure.message, failure.finish_reason, failure.status, outcome.text,
    );

    for secret in [
        KEY,
        PROMPT,
        "acct_1234567890",
        "org-private-name",
        "request_echo",
        "invalid_request_error",
    ] {
        assert!(
            !visible.contains(secret),
            "'{secret}' reached the guest-visible outcome: {visible}",
        );
    }
}

/// The same assertion on the *success* side: a completion is model output, and a
/// truncated one lands on the failure arm with its text — which is intended —
/// but the failure's **message** must still be a fixed string, not the
/// completion.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_truncated_completion_does_not_leak_into_the_failure_message() {
    let completion = "the-completion-text-that-must-not-leak-into-a-message";
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(json!({
            "choices": [{ "finish_reason": "length", "message": { "content": completion } }],
        }));
    });

    let outcome = ladder_outcome(&server, "openai", 200, None);
    let failure = outcome.failure.as_ref().expect("truncation is a failure");
    assert_eq!(failure.reason, FailureReason::Truncated);
    assert!(
        !failure.message.contains(completion),
        "the completion reached the message: {}",
        failure.message,
    );
    // The partial text is carried deliberately, so the guest keeps usable output.
    assert_eq!(outcome.text.as_deref(), Some(completion));
}

/// A `harness:` secret backs an `api_key` just as an `env:` one does.
///
/// `harness:` is a valid source for any `${secrets.X}`, so an operator may back
/// a provider credential with one — and those bindings arrive per session rather
/// than living on the dispatch. A dispatch built without them resolves such a
/// key to nothing and reports a credential failure on a blueprint that is in
/// fact correct, which is why the server threads them through.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_harness_bound_credential_resolves_and_reaches_the_auth_header() {
    const HARNESS_SECRET: &str = "HARNESS_LLM_KEY";
    let server = MockServer::start();
    let with_auth = server.mock(|when, then| {
        when.method(POST)
            .header("authorization", format!("Bearer {KEY}"));
        then.status(200).json_body(ok_body("openai"));
    });

    let yaml = format!(
        "\
name: t
secrets:
  {HARNESS_SECRET}:
    harness:
      required: true
llm:
  providers:
    p:
      type: openai
      base_url: https://placeholder.invalid
      api_key: \"${{secrets.{HARNESS_SECRET}}}\"
  models:
    {MODEL}:
      provider: p
",
    );
    let mut bp = parse(&yaml).expect("the test blueprint parses");
    bp.llm.providers.get_mut("p").unwrap().base_url = Some(server.base_url());

    let mut bindings = submilli_blueprint::HarnessSecretBindings::new();
    bindings.insert(HARNESS_SECRET.to_string(), KEY.to_string());

    let dispatch = HttpModelDispatch::new(Arc::new(bp), None, Arc::new(NetworkPolicy::allow_all()))
        .with_harness_secrets(Arc::new(bindings));

    // No `with_key`: the value comes from the bindings, not the environment.
    block_on(async { dispatch.dispatch(request(None)).await }).expect("the call resolved");

    with_auth.assert_hits(1);
}

/// An unresolvable credential is `request-rejected` and says nothing at all —
/// not the secret's name, not the resolver's message, and certainly not a
/// partial key.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_unresolvable_credential_leaks_nothing() {
    let server = MockServer::start();
    let reached = server.mock(|when, then| {
        when.method(POST);
        then.status(200).json_body(ok_body("openai"));
    });

    let yaml = format!(
        "\
name: t
secrets:
  {NEVER_SET}:
    env: {NEVER_SET}
llm:
  providers:
    p:
      type: openai
      base_url: https://placeholder.invalid
      api_key: \"${{secrets.{NEVER_SET}}}\"
  models:
    {MODEL}:
      provider: p
",
    );
    let mut bp = parse(&yaml).expect("the test blueprint parses");
    bp.llm.providers.get_mut("p").unwrap().base_url = Some(server.base_url());
    let bp = Arc::new(bp);

    // Three prompts, so "once per dispatch" is observable: the per-element
    // shape reported the same misconfiguration once for each of them.
    let error = block_on(async {
        BlueprintLlmProvider::new(
            Arc::clone(&bp),
            Arc::new(HttpModelDispatch::new(
                bp,
                None,
                Arc::new(NetworkPolicy::allow_all()),
            )),
        )
        .call(
            MODEL,
            &[PROMPT.to_string(), PROMPT.to_string(), PROMPT.to_string()],
            None,
        )
        .await
    })
    .expect_err("an unresolvable key is a dispatch-level failure");

    // A credential belongs to the configuration, not to any one prompt, so it
    // is refused once before the fan-out rather than N times inside it. Nothing
    // was dispatched, so returning `Err` discards no billed sibling success.
    assert!(
        matches!(&error, LlmCallError::Unauthorized { model } if model == MODEL),
        "expected a dispatch-level Unauthorized, got {error:?}",
    );
    // The request was never sent: an unresolvable credential fails before the
    // socket, so no prompt reaches a provider that could log it.
    reached.assert_hits(0);
    let rendered = error.to_string();
    for secret in [NEVER_SET, "secrets", PROMPT] {
        assert!(
            !rendered.contains(secret),
            "'{secret}' reached the error: {rendered}",
        );
    }
}

/// The key is never sent anywhere but the auth header — asserted on the bytes
/// the server received, not on the codec.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn the_key_reaches_the_wire_only_as_a_header() {
    let server = MockServer::start();
    // A request carrying the key anywhere in its body routes here. Google's
    // documented `?key=` query form would likewise land on the path matcher
    // below rather than on the plain route.
    let leaked_body = server.mock(|when, then| {
        when.method(POST).body_contains(KEY);
        then.status(200).json_body(ok_body("openai"));
    });
    let leaked_query = server.mock(|when, then| {
        when.method(POST).query_param("key", KEY);
        then.status(200).json_body(ok_body("openai"));
    });
    let header_only = server.mock(|when, then| {
        when.method(POST)
            .header("authorization", format!("Bearer {KEY}"));
        then.status(200).json_body(ok_body("openai"));
    });

    with_key(|| {
        block_on(async {
            dispatcher(blueprint("openai", &server.base_url(), true))
                .dispatch(request(None))
                .await
        })
    })
    .expect("the call resolved");

    leaked_body.assert_hits(0);
    leaked_query.assert_hits(0);
    header_only.assert_hits(1);
}

/// Google is the kind whose own docs offer a `?key=` query form. It must use the
/// header instead, because a URL reaches logs, spans, and errors that a header
/// does not.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn google_sends_the_key_as_a_header_not_as_a_query_parameter() {
    let server = MockServer::start();
    let via_query = server.mock(|when, then| {
        when.method(POST).query_param("key", KEY);
        then.status(200).json_body(ok_body("google"));
    });
    let via_header = server.mock(|when, then| {
        when.method(POST).header("x-goog-api-key", KEY);
        then.status(200).json_body(ok_body("google"));
    });

    with_key(|| {
        block_on(async {
            dispatcher(blueprint("google", &server.base_url(), true))
                .dispatch(request(None))
                .await
        })
    })
    .expect("the call resolved");

    via_query.assert_hits(0);
    via_header.assert_hits(1);
}

/// A body larger than the cap is refused as a transport failure, and the
/// refusal carries none of what it refused to finish reading.
///
/// The cap is 32 MiB, which no test wants to transfer, so this exercises the
/// same guard on the UTF-8 arm: a body the reader cannot decode is likewise a
/// transport failure rather than a panic or a lossy string.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_undecodable_body_is_a_transport_failure_carrying_none_of_it() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        // Invalid UTF-8: a lone continuation byte.
        then.status(200)
            .header("content-type", "application/json")
            .body(vec![0x7b, 0x80, 0x7d]);
    });

    let outcome = with_key(|| {
        block_on(async {
            dispatcher(blueprint("openai", &server.base_url(), true))
                .dispatch(request(None))
                .await
        })
    });

    let Err(ProviderFailure::Transport { detail }) = outcome else {
        panic!("expected a transport failure, got {outcome:?}");
    };
    assert!(
        detail.contains("UTF-8"),
        "the detail must name the classification: {detail}",
    );
}

/// A first-party row declaring no `base_url` falls back to its kind's own
/// endpoint rather than failing.
///
/// Checked through [`ProviderKind`] rather than by dispatching, because
/// dispatching is precisely what would contact the real provider — the one
/// property here that cannot be asserted against a mock server without
/// asserting something else instead.
#[test]
fn a_first_party_row_without_a_base_url_falls_back_to_its_own_endpoint() {
    for (kind, expected) in [
        ("anthropic", Some("https://api.anthropic.com")),
        ("google", Some("https://generativelanguage.googleapis.com")),
        ("openai", Some("https://api.openai.com")),
        // The operator supplies this one; the blueprint validator requires it.
        ("openai-compatible", None),
    ] {
        let kind = ProviderKind::from_type(kind).expect("a shipping kind");
        assert_eq!(kind.default_base_url(), expected);
    }
}

// --- helpers ---------------------------------------------------------------

/// Run one element through the **real** ladder over the **real** dispatch, so an
/// assertion is about what a guest sees rather than about an intermediate value.
fn ladder_outcome(
    server: &MockServer,
    kind: &str,
    _expected_status: u16,
    schema_json: Option<&str>,
) -> LlmOutcome {
    with_key(|| {
        block_on(async {
            let bp = blueprint(kind, &server.base_url(), true);
            BlueprintLlmProvider::new(
                Arc::clone(&bp),
                Arc::new(HttpModelDispatch::new(
                    bp,
                    None,
                    Arc::new(NetworkPolicy::allow_all()),
                )),
            )
            .call(MODEL, &[PROMPT.to_string()], schema_json)
            .await
        })
    })
    .expect("the provider resolved")
    .remove(0)
}

/// `httpmock`'s blocking `MockServer` runs its own reactor, so these tests use a
/// throwaway current-thread runtime rather than `#[tokio::test]` — the pattern
/// the interpreter's own live-HTTP fixture harness uses for the same reason.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime builds")
        .block_on(future)
}

/// A provider endpoint is an outbound destination like any other: under the
/// server's deny-private policy a `base_url` on loopback is refused whether it
/// is written as a literal address (checked before sending) or as a host name
/// (refused at resolution), and the failure names the policy.
#[tokio::test]
async fn a_private_provider_endpoint_is_blocked_by_the_network_policy() {
    for base_url in ["http://127.0.0.1:1/v1", "http://localhost:1/v1"] {
        // No key, so nothing has to resolve before the destination is judged.
        let blueprint = blueprint("openai-compatible", base_url, false);
        let dispatch =
            HttpModelDispatch::new(blueprint, None, Arc::new(NetworkPolicy::deny_private()));
        let failure = dispatch
            .dispatch(request(None))
            .await
            .expect_err("the endpoint must be refused");
        match failure {
            ProviderFailure::Transport { detail } => assert!(
                detail.contains("blocked by network policy"),
                "{base_url}: expected the policy reason, got: {detail}"
            ),
            other => panic!("{base_url}: expected a transport failure, got {other:?}"),
        }
    }
}
