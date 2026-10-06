//! Dispatch tests against a local mock HTTP server. **Zero live API calls**:
//! every request terminates at an `httpmock` listener on loopback.
//!
//! A request is asserted on as the mock's own routing condition (path, header,
//! partial body): a request that does not match is not routed, the mock answers
//! 404, and `assert_hits(1)` fails, so no assertion can pass vacuously.
//!
//! The provider `base_url` is passed on the [`EmbeddingRequest`] directly, so
//! the blueprint's own https-only endpoint validation is never weakened.

use std::sync::Arc;

use httpmock::prelude::*;
use interpreter::runtime::{EmbeddingMalformedReason, Purpose};
use serde_json::{Value, json};
use submilli_blueprint::{EmbeddingProviderType, HarnessSecretBindings, parse};

use super::*;

const KEY: &str = "sk-live-secret-key-that-must-not-leak";
const SECRET_NAME: &str = "TEST_EMB_KEY";
const INPUT: &str = "the-input-text-that-must-not-leak";
const KEY_REF: &str = "${secrets.TEST_EMB_KEY}";

fn blueprint(provider_type: &str, with_key: bool, base_url: Option<&str>) -> Arc<Blueprint> {
    let key_line = if with_key {
        format!("      api_key: \"${{secrets.{SECRET_NAME}}}\"\n")
    } else {
        String::new()
    };
    let base_line = base_url
        .map(|url| format!("      base_url: {url}\n"))
        .unwrap_or_default();
    let yaml = format!(
        "\
name: t
secrets:
  {SECRET_NAME}:
    harness: {{}}
embedding:
  providers:
    p:
      type: {provider_type}
{base_line}{key_line}  models:
    m:
      provider: p
      model: some-model
      dimensions: 2
",
    );
    match parse(&yaml) {
        Ok(bp) => Arc::new(bp),
        Err(err) => panic!("the test blueprint parses: {err:?}"),
    }
}

fn dispatcher(blueprint: Arc<Blueprint>) -> HttpEmbeddingDispatch {
    HttpEmbeddingDispatch::new(blueprint, None, Arc::new(NetworkPolicy::allow_all()))
        .expect("dispatch client")
        .with_harness_secrets(Arc::new(HarnessSecretBindings::from([(
            SECRET_NAME.into(),
            KEY.into(),
        )])))
}

fn bare_dispatcher() -> HttpEmbeddingDispatch {
    dispatcher(blueprint("voyage", true, None))
}

fn request<'a>(
    kind: EmbeddingProviderType,
    base_url: &'a str,
    api_key: Option<&'a str>,
    texts: &'a [String],
) -> EmbeddingRequest<'a> {
    EmbeddingRequest {
        provider: "p",
        provider_type: kind,
        model: "some-model",
        base_url: Some(base_url),
        api_key,
        dimensions: 2,
        send_dimensions: true,
        purpose: Purpose::Document,
        query_prompt_name: None,
        document_prompt_name: None,
        texts,
    }
}

fn texts() -> Vec<String> {
    vec![INPUT.to_string()]
}

fn ok_body(kind: EmbeddingProviderType) -> Value {
    match kind {
        EmbeddingProviderType::Google => json!({
            "embeddings": [{ "values": [0.5, 0.25] }],
            "usageMetadata": { "promptTokenCount": 4 },
        }),
        EmbeddingProviderType::HuggingFace => json!([[0.5, 0.25]]),
        _ => json!({
            "data": [{ "index": 0, "embedding": [0.5, 0.25] }],
            "usage": { "total_tokens": 4, "prompt_tokens": 4 },
        }),
    }
}

// --- round trips -----------------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn each_provider_reaches_its_own_path_with_its_own_header() {
    let cases = [
        (
            EmbeddingProviderType::Voyage,
            "/v1/embeddings",
            "authorization",
            format!("Bearer {KEY}"),
        ),
        (
            EmbeddingProviderType::OpenAi,
            "/v1/embeddings",
            "authorization",
            format!("Bearer {KEY}"),
        ),
        (
            EmbeddingProviderType::Jina,
            "/v1/embeddings",
            "authorization",
            format!("Bearer {KEY}"),
        ),
        (
            EmbeddingProviderType::Google,
            "/v1beta/models/some-model:batchEmbedContents",
            "x-goog-api-key",
            KEY.to_string(),
        ),
    ];
    let t = texts();
    for (kind, path, header, header_value) in cases {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path(path)
                .header(header, &header_value)
                .body_contains(INPUT);
            then.status(200)
                .header("content-type", "application/json")
                .json_body(ok_body(kind));
        });

        let base = server.base_url();
        let response = block_on(async {
            bare_dispatcher()
                .dispatch(request(kind, &base, Some(KEY_REF), &t))
                .await
        });

        mock.assert_hits(1);
        let response = response.unwrap_or_else(|e| panic!("{kind:?} failed: {e:?}"));
        assert_eq!(response.rows.len(), 1, "{kind:?}");
        assert_eq!(response.rows[0].values, vec![0.5, 0.25], "{kind:?}");
        assert_eq!(response.usage, Some(4), "{kind:?}");
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_huggingface_dedicated_endpoint_without_a_key_sends_no_authorization() {
    let server = MockServer::start();
    let authorized = server.mock(|when, then| {
        when.method(POST).header_exists("authorization");
        then.status(200)
            .json_body(ok_body(EmbeddingProviderType::HuggingFace));
    });
    let anonymous = server.mock(|when, then| {
        when.method(POST).path("/embed");
        then.status(200)
            .json_body(ok_body(EmbeddingProviderType::HuggingFace));
    });

    let t = texts();
    let base = server.base_url();
    let response = block_on(async {
        dispatcher(blueprint(
            "huggingface",
            false,
            Some("https://e.example.com"),
        ))
        .dispatch(request(EmbeddingProviderType::HuggingFace, &base, None, &t))
        .await
    })
    .expect("the call resolved");

    authorized.assert_hits(0);
    anonymous.assert_hits(1);
    assert_eq!(response.usage, None);
}

/// The control: a request that does not match the routing condition is a 404,
/// which is what makes the matchers above real.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_unmatched_request_is_a_free_rejection() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path("/somewhere-else");
        then.status(200)
            .json_body(ok_body(EmbeddingProviderType::OpenAi));
    });
    let t = texts();
    let base = server.base_url();
    let outcome = block_on(async {
        bare_dispatcher()
            .dispatch(request(
                EmbeddingProviderType::OpenAi,
                &base,
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    mock.assert_hits(0);
    assert_eq!(outcome, Err(DispatchFailure::Rejected(Rejection::Other)));
}

// --- failure mapping through a socket --------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn statuses_map_through_the_real_round_trip() {
    let t = texts();
    let cases: [(u16, Option<&str>, DispatchFailure); 4] = [
        (
            401,
            None,
            DispatchFailure::Rejected(Rejection::Unauthorized),
        ),
        (
            429,
            Some("30"),
            DispatchFailure::Rejected(Rejection::RateLimited {
                retry_after: Some(std::time::Duration::from_secs(30)),
            }),
        ),
        (
            503,
            None,
            DispatchFailure::Failed {
                kind: SentFailure::ProviderUnavailable,
                usage: None,
            },
        ),
        (
            400,
            None,
            DispatchFailure::Rejected(Rejection::InputTooLong { index: None }),
        ),
    ];
    for (status, retry_after, expected) in cases {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST);
            let then = then.status(status);
            let then = match retry_after {
                Some(value) => then.header("retry-after", value),
                None => then,
            };
            then.body(r#"{"error":{"message":"maximum context length"}}"#);
        });
        let base = server.base_url();
        let outcome = block_on(async {
            bare_dispatcher()
                .dispatch(request(
                    EmbeddingProviderType::OpenAi,
                    &base,
                    Some(KEY_REF),
                    &t,
                ))
                .await
        });
        assert_eq!(outcome, Err(expected), "status {status}");
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_two_hundred_that_is_not_json_is_an_invalid_body() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(200).body("<html>oops</html>");
    });
    let t = texts();
    let base = server.base_url();
    let outcome = block_on(async {
        bare_dispatcher()
            .dispatch(request(
                EmbeddingProviderType::Jina,
                &base,
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    assert_eq!(
        outcome,
        Err(DispatchFailure::Malformed {
            reason: EmbeddingMalformedReason::InvalidBody,
            usage: None
        })
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_connection_failure_never_left_and_names_no_endpoint() {
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = listener.local_addr().expect("a bound address").port();
        drop(listener);
        format!("http://127.0.0.1:{port}")
    };
    let t = texts();
    let outcome = block_on(async {
        bare_dispatcher()
            .dispatch(request(
                EmbeddingProviderType::Voyage,
                &dead,
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    assert_eq!(
        outcome,
        Err(DispatchFailure::NotSent(NotSentReason::Unreachable))
    );
    assert!(!format!("{outcome:?}").contains("127.0.0.1"));
}

// --- redirects, body cap, timeouts -----------------------------------------

/// A redirect is refused rather than carrying the key to another host. The
/// attacker mock routes only when the secret header is present, so a non-zero
/// hit count would prove the credential arrived.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_redirect_is_refused_rather_than_carrying_the_key_to_another_host() {
    let t = texts();
    for (kind, path, header) in [
        (
            EmbeddingProviderType::Google,
            "/v1beta/models/some-model:batchEmbedContents",
            "x-goog-api-key",
        ),
        (
            EmbeddingProviderType::Voyage,
            "/v1/embeddings",
            "authorization",
        ),
    ] {
        let attacker = MockServer::start();
        let collected = attacker.mock(|when, then| {
            when.method(POST).header_exists(header);
            then.status(200).json_body(ok_body(kind));
        });
        let upstream = MockServer::start();
        let redirect = upstream.mock(|when, then| {
            when.method(POST).path(path);
            then.status(307)
                .header("location", format!("{}/collected", attacker.base_url()));
        });

        let base = upstream.base_url();
        let outcome = block_on(async {
            bare_dispatcher()
                .dispatch(request(kind, &base, Some(KEY_REF), &t))
                .await
        });

        redirect.assert_hits(1);
        collected.assert_hits(0);
        assert_eq!(
            outcome,
            Err(DispatchFailure::Failed {
                kind: SentFailure::Transport,
                usage: None
            }),
            "{kind:?}"
        );
    }
}

/// A body past the cap is a transport failure, not a buffered success.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_body_over_the_cap_is_a_transport_failure() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(200)
            .body(" ".repeat(http_client::MAX_RESPONSE_BYTES + 1));
    });
    let t = texts();
    let base = server.base_url();
    let outcome = block_on(async {
        bare_dispatcher()
            .dispatch(request(
                EmbeddingProviderType::OpenAi,
                &base,
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    assert_eq!(
        outcome,
        Err(DispatchFailure::Failed {
            kind: SentFailure::Transport,
            usage: None
        })
    );
}

/// The real request timeout is ten minutes, so the mapping is asserted on the
/// classification the shared helper produces for one.
#[test]
fn only_the_timeout_classification_becomes_a_timeout() {
    let timed_out = TransportFailure {
        detail: http_client::TIMEOUT_DETAIL.to_string(),
    };
    assert_eq!(
        failed_from(timed_out),
        DispatchFailure::Failed {
            kind: SentFailure::Timeout,
            usage: None
        }
    );
    for detail in [
        "connection failed",
        "request failed",
        "response exceeded the size limit",
    ] {
        assert_eq!(
            failed_from(TransportFailure {
                detail: detail.to_string()
            }),
            DispatchFailure::Failed {
                kind: SentFailure::Transport,
                usage: None
            },
            "{detail}"
        );
    }
}

// --- credentials -----------------------------------------------------------

/// A 400 echoing the input and the key must not survive into the failure, and an
/// unresolvable key sends nothing at all.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn no_failure_carries_the_key_the_input_or_the_body() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST);
        then.status(400).json_body(json!({
            "error": { "message": format!("invalid input {INPUT} for key {KEY}") },
            "account_id": "acct_1234567890",
        }));
    });
    let t = texts();
    let base = server.base_url();
    for status_body_kind in [EmbeddingProviderType::OpenAi, EmbeddingProviderType::Google] {
        let outcome = block_on(async {
            bare_dispatcher()
                .dispatch(request(status_body_kind, &base, Some(KEY_REF), &t))
                .await
        });
        let rendered = format!("{outcome:?}");
        for secret in [KEY, INPUT, "acct_1234567890", &base] {
            assert!(!rendered.contains(secret), "'{secret}' leaked: {rendered}");
        }
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_unresolvable_key_sends_nothing() {
    let server = MockServer::start();
    let any = server.mock(|when, then| {
        when.method(POST);
        then.status(200)
            .json_body(ok_body(EmbeddingProviderType::OpenAi));
    });
    let t = texts();
    let base = server.base_url();
    let unbound = HttpEmbeddingDispatch::new(
        blueprint("openai", true, None),
        None,
        Arc::new(NetworkPolicy::allow_all()),
    )
    .expect("dispatch client");
    let outcome = block_on(async {
        unbound
            .dispatch(request(
                EmbeddingProviderType::OpenAi,
                &base,
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    any.assert_hits(0);
    assert_eq!(
        outcome,
        Err(DispatchFailure::NotSent(
            NotSentReason::CredentialUnresolved
        ))
    );
}

#[test]
fn preflight_resolves_the_key_without_a_request() {
    // Bound: passes.
    assert_eq!(block_on(bare_dispatcher().preflight("p")), Ok(()));

    // Declared but unbound: NotSent, and the failure names nothing.
    let unbound = HttpEmbeddingDispatch::new(
        blueprint("voyage", true, None),
        None,
        Arc::new(NetworkPolicy::allow_all()),
    )
    .expect("dispatch client");
    let outcome = block_on(unbound.preflight("p"));
    assert_eq!(
        outcome,
        Err(DispatchFailure::NotSent(
            NotSentReason::CredentialUnresolved
        ))
    );
    assert!(!format!("{outcome:?}").contains(SECRET_NAME));

    // A Hugging Face dedicated endpoint declaring no key passes.
    let hf = HttpEmbeddingDispatch::new(
        blueprint("huggingface", false, Some("https://e.example.com")),
        None,
        Arc::new(NetworkPolicy::allow_all()),
    )
    .expect("dispatch client");
    assert_eq!(block_on(hf.preflight("p")), Ok(()));

    // An undeclared provider is not this layer's to refuse.
    assert_eq!(block_on(bare_dispatcher().preflight("nope")), Ok(()));
}

/// A literal private address is refused by the network policy before any
/// connection: nothing was sent, and the reason is the fixed `Blocked`.
#[test]
fn a_literal_private_host_is_refused_by_the_network_policy() {
    let t = texts();
    let strict = HttpEmbeddingDispatch::new(
        blueprint("voyage", true, None),
        None,
        Arc::new(NetworkPolicy::deny_private()),
    )
    .expect("dispatch client")
    .with_harness_secrets(Arc::new(HarnessSecretBindings::from([(
        SECRET_NAME.into(),
        KEY.into(),
    )])));
    let outcome = block_on(async {
        strict
            .dispatch(request(
                EmbeddingProviderType::Voyage,
                "http://127.0.0.1:9",
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    assert_eq!(
        outcome,
        Err(DispatchFailure::NotSent(NotSentReason::Blocked))
    );
}

/// A host name the resolver refuses under the policy never connects either: the
/// refusal surfaces as a connect error whose chain names the policy, and the
/// dispatch reports only the fixed `Blocked` reason, not the chain.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_resolved_private_host_is_blocked_and_the_policy_detail_stays_out() {
    let t = texts();
    let strict = HttpEmbeddingDispatch::new(
        blueprint("voyage", true, None),
        None,
        Arc::new(NetworkPolicy::deny_private()),
    )
    .expect("dispatch client")
    .with_harness_secrets(Arc::new(HarnessSecretBindings::from([(
        SECRET_NAME.into(),
        KEY.into(),
    )])));
    let outcome = block_on(async {
        strict
            .dispatch(request(
                EmbeddingProviderType::Voyage,
                "http://localhost:9",
                Some(KEY_REF),
                &t,
            ))
            .await
    });
    assert_eq!(
        outcome,
        Err(DispatchFailure::NotSent(NotSentReason::Blocked))
    );
}

/// A current-thread runtime rather than `#[tokio::test]`, as the llm dispatch
/// tests do.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(future),
        Err(err) => panic!("a tokio runtime builds: {err}"),
    }
}

#[test]
fn client_initialization_failure_retains_cause_and_healthy_follow_up() {
    use std::error::Error;

    let bp = blueprint("openai", true, None);
    let policy = Arc::new(NetworkPolicy::allow_all());
    let builder = policy.client_builder().user_agent("invalid\nheader");
    let error = HttpEmbeddingDispatch::with_client_builder(
        Arc::clone(&bp),
        None,
        Arc::clone(&policy),
        builder,
    )
    .err()
    .expect("invalid header must fail client construction");
    assert_eq!(
        error.to_string(),
        "embedding dispatch initialization failed"
    );
    assert!(
        error
            .source()
            .expect("the cause is retained")
            .is::<reqwest::Error>()
    );
    assert!(HttpEmbeddingDispatch::new(bp, None, policy).is_ok());
}
