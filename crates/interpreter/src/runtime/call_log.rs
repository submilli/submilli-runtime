//! Call records: each host call a program made, when it ran, and, for a call that
//! reaches outside the program, what it sent and what came back.
//!
//! The [`DecisionLog`](super::DecisionLog) keeps these beside its decisions. Every gated
//! host call is a call: it starts at its gate and ends when the host function returns,
//! so its timing and outcome need nothing from the host function itself. A host function
//! whose call fetches or sends something also hands the recorder its request and
//! response ([`record_payload`]): a capped copy, a digest of the full
//! payload, and its full size.
//!
//! What is never kept: a secret's value (a `secrets.get` call keeps its timing only),
//! and the values of credential-bearing headers, which are masked before the copy and the
//! digest are taken (see [`mask_headers`]). Matching known secret values inside bodies is
//! the embedder's job when it stores a record; the runtime does not know them all.
//!
//! Hashing a payload walks bytes the program already paid fuel to send or receive, so the
//! digest adds no unbounded work of its own; like the rest of the recorder it is not
//! charged to guest fuel (see `DecisionLogConfig::max_line_capture_frames`).

use std::borrow::Cow;

use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::StoreData;
use super::decision::{CallTicket, SourceLine};

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallOutcome {
    /// The host function returned a value.
    Returned,
    /// The host function failed: a denial, a thrown error, a trap.
    Failed,
    /// The run ended while the call was still in progress (a timeout or a cancellation).
    Unfinished,
}

/// A copy of a payload body, kept as text when it is UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "encoding", content = "data")]
pub enum BodyCopy {
    Text(String),
    Base64(String),
}

/// One side of a call: what it sent, or what came back.
#[derive(Debug, Clone, Serialize)]
pub struct PayloadRecord {
    /// The structured part: a method and URL, a tool, a model, a status. Capped like a
    /// decision's context, with credential-bearing headers masked.
    pub meta: Value,
    /// A copy of the body, capped at `DecisionLogConfig::max_payload_bytes`.
    pub body: Option<BodyCopy>,
    /// SHA-256, hex, of the full meta and body (masked headers excluded).
    pub digest: String,
    /// The payload's full size, counted before any cap or masking: the body's bytes, or
    /// the meta's when there is no body.
    pub bytes: u64,
    /// The meta or body copy was cut, or dropped for the recorder's byte budget.
    pub truncated: bool,
    /// Header names whose values were masked.
    pub masked_headers: Vec<String>,
}

/// Token counts a model provider reported for one call. Absent when not reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ModelUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallRecord {
    pub call_index: u64,
    pub caller: String,
    pub capability: String,
    pub started_micros: u64,
    /// `None` while the call runs, and for an [`CallOutcome::Unfinished`] call.
    pub ended_micros: Option<u64>,
    pub outcome: Option<CallOutcome>,
    pub line: Option<SourceLine>,
    pub request: Option<Box<PayloadRecord>>,
    pub response: Option<Box<PayloadRecord>>,
    pub usage: Option<ModelUsage>,
}

/// What a host function hands the recorder for one side of a call.
pub struct Payload<'a> {
    pub meta: Value,
    pub body: Option<Cow<'a, [u8]>>,
    /// The full size, when it differs from the body handed over: a download written to
    /// disk, or a body the host function received in another form.
    pub bytes: Option<u64>,
    pub masked_headers: Vec<String>,
}

impl<'a> Payload<'a> {
    pub fn meta(meta: Value) -> Self {
        Self {
            meta,
            body: None,
            bytes: None,
            masked_headers: Vec::new(),
        }
    }

    pub fn with_body(mut self, body: &'a [u8]) -> Self {
        self.body = Some(Cow::Borrowed(body));
        self
    }

    pub fn with_owned_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(Cow::Owned(body));
        self
    }

    pub fn with_size(mut self, bytes: u64) -> Self {
        self.bytes = Some(bytes);
        self
    }

    pub fn with_masked(mut self, masked: Vec<String>) -> Self {
        self.masked_headers = masked;
        self
    }
}

/// Which side of a call a payload belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Request,
    Response,
}

/// The value a masked header carries in a record.
pub const MASKED: &str = "[masked]";

/// Whether a header's value is a credential: `Authorization`, `Proxy-Authorization`,
/// `Cookie`, `Set-Cookie`, and any name containing `key`, `token`, `secret`, or `auth`.
pub fn is_sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "cookie" | "set-cookie")
        || ["key", "token", "secret", "auth"]
            .iter()
            .any(|word| name.contains(word))
}

/// Headers as a recordable JSON array of `[name, value]` pairs with credential values
/// masked, and the names that were masked.
pub fn mask_headers(headers: &[(String, String)]) -> (Value, Vec<String>) {
    let mut masked = Vec::new();
    let pairs = headers
        .iter()
        .map(|(name, value)| {
            if is_sensitive_header(name) {
                masked.push(name.clone());
                Value::from(vec![Value::from(name.as_str()), Value::from(MASKED)])
            } else {
                Value::from(vec![
                    Value::from(name.as_str()),
                    Value::from(value.as_str()),
                ])
            }
        })
        .collect();
    (Value::Array(pairs), masked)
}

/// Builds a payload record. `max_meta` caps meta strings, `max_body` the body copy.
pub(crate) fn capture(payload: &Payload<'_>, max_meta: usize, max_body: usize) -> PayloadRecord {
    let mut hasher = Sha256::new();
    let meta_text = serde_json::to_vec(&payload.meta).unwrap_or_default();
    hasher.update((meta_text.len() as u64).to_le_bytes());
    hasher.update(&meta_text);
    let body = payload.body.as_deref();
    if let Some(body) = body {
        hasher.update(body);
    }
    let digest = format!("{:x}", hasher.finalize());
    let (meta, mut truncated) = super::decision::cap_context(&payload.meta, max_meta);
    let full = payload
        .bytes
        .unwrap_or_else(|| body.map_or(meta_text.len(), <[u8]>::len) as u64);
    let body = body.map(|body| {
        let kept = body.get(..max_body.min(body.len())).unwrap_or_default();
        truncated |= kept.len() < body.len();
        copy_body(kept)
    });
    PayloadRecord {
        meta,
        body,
        digest,
        bytes: full,
        truncated,
        masked_headers: payload.masked_headers.clone(),
    }
}

fn copy_body(bytes: &[u8]) -> BodyCopy {
    match std::str::from_utf8(bytes) {
        Ok(text) => BodyCopy::Text(text.to_owned()),
        // A cut at the cap can split a character; keep the valid prefix as text.
        Err(error) if error.error_len().is_none() && error.valid_up_to() > 0 => {
            BodyCopy::Text(String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned())
        }
        Err(_) => BodyCopy::Base64(base64::engine::general_purpose::STANDARD.encode(bytes)),
    }
}

/// Bytes a payload record holds, for the recorder's budget. Parsed JSON costs several
/// times its text; charged generously as decision payloads are.
pub(crate) fn payload_cost(record: &PayloadRecord) -> u64 {
    let meta = serde_json::to_string(&record.meta).map_or(0, |text| text.len()) as u64;
    let body = match &record.body {
        Some(BodyCopy::Text(text) | BodyCopy::Base64(text)) => text.len() as u64,
        None => 0,
    };
    meta.saturating_mul(2)
        .saturating_add(body)
        .saturating_add(record.digest.len() as u64)
}

/// Hands one side of the call `ticket` names to the recorder, when one is installed.
/// `payload` runs only then, so building it costs nothing without a recorder.
pub(crate) fn record_payload<'a>(
    store: &impl wasmtime::AsContext<Data = StoreData>,
    ticket: Option<CallTicket>,
    side: Side,
    payload: impl FnOnce() -> Payload<'a>,
) {
    let Some(ticket) = ticket else {
        return;
    };
    if let Some(recorder) = store.as_context().data().security_check.recorder() {
        recorder.call_payload(ticket.call_index, side, payload());
    }
}

/// Hands the model usage of the call `ticket` names to the recorder.
pub(crate) fn record_usage(
    store: &impl wasmtime::AsContext<Data = StoreData>,
    ticket: Option<CallTicket>,
    usage: ModelUsage,
) {
    let Some(ticket) = ticket else {
        return;
    };
    if let Some(recorder) = store.as_context().data().security_check.recorder() {
        recorder.call_usage(ticket.call_index, usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What each catalog capability's calls keep beyond their timing and outcome. A
    /// capability added to the catalog fails [`every_capability_has_a_recording_class`]
    /// until it is placed here, so a new kind of call cannot skip the call log unnoticed.
    const REQUEST_AND_RESPONSE: &[&str] = &[
        "http.get",
        "http.post",
        "http.put",
        "http.patch",
        "http.delete",
        "http.head",
        "http.options",
        "http.download",
        "llm.call",
        "mcp.<server>",
    ];
    /// The request is the decision's context; the response is kept for whole reads,
    /// `exists`, and `size`. Streaming reads (`lines`, `bytes`, `list`) and `stat` and
    /// `peek` keep their timing only for now.
    const RESPONSE_ONLY: &[&str] = &["fs.read", "fs.stat", "session.read", "session.list"];
    /// Writes, git, and secrets: timing and outcome only. A secret's value is never kept.
    const TIMING_ONLY: &[&str] = &[
        "fs.write",
        "fs.list",
        "fs.mkdir",
        "fs.remove",
        "fs.move",
        "fs.copy",
        "git.init",
        "git.clone",
        "git.fetch",
        "git.commit",
        "secrets.get",
        "session.write",
        "session.remove",
    ];

    #[test]
    fn every_capability_has_a_recording_class() {
        for group in crate::stdlib::capabilities::catalog() {
            for capability in group.capabilities {
                let classes = [REQUEST_AND_RESPONSE, RESPONSE_ONLY, TIMING_ONLY]
                    .iter()
                    .filter(|class| class.contains(&capability.name))
                    .count();
                assert_eq!(
                    classes, 1,
                    "{} must be in exactly one recording class",
                    capability.name
                );
            }
        }
    }

    #[test]
    fn every_gating_module_ends_the_calls_it_begins() {
        for group in crate::stdlib::capabilities::catalog() {
            // The catalog names `@mcp/<server>` imports; their calls dispatch through the
            // internal MCP module.
            let module = match group.module {
                "@mcp" => crate::runtime::mcp::MCP_MODULE_NAME,
                module => module,
            };
            assert!(
                crate::runtime::host::begins_calls(module),
                "{module} gates capabilities, so its host calls must end their calls"
            );
        }
    }

    #[test]
    fn credential_headers_are_masked_by_name() {
        let headers = [
            ("Authorization", "Bearer t"),
            ("x-api-key", "k"),
            ("X-OAuth-Scopes", "repo"),
            ("cookie", "c"),
            ("Accept", "application/json"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned()));
        let (value, masked) = mask_headers(&headers);
        assert_eq!(
            masked,
            ["Authorization", "x-api-key", "X-OAuth-Scopes", "cookie"]
        );
        let text = value.to_string();
        assert!(!text.contains("Bearer t") && !text.contains("\"k\"") && !text.contains("repo"));
        assert!(text.contains("application/json"));
    }

    #[test]
    fn a_capped_body_keeps_the_full_size_and_digest() {
        let body = vec![b'a'; 100];
        let payload =
            Payload::meta(serde_json::json!({ "url": "https://x.test" })).with_body(&body);
        let record = capture(&payload, 1024, 10);
        assert!(record.truncated);
        assert_eq!(record.body, Some(BodyCopy::Text("a".repeat(10))));
        let whole = capture(&payload, 1024, 1000);
        assert_eq!(
            record.digest, whole.digest,
            "the digest covers the full body"
        );
        assert_eq!(record.bytes, 100);
        assert_eq!(whole.bytes, 100);
    }

    #[test]
    fn binary_bodies_are_kept_as_base64() {
        let payload = Payload::meta(Value::Null).with_body(&[0xff, 0x00, 0xfe]);
        assert!(matches!(
            capture(&payload, 16, 16).body,
            Some(BodyCopy::Base64(_))
        ));
    }

    #[test]
    fn a_cut_inside_a_character_keeps_the_text_before_it() {
        let text = "aé".as_bytes();
        let payload = Payload::meta(Value::Null).with_body(text);
        assert_eq!(
            capture(&payload, 16, 2).body,
            Some(BodyCopy::Text("a".into()))
        );
    }
}
