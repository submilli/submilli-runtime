//! The four provider wire formats, as pure functions.
//!
//! One kind per [`ProviderKind`] arm: where the request goes, what header
//! carries the key, what the body looks like, how a JSON Schema is attached, and
//! where the text, the usage, and the stop reason come back from. Everything
//! here is a pure function over `serde_json` values so the whole matrix is
//! testable without a socket — the HTTP round trip that uses it lives in
//! [`super::dispatch`].
//!
//! **Why the schema attachment is per-kind and not a shared helper.** The four
//! kinds disagree about more than a field name. OpenAI takes a `response_format`
//! envelope wrapping a *named* schema; Google takes a bare schema beside a MIME
//! type that must be set with it; Anthropic takes an `output_config.format`
//! envelope of its own. A shared helper would have to be a match anyway, and
//! flattening them into "attach the schema" is exactly the mistake that ships
//! `{"type":"json_object"}` — a request that asks for *some* JSON and silently
//! drops the shape, with no error and a response that parses. Each arm states
//! its own envelope, and each arm is asserted on the serialized body.
//!
//! **The output cap is spelled four ways too**, and one of them is load-bearing
//! in a way that is easy to miss: `openai-compatible` takes `max_tokens` where
//! first-party OpenAI takes `max_completion_tokens`, because most compatible
//! servers predate that rename and *ignore* the newer field. A dropped cap is
//! silent, and the cap is what makes the budget reservation an upper bound
//! rather than a guess.
//!
//! **Nothing in this module constructs an error that carries a body, a prompt,
//! or a completion.** Parsing returns [`ProviderFailure`] values whose
//! body-bearing fields exist for the classification ladder to *read* and drop
//! (R13/KTD7); the ladder in [`super::provider`] is what enforces that, and this
//! module's job is only to fill them faithfully.

use serde_json::{Map, Value, json};

use super::provider::{ProviderFailure, ProviderResponse, ProviderUsage, StopReason};

/// The name a schema is given where the kind requires one.
///
/// OpenAI's `json_schema` envelope makes `name` mandatory — a request without it
/// is a 400. It never reaches the guest and carries no meaning to the model, so
/// it is one fixed identifier rather than something derived from the model or
/// the prompt: deriving it would put guest-controlled text on the wire in a
/// field that exists only to satisfy a schema validator.
const SCHEMA_NAME: &str = "submilli_response";

/// Anthropic's API version header. Pinned rather than tracked: the header is a
/// dated contract, and a floating value would change the response shape under a
/// parser that was verified against one.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic requires `max_tokens` on every request — there is no "unset". When
/// the model declares no `output_reserve` and the caller passes no cap, a value
/// still has to go on the wire, so this is the floor that keeps the request
/// well-formed rather than a limit anyone chose.
const ANTHROPIC_DEFAULT_MAX_TOKENS: u64 = 4096;

/// Which wire format a provider row speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    Google,
    OpenAi,
    /// An operator-supplied endpoint speaking OpenAI's chat/completions shape.
    /// The body is OpenAI's; only the endpoint and the structured-output opt-out
    /// differ.
    OpenAiCompatible,
}

impl ProviderKind {
    /// Parse the blueprint's `type:` string. The blueprint validator has already
    /// refused anything outside the shipping set, so an unrecognized value here
    /// is a bug rather than operator input — but it is reported, not panicked
    /// on, because a `Blueprint` can also be built in memory.
    pub fn from_type(provider_type: &str) -> Option<Self> {
        match provider_type {
            "anthropic" => Some(Self::Anthropic),
            "google" => Some(Self::Google),
            "openai" => Some(Self::OpenAi),
            "openai-compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }

    /// The endpoint a first-party kind uses when the row declares no `base_url`.
    /// `openai-compatible` has none — the blueprint validator requires the row to
    /// declare one — so it reports `None`.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("https://api.anthropic.com"),
            Self::Google => Some("https://generativelanguage.googleapis.com"),
            Self::OpenAi => Some("https://api.openai.com"),
            Self::OpenAiCompatible => None,
        }
    }
}

/// One outbound request, fully formed: where it goes, what rides with it, and
/// what it says.
///
/// The key lives in `headers` and nowhere else. It is never copied into `url`
/// or `body`, so a diagnostic that renders either cannot leak it — Google's
/// documented `?key=` query form is deliberately not used for exactly that
/// reason (see [`build_request`]).
#[derive(Debug, Clone)]
pub struct WireRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

/// Build the request for one dispatch.
///
/// `base_url` is the row's `base_url` if it declared one, else the kind's
/// default. `api_key` is already resolved; it is placed in a header and
/// returned, never logged.
pub fn build_request(
    kind: ProviderKind,
    base_url: &str,
    model: &str,
    prompt: &str,
    schema_json: Option<&Value>,
    output_cap: Option<u64>,
    api_key: Option<&str>,
) -> WireRequest {
    let base = base_url.trim_end_matches('/');
    match kind {
        ProviderKind::Anthropic => {
            anthropic_request(base, model, prompt, schema_json, output_cap, api_key)
        }
        ProviderKind::Google => {
            google_request(base, model, prompt, schema_json, output_cap, api_key)
        }
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => {
            openai_request(kind, base, model, prompt, schema_json, output_cap, api_key)
        }
    }
}

/// Anthropic's Messages API.
///
/// Auth is `x-api-key`, **not** `Authorization: Bearer` — the one kind where the
/// OpenAI habit produces a 401 rather than a working call. `max_tokens` is
/// required, so it is always present.
///
/// Structured output uses the **native** `output_config.format` envelope with
/// `type: "json_schema"`, which is generally available and needs no beta header.
/// The older `output_format` spelling and the beta header are accepted only
/// transitionally, and the tool-calling workaround — declare one tool, force it
/// with `tool_choice`, read the object out of the tool block's `input` — is what
/// the native field replaced. The native form is used because it is the one the
/// API documents today, and because it delivers the object as text, which is the
/// shape the typed path already parses.
fn anthropic_request(
    base: &str,
    model: &str,
    prompt: &str,
    schema_json: Option<&Value>,
    output_cap: Option<u64>,
    api_key: Option<&str>,
) -> WireRequest {
    let mut body = json!({
        "model": model,
        "max_tokens": output_cap.unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS),
        "messages": [{ "role": "user", "content": prompt }],
    });
    if let Some(schema) = schema_json {
        body.as_object_mut()
            .expect("the body is a JSON object")
            .insert(
                "output_config".to_string(),
                json!({ "format": { "type": "json_schema", "schema": schema } }),
            );
    }
    let mut headers = vec![(
        "anthropic-version".to_string(),
        ANTHROPIC_VERSION.to_string(),
    )];
    if let Some(key) = api_key {
        headers.push(("x-api-key".to_string(), key.to_string()));
    }
    WireRequest {
        url: format!("{base}/v1/messages"),
        headers,
        body,
    }
}

/// Google's `generateContent`.
///
/// The model name is **in the path**, so it is percent-safe only because model
/// identifiers are declared in the blueprint; the `models/` prefix is added when
/// the declaration omits it, since both spellings are in circulation.
///
/// Auth rides `x-goog-api-key`. Google also documents a `?key=` query parameter
/// and it is deliberately not used: a URL is the single most likely thing to
/// reach a log, a span attribute, or an error, and a key in the query string
/// leaks through every one of them. The header form keeps the key out of
/// [`WireRequest::url`] entirely.
///
/// The schema attaches as `generationConfig.responseSchema` **with**
/// `responseMimeType: "application/json"`. The MIME type alone asks for "some
/// JSON" — Google's exact analogue of OpenAI's `json_object` trap — so the two
/// are set together or not at all.
fn google_request(
    base: &str,
    model: &str,
    prompt: &str,
    schema_json: Option<&Value>,
    output_cap: Option<u64>,
    api_key: Option<&str>,
) -> WireRequest {
    let path_model = if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    };

    let mut generation_config = Map::new();
    if let Some(cap) = output_cap {
        generation_config.insert("maxOutputTokens".to_string(), json!(cap));
    }
    if let Some(schema) = schema_json {
        generation_config.insert("responseMimeType".to_string(), json!("application/json"));
        generation_config.insert("responseSchema".to_string(), schema.clone());
    }

    let mut body = json!({
        "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
    });
    if !generation_config.is_empty() {
        body.as_object_mut()
            .expect("the body is a JSON object")
            .insert(
                "generationConfig".to_string(),
                Value::Object(generation_config),
            );
    }

    let mut headers = Vec::new();
    if let Some(key) = api_key {
        headers.push(("x-goog-api-key".to_string(), key.to_string()));
    }
    WireRequest {
        url: format!("{base}/v1beta/{path_model}:generateContent"),
        headers,
        body,
    }
}

/// OpenAI's chat/completions, shared with `openai-compatible`.
///
/// The schema attaches as the full `json_schema` envelope:
///
/// ```json
/// { "type": "json_schema", "json_schema": { "name": "...", "schema": { ... } } }
/// ```
///
/// **Not `{"type":"json_object"}`.** That form is accepted by every one of these
/// endpoints and asks the model only for syntactically valid JSON of *any*
/// shape: the schema is dropped with no error, no warning, and a response that
/// parses. It is the defect this project already carries on the
/// `openai-compatible` path, and reproducing it here would make a typed call a
/// lie on three kinds instead of one. `name` is required by the envelope, so
/// [`SCHEMA_NAME`] fills it.
///
/// `strict` is deliberately **not** set. Strict mode constrains the accepted
/// schema surface — every object needs `additionalProperties: false` and must
/// list every property as required — so setting it would reject schemas the
/// emitter legitimately produces, turning a working typed call into a 400. It is
/// also unevenly implemented across compatible servers, so the same flag that
/// narrows the surface on OpenAI can be mishandled elsewhere. The unconditional
/// structural check on the way back is what makes omitting it safe: a response
/// that does not match the type fails loudly regardless.
///
/// **The output cap is the one field the two kinds spell differently.** OpenAI
/// deprecated `max_tokens` in favor of `max_completion_tokens`, which reasoning
/// models require; most openai-compatible servers predate that rename and either
/// ignore `max_completion_tokens` or reject it. Ignoring it is the dangerous
/// half — the cap silently vanishes, and the cap is what makes the budget
/// reservation an upper bound rather than a guess — so each kind sends the
/// spelling its own servers honor.
fn openai_request(
    kind: ProviderKind,
    base: &str,
    model: &str,
    prompt: &str,
    schema_json: Option<&Value>,
    output_cap: Option<u64>,
    api_key: Option<&str>,
) -> WireRequest {
    let mut body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let object = body.as_object_mut().expect("the body is a JSON object");
    if let Some(cap) = output_cap {
        let field = match kind {
            ProviderKind::OpenAiCompatible => "max_tokens",
            _ => "max_completion_tokens",
        };
        object.insert(field.to_string(), json!(cap));
    }
    if let Some(schema) = schema_json {
        object.insert(
            "response_format".to_string(),
            json!({
                "type": "json_schema",
                "json_schema": { "name": SCHEMA_NAME, "schema": schema },
            }),
        );
    }

    let mut headers = Vec::new();
    if let Some(key) = api_key {
        headers.push(("authorization".to_string(), format!("Bearer {key}")));
    }
    WireRequest {
        url: format!("{base}/v1/chat/completions"),
        headers,
        body,
    }
}

/// Parse a 2xx body into a [`ProviderResponse`].
///
/// A body that does not parse at all, or that carries no recognizable content,
/// is **not** silently turned into an empty success: it becomes a
/// [`ProviderFailure::NoObjectGenerated`] with no stop reason, which the ladder
/// classifies as `invalid-output`. Returning `Ok` with `text: None` here would
/// make a garbled 200 indistinguishable from a model that chose to say nothing,
/// and KTD1 exists precisely to keep those apart.
pub fn parse_response(kind: ProviderKind, body: &str) -> Result<ProviderResponse, ProviderFailure> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Err(ProviderFailure::NoObjectGenerated {
            text: None,
            usage: ProviderUsage::default(),
            stop_reason: None,
        });
    };
    match kind {
        ProviderKind::Anthropic => parse_anthropic(&value),
        ProviderKind::Google => parse_google(&value),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => parse_openai(&value),
    }
}

/// Anthropic: text lives in `content[].text`, and a native structured result
/// arrives in that same field as a JSON string.
///
/// `content` is an array of typed blocks and a `thinking` block can precede the
/// answer, so the text blocks are selected by `type` rather than by index — a
/// parser that reads `content[0]` returns a reasoning trace instead of the
/// answer the moment a thinking model is used.
///
/// A `tool_use` block's `input` is read as a fallback. Nothing this file sends
/// produces one now that structured output is native, but the transitional
/// tool-calling form still exists in the wild, and the object there is an object
/// rather than a string — so it is re-serialized, because the typed path parses
/// [`ProviderResponse::text`] as JSON and would otherwise see nothing at all.
fn parse_anthropic(value: &Value) -> Result<ProviderResponse, ProviderFailure> {
    let stop_reason = match value.get("stop_reason") {
        Some(Value::String(raw)) => Some(map_anthropic_stop(raw)),
        // `null` is what a streaming frame carries mid-flight; on a complete
        // non-streaming response its absence means the provider did not say,
        // which is not a natural stop.
        _ => None,
    };
    let usage = anthropic_usage(value);

    let blocks = value.get("content").and_then(Value::as_array);
    let text = blocks.and_then(|blocks| {
        // Text blocks first: that is where a native structured answer arrives,
        // and `thinking` blocks are skipped by selecting on `type`.
        let joined: Vec<&str> = blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect();
        if !joined.is_empty() {
            return Some(joined.join(""));
        }
        // The transitional tool-calling form. Its `input` is an object, so it is
        // re-serialized into the JSON text the typed path expects.
        blocks.iter().find_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("tool_use"))
                .then(|| block.get("input"))
                .flatten()
                .map(Value::to_string)
        })
    });

    finish(stop_reason, text, usage)
}

fn anthropic_usage(value: &Value) -> ProviderUsage {
    let usage = value.get("usage");
    ProviderUsage {
        input_tokens: number_at(usage, "input_tokens"),
        output_tokens: number_at(usage, "output_tokens"),
    }
}

/// Anthropic's stop reasons.
///
/// `refusal` is this API's content-filter spelling, and it is the one that most
/// needs saying: a safety classifier returns it as an ordinary **HTTP 200**, not
/// an error. Nothing throws, so a client that keys success off "the request
/// succeeded" reports a refusal as a clean answer — KTD1's failure mode, arriving
/// by the route the provider makes easiest.
///
/// `model_context_window_exceeded` is likewise a *stop reason*, not an HTTP
/// error: the answer ran into the context window rather than the `max_tokens`
/// cap. Both are the completion being cut short, so both are `Length` — the raw
/// spelling still travels as the diagnostic finish reason for anyone who needs
/// to tell them apart.
fn map_anthropic_stop(raw: &str) -> StopReason {
    match raw {
        "end_turn" | "stop_sequence" => StopReason::Stop,
        // The transitional tool-calling form ends here, and it is the
        // *successful* end of such a request — treating it as anything else
        // would report every one of those calls as a failure.
        "tool_use" => StopReason::Stop,
        "max_tokens" | "model_context_window_exceeded" => StopReason::Length,
        "refusal" => StopReason::ContentFilter,
        other => StopReason::Other(other.to_string()),
    }
}

/// Google: text lives in `candidates[0].content.parts[].text`, and a structured
/// result arrives in that same text as a JSON string — Google constrains the
/// text rather than moving the object elsewhere, so no re-serialization is
/// needed.
fn parse_google(value: &Value) -> Result<ProviderResponse, ProviderFailure> {
    let candidate = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first());

    // A *prompt* blocked before generation produces no candidates at all, and
    // says why in `promptFeedback.blockReason` instead of in a finish reason.
    // Read before the candidate, because there is no candidate to read: left to
    // the ordinary path this would fall through to "no stop reason" and be
    // classified as an unreadable body, turning a content-filtered request into
    // an `invalid-output` the guest cannot act on.
    let blocked = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str);

    let stop_reason = match (blocked, candidate.and_then(|c| c.get("finishReason"))) {
        (Some(_), _) => Some(StopReason::ContentFilter),
        (None, Some(Value::String(raw))) => Some(map_google_stop(raw)),
        _ => None,
    };

    let usage = value.get("usageMetadata");
    let usage = ProviderUsage {
        input_tokens: number_at(usage, "promptTokenCount"),
        output_tokens: number_at(usage, "candidatesTokenCount"),
    };

    let text = candidate
        .and_then(|c| c.pointer("/content/parts"))
        .and_then(Value::as_array)
        .and_then(|parts| {
            let joined: Vec<&str> = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            (!joined.is_empty()).then(|| joined.join(""))
        });

    finish(stop_reason, text, usage)
}

/// Google's finish reasons. `SAFETY`, `RECITATION`, `BLOCKLIST`, `PROHIBITED_CONTENT`
/// and `SPII` are all the filter stopping the answer; collapsing them into one
/// arm is right because a guest branches on `reason`, and the raw spelling still
/// travels as the diagnostic finish reason.
fn map_google_stop(raw: &str) -> StopReason {
    match raw {
        "STOP" => StopReason::Stop,
        "MAX_TOKENS" => StopReason::Length,
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => {
            StopReason::ContentFilter
        }
        "OTHER" => StopReason::Error,
        other => StopReason::Other(other.to_string()),
    }
}

/// OpenAI and openai-compatible: text lives in
/// `choices[0].message.content`, and a structured result arrives in that same
/// field as a JSON string.
///
/// A `refusal` on the message is read *before* the content, because a refused
/// request carries `content: null` alongside it — reading content first would
/// report a refusal as an empty answer.
fn parse_openai(value: &Value) -> Result<ProviderResponse, ProviderFailure> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());

    let refused = choice
        .and_then(|c| c.pointer("/message/refusal"))
        .is_some_and(|refusal| !refusal.is_null());

    let stop_reason = if refused {
        Some(StopReason::ContentFilter)
    } else {
        match choice.and_then(|c| c.get("finish_reason")) {
            Some(Value::String(raw)) => Some(map_openai_stop(raw)),
            _ => None,
        }
    };

    let usage = value.get("usage");
    let usage = ProviderUsage {
        input_tokens: number_at(usage, "prompt_tokens"),
        output_tokens: number_at(usage, "completion_tokens"),
    };

    let text = choice
        .and_then(|c| c.pointer("/message/content"))
        .and_then(Value::as_str)
        .map(str::to_string);

    finish(stop_reason, text, usage)
}

/// OpenAI's finish reasons. `content_filter` is the filter arm; `tool_calls` and
/// `function_call` are neither a natural stop nor a failure this runtime
/// classifies, so they travel as themselves.
fn map_openai_stop(raw: &str) -> StopReason {
    match raw {
        "stop" => StopReason::Stop,
        "length" => StopReason::Length,
        "content_filter" => StopReason::ContentFilter,
        other => StopReason::Other(other.to_string()),
    }
}

/// The shared tail of every parser.
///
/// A response with no stop reason **and** no text is a body this runtime could
/// not read, not a successful empty answer — see [`parse_response`]. A response
/// with a stop reason and no text is a real outcome (a filtered completion is
/// exactly that) and resolves normally so the ladder can classify it.
fn finish(
    stop_reason: Option<StopReason>,
    text: Option<String>,
    usage: ProviderUsage,
) -> Result<ProviderResponse, ProviderFailure> {
    match stop_reason {
        Some(stop_reason) => Ok(ProviderResponse {
            stop_reason,
            text,
            usage,
        }),
        None => Err(ProviderFailure::NoObjectGenerated {
            text,
            usage,
            stop_reason: None,
        }),
    }
}

/// Read a usage count as an `f64`.
///
/// Absent, null, and non-numeric all become `None` rather than `0` — KTD3's rule
/// that indeterminate is not free, applied at the point the number is read
/// rather than after a zero has already been invented. A JSON number is always
/// finite, so the finite guard downstream is belt-and-braces here; it is what
/// catches a non-finite value arriving by any other route.
fn number_at(parent: Option<&Value>, field: &str) -> Option<f64> {
    parent?.get(field)?.as_f64()
}

/// Build the failure for a non-2xx response.
///
/// Everything needed to classify is carried: the status, the provider's message,
/// the raw body, and any `retry-after`. The ladder reads them and drops them —
/// this function's contract is to hand over what classification needs, not to
/// decide what a guest sees.
pub fn build_failure(status: u16, body: &str, retry_after_secs: Option<u64>) -> ProviderFailure {
    ProviderFailure::ApiCall {
        status: Some(status),
        message: error_message(body),
        response_body: Some(body.to_string()),
        retry_after_secs,
    }
}

/// Pull the provider's own error message out of an error body.
///
/// All four kinds nest it under `error.message` (Google under `error.message`
/// too, despite the different success shape). The raw body is the fallback, and
/// it is safe as a fallback because the caller's contract is that both are read
/// for classification and dropped.
fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests;
