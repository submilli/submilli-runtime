//! KTD19's redaction: every known secret value is cut out of what the store writes,
//! verbatim and in its base64, URL-encoded, and JSON-escaped forms.
//!
//! The runtime already keeps `secrets.get` results out of call records and masks
//! credential-bearing headers before it copies a payload. What it cannot do is find a
//! secret's value inside a body, a decision's context, or console output, because it
//! does not know the values. The playground does: it learns every value a run could
//! read (the secret store's answers, the harness secrets a request supplied, and the
//! development values `bind` will hold), and this pass replaces each occurrence before
//! anything reaches disk.
//!
//! Only values are redacted, never object keys: a record's keys are its field names, and
//! a key that changed would leave the record unreadable or collide with another. A value
//! the record's own format fixes (an enum tag such as `call-started`, a counter) that a
//! secret happens to match is left as it is, since the format makes it public anyway; see
//! [`KnownSecrets::redact_record`].

use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError, RwLock};

use aho_corasick::{AhoCorasick, Input, MatchKind};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use submilli_server::{SecretStore, SecretStoreError};

/// What a redacted secret reads as in the store.
pub(crate) const REDACTED: &str = "[redacted]";

/// Shortest value treated as a secret. Shorter ones would cut ordinary words and enum
/// tags out of every record; a credential is never this short.
pub(crate) const MIN_SECRET_BYTES: usize = 6;

/// The secret values the playground knows, shared by everything that writes the store.
/// Held only in memory: the set itself is never written anywhere.
#[derive(Clone, Default)]
pub(crate) struct KnownSecrets {
    inner: Arc<RwLock<Patterns>>,
}

#[derive(Default)]
struct Patterns {
    values: BTreeSet<String>,
    /// Every form of every value, longest first.
    forms: Vec<Vec<u8>>,
    /// One automaton over every form, matching the longest form at each place. `None`
    /// only when building it failed (a pattern set beyond its size limits); the forms are
    /// then searched one by one.
    matcher: Option<AhoCorasick>,
}

/// A record with every known secret cut out: the record read back from its redacted
/// form, and that form.
pub(crate) struct Redacted<T> {
    pub(crate) record: T,
    pub(crate) value: Value,
}

/// The most groups of changed values tried one by one when a redacted record does not
/// read back, bounding the work a record whose format collides with a secret costs.
const MAX_PROBED_SHAPES: usize = 64;

/// The marker the runtime's recorder ends a cut context string with:
/// `…[truncated, <kept> of <total> bytes kept]`.
const CUT_MARKER_START: &str = "…[truncated, ";
const CUT_MARKER_END: &str = " bytes kept]";

impl KnownSecrets {
    /// Learns a value. Values shorter than [`MIN_SECRET_BYTES`] are ignored.
    pub(crate) fn add(&self, value: &str) {
        if value.len() < MIN_SECRET_BYTES {
            return;
        }
        let mut patterns = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        if !patterns.values.insert(value.to_owned()) {
            return;
        }
        let mut forms: BTreeSet<Vec<u8>> = patterns.forms.drain(..).collect();
        forms.extend(encoded_forms(value));
        let mut forms: Vec<Vec<u8>> = forms.into_iter().collect();
        forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
        patterns.matcher = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&forms)
            .ok();
        patterns.forms = forms;
    }

    pub(crate) fn add_all<'a>(&self, values: impl IntoIterator<Item = &'a String>) {
        for value in values {
            self.add(value);
        }
    }

    /// `text` with every known secret cut out.
    pub(crate) fn redact_text(&self, text: &str) -> String {
        let patterns = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        patterns.redact_str(text, false)
    }

    /// `record` serialized with every known secret cut out of its values, and read back.
    ///
    /// Every string value is redacted, and every number that is a known secret becomes
    /// [`REDACTED`]. A string the runtime cut at a cap (a context string ending in its
    /// truncation marker, or the body copy of a payload marked truncated) also loses a
    /// trailing start of any form of a secret, at least [`MIN_SECRET_BYTES`] long, that
    /// the cut left. A base64 body copy (`{"encoding": "base64", "data": ...}`) is decoded,
    /// redacted as bytes, and encoded again, since a secret inside binary data does not
    /// appear in its base64 text at a fixed alignment.
    ///
    /// When the redacted form does not read back as `T`, a changed value was one the
    /// format fixes: an enum tag, or a number in a typed field. Changes are grouped by
    /// where they sit in the record (the path of field names, any array index), each group
    /// is tried alone, and the groups that break reading are left unredacted: their values
    /// are the format's own words and counters, public in every record. An error when even
    /// that does not read back, or the record does not serialize; the caller then writes
    /// nothing rather than something unredacted.
    pub(crate) fn redact_record<T: Serialize + DeserializeOwned>(
        &self,
        record: &T,
    ) -> Result<Redacted<T>, String> {
        let serialize = || serde_json::to_value(record).map_err(|error| error.to_string());
        let patterns = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        let mut value = serialize()?;
        let changed = patterns.redact_tree(&mut value, &BTreeSet::new());
        if let Ok(record) = T::deserialize(&value) {
            return Ok(Redacted { record, value });
        }
        if changed.len() > MAX_PROBED_SHAPES {
            return Err(format!(
                "redacting its secrets changed {} kinds of field and left it unreadable",
                changed.len()
            ));
        }
        let mut fixed = BTreeSet::new();
        for shape in &changed {
            let mut others = changed.clone();
            others.remove(shape);
            let mut probe = serialize()?;
            patterns.redact_tree(&mut probe, &others);
            if T::deserialize(&probe).is_err() {
                fixed.insert(shape.clone());
            }
        }
        let mut value = serialize()?;
        patterns.redact_tree(&mut value, &fixed);
        match T::deserialize(&value) {
            Ok(record) => Ok(Redacted { record, value }),
            Err(error) => Err(format!("redacting its secrets left it unreadable: {error}")),
        }
    }
}

impl Patterns {
    fn contains(&self, haystack: &[u8]) -> bool {
        if self.forms.is_empty() {
            return false;
        }
        if let Some(matcher) = &self.matcher
            && let Ok(mut matches) = matcher.try_find_iter(Input::new(haystack))
        {
            return matches.next().is_some();
        }
        self.forms.iter().any(|form| find(haystack, form).is_some())
    }

    /// `bytes` with every form cut out, the longest form first where two overlap.
    fn redact_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        if let Some(matcher) = &self.matcher
            && let Ok(matches) = matcher.try_find_iter(Input::new(bytes))
        {
            let mut out = Vec::with_capacity(bytes.len());
            let mut kept = 0;
            for found in matches {
                out.extend_from_slice(bytes.get(kept..found.start()).unwrap_or_default());
                out.extend_from_slice(REDACTED.as_bytes());
                kept = found.end();
            }
            out.extend_from_slice(bytes.get(kept..).unwrap_or_default());
            return out;
        }
        let mut current = bytes.to_vec();
        for form in &self.forms {
            if find(&current, form).is_none() {
                continue;
            }
            let mut out = Vec::with_capacity(current.len());
            let mut rest = current.as_slice();
            while let Some(at) = find(rest, form) {
                out.extend_from_slice(rest.get(..at).unwrap_or_default());
                out.extend_from_slice(REDACTED.as_bytes());
                rest = rest
                    .get(at.saturating_add(form.len())..)
                    .unwrap_or_default();
            }
            out.extend_from_slice(rest);
            current = out;
        }
        current
    }

    /// `bytes` redacted, and when `cut`, also without a trailing start of any form that
    /// the cut left (at least [`MIN_SECRET_BYTES`] of it).
    fn redact_cut_bytes(&self, bytes: &[u8], cut: bool) -> Vec<u8> {
        let mut out = self.redact_bytes(bytes);
        if cut {
            let tail = self.cut_form_len(&out);
            if tail > 0 {
                out.truncate(out.len().saturating_sub(tail));
                out.extend_from_slice(REDACTED.as_bytes());
            }
        }
        out
    }

    /// The length of the longest start of a form, shorter than the form and at least
    /// [`MIN_SECRET_BYTES`], that `bytes` ends with; 0 for none.
    fn cut_form_len(&self, bytes: &[u8]) -> usize {
        let mut longest = 0;
        for form in &self.forms {
            let most = form.len().saturating_sub(1).min(bytes.len());
            for len in (MIN_SECRET_BYTES.max(longest + 1)..=most).rev() {
                if form.get(..len).is_some_and(|start| bytes.ends_with(start)) {
                    longest = len;
                    break;
                }
            }
        }
        longest
    }

    /// `text` redacted. A string ending in the runtime's truncation marker is treated as
    /// cut where the marker starts; otherwise `cut` says whether its end is a cut.
    fn redact_str(&self, text: &str, cut: bool) -> String {
        let (kept, marker, cut) = match split_cut_marker(text) {
            Some((kept, marker)) => (kept, marker, true),
            None => (text, "", cut),
        };
        if !cut && !self.contains(kept.as_bytes()) {
            return text.to_owned();
        }
        let cleaned = self.redact_cut_bytes(kept.as_bytes(), cut);
        // Every form is cut whole and the marker is ASCII, but a non-UTF-8 form (none is
        // today) could split a character; the lossy conversion keeps the result a string.
        let mut cleaned = String::from_utf8(cleaned)
            .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
        cleaned.push_str(marker);
        cleaned
    }

    /// Redacts the values in `value` in place, except those at a shape in `skip`, and
    /// returns the shapes of the values it changed.
    fn redact_tree(&self, value: &mut Value, skip: &BTreeSet<String>) -> BTreeSet<String> {
        let mut walk = Walk {
            patterns: self,
            skip,
            path: Vec::new(),
            changed: BTreeSet::new(),
        };
        if !self.forms.is_empty() {
            walk.value(value, false);
        }
        walk.changed
    }
}

/// One pass of [`Patterns::redact_tree`]: where it is, and what it changed.
struct Walk<'a> {
    patterns: &'a Patterns,
    skip: &'a BTreeSet<String>,
    /// Field names from the root; `*` for an array index.
    path: Vec<String>,
    changed: BTreeSet<String>,
}

impl Walk<'_> {
    fn shape(&self) -> String {
        self.path.join("\u{1f}")
    }

    /// Replaces `value` with `redacted` unless its shape is skipped.
    fn replace(&mut self, value: &mut Value, redacted: Value) {
        let shape = self.shape();
        if !self.skip.contains(&shape) {
            *value = redacted;
            self.changed.insert(shape);
        }
    }

    /// `cut`: `value` is a string the runtime cut at its end, or a body copy holding one.
    fn value(&mut self, value: &mut Value, cut: bool) {
        match value {
            Value::String(text) => {
                let redacted = self.patterns.redact_str(text, cut);
                if redacted != *text {
                    self.replace(value, Value::String(redacted));
                }
            }
            Value::Number(number) => {
                if self.patterns.values.contains(&number.to_string()) {
                    self.replace(value, Value::String(REDACTED.to_owned()));
                }
            }
            Value::Array(items) => {
                self.path.push("*".to_owned());
                for item in items {
                    self.value(item, cut);
                }
                self.path.pop();
            }
            Value::Object(fields) => {
                let copy = body_copy_encoding(fields);
                // A payload record whose copy was cut: its body ends at the cap.
                let payload_cut = fields.get("truncated") == Some(&Value::Bool(true));
                for (key, item) in fields.iter_mut() {
                    self.path.push(key.clone());
                    match (copy, key.as_str()) {
                        (Some(Encoding::Base64), "data") => self.base64_copy(item, cut),
                        (Some(Encoding::Text), "data") => self.value(item, cut),
                        (None, "body") => self.value(item, payload_cut),
                        _ => self.value(item, false),
                    }
                    self.path.pop();
                }
            }
            Value::Null | Value::Bool(_) => {}
        }
    }

    /// A base64 body copy's data, redacted through its decoded bytes; as text when it
    /// does not decode.
    fn base64_copy(&mut self, data: &mut Value, cut: bool) {
        let Value::String(text) = data else {
            return self.value(data, cut);
        };
        let Ok(decoded) = STANDARD.decode(text.as_bytes()) else {
            return self.value(data, cut);
        };
        let cleaned = self.patterns.redact_cut_bytes(&decoded, cut);
        // The text may also carry a secret's own base64 form at the copy's alignment.
        let encoded = self.patterns.redact_str(&STANDARD.encode(&cleaned), false);
        if encoded != *text {
            self.replace(data, Value::String(encoded));
        }
    }
}

#[derive(Clone, Copy)]
enum Encoding {
    Text,
    Base64,
}

/// How `fields` holds a body copy (`{"encoding": "text" | "base64", "data": ...}`), if it
/// is one.
fn body_copy_encoding(fields: &serde_json::Map<String, Value>) -> Option<Encoding> {
    if fields.len() != 2 || !fields.contains_key("data") {
        return None;
    }
    match fields.get("encoding").and_then(Value::as_str) {
        Some("text") => Some(Encoding::Text),
        Some("base64") => Some(Encoding::Base64),
        _ => None,
    }
}

/// `text` split before the runtime's truncation marker, when it ends in one.
fn split_cut_marker(text: &str) -> Option<(&str, &str)> {
    let at = text.rfind(CUT_MARKER_START)?;
    let marker = text.get(at..)?;
    let counts = marker
        .strip_prefix(CUT_MARKER_START)?
        .strip_suffix(CUT_MARKER_END)?;
    let (kept, total) = counts.split_once(" of ")?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(kept) || !digits(total) {
        return None;
    }
    Some((text.get(..at)?, marker))
}

/// The developer's secret store, as the playground's server reads it: every value it
/// answers or is given (a refreshed OAuth token, say) becomes a known secret before the
/// run that asked for it can record it anywhere.
pub(crate) struct WatchedSecretStore {
    inner: Arc<dyn SecretStore>,
    known: KnownSecrets,
}

impl WatchedSecretStore {
    pub(crate) fn new(inner: Arc<dyn SecretStore>, known: KnownSecrets) -> Self {
        Self { inner, known }
    }
}

#[async_trait::async_trait]
impl SecretStore for WatchedSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        let value = self.inner.get(key).await?;
        if let Some(value) = &value {
            self.known.add(value);
        }
        Ok(value)
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        self.known.add(value);
        self.inner.put(key, value).await
    }

    async fn delete(&self, key: &str) -> Result<bool, SecretStoreError> {
        self.inner.delete(key).await
    }

    async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretStoreError> {
        self.inner.list(prefix).await
    }
}

/// The forms a secret can take in a record: as is; base64 (standard and URL-safe, with
/// and without padding); URL-encoded (strict percent-encoding and form encoding, with
/// upper- and lowercase hex); and JSON-escaped inside a text body (the standard escapes,
/// with `/` as `\/`, and with characters as `\uXXXX` the ways common encoders write them).
fn encoded_forms(value: &str) -> BTreeSet<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut forms = BTreeSet::new();
    forms.insert(bytes.to_vec());
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        forms.insert(engine.encode(bytes).into_bytes());
    }
    let percent = utf8_percent_encode(value, NON_ALPHANUMERIC).to_string();
    let form: String = url::form_urlencoded::byte_serialize(bytes).collect();
    for encoded in [percent, form] {
        forms.insert(lowercase_percent_hex(&encoded).into_bytes());
        forms.insert(encoded.into_bytes());
    }
    // The escapes serde_json and most encoders write.
    let json = serde_json::to_string(value).unwrap_or_default();
    let json = json
        .strip_prefix('"')
        .and_then(|json| json.strip_suffix('"'))
        .unwrap_or(value);
    forms.insert(json.replace('/', "\\/").into_bytes());
    forms.insert(json.as_bytes().to_vec());
    // `\uXXXX` for every character but letters and digits; the same keeping `_`, `-`,
    // and `.`; for every character outside ASCII (Python's `ensure_ascii`); and for `<`,
    // `>`, and `&` (Go's HTML-safe JSON).
    let every = |c: char| !c.is_ascii_alphanumeric();
    let punctuation = |c: char| !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-' | '.');
    let non_ascii = |c: char| !c.is_ascii();
    let html = |c: char| matches!(c, '<' | '>' | '&');
    for escape in [every as fn(char) -> bool, punctuation, non_ascii, html] {
        for upper in [false, true] {
            forms.insert(unicode_escaped(json, escape, upper).into_bytes());
        }
    }
    forms.retain(|form| !form.is_empty());
    forms
}

/// `encoded` with each `%XX` escape's hex digits in lowercase.
fn lowercase_percent_hex(encoded: &str) -> String {
    let mut out = String::with_capacity(encoded.len());
    let mut hex_left = 0;
    for c in encoded.chars() {
        if hex_left > 0 {
            out.push(c.to_ascii_lowercase());
            hex_left -= 1;
        } else {
            if c == '%' {
                hex_left = 2;
            }
            out.push(c);
        }
    }
    out
}

/// `json` (already JSON-escaped text) with each character `escape` picks written as
/// `\uXXXX` (two of them, a surrogate pair, outside the Basic Multilingual Plane). An
/// existing escape is left as it is.
fn unicode_escaped(json: &str, escape: impl Fn(char) -> bool, upper: bool) -> String {
    let mut out = String::with_capacity(json.len().saturating_mul(2));
    let mut chars = json.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(c);
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if !escape(c) {
            out.push(c);
            continue;
        }
        let mut units = [0_u16; 2];
        for unit in c.encode_utf16(&mut units) {
            if upper {
                out.push_str(&format!("\\u{unit:04X}"));
            } else {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    const SECRET: &str = "sk_live_9f8e7d+/=?&";

    fn known() -> KnownSecrets {
        let known = KnownSecrets::default();
        known.add(SECRET);
        known
    }

    fn redact(known: &KnownSecrets, value: Value) -> Value {
        known.redact_record(&value).unwrap().value
    }

    #[test]
    fn every_form_of_a_secret_is_cut_from_text() {
        let known = known();
        let percent = utf8_percent_encode(SECRET, NON_ALPHANUMERIC).to_string();
        let forms = [
            SECRET.to_owned(),
            STANDARD.encode(SECRET),
            STANDARD_NO_PAD.encode(SECRET),
            URL_SAFE_NO_PAD.encode(SECRET),
            percent.clone(),
            percent.to_ascii_lowercase(),
            url::form_urlencoded::byte_serialize(SECRET.as_bytes()).collect(),
            // JSON-escaped inside a text body: `\/`, and `\u` escapes.
            SECRET.replace('/', "\\/"),
            "sk_live_9f8e7d\\u002b\\u002f\\u003d\\u003f\\u0026".to_owned(),
            "sk_live_9f8e7d+/=?\\u0026".to_owned(),
        ];
        for form in forms {
            let text = format!("before {form} after");
            assert_eq!(
                known.redact_text(&text),
                format!("before {REDACTED} after"),
                "{form}"
            );
        }
    }

    #[test]
    fn a_secret_inside_a_binary_body_copy_is_cut_at_any_alignment() {
        let known = known();
        for pad in 0..3 {
            let mut body = vec![0xff_u8; pad];
            body.extend_from_slice(SECRET.as_bytes());
            body.push(0xfe);
            let value = redact(
                &known,
                serde_json::json!({
                    "encoding": "base64",
                    "data": STANDARD.encode(&body),
                }),
            );
            let data = STANDARD.decode(value["data"].as_str().unwrap()).unwrap();
            assert!(find(&data, SECRET.as_bytes()).is_none());
            assert!(find(&data, REDACTED.as_bytes()).is_some(), "pad {pad}");
        }
    }

    #[test]
    fn values_are_redacted_but_keys_are_not() {
        let known = known();
        let value = redact(
            &known,
            serde_json::json!({
                SECRET: [1, {"inner": format!("x{SECRET}y")}],
                "n": 5,
            }),
        );
        assert_eq!(
            value,
            serde_json::json!({ SECRET: [1, {"inner": format!("x{REDACTED}y")}], "n": 5 })
        );
    }

    #[test]
    fn a_number_equal_to_a_numeric_secret_is_redacted() {
        let known = KnownSecrets::default();
        known.add("4242424242");
        let value = redact(
            &known,
            serde_json::json!({ "pin": 4_242_424_242_u64, "n": 42, "s": "x4242424242" }),
        );
        assert_eq!(
            value,
            serde_json::json!({ "pin": REDACTED, "n": 42, "s": format!("x{REDACTED}") })
        );
    }

    #[test]
    fn a_secret_cut_by_a_context_cap_loses_what_the_cap_kept_of_it() {
        let known = known();
        let kept = format!("Bearer {}", &SECRET[..10]);
        let capped = format!("{kept}…[truncated, {} of 400 bytes kept]", kept.len());
        let value = redact(&known, serde_json::json!({ "header": capped }));
        assert_eq!(
            value["header"],
            format!(
                "Bearer {REDACTED}…[truncated, {} of 400 bytes kept]",
                kept.len()
            )
        );
        // An uncut string keeps an ending that merely looks like a secret's start.
        let value = redact(&known, serde_json::json!({ "s": &SECRET[..10] }));
        assert_eq!(value["s"], &SECRET[..10]);
    }

    #[test]
    fn a_secret_cut_by_a_body_cap_loses_what_the_cap_kept_of_it() {
        let known = known();
        let cut_text = format!("{{\"token\":\"{}", &SECRET[..12]);
        let mut cut_binary = vec![0xff_u8, 0x00];
        cut_binary.extend_from_slice(&SECRET.as_bytes()[..8]);
        for (body, truncated) in [
            (
                serde_json::json!({"encoding": "text", "data": cut_text}),
                true,
            ),
            (
                serde_json::json!({"encoding": "base64", "data": STANDARD.encode(&cut_binary)}),
                true,
            ),
            (
                serde_json::json!({"encoding": "text", "data": cut_text}),
                false,
            ),
        ] {
            let value = redact(
                &known,
                serde_json::json!({ "meta": null, "body": body, "truncated": truncated }),
            );
            let data = value["body"]["data"].as_str().unwrap().to_owned();
            let data = if value["body"]["encoding"] == "base64" {
                STANDARD.decode(&data).unwrap()
            } else {
                data.into_bytes()
            };
            assert_eq!(
                data.ends_with(REDACTED.as_bytes()),
                truncated,
                "{}",
                String::from_utf8_lossy(&data)
            );
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case", tag = "kind")]
    enum Probe {
        CallStarted {
            capability: String,
            started_micros: u64,
            context: Value,
        },
    }

    #[test]
    fn values_the_format_fixes_are_left_and_the_record_still_reads() {
        let known = KnownSecrets::default();
        known.add("started");
        known.add("1234567");
        let probe = Probe::CallStarted {
            capability: "restarted".into(),
            started_micros: 1_234_567,
            context: serde_json::json!({ "n": 1_234_567, "s": "started" }),
        };
        let redacted = known.redact_record(&probe).unwrap();
        assert_eq!(
            redacted.record,
            Probe::CallStarted {
                capability: format!("re{REDACTED}"),
                started_micros: 1_234_567,
                context: serde_json::json!({ "n": REDACTED, "s": REDACTED }),
            }
        );
        assert_eq!(redacted.value["kind"], "call-started");
    }

    #[test]
    fn short_values_are_not_treated_as_secrets() {
        let known = KnownSecrets::default();
        known.add("abc");
        known.add("");
        assert_eq!(known.redact_text("abc kind"), "abc kind");
    }
}
