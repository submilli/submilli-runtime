//! Wire-format tests: pure functions over recorded-shape bodies, no sockets.

use super::*;

const TEXTS: [&str; 2] = ["alpha", "beta"];

fn texts() -> Vec<String> {
    TEXTS.iter().map(ToString::to_string).collect()
}

fn request<'a>(
    kind: EmbeddingProviderType,
    model: &'a str,
    purpose: Purpose,
    texts: &'a [String],
) -> EmbeddingRequest<'a> {
    EmbeddingRequest {
        provider: "p",
        provider_type: kind,
        model,
        base_url: None,
        api_key: None,
        dimensions: 256,
        send_dimensions: true,
        purpose,
        query_prompt_name: None,
        document_prompt_name: None,
        texts,
    }
}

fn build(req: &EmbeddingRequest<'_>) -> WireRequest {
    build_request(req, Some("k")).expect("the request builds")
}

fn has_header(wire: &WireRequest, name: &str, value: &str) -> bool {
    wire.headers.iter().any(|(n, v)| n == name && v == value)
}

// --- request bodies --------------------------------------------------------

#[test]
fn voyage_sends_input_type_dimension_and_no_truncation() {
    let t = texts();
    for (purpose, expected) in [(Purpose::Query, "query"), (Purpose::Document, "document")] {
        let wire = build(&request(
            EmbeddingProviderType::Voyage,
            "voyage-3",
            purpose,
            &t,
        ));
        assert_eq!(wire.url, "https://api.voyageai.com/v1/embeddings");
        assert!(has_header(&wire, "authorization", "Bearer k"));
        assert_eq!(wire.body["input_type"], expected);
        assert_eq!(wire.body["truncation"], false);
        assert_eq!(wire.body["output_dimension"], 256);
        assert_eq!(wire.body["input"], json!(["alpha", "beta"]));
    }
}

#[test]
fn openai_query_and_document_bodies_are_byte_identical() {
    let t = texts();
    let q = build(&request(
        EmbeddingProviderType::OpenAi,
        "text-embedding-3-small",
        Purpose::Query,
        &t,
    ));
    let d = build(&request(
        EmbeddingProviderType::OpenAi,
        "text-embedding-3-small",
        Purpose::Document,
        &t,
    ));
    assert_eq!(q.body.to_string(), d.body.to_string());
    assert_eq!(q.url, "https://api.openai.com/v1/embeddings");
    assert!(q.body.get("truncate").is_none() && q.body.get("truncation").is_none());
    assert!(q.body.get("input_type").is_none() && q.body.get("task").is_none());
}

#[test]
fn dimensions_are_sent_only_when_asked() {
    let t = texts();
    let mut req = request(
        EmbeddingProviderType::OpenAi,
        "text-embedding-ada-002",
        Purpose::Document,
        &t,
    );
    req.send_dimensions = false;
    assert!(build(&req).body.get("dimensions").is_none());

    req.model = "text-embedding-3-small";
    req.send_dimensions = true;
    assert_eq!(build(&req).body["dimensions"], 256);

    for kind in [EmbeddingProviderType::Voyage, EmbeddingProviderType::Jina] {
        let mut req = request(kind, "m", Purpose::Document, &t);
        req.send_dimensions = false;
        let body = build(&req).body.to_string();
        assert!(!body.contains("dimension"), "{kind:?}: {body}");
    }
}

#[test]
fn jina_sends_task_and_no_truncation() {
    let t = texts();
    for (purpose, expected) in [
        (Purpose::Query, "retrieval.query"),
        (Purpose::Document, "retrieval.passage"),
    ] {
        let wire = build(&request(
            EmbeddingProviderType::Jina,
            "jina-v3",
            purpose,
            &t,
        ));
        assert_eq!(wire.url, "https://api.jina.ai/v1/embeddings");
        assert_eq!(wire.body["task"], expected);
        assert_eq!(wire.body["truncate"], false);
        assert_eq!(wire.body["dimensions"], 256);
    }
}

#[test]
fn google_001_sends_task_type_and_one_part_per_text() {
    let t = texts();
    for (purpose, expected) in [
        (Purpose::Query, "RETRIEVAL_QUERY"),
        (Purpose::Document, "RETRIEVAL_DOCUMENT"),
    ] {
        let wire = build(&request(
            EmbeddingProviderType::Google,
            "gemini-embedding-001",
            purpose,
            &t,
        ));
        assert_eq!(
            wire.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:batchEmbedContents"
        );
        assert!(has_header(&wire, "x-goog-api-key", "k"));
        assert!(!wire.url.contains('k') || !wire.url.contains("key="));
        let items = wire.body["requests"].as_array().expect("requests array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["taskType"], expected);
        assert_eq!(items[0]["model"], "models/gemini-embedding-001");
        assert_eq!(items[0]["outputDimensionality"], 256);
        assert_eq!(items[1]["content"]["parts"], json!([{ "text": "beta" }]));
    }
}

#[test]
fn google_embedding_2_uses_text_templates_and_no_task_type() {
    let t = texts();
    let q = build(&request(
        EmbeddingProviderType::Google,
        "gemini-embedding-2",
        Purpose::Query,
        &t,
    ));
    let items = q.body["requests"].as_array().expect("requests array");
    assert_eq!(
        items[0]["content"]["parts"][0]["text"],
        "task: search result | query: alpha"
    );
    assert!(items[0].get("taskType").is_none());

    let d = build(&request(
        EmbeddingProviderType::Google,
        "models/gemini-embedding-2",
        Purpose::Document,
        &t,
    ));
    let items = d.body["requests"].as_array().expect("requests array");
    assert_eq!(
        items[1]["content"]["parts"][0]["text"],
        "title: none | text: beta"
    );
    assert!(
        d.url
            .contains("/models/gemini-embedding-2:batchEmbedContents")
    );
}

#[test]
fn huggingface_prompt_name_only_when_declared() {
    let t = texts();
    let mut req = request(
        EmbeddingProviderType::HuggingFace,
        "org/model",
        Purpose::Query,
        &t,
    );
    assert!(build(&req).body.get("prompt_name").is_none());

    req.query_prompt_name = Some("q");
    req.document_prompt_name = Some("d");
    assert_eq!(build(&req).body["prompt_name"], "q");
    req.purpose = Purpose::Document;
    assert_eq!(build(&req).body["prompt_name"], "d");

    // Declared for one purpose only: the other sends none.
    req.document_prompt_name = None;
    assert!(build(&req).body.get("prompt_name").is_none());
}

#[test]
fn huggingface_routes_and_flags() {
    let t = texts();
    let mut req = request(
        EmbeddingProviderType::HuggingFace,
        "org/model",
        Purpose::Document,
        &t,
    );
    let shared = build(&req);
    assert_eq!(
        shared.url,
        "https://router.huggingface.co/hf-inference/models/org/model/pipeline/feature-extraction"
    );
    assert_eq!(shared.body["truncate"], false);
    assert_eq!(shared.body["inputs"], json!(["alpha", "beta"]));
    assert!(shared.body.get("dimensions").is_none(), "shared has none");

    req.base_url = Some("https://abc.endpoints.huggingface.cloud/");
    let dedicated = build(&req);
    assert_eq!(
        dedicated.url,
        "https://abc.endpoints.huggingface.cloud/embed"
    );
    assert_eq!(dedicated.body["truncate"], false);
    assert_eq!(dedicated.body["dimensions"], 256);
}

#[test]
fn huggingface_dedicated_without_a_key_sends_no_authorization() {
    let t = texts();
    let mut req = request(
        EmbeddingProviderType::HuggingFace,
        "m",
        Purpose::Document,
        &t,
    );
    req.base_url = Some("https://e.example.com");
    let wire = build_request(&req, None).expect("builds");
    assert!(wire.headers.is_empty(), "{:?}", wire.headers);
}

#[test]
fn a_configured_base_url_replaces_the_default_host() {
    let t = texts();
    for (kind, path) in [
        (EmbeddingProviderType::Voyage, "/v1/embeddings"),
        (EmbeddingProviderType::OpenAi, "/v1/embeddings"),
        (EmbeddingProviderType::Jina, "/v1/embeddings"),
        (
            EmbeddingProviderType::Google,
            "/v1beta/models/m:batchEmbedContents",
        ),
    ] {
        let mut req = request(kind, "m", Purpose::Document, &t);
        req.base_url = Some("https://proxy.example.com/");
        assert_eq!(build(&req).url, format!("https://proxy.example.com{path}"));
    }
}

#[test]
fn a_model_that_would_escape_the_route_is_refused() {
    let t = texts();
    for model in ["..", "a/../b", ""] {
        let req = request(
            EmbeddingProviderType::HuggingFace,
            model,
            Purpose::Query,
            &t,
        );
        assert_eq!(
            build_request(&req, None).err(),
            Some(InvalidModel),
            "{model:?}"
        );
    }
    let req = request(EmbeddingProviderType::Google, "a b?c", Purpose::Query, &t);
    let wire = build(&req);
    assert!(
        wire.url.contains("a%20b%3Fc:batchEmbedContents"),
        "{}",
        wire.url
    );
}

#[test]
fn the_key_appears_only_in_headers() {
    let t = texts();
    for kind in EmbeddingProviderType::ALL {
        let req = request(kind, "m", Purpose::Query, &t);
        let wire = build_request(&req, Some("sekrit")).expect("builds");
        assert!(!wire.url.contains("sekrit"), "{kind:?}");
        assert!(!wire.body.to_string().contains("sekrit"), "{kind:?}");
        assert!(
            wire.headers.iter().any(|(_, v)| v.contains("sekrit")),
            "{kind:?}"
        );
    }
}

// --- success bodies --------------------------------------------------------

#[test]
fn voyage_openai_and_jina_parse_rows_index_and_usage() {
    let voyage = r#"{"object":"list","data":[{"object":"embedding","embedding":[0.5,-1.0],"index":1},{"object":"embedding","embedding":[2.0,3.0],"index":0}],"model":"voyage-3","usage":{"total_tokens":12}}"#;
    let openai = r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.5,-1.0]}],"model":"x","usage":{"prompt_tokens":5,"total_tokens":5}}"#;
    let jina = r#"{"data":[{"index":0,"embedding":[1.0,2.0]}],"usage":{"total_tokens":9,"prompt_tokens":0}}"#;

    let v = parse_response(EmbeddingProviderType::Voyage, voyage).expect("voyage parses");
    assert_eq!(v.usage, Some(12));
    assert_eq!(v.rows[0].index, Some(1));
    assert_eq!(v.rows[0].values, vec![0.5, -1.0]);

    let o = parse_response(EmbeddingProviderType::OpenAi, openai).expect("openai parses");
    assert_eq!(o.usage, Some(5));
    assert_eq!(o.rows.len(), 1);

    // Jina's `prompt_tokens: 0` must not mask `total_tokens`.
    let j = parse_response(EmbeddingProviderType::Jina, jina).expect("jina parses");
    assert_eq!(j.usage, Some(9));
}

#[test]
fn a_usage_of_zero_or_absent_is_unreported() {
    let zero = r#"{"data":[{"index":0,"embedding":[1.0]}],"usage":{"total_tokens":0}}"#;
    let none = r#"{"data":[{"index":0,"embedding":[1.0]}]}"#;
    for body in [zero, none] {
        let r = parse_response(EmbeddingProviderType::Voyage, body).expect("parses");
        assert_eq!(r.usage, None);
    }
}

#[test]
fn google_parses_positional_rows_and_treats_zero_usage_as_unreported() {
    let body = r#"{"embeddings":[{"values":[0.1,0.2]},{"values":[0.3,0.4]}],"usageMetadata":{"promptTokenCount":7,"totalTokenCount":7}}"#;
    let r = parse_response(EmbeddingProviderType::Google, body).expect("parses");
    assert_eq!(r.rows.len(), 2);
    assert!(r.rows.iter().all(|row| row.index.is_none()));
    assert_eq!(r.usage, Some(7));

    let zero = r#"{"embeddings":[{"values":[0.1]}],"usageMetadata":{"promptTokenCount":0}}"#;
    assert_eq!(
        parse_response(EmbeddingProviderType::Google, zero)
            .expect("parses")
            .usage,
        None
    );
}

#[test]
fn huggingface_parses_positional_rows_with_no_usage() {
    let r = parse_response(EmbeddingProviderType::HuggingFace, "[[0.1,0.2],[0.3,0.4]]")
        .expect("parses");
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[1].values, vec![0.3, 0.4]);
    assert_eq!(r.usage, None);
    assert!(r.rows.iter().all(|row| row.index.is_none()));
}

#[test]
fn a_token_level_huggingface_response_is_an_invalid_body() {
    let failure = parse_response(
        EmbeddingProviderType::HuggingFace,
        "[[[0.1,0.2],[0.3,0.4]]]",
    )
    .expect_err("3-D is refused");
    assert_eq!(failure, invalid_body());
}

#[test]
fn unparseable_bodies_are_invalid_for_every_provider() {
    for kind in EmbeddingProviderType::ALL {
        for body in ["", "not json", "{}", r#"{"data":[{"embedding":["x"]}]}"#] {
            assert_eq!(
                parse_response(kind, body).expect_err("refused"),
                invalid_body(),
                "{kind:?} {body:?}"
            );
        }
    }
}

#[test]
fn partially_indexed_rows_are_an_invalid_body() {
    let body = r#"{"data":[{"index":0,"embedding":[1.0]},{"embedding":[2.0]}]}"#;
    assert_eq!(
        parse_response(EmbeddingProviderType::OpenAi, body).expect_err("refused"),
        invalid_body()
    );
}

// --- failures --------------------------------------------------------------

fn classify(kind: EmbeddingProviderType, status: u16, body: &str) -> DispatchFailure {
    classify_failure(kind, status, body, Some(7), 4)
}

#[test]
fn auth_rate_limit_server_and_other_statuses_map() {
    for kind in EmbeddingProviderType::ALL {
        for status in [401, 403] {
            assert_eq!(
                classify(kind, status, "{}"),
                DispatchFailure::Rejected(Rejection::Unauthorized)
            );
        }
        assert_eq!(
            classify(kind, 429, "{}"),
            DispatchFailure::Rejected(Rejection::RateLimited {
                retry_after: Some(Duration::from_secs(7))
            })
        );
        assert_eq!(
            classify(kind, 404, "{}"),
            DispatchFailure::Rejected(Rejection::Other)
        );
        assert_eq!(
            classify(kind, 503, "{}"),
            DispatchFailure::Failed {
                kind: SentFailure::ProviderUnavailable,
                usage: None
            }
        );
        assert_eq!(
            classify(kind, 307, ""),
            DispatchFailure::Failed {
                kind: SentFailure::Transport,
                usage: None
            }
        );
    }
}

#[test]
fn recorded_length_errors_map_to_input_too_long() {
    let cases = [
        (
            EmbeddingProviderType::OpenAi,
            400,
            r#"{"error":{"message":"This model's maximum context length is 8192 tokens, however you requested 9000 tokens.","code":"context_length_exceeded"}}"#,
            None,
        ),
        (
            EmbeddingProviderType::Voyage,
            400,
            // Recorded from the live API (voyage-2, truncation: false), 2026-10-05.
            r#"{"detail":"Request to model 'voyage-2' failed. The example at index 0 in your batch has too many tokens and does not fit into the model's context window of 4000 tokens. Please lower the number of tokens in the listed example(s) or use truncation."}"#,
            Some(0),
        ),
        (
            EmbeddingProviderType::Jina,
            400,
            // Recorded from the live API (jina-embeddings-v3, truncate: false), 2026-10-05.
            r#"{"detail":{"message":"Input text exceeds the model's maximum of 8194 tokens. Use 'truncate: true' to automatically truncate, or split into smaller chunks.","request_id":"r","code":"INPUT_TOKEN_LIMIT_EXCEEDED"}}"#,
            None,
        ),
        (
            EmbeddingProviderType::HuggingFace,
            413,
            r#"{"error":"Input validation error: inputs must have less than 512 tokens","error_type":"validation"}"#,
            None,
        ),
        (
            EmbeddingProviderType::HuggingFace,
            422,
            r#"{"error":"input is too long"}"#,
            None,
        ),
        (
            EmbeddingProviderType::Google,
            400,
            r#"{"error":{"code":400,"message":"Unable to submit request because the input token count is 3000 but the model supports up to 2048.","status":"INVALID_ARGUMENT"}}"#,
            None,
        ),
    ];
    for (kind, status, body, index) in cases {
        assert_eq!(
            classify(kind, status, body),
            DispatchFailure::Rejected(Rejection::InputTooLong { index }),
            "{kind:?}"
        );
    }
}

/// A whole-request size or batch-size error is not an input being too long: the
/// fix is sending fewer texts, not shortening one.
#[test]
fn payload_and_batch_size_errors_are_not_input_too_long() {
    let cases = [
        (
            EmbeddingProviderType::Google,
            400,
            r#"{"error":{"code":400,"message":"Request payload size exceeds the limit: 10000 bytes.","status":"INVALID_ARGUMENT"}}"#,
        ),
        (
            EmbeddingProviderType::Voyage,
            400,
            r#"{"detail":"Request to model 'voyage-3' failed. The max allowed tokens per submitted batch is 120000. Your batch has 130000 tokens."}"#,
        ),
        (
            EmbeddingProviderType::Voyage,
            400,
            r#"{"detail":"The number of inputs exceeds the maximum of 1000 per request."}"#,
        ),
        (
            EmbeddingProviderType::Jina,
            400,
            r#"{"detail":"Batch size exceeds the limit"}"#,
        ),
        (
            EmbeddingProviderType::HuggingFace,
            422,
            r#"{"error":"batch size 64 > maximum allowed batch size 32"}"#,
        ),
        (
            EmbeddingProviderType::Google,
            400,
            r#"{"error":{"message":"The request exceeds the allowed number of requests"}}"#,
        ),
    ];
    for (kind, status, body) in cases {
        assert_eq!(
            classify(kind, status, body),
            DispatchFailure::Rejected(Rejection::Other),
            "{kind:?}: {body}"
        );
    }
}

#[test]
fn a_named_input_index_is_recovered_only_when_in_range() {
    let named = r#"{"error":{"message":"requests[2]: exceeds the maximum number of tokens"}}"#;
    assert_eq!(
        classify(EmbeddingProviderType::Google, 400, named),
        DispatchFailure::Rejected(Rejection::InputTooLong { index: Some(2) })
    );
    let out_of_range =
        r#"{"error":{"message":"requests[9]: exceeds the maximum number of tokens"}}"#;
    assert_eq!(
        classify(EmbeddingProviderType::Google, 400, out_of_range),
        DispatchFailure::Rejected(Rejection::InputTooLong { index: None })
    );
    let inputs = r#"too long: inputs[1]"#;
    assert_eq!(
        classify(EmbeddingProviderType::HuggingFace, 413, inputs),
        DispatchFailure::Rejected(Rejection::InputTooLong { index: Some(1) })
    );
}

#[test]
fn an_unrelated_400_is_other_and_a_length_phrase_on_a_500_is_not_a_length_error() {
    assert_eq!(
        classify(
            EmbeddingProviderType::OpenAi,
            400,
            r#"{"error":"bad model"}"#
        ),
        DispatchFailure::Rejected(Rejection::Other)
    );
    assert_eq!(
        classify(EmbeddingProviderType::OpenAi, 500, "maximum context length"),
        DispatchFailure::Failed {
            kind: SentFailure::ProviderUnavailable,
            usage: None
        }
    );
}

#[test]
fn google_invalid_key_is_a_400_that_means_unauthorized() {
    let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
    assert_eq!(
        classify(EmbeddingProviderType::Google, 400, body),
        DispatchFailure::Rejected(Rejection::Unauthorized)
    );
}

#[test]
fn failures_never_carry_body_text() {
    let secret = "private-input-text-echo";
    let body = format!(r#"{{"error":"too long: {secret}"}}"#);
    let rendered = format!(
        "{:?}",
        classify(EmbeddingProviderType::HuggingFace, 400, &body)
    );
    assert!(!rendered.contains(secret), "{rendered}");
}

#[test]
fn voyage_context_window_errors_are_input_too_long_with_the_prose_index() {
    let body = r#"{"detail":"The example at index 0 in your batch has too many tokens and does not fit into the model's context window of 32000 tokens."}"#;
    assert_eq!(
        classify(EmbeddingProviderType::Voyage, 400, body),
        DispatchFailure::Rejected(Rejection::InputTooLong { index: Some(0) })
    );
    let no_index = r#"{"detail":"Input exceeds the context length."}"#;
    assert_eq!(
        classify(EmbeddingProviderType::Voyage, 400, no_index),
        DispatchFailure::Rejected(Rejection::InputTooLong { index: None })
    );
}
