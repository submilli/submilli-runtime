//! Wire-format tests. **Zero live API calls** — every assertion is against a
//! serialized request or a literal response body.
//!
//! **Every per-kind assertion runs for every kind.** The tests are written as
//! tables over [`ProviderKind::ALL`] rather than as one test per kind, because a
//! suite that checks the endpoint on one kind and calls it "endpoints are
//! right" is exactly the vacuity this file exists to avoid: narrowing the
//! implementation to a single kind has to break something.

use serde_json::{Value, json};

use super::*;

impl ProviderKind {
    /// Every shipping kind. The tables below iterate this, so a kind added later
    /// is one every per-kind assertion immediately covers rather than one they
    /// silently skip.
    const ALL: [ProviderKind; 4] = [
        ProviderKind::Anthropic,
        ProviderKind::Google,
        ProviderKind::OpenAi,
        ProviderKind::OpenAiCompatible,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::OpenAi => "openai",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

const BASE: &str = "https://example.test";
const MODEL: &str = "m-1";
const PROMPT: &str = "summarize the incident";
const KEY: &str = "sk-secret-value-do-not-leak";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": { "severity": { "type": "string" } },
        "required": ["severity"],
    })
}

fn request(kind: ProviderKind, schema_json: Option<&Value>) -> WireRequest {
    build_request(kind, BASE, MODEL, PROMPT, schema_json, Some(256), Some(KEY))
}

fn header<'a>(req: &'a WireRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

// --- endpoints -------------------------------------------------------------

/// Every kind's path, asserted per kind. Narrowing the implementation to one
/// kind's path fails three of these four.
#[test]
fn each_kind_sends_its_own_endpoint() {
    let expected = [
        (ProviderKind::Anthropic, "https://example.test/v1/messages"),
        (
            ProviderKind::Google,
            "https://example.test/v1beta/models/m-1:generateContent",
        ),
        (
            ProviderKind::OpenAi,
            "https://example.test/v1/chat/completions",
        ),
        (
            ProviderKind::OpenAiCompatible,
            "https://example.test/v1/chat/completions",
        ),
    ];
    for (kind, url) in expected {
        assert_eq!(request(kind, None).url, url, "{}", kind.label());
    }
}

/// The two OpenAI-shaped kinds share a path, and that is a deliberate property
/// rather than a coincidence — asserted so a change that gives one of them its
/// own endpoint is a test failure rather than a surprise.
#[test]
fn openai_and_openai_compatible_share_a_path() {
    assert_eq!(
        request(ProviderKind::OpenAi, None).url,
        request(ProviderKind::OpenAiCompatible, None).url,
    );
}

#[test]
fn a_trailing_slash_on_the_base_url_does_not_double() {
    let req = build_request(
        ProviderKind::OpenAi,
        "https://example.test/",
        MODEL,
        PROMPT,
        None,
        None,
        Some(KEY),
    );
    assert_eq!(req.url, "https://example.test/v1/chat/completions");
}

/// Google puts the model in the path and both spellings are in circulation, so
/// an already-prefixed name must not be prefixed twice.
#[test]
fn google_does_not_double_the_models_prefix() {
    let req = build_request(
        ProviderKind::Google,
        BASE,
        "models/gemini-2.5-pro",
        PROMPT,
        None,
        None,
        Some(KEY),
    );
    assert_eq!(
        req.url,
        "https://example.test/v1beta/models/gemini-2.5-pro:generateContent",
    );
}

#[test]
fn first_party_kinds_have_a_default_endpoint_and_openai_compatible_does_not() {
    assert_eq!(
        ProviderKind::Anthropic.default_base_url(),
        Some("https://api.anthropic.com"),
    );
    assert_eq!(
        ProviderKind::Google.default_base_url(),
        Some("https://generativelanguage.googleapis.com"),
    );
    assert_eq!(
        ProviderKind::OpenAi.default_base_url(),
        Some("https://api.openai.com"),
    );
    assert_eq!(ProviderKind::OpenAiCompatible.default_base_url(), None);
}

// --- auth ------------------------------------------------------------------

/// The per-kind auth header, asserted per kind. Anthropic's `x-api-key` is the
/// one that an `Authorization: Bearer` habit gets wrong, and Google's
/// `x-goog-api-key` is the second — so the table checks the exact header name
/// *and* that the wrong one is absent.
#[test]
fn each_kind_carries_its_own_auth_header() {
    let cases = [
        (ProviderKind::Anthropic, "x-api-key", KEY.to_string()),
        (ProviderKind::Google, "x-goog-api-key", KEY.to_string()),
        (
            ProviderKind::OpenAi,
            "authorization",
            format!("Bearer {KEY}"),
        ),
        (
            ProviderKind::OpenAiCompatible,
            "authorization",
            format!("Bearer {KEY}"),
        ),
    ];
    for (kind, name, value) in cases {
        let req = request(kind, None);
        assert_eq!(header(&req, name), Some(value.as_str()), "{}", kind.label());

        // And the headers the other kinds use are not also present, so a
        // shotgun implementation that sets all three would fail here.
        for other in ["x-api-key", "x-goog-api-key", "authorization"] {
            if other != name {
                assert_eq!(header(&req, other), None, "{} sent {other}", kind.label());
            }
        }
    }
}

#[test]
fn anthropic_pins_the_api_version_header() {
    let req = request(ProviderKind::Anthropic, None);
    assert_eq!(header(&req, "anthropic-version"), Some("2023-06-01"));
}

/// The key goes in a header and nowhere else. A URL is the single most
/// leak-prone field on the request — Google documents a `?key=` form that this
/// implementation refuses for precisely that reason.
#[test]
fn the_key_never_appears_in_the_url_or_the_body() {
    for kind in ProviderKind::ALL {
        let req = request(kind, Some(&schema()));
        assert!(
            !req.url.contains(KEY),
            "{} put the key in the url: {}",
            kind.label(),
            req.url,
        );
        assert!(
            !req.body.to_string().contains(KEY),
            "{} put the key in the body",
            kind.label(),
        );
    }
}

/// A row that declares no key sends no auth header at all, rather than sending
/// an empty or literal-placeholder one. A local endpoint may legitimately need
/// none.
#[test]
fn no_key_means_no_auth_header() {
    for kind in ProviderKind::ALL {
        let req = build_request(kind, BASE, MODEL, PROMPT, None, None, None);
        for name in ["x-api-key", "x-goog-api-key", "authorization"] {
            assert_eq!(header(&req, name), None, "{}", kind.label());
        }
    }
}

// --- request bodies --------------------------------------------------------

/// Each kind's body shape and where the prompt sits in it, per kind.
#[test]
fn each_kind_puts_the_prompt_where_its_api_expects_it() {
    let cases = [
        (ProviderKind::Anthropic, "/messages/0/content"),
        (ProviderKind::Google, "/contents/0/parts/0/text"),
        (ProviderKind::OpenAi, "/messages/0/content"),
        (ProviderKind::OpenAiCompatible, "/messages/0/content"),
    ];
    for (kind, pointer) in cases {
        let body = request(kind, None).body;
        assert_eq!(
            body.pointer(pointer).and_then(Value::as_str),
            Some(PROMPT),
            "{} body was {body}",
            kind.label(),
        );
    }
}

/// The model name travels in the body for three kinds and in the path for
/// Google — asserted so a refactor that "unifies" them breaks a test.
#[test]
fn the_model_reaches_each_kind_the_way_its_api_names_models() {
    for kind in ProviderKind::ALL {
        let req = request(kind, None);
        match kind {
            ProviderKind::Google => {
                assert!(req.url.contains(MODEL), "{}", kind.label());
                assert_eq!(req.body.get("model"), None, "{}", kind.label());
            }
            _ => assert_eq!(
                req.body.get("model").and_then(Value::as_str),
                Some(MODEL),
                "{}",
                kind.label(),
            ),
        }
    }
}

/// The output cap is the field that makes the budget reservation an upper bound
/// rather than an estimate, so each kind has to spell it the way its API does.
#[test]
fn each_kind_spells_the_output_cap_its_own_way() {
    let cases = [
        (ProviderKind::Anthropic, "/max_tokens"),
        (ProviderKind::Google, "/generationConfig/maxOutputTokens"),
        (ProviderKind::OpenAi, "/max_completion_tokens"),
        // Not `max_completion_tokens`: most compatible servers predate that
        // rename and ignore it, which drops the cap silently.
        (ProviderKind::OpenAiCompatible, "/max_tokens"),
    ];
    for (kind, pointer) in cases {
        let body = request(kind, None).body;
        assert_eq!(
            body.pointer(pointer).and_then(Value::as_u64),
            Some(256),
            "{} body was {body}",
            kind.label(),
        );
    }
}

/// The two OpenAI-shaped kinds share a body *except* for the cap's spelling.
/// Asserted as a difference so a refactor that merges them — the natural thing
/// to do, since every other field matches — reintroduces the silent cap drop and
/// fails here.
#[test]
fn the_openai_kinds_spell_the_cap_differently_from_each_other() {
    let first_party = request(ProviderKind::OpenAi, None).body;
    let compatible = request(ProviderKind::OpenAiCompatible, None).body;

    assert_eq!(first_party.get("max_tokens"), None);
    assert_eq!(compatible.get("max_completion_tokens"), None);
    assert_eq!(
        first_party
            .get("max_completion_tokens")
            .and_then(Value::as_u64),
        Some(256),
    );
    assert_eq!(
        compatible.get("max_tokens").and_then(Value::as_u64),
        Some(256)
    );
}

/// Anthropic requires `max_tokens` on every request, so an absent cap still has
/// to produce one — and the other kinds must *not* invent one, because an
/// invented cap silently truncates.
#[test]
fn anthropic_always_sends_max_tokens_and_the_others_omit_an_absent_cap() {
    let anthropic = build_request(
        ProviderKind::Anthropic,
        BASE,
        MODEL,
        PROMPT,
        None,
        None,
        Some(KEY),
    );
    assert_eq!(
        anthropic
            .body
            .pointer("/max_tokens")
            .and_then(Value::as_u64),
        Some(4096),
    );

    for kind in [
        ProviderKind::OpenAi,
        ProviderKind::OpenAiCompatible,
        ProviderKind::Google,
    ] {
        let body = build_request(kind, BASE, MODEL, PROMPT, None, None, Some(KEY)).body;
        for absent in ["max_completion_tokens", "max_tokens"] {
            assert_eq!(body.get(absent), None, "{} invented {absent}", kind.label());
        }
        assert_eq!(
            body.pointer("/generationConfig/maxOutputTokens"),
            None,
            "{}",
            kind.label(),
        );
    }
}

// --- schema attachment -----------------------------------------------------

/// The unit's whole point, asserted per kind on the serialized body: each kind
/// receives the *schema itself*, in its own envelope, at its own path.
#[test]
fn each_kind_attaches_the_schema_in_its_own_envelope() {
    let cases = [
        (ProviderKind::Anthropic, "/output_config/format/schema"),
        (ProviderKind::Google, "/generationConfig/responseSchema"),
        (ProviderKind::OpenAi, "/response_format/json_schema/schema"),
        (
            ProviderKind::OpenAiCompatible,
            "/response_format/json_schema/schema",
        ),
    ];
    for (kind, pointer) in cases {
        let body = request(kind, Some(&schema())).body;
        assert_eq!(
            body.pointer(pointer),
            Some(&schema()),
            "{} did not carry the schema at {pointer}; body was {body}",
            kind.label(),
        );
    }
}

/// The exact defect this unit exists to not reproduce.
///
/// `{"type":"json_object"}` is accepted by every OpenAI-shaped endpoint and asks
/// the model only for *some* JSON: the schema vanishes with no error and the
/// response parses. Asserting the envelope's `type` positively and the
/// `json_object` spelling negatively means a regression to the broken form
/// fails here rather than in production.
#[test]
fn openai_shaped_kinds_send_json_schema_not_json_object() {
    for kind in [ProviderKind::OpenAi, ProviderKind::OpenAiCompatible] {
        let body = request(kind, Some(&schema())).body;
        assert_eq!(
            body.pointer("/response_format/type")
                .and_then(Value::as_str),
            Some("json_schema"),
            "{}",
            kind.label(),
        );
        assert!(
            !body.to_string().contains("json_object"),
            "{} sent the shapeless json_object form: {body}",
            kind.label(),
        );
        // The envelope requires a name; without it the request is a 400.
        assert_eq!(
            body.pointer("/response_format/json_schema/name")
                .and_then(Value::as_str),
            Some("submilli_response"),
            "{}",
            kind.label(),
        );
    }
}

/// Google's analogue of the same trap: the MIME type alone asks for shapeless
/// JSON, so it is never set without the schema beside it.
#[test]
fn google_sets_the_json_mime_type_only_together_with_the_schema() {
    let typed = request(ProviderKind::Google, Some(&schema())).body;
    assert_eq!(
        typed
            .pointer("/generationConfig/responseMimeType")
            .and_then(Value::as_str),
        Some("application/json"),
    );

    let untyped = request(ProviderKind::Google, None).body;
    assert_eq!(
        untyped.pointer("/generationConfig/responseMimeType"),
        None,
        "an untyped call asked for JSON without a shape",
    );
}

/// Anthropic's native envelope declares its own `type`, and the schema hangs off
/// `output_config.format` rather than the transitional top-level `output_format`
/// or the older forced-tool workaround.
#[test]
fn anthropic_uses_the_native_json_schema_envelope() {
    let body = request(ProviderKind::Anthropic, Some(&schema())).body;
    assert_eq!(
        body.pointer("/output_config/format/type")
            .and_then(Value::as_str),
        Some("json_schema"),
    );
    // The superseded spellings are not also sent: a request carrying both the
    // native field and a forced tool asks for the shape twice and can be
    // answered either way.
    let rendered = body.to_string();
    for superseded in ["tool_choice", "input_schema"] {
        assert!(
            !rendered.contains(superseded),
            "sent the superseded {superseded} form: {rendered}",
        );
    }
}

/// An untyped call carries no schema machinery at all on any kind — so a typed
/// assertion above cannot pass merely because the field is always present.
#[test]
fn an_untyped_call_attaches_nothing_on_any_kind() {
    for kind in ProviderKind::ALL {
        let body = request(kind, None).body;
        let rendered = body.to_string();
        for absent in [
            "response_format",
            "responseSchema",
            "responseMimeType",
            "output_config",
            "input_schema",
            "tool_choice",
            "tools",
        ] {
            assert!(
                !rendered.contains(absent),
                "{} attached {absent} on an untyped call: {rendered}",
                kind.label(),
            );
        }
    }
}

// --- response parsing ------------------------------------------------------

fn ok_body(kind: ProviderKind) -> String {
    let body = match kind {
        ProviderKind::Anthropic => json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "hello" }],
            "usage": { "input_tokens": 11, "output_tokens": 5 },
        }),
        ProviderKind::Google => json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": { "parts": [{ "text": "hello" }] },
            }],
            "usageMetadata": { "promptTokenCount": 11, "candidatesTokenCount": 5 },
        }),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => json!({
            "choices": [{ "finish_reason": "stop", "message": { "content": "hello" } }],
            "usage": { "prompt_tokens": 11, "completion_tokens": 5 },
        }),
    };
    body.to_string()
}

#[test]
fn each_kind_reads_text_usage_and_a_natural_stop() {
    for kind in ProviderKind::ALL {
        let response = parse_response(kind, &ok_body(kind))
            .unwrap_or_else(|_| panic!("{} did not parse", kind.label()));
        assert_eq!(response.stop_reason, StopReason::Stop, "{}", kind.label());
        assert_eq!(response.text.as_deref(), Some("hello"), "{}", kind.label());
        assert_eq!(
            response.usage,
            ProviderUsage::reported(11.0, 5.0),
            "{}",
            kind.label(),
        );
    }
}

/// The structured-output contract: the model's JSON has to arrive as
/// [`ProviderResponse::text`], because that is the field the typed path parses.
/// All four kinds deliver it as a JSON *string* in their text field.
#[test]
fn a_structured_response_arrives_as_parseable_text_on_every_kind() {
    let object = json!({ "severity": "high" });
    let bodies = [
        (
            ProviderKind::Anthropic,
            json!({
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "{\"severity\":\"high\"}" }],
            }),
        ),
        (
            ProviderKind::Google,
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": { "parts": [{ "text": "{\"severity\":\"high\"}" }] },
                }],
            }),
        ),
        (
            ProviderKind::OpenAi,
            json!({
                "choices": [{
                    "finish_reason": "stop",
                    "message": { "content": "{\"severity\":\"high\"}" },
                }],
            }),
        ),
        (
            ProviderKind::OpenAiCompatible,
            json!({
                "choices": [{
                    "finish_reason": "stop",
                    "message": { "content": "{\"severity\":\"high\"}" },
                }],
            }),
        ),
    ];
    for (kind, body) in bodies {
        let response = parse_response(kind, &body.to_string())
            .unwrap_or_else(|_| panic!("{} did not parse", kind.label()));
        let text = response
            .text
            .unwrap_or_else(|| panic!("{} produced no text", kind.label()));
        let parsed: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} text was not JSON: {text} ({e})", kind.label()));
        assert_eq!(parsed, object, "{}", kind.label());
    }
}

/// A tool call is the *successful* end of a transitional-form typed request.
/// Reading `tool_use` as anything but a natural stop would report every one of
/// those calls as a failure, and the object has to be re-serialized out of
/// `input` because the typed path parses text.
#[test]
fn anthropics_transitional_tool_form_still_parses() {
    let body = json!({
        "stop_reason": "tool_use",
        "content": [{ "type": "tool_use", "input": { "a": 1 } }],
    });
    let response = parse_response(ProviderKind::Anthropic, &body.to_string()).unwrap();
    assert_eq!(response.stop_reason, StopReason::Stop);
    assert_eq!(response.text.as_deref(), Some(r#"{"a":1}"#));
}

/// A thinking model emits a `thinking` block before the answer. Selecting text
/// blocks by `type` rather than by index is what keeps the answer the answer —
/// a parser reading `content[0]` returns the reasoning trace instead.
#[test]
fn anthropic_skips_a_thinking_block_and_reads_the_answer() {
    let body = json!({
        "stop_reason": "end_turn",
        "content": [
            { "type": "thinking", "thinking": "internal reasoning the guest must not get" },
            { "type": "text", "text": "the answer" },
        ],
    });
    let response = parse_response(ProviderKind::Anthropic, &body.to_string()).unwrap();
    assert_eq!(response.text.as_deref(), Some("the answer"));
}

/// Anthropic returns a safety refusal as an ordinary **HTTP 200** — nothing
/// throws, and the only thing distinguishing it from a real answer is the stop
/// reason. This is KTD1's failure mode arriving by the route the provider makes
/// easiest, so it gets its own test rather than only a table row.
#[test]
fn an_anthropic_refusal_arrives_as_a_successful_response_and_is_still_not_ok() {
    let body = json!({ "stop_reason": "refusal", "content": [] });
    let response = parse_response(ProviderKind::Anthropic, &body.to_string()).unwrap();
    assert_eq!(response.stop_reason, StopReason::ContentFilter);
}

/// Running into the context window is a *stop reason* on Anthropic, not an HTTP
/// error — a completion cut short, so it classifies with truncation.
#[test]
fn anthropics_context_window_stop_reason_is_truncation() {
    let body = json!({
        "stop_reason": "model_context_window_exceeded",
        "content": [{ "type": "text", "text": "partial" }],
    });
    let response = parse_response(ProviderKind::Anthropic, &body.to_string()).unwrap();
    assert_eq!(response.stop_reason, StopReason::Length);
    assert_eq!(response.text.as_deref(), Some("partial"));
}

/// Google blocks a *prompt* before generation: no candidates at all, and the
/// reason lives in `promptFeedback.blockReason` instead of a finish reason. Left
/// to the ordinary path this reads as a body with no stop reason and classifies
/// as `invalid-output` — telling a guest its schema was wrong when in fact its
/// request was filtered.
#[test]
fn a_google_blocked_prompt_is_content_filtered_not_unreadable() {
    let body = json!({
        "promptFeedback": { "blockReason": "SAFETY" },
        "usageMetadata": { "promptTokenCount": 9 },
    });
    let response = parse_response(ProviderKind::Google, &body.to_string())
        .expect("a blocked prompt is a readable response");
    assert_eq!(response.stop_reason, StopReason::ContentFilter);
    assert_eq!(response.text, None);
    // The prompt was still read, and was still billed for.
    assert_eq!(response.usage.input_tokens, Some(9.0));
}

// --- stop reasons ----------------------------------------------------------

/// Every kind's full stop-reason vocabulary, mapped. The `Other` arms prove the
/// closed set is not silently widened, and that an unrecognized spelling
/// survives verbatim for diagnosis.
#[test]
fn every_stop_reason_maps_to_the_right_arm() {
    let cases: [(ProviderKind, &str, StopReason); 18] = [
        (ProviderKind::Anthropic, "end_turn", StopReason::Stop),
        (ProviderKind::Anthropic, "stop_sequence", StopReason::Stop),
        (ProviderKind::Anthropic, "tool_use", StopReason::Stop),
        (ProviderKind::Anthropic, "max_tokens", StopReason::Length),
        (
            ProviderKind::Anthropic,
            "model_context_window_exceeded",
            StopReason::Length,
        ),
        (
            ProviderKind::Anthropic,
            "refusal",
            StopReason::ContentFilter,
        ),
        (
            ProviderKind::Anthropic,
            "pause_turn",
            StopReason::Other("pause_turn".to_string()),
        ),
        (ProviderKind::Google, "STOP", StopReason::Stop),
        (ProviderKind::Google, "MAX_TOKENS", StopReason::Length),
        (ProviderKind::Google, "SAFETY", StopReason::ContentFilter),
        (
            ProviderKind::Google,
            "RECITATION",
            StopReason::ContentFilter,
        ),
        (
            ProviderKind::Google,
            "PROHIBITED_CONTENT",
            StopReason::ContentFilter,
        ),
        (ProviderKind::Google, "OTHER", StopReason::Error),
        (
            ProviderKind::Google,
            "MALFORMED_FUNCTION_CALL",
            StopReason::Other("MALFORMED_FUNCTION_CALL".to_string()),
        ),
        (ProviderKind::OpenAi, "stop", StopReason::Stop),
        (ProviderKind::OpenAi, "length", StopReason::Length),
        (
            ProviderKind::OpenAi,
            "content_filter",
            StopReason::ContentFilter,
        ),
        (
            ProviderKind::OpenAiCompatible,
            "tool_calls",
            StopReason::Other("tool_calls".to_string()),
        ),
    ];

    for (kind, raw, expected) in cases {
        let body = stop_reason_body(kind, raw);
        let response = parse_response(kind, &body)
            .unwrap_or_else(|_| panic!("{} / {raw} did not parse", kind.label()));
        assert_eq!(response.stop_reason, expected, "{} / {raw}", kind.label(),);
    }
}

fn stop_reason_body(kind: ProviderKind, raw: &str) -> String {
    match kind {
        ProviderKind::Anthropic => json!({
            "stop_reason": raw,
            "content": [{ "type": "text", "text": "partial" }],
        }),
        ProviderKind::Google => json!({
            "candidates": [{
                "finishReason": raw,
                "content": { "parts": [{ "text": "partial" }] },
            }],
        }),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => json!({
            "choices": [{ "finish_reason": raw, "message": { "content": "partial" } }],
        }),
    }
    .to_string()
}

/// OpenAI reports a refusal as a `refusal` field with `content: null` and a
/// `stop` finish reason — so reading the finish reason first would report a
/// refused request as a clean, empty success. KTD1's whole point, in the one
/// place the provider actively disguises it.
#[test]
fn an_openai_refusal_outranks_its_stop_finish_reason() {
    let body = json!({
        "choices": [{
            "finish_reason": "stop",
            "message": { "content": null, "refusal": "I can't help with that." },
        }],
    });
    for kind in [ProviderKind::OpenAi, ProviderKind::OpenAiCompatible] {
        let response = parse_response(kind, &body.to_string()).unwrap();
        assert_eq!(
            response.stop_reason,
            StopReason::ContentFilter,
            "{}",
            kind.label(),
        );
    }
}

/// A truncated answer keeps its partial text, on every kind — KTD1 accepts
/// marking truncation `ok: false` only because the text survives on the failure
/// arm.
#[test]
fn a_truncated_answer_keeps_its_partial_text() {
    let cases = [
        (ProviderKind::Anthropic, "max_tokens"),
        (ProviderKind::Google, "MAX_TOKENS"),
        (ProviderKind::OpenAi, "length"),
        (ProviderKind::OpenAiCompatible, "length"),
    ];
    for (kind, raw) in cases {
        let response = parse_response(kind, &stop_reason_body(kind, raw)).unwrap();
        assert_eq!(response.stop_reason, StopReason::Length, "{}", kind.label());
        assert_eq!(
            response.text.as_deref(),
            Some("partial"),
            "{} discarded the partial text",
            kind.label(),
        );
    }
}

// --- usage (KTD3) ----------------------------------------------------------

/// Absent usage is `None`, on every kind. Never `Some(0)`: indeterminate is not
/// free, and a zero would tell the reconciler a call cost nothing.
#[test]
fn absent_usage_is_none_never_zero() {
    for kind in ProviderKind::ALL {
        let response = parse_response(kind, &stop_reason_body(kind, natural_stop(kind))).unwrap();
        assert_eq!(response.usage.input_tokens, None, "{}", kind.label());
        assert_eq!(response.usage.output_tokens, None, "{}", kind.label());
        let (input, output) = resolved(&response.usage);
        assert_eq!(input, None, "{}", kind.label());
        assert_eq!(output, None, "{}", kind.label());
    }
}

/// Separately-billed token fields are counted, not dropped.
///
/// Anthropic bills cache reads and cache writes *in addition* to
/// `input_tokens`, and Google bills `thoughtsTokenCount` in addition to
/// `candidatesTokenCount` — neither parent field includes the extra. Charging
/// only the parent hands the budget a fraction of real spend, which is the same
/// "spend the ceiling cannot see" failure as forgiving an unreported count,
/// arriving through the reported path instead of the null one.
#[test]
fn separately_billed_token_fields_are_added_to_the_reported_counts() {
    let anthropic = parse_response(
        ProviderKind::Anthropic,
        &json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "x" }],
            "usage": {
                "input_tokens": 10,
                "cache_creation_input_tokens": 300,
                "cache_read_input_tokens": 700,
                "output_tokens": 5,
            },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        anthropic.usage.input_tokens,
        Some(1010.0),
        "cached input is billed on top of input_tokens, so it has to be charged"
    );
    assert_eq!(anthropic.usage.output_tokens, Some(5.0));

    let google = parse_response(
        ProviderKind::Google,
        &json!({
            "candidates": [{ "finishReason": "STOP", "content": { "parts": [{ "text": "x" }] } }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 100,
                "thoughtsTokenCount": 5000,
            },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        google.usage.output_tokens,
        Some(5100.0),
        "a thinking model's reasoning tokens are the expensive half and are billed separately"
    );
    assert_eq!(google.usage.input_tokens, Some(10.0));
}

/// A bad field does not decide what its siblings report.
///
/// `finite_count` rejects non-finite and negative counts, but it runs on the
/// *total*, one layer after these are summed. So a `NaN` in one field would
/// poison an otherwise good sum into `None`, and a negative one would silently
/// subtract from a real count — undercharging the budget in exactly the
/// direction that summing these fields exists to prevent.
#[test]
fn a_non_finite_or_negative_field_is_skipped_rather_than_poisoning_the_sum() {
    let with_nan = parse_response(
        ProviderKind::Anthropic,
        &json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "x" }],
            // A provider cannot literally send NaN in JSON, but it can send a
            // value that parses to one, and `null` and non-numeric already
            // reach here as `None`. This pins the arithmetic, not the wire.
            "usage": { "input_tokens": 10, "cache_read_input_tokens": 700, "output_tokens": 5 },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(with_nan.usage.input_tokens, Some(710.0));

    let with_negative = parse_response(
        ProviderKind::Anthropic,
        &json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "x" }],
            "usage": { "input_tokens": -5, "cache_read_input_tokens": 700, "output_tokens": 5 },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        with_negative.usage.input_tokens,
        Some(700.0),
        "a negative field is skipped, not subtracted from a real sibling"
    );
}

/// Summing the extra fields must not manufacture a zero out of an absent one.
///
/// `None` and `Some(0)` mean different things to the reconciler — nothing
/// measured versus measured nothing — so a response carrying only the parent
/// field reports exactly the parent, and one carrying none of them stays `None`
/// and keeps its held reserve.
#[test]
fn summing_extra_usage_fields_preserves_the_absent_case() {
    let only_parent = parse_response(
        ProviderKind::Anthropic,
        &json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "x" }],
            "usage": { "input_tokens": 10, "output_tokens": 5 },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(only_parent.usage.input_tokens, Some(10.0));

    let only_cache = parse_response(
        ProviderKind::Anthropic,
        &json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "x" }],
            "usage": { "cache_read_input_tokens": 700, "output_tokens": 5 },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        only_cache.usage.input_tokens,
        Some(700.0),
        "a fully-cached prompt reports no input_tokens, and the cache read is still real spend"
    );

    let google_no_thoughts = parse_response(
        ProviderKind::Google,
        &json!({
            "candidates": [{ "finishReason": "STOP", "content": { "parts": [{ "text": "x" }] } }],
            "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 100 },
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(google_no_thoughts.usage.output_tokens, Some(100.0));
}

/// A usage object present but with null or non-numeric counts is equally
/// indeterminate — the half-reported case, which a parser reading `as_u64()`
/// with `unwrap_or(0)` would turn into a free call.
#[test]
fn null_and_non_numeric_usage_counts_are_none() {
    let bodies = [
        (
            ProviderKind::Anthropic,
            json!({
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "x" }],
                "usage": { "input_tokens": null, "output_tokens": "many" },
            }),
        ),
        (
            ProviderKind::Google,
            json!({
                "candidates": [{ "finishReason": "STOP", "content": { "parts": [{ "text": "x" }] } }],
                "usageMetadata": { "promptTokenCount": null, "candidatesTokenCount": "many" },
            }),
        ),
        (
            ProviderKind::OpenAi,
            json!({
                "choices": [{ "finish_reason": "stop", "message": { "content": "x" } }],
                "usage": { "prompt_tokens": null, "completion_tokens": "many" },
            }),
        ),
    ];
    for (kind, body) in bodies {
        let response = parse_response(kind, &body.to_string()).unwrap();
        assert_eq!(response.usage.input_tokens, None, "{}", kind.label());
        assert_eq!(response.usage.output_tokens, None, "{}", kind.label());
    }
}

/// A genuinely reported zero stays zero. The rule is that *absent* is not zero,
/// not that zero is impossible — collapsing a real zero to `None` would make the
/// reconciler hold a conservative reserve for a call that truly cost nothing.
#[test]
fn a_reported_zero_is_kept_as_zero() {
    let body = json!({
        "choices": [{ "finish_reason": "stop", "message": { "content": "" } }],
        "usage": { "prompt_tokens": 0, "completion_tokens": 0 },
    });
    let response = parse_response(ProviderKind::OpenAi, &body.to_string()).unwrap();
    assert_eq!(resolved(&response.usage), (Some(0), Some(0)));
}

/// A non-finite count reaches `None`, never `0` — the guard the `f64` shape
/// exists for.
#[test]
fn a_non_finite_usage_count_resolves_to_none() {
    let usage = ProviderUsage {
        input_tokens: Some(f64::NAN),
        output_tokens: Some(f64::INFINITY),
    };
    assert_eq!(resolved(&usage), (None, None));
}

fn natural_stop(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "end_turn",
        ProviderKind::Google => "STOP",
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => "stop",
    }
}

/// `ProviderUsage::resolved` is private to the provider module; this reproduces
/// the one property under test here (finite-or-`None`) so the KTD3 assertions
/// read against the same rule the ladder applies.
fn resolved(usage: &ProviderUsage) -> (Option<u64>, Option<u64>) {
    fn one(value: Option<f64>) -> Option<u64> {
        let value = value?;
        (value.is_finite() && value >= 0.0 && value <= u64::MAX as f64).then_some(value as u64)
    }
    (one(usage.input_tokens), one(usage.output_tokens))
}

// --- unreadable success bodies ---------------------------------------------

/// A 200 this runtime cannot read is not an empty success. Reporting it as
/// `Ok(text: None)` would make a garbled body indistinguishable from a model
/// that chose to say nothing — the one ambiguity KTD1 calls truthful, which it
/// would then stop being.
#[test]
fn an_unreadable_success_body_is_not_a_silent_empty_success() {
    for kind in ProviderKind::ALL {
        for body in ["not json at all", "{}", r#"{"unexpected":true}"#] {
            let outcome = parse_response(kind, body);
            assert!(
                matches!(outcome, Err(ProviderFailure::NoObjectGenerated { .. })),
                "{} reported {body} as {outcome:?}",
                kind.label(),
            );
        }
    }
}

// --- failures --------------------------------------------------------------

#[test]
fn a_failure_carries_the_status_body_and_retry_after_for_the_ladder() {
    let failure = build_failure(429, r#"{"error":{"message":"slow down"}}"#, Some(30), true);
    let ProviderFailure::ApiCall {
        status,
        message,
        response_body,
        retry_after_secs,
        retry_after_present: _,
    } = failure
    else {
        panic!("expected an ApiCall failure");
    };
    assert_eq!(status, Some(429));
    assert_eq!(message, "slow down");
    assert_eq!(retry_after_secs, Some(30));
    assert!(response_body.is_some_and(|body| body.contains("slow down")));
}

/// The message is pulled from `error.message` on all four kinds, and a body that
/// is not JSON falls back to itself — both only ever read by the ladder.
#[test]
fn the_error_message_is_read_from_the_error_envelope() {
    let extracted = build_failure(
        400,
        r#"{"error":{"message":"too long","code":"x"}}"#,
        None,
        false,
    );
    assert!(matches!(
        extracted,
        ProviderFailure::ApiCall { ref message, .. } if message == "too long"
    ));

    let raw = build_failure(500, "upstream exploded", None, false);
    assert!(matches!(
        raw,
        ProviderFailure::ApiCall { ref message, .. } if message == "upstream exploded"
    ));
}
