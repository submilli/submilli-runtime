//! The five embedding wire formats, as pure functions.
//!
//! One request builder and one response parser per [`EmbeddingProviderType`]:
//! where the request goes, what header carries the key, what the body looks
//! like, how a query is told from a document, how dimensions and truncation are
//! spelled, and where the vectors and the usage come back from. Everything here
//! is a pure function over `serde_json` values so the whole matrix is testable
//! without a socket; the HTTP round trip lives in [`super::dispatch`].
//!
//! | Provider | Purpose | Dimensions | No-truncate | Usage | Order |
//! |---|---|---|---|---|---|
//! | Voyage | `input_type` | `output_dimension` | `truncation: false` | `usage.total_tokens` | `data[].index` |
//! | OpenAI | none | `dimensions` | native rejection | `usage.prompt_tokens` | `data[].index` |
//! | Google | `taskType`, or a text template for `gemini-embedding-2` | `outputDimensionality` | none (bytes bounded before sending) | `usageMetadata`, per batch (`gemini-embedding-2` only) | positional |
//! | Jina | `task` | `dimensions` | `truncate: false` | `usage.total_tokens` | `data[].index` |
//! | Hugging Face | `prompt_name`, when declared | `dimensions` (dedicated only) | `truncate: false` | none | positional |
//!
//! **Length errors are recognized from the body.** No provider signals "an input
//! was too long" structurally, so a 400, 413 or 422 whose lowercased body
//! contains one of a per-provider set of phrases (see [`length_markers`]) is an
//! [`Rejection::InputTooLong`]. The phrases are heuristics, not contracts; a
//! miss degrades to [`Rejection::Other`], which is still a free, non-retryable
//! rejection.
//!
//! **Nothing in this module returns a response body, an input text or a key.**
//! Failures are classifications; the body is read for phrase matching and
//! dropped.

use std::time::Duration;

use interpreter::runtime::{EmbeddingMalformedReason, Purpose};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use submilli_blueprint::{EmbeddingProviderType, embedding_limits};

use super::{
    DispatchFailure, DispatchResponse, DispatchRow, EmbeddingRequest, Rejection, SentFailure,
};

/// One outbound request, fully formed. The key lives in `headers` and nowhere
/// else, so rendering `url` or `body` cannot leak it (Google's `?key=` form is
/// deliberately not used).
#[derive(Debug, Clone)]
pub struct WireRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

/// The model name cannot be placed in a URL path safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidModel;

/// The host a provider type uses when the row declares no `base_url`.
fn default_base_url(kind: EmbeddingProviderType) -> &'static str {
    match kind {
        EmbeddingProviderType::Voyage => "https://api.voyageai.com",
        EmbeddingProviderType::OpenAi => "https://api.openai.com",
        EmbeddingProviderType::Google => "https://generativelanguage.googleapis.com",
        EmbeddingProviderType::Jina => "https://api.jina.ai",
        EmbeddingProviderType::HuggingFace => "https://router.huggingface.co",
    }
}

/// Build the request for one sub-batch. `api_key` is already resolved.
pub fn build_request(
    req: &EmbeddingRequest<'_>,
    api_key: Option<&str>,
) -> Result<WireRequest, InvalidModel> {
    let base = req
        .base_url
        .unwrap_or_else(|| default_base_url(req.provider_type))
        .trim_end_matches('/');
    match req.provider_type {
        EmbeddingProviderType::Voyage => Ok(voyage_request(base, req, api_key)),
        EmbeddingProviderType::OpenAi => Ok(openai_request(base, req, api_key)),
        EmbeddingProviderType::Jina => Ok(jina_request(base, req, api_key)),
        EmbeddingProviderType::Google => google_request(base, req, api_key),
        EmbeddingProviderType::HuggingFace => huggingface_request(base, req, api_key),
    }
}

fn bearer(api_key: Option<&str>) -> Vec<(String, String)> {
    api_key
        .map(|key| vec![("authorization".to_string(), format!("Bearer {key}"))])
        .unwrap_or_default()
}

fn voyage_request(base: &str, req: &EmbeddingRequest<'_>, api_key: Option<&str>) -> WireRequest {
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("input".into(), json!(req.texts));
    body.insert(
        "input_type".into(),
        json!(match req.purpose {
            Purpose::Query => "query",
            Purpose::Document => "document",
        }),
    );
    body.insert("truncation".into(), json!(false));
    if req.send_dimensions {
        body.insert("output_dimension".into(), json!(req.dimensions));
    }
    WireRequest {
        url: format!("{base}/v1/embeddings"),
        headers: bearer(api_key),
        body: Value::Object(body),
    }
}

/// OpenAI has no purpose and no truncation flag (an over-long input is a native
/// error), so a query and a document body are byte-identical.
fn openai_request(base: &str, req: &EmbeddingRequest<'_>, api_key: Option<&str>) -> WireRequest {
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("input".into(), json!(req.texts));
    body.insert("encoding_format".into(), json!("float"));
    if req.send_dimensions {
        body.insert("dimensions".into(), json!(req.dimensions));
    }
    WireRequest {
        url: format!("{base}/v1/embeddings"),
        headers: bearer(api_key),
        body: Value::Object(body),
    }
}

fn jina_request(base: &str, req: &EmbeddingRequest<'_>, api_key: Option<&str>) -> WireRequest {
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("input".into(), json!(req.texts));
    body.insert(
        "task".into(),
        json!(match req.purpose {
            Purpose::Query => "retrieval.query",
            Purpose::Document => "retrieval.passage",
        }),
    );
    body.insert("truncate".into(), json!(false));
    if req.send_dimensions {
        body.insert("dimensions".into(), json!(req.dimensions));
    }
    WireRequest {
        url: format!("{base}/v1/embeddings"),
        headers: bearer(api_key),
        body: Value::Object(body),
    }
}

/// Google has no truncation switch that `gemini-embedding-001` honors: it
/// silently truncates over-length input and succeeds, even with
/// `embedContentConfig.autoTruncate: false` (confirmed live, 2026-10-05), so
/// the runtime bounds input bytes before sending. A batch holds at most 100
/// requests and no byte cap applies (12 KB and 81 KB requests succeeded).
fn google_request(
    base: &str,
    req: &EmbeddingRequest<'_>,
    api_key: Option<&str>,
) -> Result<WireRequest, InvalidModel> {
    let model = embedding_limits::google_model_id(req.model);
    let path_model = encode_path(model, false)?;
    let templated = embedding_limits::is_templated_google_model(model);

    let requests: Vec<Value> = req
        .texts
        .iter()
        .map(|text| {
            let mut item = Map::new();
            item.insert("model".into(), json!(format!("models/{model}")));
            let text = if templated {
                templated_text(req.purpose, text)
            } else {
                text.clone()
            };
            // Exactly one part per text: parts of one content aggregate into a
            // single vector.
            item.insert("content".into(), json!({ "parts": [{ "text": text }] }));
            if !templated {
                item.insert(
                    "taskType".into(),
                    json!(match req.purpose {
                        Purpose::Query => "RETRIEVAL_QUERY",
                        Purpose::Document => "RETRIEVAL_DOCUMENT",
                    }),
                );
            }
            if req.send_dimensions {
                item.insert("outputDimensionality".into(), json!(req.dimensions));
            }
            Value::Object(item)
        })
        .collect();

    Ok(WireRequest {
        url: format!("{base}/v1beta/models/{path_model}:batchEmbedContents"),
        headers: api_key
            .map(|key| vec![("x-goog-api-key".to_string(), key.to_string())])
            .unwrap_or_default(),
        body: json!({ "requests": requests }),
    })
}

fn templated_text(purpose: Purpose, text: &str) -> String {
    match purpose {
        Purpose::Query => format!("{}{text}", embedding_limits::GOOGLE_QUERY_TEMPLATE_PREFIX),
        Purpose::Document => format!(
            "{}{text}",
            embedding_limits::GOOGLE_DOCUMENT_TEMPLATE_PREFIX
        ),
    }
}

/// A configured `base_url` selects the dedicated endpoint (`<base>/embed`);
/// without one the shared router's feature-extraction route is used. With no
/// `api_key` no `Authorization` header is sent, and no ambient token is read.
fn huggingface_request(
    base: &str,
    req: &EmbeddingRequest<'_>,
    api_key: Option<&str>,
) -> Result<WireRequest, InvalidModel> {
    let dedicated = embedding_limits::is_dedicated_endpoint(req.provider_type, req.base_url);
    let url = if dedicated {
        format!("{base}/embed")
    } else {
        let model = encode_path(req.model, true)?;
        format!("{base}/hf-inference/models/{model}/pipeline/feature-extraction")
    };

    let mut body = Map::new();
    // Always an array, so the response is always `[[f32...], ...]`.
    body.insert("inputs".into(), json!(req.texts));
    body.insert("truncate".into(), json!(false));
    let prompt_name = match req.purpose {
        Purpose::Query => req.query_prompt_name,
        Purpose::Document => req.document_prompt_name,
    };
    if let Some(name) = prompt_name {
        body.insert("prompt_name".into(), json!(name));
    }
    // The shared router has no dimensions parameter.
    if dedicated && req.send_dimensions {
        body.insert("dimensions".into(), json!(req.dimensions));
    }
    Ok(WireRequest {
        url,
        headers: bearer(api_key),
        body: Value::Object(body),
    })
}

/// Percent-encode a model name for a URL path, refusing dot segments (a URL
/// parser would collapse them and could leave the intended route).
fn encode_path(model: &str, keep_slash: bool) -> Result<String, InvalidModel> {
    if model.is_empty() || model.split('/').any(|seg| seg == "." || seg == "..") {
        return Err(InvalidModel);
    }
    let mut out = String::with_capacity(model.len());
    for byte in model.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    Ok(out)
}

// --- responses -------------------------------------------------------------

fn invalid_body() -> DispatchFailure {
    DispatchFailure::Malformed {
        reason: EmbeddingMalformedReason::InvalidBody,
        usage: None,
    }
}

/// Parse a 2xx body into rows and usage. A body that is not the provider's
/// shape, or whose vectors are token-level (3-D), is
/// [`EmbeddingMalformedReason::InvalidBody`]; non-finite values pass through for
/// the core's validation.
pub fn parse_response(
    kind: EmbeddingProviderType,
    body: &str,
) -> Result<DispatchResponse, DispatchFailure> {
    match kind {
        EmbeddingProviderType::Voyage
        | EmbeddingProviderType::OpenAi
        | EmbeddingProviderType::Jina => parse_data_response(body),
        EmbeddingProviderType::Google => parse_google_response(body),
        EmbeddingProviderType::HuggingFace => parse_huggingface_response(body),
    }
}

#[derive(Deserialize)]
struct DataResponse {
    data: Vec<DataRow>,
    #[serde(default)]
    usage: Option<DataUsage>,
}

#[derive(Deserialize)]
struct DataRow {
    #[serde(default)]
    index: Option<usize>,
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct DataUsage {
    /// OpenAI reports `prompt_tokens`; Voyage and Jina report `total_tokens`.
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

/// The first of two counts that is nonzero; zero means unreported.
fn first_reported(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    [first, second]
        .into_iter()
        .flatten()
        .find(|tokens| *tokens > 0)
}

fn parse_data_response(body: &str) -> Result<DispatchResponse, DispatchFailure> {
    let parsed: DataResponse = serde_json::from_str(body).map_err(|_| invalid_body())?;
    let indexed = parsed.data.iter().filter(|row| row.index.is_some()).count();
    if indexed != 0 && indexed != parsed.data.len() {
        return Err(invalid_body());
    }
    let usage = parsed
        .usage
        .and_then(|u| first_reported(u.prompt_tokens, u.total_tokens));
    Ok(DispatchResponse {
        rows: parsed
            .data
            .into_iter()
            .map(|row| DispatchRow {
                index: row.index,
                values: row.embedding,
            })
            .collect(),
        usage,
    })
}

#[derive(Deserialize)]
struct GoogleResponse {
    embeddings: Vec<GoogleEmbedding>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<GoogleUsage>,
}

#[derive(Deserialize)]
struct GoogleEmbedding {
    values: Vec<f32>,
}

#[derive(Deserialize)]
struct GoogleUsage {
    #[serde(default, rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(default, rename = "totalTokenCount")]
    total_token_count: Option<u64>,
}

/// Rows are positional; usage is per batch and 0 means unreported.
fn parse_google_response(body: &str) -> Result<DispatchResponse, DispatchFailure> {
    let parsed: GoogleResponse = serde_json::from_str(body).map_err(|_| invalid_body())?;
    let usage = parsed
        .usage_metadata
        .and_then(|u| first_reported(u.prompt_token_count, u.total_token_count));
    Ok(DispatchResponse {
        rows: parsed
            .embeddings
            .into_iter()
            .map(|row| DispatchRow {
                index: None,
                values: row.values,
            })
            .collect(),
        usage,
    })
}

/// `[[f32...], ...]`, positional, no usage. A 3-D (token-level) array fails the
/// `Vec<Vec<f32>>` shape and is therefore an invalid body.
fn parse_huggingface_response(body: &str) -> Result<DispatchResponse, DispatchFailure> {
    let rows: Vec<Vec<f32>> = serde_json::from_str(body).map_err(|_| invalid_body())?;
    Ok(DispatchResponse {
        rows: rows
            .into_iter()
            .map(|values| DispatchRow {
                index: None,
                values,
            })
            .collect(),
        usage: None,
    })
}

// --- failures --------------------------------------------------------------

/// Classify a non-2xx response. `retry_after_secs` is the parsed delta-seconds
/// header; `input_count` bounds a recovered input index.
pub fn classify_failure(
    kind: EmbeddingProviderType,
    status: u16,
    body: &str,
    retry_after_secs: Option<u64>,
    input_count: usize,
) -> DispatchFailure {
    match status {
        401 | 403 => DispatchFailure::Rejected(Rejection::Unauthorized),
        429 => DispatchFailure::Rejected(Rejection::RateLimited {
            retry_after: retry_after_secs.map(Duration::from_secs),
        }),
        400..=499 => {
            DispatchFailure::Rejected(classify_client_error(kind, status, body, input_count))
        }
        500..=599 => DispatchFailure::Failed {
            kind: SentFailure::ProviderUnavailable,
            usage: None,
        },
        // A redirect or informational status: the endpoint is not what the
        // operator declared.
        _ => DispatchFailure::Failed {
            kind: SentFailure::Transport,
            usage: None,
        },
    }
}

fn classify_client_error(
    kind: EmbeddingProviderType,
    status: u16,
    body: &str,
    input_count: usize,
) -> Rejection {
    let lower = body.to_ascii_lowercase();
    // Google answers an unusable key with a 400.
    if kind == EmbeddingProviderType::Google
        && (lower.contains("api key not valid") || lower.contains("api_key_invalid"))
    {
        return Rejection::Unauthorized;
    }
    let length_status = matches!(status, 400 | 413 | 422);
    if length_status && length_markers(kind).iter().any(|m| lower.contains(m)) {
        return Rejection::InputTooLong {
            index: named_index(&lower, input_count),
        };
    }
    Rejection::Other
}

/// Phrases (lowercase) that mark an over-long-input error, per provider.
///
/// Deliberately narrow: each phrase is about one input's length. A bare
/// "exceed" would also match a whole-request payload-size or batch-size error
/// ("Request payload size exceeds the limit"), which sending fewer texts fixes
/// and which is not the input being too long. A miss is cheap (a free
/// [`Rejection::Other`]); a false hit tells the program to shorten a text that
/// was fine.
fn length_markers(kind: EmbeddingProviderType) -> &'static [&'static str] {
    match kind {
        EmbeddingProviderType::OpenAi => &[
            "maximum context length",
            "maximum input length",
            "context_length_exceeded",
        ],
        EmbeddingProviderType::Voyage => &[
            "too long",
            "input length",
            "tokens per input",
            "context length",
            "context window",
            "too many tokens",
        ],
        EmbeddingProviderType::Jina => &[
            "input_token_limit_exceeded",
            "too long",
            "input length",
            "context length",
        ],
        EmbeddingProviderType::HuggingFace => &["too long", "truncate", "must have less than"],
        EmbeddingProviderType::Google => &[
            "input token count",
            "token limit for input",
            "exceeds the maximum number of tokens",
            "too long",
        ],
    }
}

/// The sub-batch-local index of the offending input, when the error names one
/// as `input[N]`, `inputs[N]`, `requests[N]` or `data[N]`, or in prose as
/// "example at index N".
fn named_index(lower: &str, input_count: usize) -> Option<usize> {
    ["inputs[", "input[", "requests[", "data[", "at index "]
        .iter()
        .find_map(|prefix| {
            let start = lower.find(prefix)? + prefix.len();
            let digits: String = lower
                .get(start..)?
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse::<usize>().ok()
        })
        .filter(|index| *index < input_count)
}

#[cfg(test)]
mod tests;
