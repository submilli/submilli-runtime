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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::StoreData;
use super::decision::{CallTicket, SourceLine};

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallOutcome {
    /// The host function returned a value.
    Returned,
    /// The host function failed: a denial, a thrown error, a trap.
    Failed,
    /// The run ended while the call was still in progress (a timeout or a cancellation).
    Unfinished,
}

impl PayloadRecord {
    /// This record without its meta, body, and masked names: the digest, the size, and the
    /// flags.
    pub(crate) fn digest_only(&self) -> Self {
        Self {
            meta: Value::Null,
            body: None,
            digest: self.digest.clone(),
            bytes: self.bytes,
            truncated: self.truncated,
            masked_headers: Vec::new(),
        }
    }
}

/// A copy of a payload body, kept as text when it is UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "encoding", content = "data")]
pub enum BodyCopy {
    Text(String),
    Base64(String),
}

/// One side of a call: what it sent, or what came back.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Header names whose values were masked: at most 64, each capped like a meta string.
    pub masked_headers: Vec<String>,
}

/// Token counts a model provider reported for one call. Absent when not reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// One host call. Times are microseconds measured from the recorder's start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord {
    /// Run-wide, in call order; the same index the call's decisions carry.
    pub call_index: u64,
    pub caller: String,
    pub capability: String,
    /// When the call began, at its security gate.
    pub started_micros: u64,
    /// `None` while the call runs, and for an [`CallOutcome::Unfinished`] call.
    pub ended_micros: Option<u64>,
    pub outcome: Option<CallOutcome>,
    /// The program line that led to the call, when known.
    pub line: Option<SourceLine>,
    /// What the call sent, when it reached outside the program.
    pub request: Option<Box<PayloadRecord>>,
    /// What came back, or the failure that came instead.
    pub response: Option<Box<PayloadRecord>>,
    pub usage: Option<ModelUsage>,
}

impl CallRecord {
    /// A copy without the payload bodies and meta, keeping each side's digest, size, and
    /// flags: what an observer that reports sizes needs, without copying up to
    /// `max_payload_bytes` per side.
    pub fn without_bodies(&self) -> Self {
        let strip = |side: &Option<Box<PayloadRecord>>| {
            side.as_ref().map(|payload| Box::new(payload.digest_only()))
        };
        Self {
            call_index: self.call_index,
            caller: self.caller.clone(),
            capability: self.capability.clone(),
            started_micros: self.started_micros,
            ended_micros: self.ended_micros,
            outcome: self.outcome,
            line: self.line,
            request: strip(&self.request),
            response: strip(&self.response),
            usage: self.usage,
        }
    }
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
/// `Cookie`, `Set-Cookie`, and any name containing `key`, `token`, `secret`, `auth`,
/// `password`, `passwd`, `signature`, or `credential`.
pub fn is_sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "cookie" | "set-cookie")
        || [
            "key",
            "token",
            "secret",
            "auth",
            "password",
            "passwd",
            "signature",
            "credential",
        ]
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

/// A URL as a record keeps it: the userinfo and the values of credential-named query
/// parameters masked, and the fragment dropped. A URL that does not parse keeps nothing
/// of its authority, path, or query, so a credential in it cannot survive.
pub fn mask_url(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return unparsable_url(url);
    };
    let mut base = parsed.clone();
    base.set_query(None);
    base.set_fragment(None);
    let Some(mut text) = with_masked_userinfo(base) else {
        return unparsable_url(url);
    };
    if let Some(query) = parsed.query() {
        text.push('?');
        text.push_str(&mask_query(query));
    }
    text
}

/// `url` as text with its userinfo, if it has any, replaced by [`MASKED`].
///
/// The mask is spliced into the text: the `url` crate percent-encodes the brackets of
/// [`MASKED`] when it is set as a username. `None` when the text is not shaped as the
/// splice expects.
fn with_masked_userinfo(mut url: url::Url) -> Option<String> {
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    if !has_userinfo || !url.has_authority() {
        return Some(url.into());
    }
    let at = url.scheme().len().saturating_add("://".len());
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    let whole = url.as_str();
    let head = whole.get(..at)?;
    let tail = whole.get(at..)?;
    // Userinfo that survived the clearing would leak through the splice.
    let authority = tail.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return None;
    }
    Some(format!("{head}{MASKED}@{tail}"))
}

/// A query with the values of credential-named parameters masked. Parameters separate at
/// `&` and at `;`, and each keeps its own separator.
fn mask_query(query: &str) -> String {
    query
        .split_inclusive(['&', ';'])
        .map(|segment| {
            let (pair, separator) = match segment.char_indices().last() {
                Some((at, '&' | ';')) => segment.split_at(at),
                _ => (segment, ""),
            };
            let name = pair.split_once('=').map_or(pair, |(name, _)| name);
            if is_sensitive_param(name) {
                format!("{name}={MASKED}{separator}")
            } else {
                segment.to_owned()
            }
        })
        .collect()
}

/// Whether a query parameter's value is a credential, by the rule for header names.
fn is_sensitive_param(raw_name: &str) -> bool {
    let name = percent_encoding::percent_decode_str(raw_name).decode_utf8_lossy();
    is_sensitive_header(&name)
}

const UNPARSABLE_URL: &str = "[unparsable url]";

/// What a URL that does not parse is recorded as: its scheme, when it plainly has one,
/// and a placeholder. Everything after the scheme could hold a credential.
fn unparsable_url(url: &str) -> String {
    let scheme = url
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .filter(|scheme| {
            let mut chars = scheme.chars();
            chars
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });
    match scheme {
        Some(scheme) => format!("{scheme}://{UNPARSABLE_URL}"),
        None => UNPARSABLE_URL.to_owned(),
    }
}

/// Most masked header names a payload record keeps.
const MAX_MASKED_HEADERS: usize = 64;

/// The length of a payload digest as [`payload_cost`] charges it: SHA-256 as hex.
pub(crate) const DIGEST_HEX_BYTES: u64 = 64;

/// What [`payload_cost`] charges a [`PayloadRecord::digest_only`] record: its digest and a
/// `null` meta.
pub(crate) const DIGEST_ONLY_COST: u64 = DIGEST_HEX_BYTES + 2 * "null".len() as u64;

/// Builds a payload record. `max_meta` caps meta strings. The body copy is sized to the
/// `room` the recorder has left for the whole record and capped at `max_body` raw bytes;
/// see [`body_allowance`].
pub(crate) fn capture(
    payload: &Payload<'_>,
    max_meta: usize,
    room: u64,
    max_body: usize,
) -> PayloadRecord {
    let meta_text = serde_json::to_vec(&payload.meta).unwrap_or_default();
    let body = payload.body.as_deref();
    let digest = digest_of(&meta_text, body);
    let (meta, mut truncated) = super::decision::cap_context(&payload.meta, max_meta);
    if payload.masked_headers.len() > MAX_MASKED_HEADERS {
        truncated = true;
    }
    let masked_headers: Vec<String> = payload
        .masked_headers
        .iter()
        .take(MAX_MASKED_HEADERS)
        .map(|name| super::decision::cap_text(name, max_meta, &mut truncated))
        .collect();
    let full = payload
        .bytes
        .unwrap_or_else(|| body.map_or(meta_text.len(), <[u8]>::len) as u64);
    let body = body.and_then(|body| {
        // Sized from the capped forms the record will be charged for.
        let overhead = record_overhead(&meta, &masked_headers);
        let max_body = body_allowance(body, overhead, room, max_body);
        if max_body == 0 && !body.is_empty() {
            // No room for any of it: no copy is made.
            truncated = true;
            return None;
        }
        let kept = body.get(..max_body.min(body.len())).unwrap_or_default();
        let cut = kept.len() < body.len();
        truncated |= cut;
        Some(copy_body(kept, cut))
    });
    PayloadRecord {
        meta,
        body,
        digest,
        bytes: full,
        truncated,
        masked_headers,
    }
}

/// SHA-256, hex, of a payload: the meta's JSON text, then the body. The length prefix
/// marks where the meta ends and the body begins, so two payloads that split the same
/// bytes differently do not share a digest.
fn digest_of(meta_text: &[u8], body: Option<&[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update((meta_text.len() as u64).to_le_bytes());
    hasher.update(meta_text);
    if let Some(body) = body {
        hasher.update(body);
    }
    format!("{:x}", hasher.finalize())
}

impl Payload<'_> {
    /// The digest a record of this payload carries ([`PayloadRecord::digest`]), whatever
    /// the recorder's caps keep of it. Lets a transport compute what the call log
    /// recorded for a request.
    pub fn digest(&self) -> String {
        let meta_text = serde_json::to_vec(&self.meta).unwrap_or_default();
        digest_of(&meta_text, self.body.as_deref())
    }
}

/// How a body copy is kept.
enum BodyForm {
    /// As text: the first `valid` bytes.
    Text {
        valid: usize,
    },
    Base64,
}

/// The form a copy of `bytes` takes: text when it is UTF-8, else base64. A copy that was
/// `cut` may end inside a character; the text before it is kept. A body that was not cut
/// and is not UTF-8 is binary, however it ends. A cut inside the first character has no
/// text before it, so it is binary too.
fn body_form(bytes: &[u8], cut: bool) -> BodyForm {
    match std::str::from_utf8(bytes) {
        Ok(_) => BodyForm::Text { valid: bytes.len() },
        Err(error) if cut && error.error_len().is_none() && error.valid_up_to() > 0 => {
            BodyForm::Text {
                valid: error.valid_up_to(),
            }
        }
        Err(_) => BodyForm::Base64,
    }
}

fn copy_body(bytes: &[u8], cut: bool) -> BodyCopy {
    match body_form(bytes, cut) {
        BodyForm::Text { valid } => {
            let text = bytes.get(..valid).unwrap_or_default();
            BodyCopy::Text(String::from_utf8_lossy(text).into_owned())
        }
        BodyForm::Base64 => {
            BodyCopy::Base64(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
    }
}

/// Bytes a record holds besides its body and digest, for the recorder's budget: the meta
/// (parsed JSON costs several times its text, charged generously as decision payloads
/// are) and the masked names.
fn record_overhead(meta: &Value, masked: &[String]) -> u64 {
    let meta = serde_json::to_string(meta).map_or(0, |text| text.len()) as u64;
    let masked: u64 = masked
        .iter()
        .map(|name| name.len().saturating_add(std::mem::size_of::<String>()) as u64)
        .fold(0, u64::saturating_add);
    meta.saturating_mul(2).saturating_add(masked)
}

/// Bytes a payload record holds, for the recorder's budget.
pub(crate) fn payload_cost(record: &PayloadRecord) -> u64 {
    let body = match &record.body {
        Some(BodyCopy::Text(text) | BodyCopy::Base64(text)) => text.len() as u64,
        None => 0,
    };
    record_overhead(&record.meta, &record.masked_headers)
        .saturating_add(body)
        .saturating_add(record.digest.len() as u64)
}

/// How many raw bytes of `body` to copy, given the `room` the recorder has left for the
/// whole record, the record's `overhead` besides its digest ([`record_overhead`] of the
/// capped meta and names), and the `max_body` cap on the copy. A cut body costs what it
/// keeps, so the copy is sized to leave room for the rest of the record and, for a body
/// kept as base64, its expansion. Zero when nothing fits: the body is not copied at all.
fn body_allowance(body: &[u8], overhead: u64, room: u64, max_body: usize) -> usize {
    let available = room
        .saturating_sub(overhead)
        .saturating_sub(DIGEST_HEX_BYTES);
    let available = usize::try_from(available).unwrap_or(usize::MAX);
    let as_text = available.min(max_body).min(body.len());
    let probe = body.get(..as_text).unwrap_or_default();
    if matches!(
        body_form(probe, as_text < body.len()),
        BodyForm::Text { .. }
    ) {
        return as_text;
    }
    // Whole base64 groups: four characters per three bytes, rounded down.
    (available / 4).saturating_mul(3).min(max_body)
}

/// Hands one side of the call `ticket` names to the recorder, when one is installed and
/// would keep it. `payload` runs only then, so building it costs nothing otherwise.
pub(crate) fn record_payload<'a>(
    store: &impl wasmtime::AsContext<Data = StoreData>,
    ticket: Option<CallTicket>,
    side: Side,
    payload: impl FnOnce() -> Payload<'a>,
) {
    let Some(ticket) = ticket else {
        return;
    };
    if let Some(recorder) = store.as_context().data().security_check.recorder()
        && recorder.wants_payload(ticket.call_index)
    {
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
    ///
    /// A redirect hop (`redirect_guard`) begins its own `http.<verb>` call with no
    /// payloads: hops are timing-only calls whose decisions carry the hop, so the class
    /// of `http.*` holds for the request's own call, not for its hops.
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
        "embedding.embed",
        "mcp.<server>",
    ];
    /// The request is the decision's context; the response is kept for whole reads and
    /// session reads.
    const RESPONSE_ONLY: &[&str] = &["fs.read", "session.read", "session.list"];
    /// Writes, git, secrets, and the reads whose results are deferred to playback
    /// (`stat`, `peek`, `list`, and the streaming reads): timing and outcome only. A
    /// secret's value is never kept. `fs.exists` and `fs.size` gate as `fs.stat` and do
    /// keep their answer, which no class requires.
    const TIMING_ONLY: &[&str] = &[
        "fs.stat",
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
        for group in crate::stdlib::capabilities::all_groups() {
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
        for group in crate::stdlib::capabilities::all_groups() {
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

    /// Modules that gate without being a catalog group: `submilli:code` reads gate as
    /// `fs.read`, and `submilli:security` gates a package's own `security.check`.
    #[test]
    fn gating_modules_outside_the_catalog_end_their_calls_too() {
        for module in [
            crate::stdlib::code::MODULE_NAME,
            crate::stdlib::security::MODULE_NAME,
        ] {
            assert!(
                crate::runtime::host::begins_calls(module),
                "{module} gates, so its host calls must end their calls"
            );
        }
        assert!(!crate::runtime::host::begins_calls("submilli:test"));
    }

    /// Every source file that gates a capability or begins a call, with the module whose
    /// host functions run it. `None` marks the shared helpers: they run inside whichever
    /// module calls them.
    const CALLING_FILES: &[(&str, Option<&str>)] = &[
        (
            "src/stdlib/code/mod.rs",
            Some(crate::stdlib::code::MODULE_NAME),
        ),
        (
            "src/stdlib/embedding/mod.rs",
            Some(crate::stdlib::embedding::MODULE_NAME),
        ),
        ("src/stdlib/fs/mod.rs", Some(crate::stdlib::fs::MODULE_NAME)),
        (
            "src/stdlib/git/mod.rs",
            Some(crate::stdlib::git::MODULE_NAME),
        ),
        (
            "src/stdlib/http/mod.rs",
            Some(crate::stdlib::http::MODULE_NAME),
        ),
        (
            "src/stdlib/http/redirect_guard.rs",
            Some(crate::stdlib::http::MODULE_NAME),
        ),
        (
            "src/stdlib/llm/mod.rs",
            Some(crate::stdlib::llm::MODULE_NAME),
        ),
        (
            "src/stdlib/secrets.rs",
            Some(crate::stdlib::secrets::MODULE_NAME),
        ),
        (
            "src/stdlib/security.rs",
            Some(crate::stdlib::security::MODULE_NAME),
        ),
        (
            "src/stdlib/session/mod.rs",
            Some(crate::stdlib::session::MODULE_NAME),
        ),
        (
            "src/runtime/mcp.rs",
            Some(crate::runtime::mcp::MCP_MODULE_NAME),
        ),
        ("src/stdlib/shared.rs", None),
    ];

    fn rust_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    /// A file that begins calls but whose module is not in `CALL_MODULES` would leave
    /// its calls open until the run ends. The scan finds such a file even when it is
    /// new; the table must then name it and its module.
    #[test]
    fn every_file_that_begins_calls_belongs_to_a_call_module() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        rust_files(&root.join("src/stdlib"), &mut files);
        files.push(root.join("src/runtime/mcp.rs"));
        let needles = [
            "check_security_call(",
            "check_security(",
            "begin_call(",
            "begin_recorded_call(",
            "audit_entry_denial(",
            "audit_denial_in(",
            "authorize_capability(",
            "check_and_audit(",
        ];
        let mut found = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if needles.iter().any(|needle| text.contains(needle)) {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(relative);
            }
        }
        found.sort();
        let mut table: Vec<&str> = CALLING_FILES.iter().map(|(file, _)| *file).collect();
        table.sort_unstable();
        assert_eq!(
            found, table,
            "a file that gates or begins calls must be listed in CALLING_FILES with its module"
        );
        for (file, module) in CALLING_FILES {
            if let Some(module) = module {
                assert!(
                    crate::runtime::host::begins_calls(module),
                    "{file} gates through {module}, which is missing from CALL_MODULES"
                );
            }
        }
    }

    #[test]
    fn url_credentials_are_masked_before_recording() {
        let masked =
            mask_url("https://user:pw@api.test:8443/v1/x?q=1&api_key=abc&Token=t&flag#frag");
        assert_eq!(
            masked,
            "https://[masked]@api.test:8443/v1/x?q=1&api_key=[masked]&Token=[masked]&flag"
        );
        let plain = mask_url("https://api.test/a?page=2");
        assert_eq!(plain, "https://api.test/a?page=2");
        let encoded = mask_url("https://api.test/?access%5Ftoken=abc&x=1");
        assert!(!encoded.contains("abc"), "{encoded}");
        let signed =
            mask_url("https://s3.test/o?X-Amz-Signature=sig&X-Amz-Credential=id&password=pw");
        assert_eq!(
            signed,
            "https://s3.test/o?X-Amz-Signature=[masked]&X-Amz-Credential=[masked]&password=[masked]"
        );
        let only_user = mask_url("https://tok@api.test/");
        assert_eq!(only_user, "https://[masked]@api.test/");
    }

    #[test]
    fn an_unparsable_url_keeps_only_its_scheme() {
        assert_eq!(
            mask_url("http://u:p@exa mple.test/p?token=abc"),
            "http://[unparsable url]"
        );
        // An invalid port: a fallback that kept the text before the last `@` would leak.
        assert_eq!(
            mask_url("https://user:abc/def@host/"),
            "https://[unparsable url]"
        );
        assert_eq!(mask_url("notaurl?api_key=SECRET://x"), "[unparsable url]");
        assert_eq!(mask_url("not a url token=abc"), "[unparsable url]");
        assert_eq!(mask_url(""), "[unparsable url]");
    }

    #[test]
    fn query_parameters_separate_at_semicolons_too() {
        assert_eq!(
            mask_url("https://h.test/?q=1;token=SECRET&x=2;api_key=K"),
            "https://h.test/?q=1;token=[masked]&x=2;api_key=[masked]"
        );
    }

    #[test]
    fn masked_header_names_are_capped_in_number_and_length() {
        let names: Vec<String> = (0..200).map(|index| format!("x-token-{index}")).collect();
        let long = "k".repeat(10_000);
        let payload = Payload::meta(Value::Null)
            .with_masked(names.iter().cloned().chain([long.clone()]).collect());
        let record = capture(&payload, 32, u64::MAX, 16);
        assert_eq!(record.masked_headers.len(), MAX_MASKED_HEADERS);
        assert!(record.truncated);
        let long_record = capture(
            &Payload::meta(Value::Null).with_masked(vec![long]),
            32,
            u64::MAX,
            16,
        );
        assert!(long_record.masked_headers[0].len() < 200);
        assert!(
            payload_cost(&record)
                >= record.masked_headers.iter().map(String::len).sum::<usize>() as u64,
            "the names are charged"
        );
    }

    #[test]
    fn a_body_that_ends_mid_character_is_binary_unless_it_was_cut() {
        let payload = Payload::meta(Value::Null).with_body(b"a\xC3");
        let whole = capture(&payload, 16, u64::MAX, 16);
        assert!(matches!(whole.body, Some(BodyCopy::Base64(_))));
        assert!(!whole.truncated);
        let longer = Payload::meta(Value::Null).with_body(b"a\xC3b");
        let cut = capture(&longer, 16, u64::MAX, 2);
        assert!(cut.truncated);
        assert_eq!(cut.body, Some(BodyCopy::Text("a".into())));
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
        let record = capture(&payload, 1024, u64::MAX, 10);
        assert!(record.truncated);
        assert_eq!(record.body, Some(BodyCopy::Text("a".repeat(10))));
        let whole = capture(&payload, 1024, u64::MAX, 1000);
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
            capture(&payload, 16, u64::MAX, 16).body,
            Some(BodyCopy::Base64(_))
        ));
    }

    /// Whatever the room, a body the allowance lets through is charged within it: no copy
    /// is made only to be dropped for the budget.
    fn assert_a_kept_body_fits_its_room(payload: &Payload<'_>, max_meta: usize, rooms: u64) {
        let mut kept_somewhere = false;
        for room in 0..rooms {
            let record = capture(payload, max_meta, room, usize::MAX);
            if record.body.is_some() {
                kept_somewhere = true;
                assert!(
                    payload_cost(&record) <= room,
                    "room {room}: the record costs {}",
                    payload_cost(&record)
                );
            }
        }
        assert!(
            kept_somewhere,
            "the sweep must reach rooms that keep a body"
        );
    }

    #[test]
    fn a_whole_body_ending_mid_character_is_sized_as_base64() {
        let payload = Payload::meta(Value::Null).with_body(b"abcdefgh\xC3");
        assert_a_kept_body_fits_its_room(&payload, 16, 200);
    }

    #[test]
    fn a_cut_inside_the_first_character_is_sized_as_base64() {
        let payload = Payload::meta(Value::Null).with_body("éééééééé".as_bytes());
        assert_a_kept_body_fits_its_room(&payload, 16, 200);
    }

    #[test]
    fn masked_names_are_sized_as_capped_not_as_given() {
        let payload = Payload::meta(Value::Null)
            .with_body(&[b'x'; 200])
            .with_masked(vec!["k".repeat(10_000), "x-token".into()]);
        assert_a_kept_body_fits_its_room(&payload, 32, 500);
    }

    #[test]
    fn a_meta_string_just_over_its_cap_is_sized_as_capped() {
        let payload =
            Payload::meta(serde_json::json!({ "u": "a".repeat(33) })).with_body(&[b'x'; 200]);
        assert_a_kept_body_fits_its_room(&payload, 32, 500);
    }

    #[test]
    fn the_raw_body_cap_is_not_shrunk_by_the_base64_expansion() {
        let body = vec![0xffu8; 900 * 1024];
        let payload = Payload::meta(Value::Null).with_body(&body);
        let record = capture(&payload, 16, 8 * 1024 * 1024, 1024 * 1024);
        assert!(!record.truncated);
        let Some(BodyCopy::Base64(text)) = &record.body else {
            panic!("a binary body is kept as base64: {:?}", record.body);
        };
        let kept = base64::engine::general_purpose::STANDARD.decode(text);
        assert_eq!(kept.map(|bytes| bytes.len()).ok(), Some(body.len()));
        let cut = capture(&payload, 16, 8 * 1024 * 1024, 1024);
        assert!(cut.truncated, "the cap still bounds the raw copy");
    }

    #[test]
    fn a_digest_only_record_costs_what_the_constant_says() {
        let record = capture(
            &Payload::meta(serde_json::json!({ "a": 1 })).with_body(b"body"),
            16,
            u64::MAX,
            16,
        );
        let bare = record.digest_only();
        assert!(bare.body.is_none() && bare.masked_headers.is_empty());
        assert_eq!(payload_cost(&bare), DIGEST_ONLY_COST);
    }

    #[test]
    fn a_cut_inside_a_character_keeps_the_text_before_it() {
        let text = "aé".as_bytes();
        let payload = Payload::meta(Value::Null).with_body(text);
        assert_eq!(
            capture(&payload, 16, u64::MAX, 2).body,
            Some(BodyCopy::Text("a".into()))
        );
    }
}
