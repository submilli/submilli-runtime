//! KTD19's redaction: every known secret value is cut out of what the store writes,
//! verbatim and in its base64, URL-encoded, and JSON-escaped forms. Where the
//! occurrences of two secrets overlap, the whole span they cover is cut.
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
//!
//! Two lengths bound what is found. A value shorter than [`MIN_SECRET_BYTES`] (6 bytes)
//! is not learned at all, and neither is a value of only whitespace. Inside longer base64
//! text, a secret's base64 is matched only by the part of it that does not depend on
//! the bytes around it, and that part must be at least [`MIN_SHIFTED_BASE64_CHARS`]
//! (8 characters) long. A 6-byte secret's part is that long only when the secret starts
//! on a three-byte boundary of the encoded data (alignment 0): such a secret is found
//! base64-encoded on its own or after a multiple of three bytes, but not inside
//! `btoa("user:" + secret)`. From 7 bytes on, every alignment is found.

use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::{Arc, PoisonError, RwLock};

use aho_corasick::{AhoCorasick, Input, MatchKind};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use submilli_server::{SecretStore, SecretStoreError};

/// What a redacted secret reads as in the store.
pub(crate) const REDACTED: &str = "[redacted]";

/// Shortest value treated as a secret. Shorter ones would cut ordinary words and enum
/// tags out of every record; a credential is never this short.
pub(crate) const MIN_SECRET_BYTES: usize = 6;

/// Shortest part of a secret's base64 at another alignment treated as a form of it: as
/// long as the base64 of the shortest secret.
const MIN_SHIFTED_BASE64_CHARS: usize = 8;

/// RFC 3986's unreserved characters, which most URL encoders leave as they are.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// What JavaScript's `encodeURIComponent` leaves as it is.
const URI_COMPONENT: &AsciiSet = &UNRESERVED
    .remove(b'!')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*');

/// What Python's `urllib.parse.quote` leaves as it is by default.
const URL_PATH: &AsciiSet = &UNRESERVED.remove(b'/');

/// The secret values the playground knows, shared by everything that writes the store.
/// Held only in memory: the set itself is never written anywhere.
#[derive(Clone, Default)]
pub(crate) struct KnownSecrets {
    inner: Arc<RwLock<Patterns>>,
}

#[derive(Default)]
struct Patterns {
    values: BTreeSet<String>,
    /// Every form of every value, longest first, any percent escape's hex in uppercase.
    forms: Vec<Vec<u8>>,
    /// One automaton over every form, reporting every match, overlapping ones too. `None`
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

/// The most shapes of changed values tried one by one when a redacted record does not
/// read back, bounding the work a record whose format collides with a secret costs. A
/// value's shape is where it sits in its record: the path of field names to it, with `*`
/// for any array index.
const MAX_PROBED_SHAPES: usize = 64;

/// The marker the runtime's recorder ends a cut context string with:
/// `…[truncated, <kept> of <total> bytes kept]`.
const CUT_MARKER_START: &str = "…[truncated, ";
const CUT_MARKER_END: &str = " bytes kept]";

impl KnownSecrets {
    /// Learns a value, and its trimmed form too when that differs (a secret file's
    /// trailing newline, say, that a program strips before using it). A value, or trimmed
    /// form, shorter than [`MIN_SECRET_BYTES`] is ignored, and so is a value of only
    /// whitespace: redacting it would cut ordinary indentation.
    pub(crate) fn add(&self, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        let mut patterns = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        let mut learned = Vec::new();
        for candidate in [value, trimmed] {
            if candidate.len() >= MIN_SECRET_BYTES && patterns.values.insert(candidate.to_owned()) {
                learned.push(candidate);
            }
        }
        if learned.is_empty() {
            return;
        }
        let mut forms: BTreeSet<Vec<u8>> = patterns.forms.drain(..).collect();
        for candidate in learned {
            forms.extend(encoded_forms(candidate));
        }
        let mut forms: Vec<Vec<u8>> = forms.into_iter().collect();
        forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
        patterns.matcher = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
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
        patterns.redact_str(text, Ending::Whole)
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
    /// format fixes: an enum tag, or a number in a typed field. The changed values'
    /// shapes (see [`MAX_PROBED_SHAPES`]) are each tried alone, and the shapes whose
    /// redaction breaks reading are left unredacted: their values are the format's own
    /// words and counters, public in every record. No record type puts user data at a
    /// shape it also gives a tag or a typed number, so this never keeps user data. An
    /// error when even that does not read back, when more than [`MAX_PROBED_SHAPES`]
    /// shapes changed, or when the record does not serialize; the caller then writes
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
                "redacting its secrets changed values of {} shapes, more than the {} probed, \
                 and left it unreadable",
                changed.len(),
                MAX_PROBED_SHAPES
            ));
        }
        let mut format_owned = BTreeSet::new();
        for shape in &changed {
            // Redact this shape alone: skip every other changed one.
            let every_other_shape: BTreeSet<String> = changed
                .iter()
                .filter(|other| *other != shape)
                .cloned()
                .collect();
            let mut probe = serialize()?;
            patterns.redact_tree(&mut probe, &every_other_shape);
            if T::deserialize(&probe).is_err() {
                format_owned.insert(shape.clone());
            }
        }
        let mut value = serialize()?;
        patterns.redact_tree(&mut value, &format_owned);
        match T::deserialize(&value) {
            Ok(record) => Ok(Redacted { record, value }),
            Err(error) => Err(format!("redacting its secrets left it unreadable: {error}")),
        }
    }
}

impl Patterns {
    fn contains(&self, haystack: &[u8]) -> bool {
        !self.spans(haystack).is_empty()
    }

    /// Where `haystack` holds a form, the spans of overlapping or adjacent matches merged,
    /// in order. A percent escape matches in either hex case.
    fn spans(&self, haystack: &[u8]) -> Vec<Range<usize>> {
        if self.forms.is_empty() {
            return Vec::new();
        }
        let mut found = Vec::new();
        self.find_matches(haystack, &mut found);
        // The forms' escapes are in uppercase; the same text with the haystack's in
        // uppercase too has the same length, so its matches are spans of the haystack.
        if let Some(uppercased) = uppercase_percent_hex(haystack) {
            self.find_matches(&uppercased, &mut found);
        }
        merge_spans(found)
    }

    /// Every match of every form in `haystack`, overlapping ones included.
    fn find_matches(&self, haystack: &[u8], found: &mut Vec<Range<usize>>) {
        if let Some(matcher) = &self.matcher
            && let Ok(matches) = matcher.try_find_overlapping_iter(Input::new(haystack))
        {
            found.extend(matches.map(|found| found.range()));
            return;
        }
        for form in &self.forms {
            found.extend(find_all(haystack, form));
        }
    }

    /// `bytes` with every span a form covers cut out.
    fn redact_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        let mut kept = 0;
        for span in self.spans(bytes) {
            out.extend_from_slice(bytes.get(kept..span.start).unwrap_or_default());
            out.extend_from_slice(REDACTED.as_bytes());
            kept = span.end;
        }
        out.extend_from_slice(bytes.get(kept..).unwrap_or_default());
        out
    }

    /// `bytes` redacted, and when they end at a cut, also without a trailing start of any
    /// form that the cut left (at least [`MIN_SECRET_BYTES`] of it).
    fn redact_cut_bytes(&self, bytes: &[u8], ending: Ending) -> Vec<u8> {
        let mut out = self.redact_bytes(bytes);
        if ending == Ending::Cut {
            let tail = match uppercase_percent_hex(&out) {
                Some(uppercased) => self.cut_form_len(&out).max(self.cut_form_len(&uppercased)),
                None => self.cut_form_len(&out),
            };
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
    /// cut where the marker starts; otherwise `ending` says whether its end is a cut.
    fn redact_str(&self, text: &str, ending: Ending) -> String {
        let (kept, marker, ending) = match split_cut_marker(text) {
            Some((kept, marker)) => (kept, marker, Ending::Cut),
            None => (text, "", ending),
        };
        if ending == Ending::Whole && !self.contains(kept.as_bytes()) {
            return text.to_owned();
        }
        let cleaned = self.redact_cut_bytes(kept.as_bytes(), ending);
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
            walk.value(value, Ending::Whole);
        }
        walk.changed
    }
}

/// Whether a string, or a body copy holding one, ends where the runtime cut it at a cap.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Whole,
    Cut,
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

    fn value(&mut self, value: &mut Value, ending: Ending) {
        match value {
            Value::String(text) => {
                let redacted = self.patterns.redact_str(text, ending);
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
                    self.value(item, ending);
                }
                self.path.pop();
            }
            Value::Object(fields) => {
                let copy = body_copy_encoding(fields);
                // A payload record whose copy was cut: its body ends at the cap.
                let body_ending = if fields.get("truncated") == Some(&Value::Bool(true)) {
                    Ending::Cut
                } else {
                    Ending::Whole
                };
                for (key, item) in fields.iter_mut() {
                    self.path.push(key.clone());
                    match (copy, key.as_str()) {
                        (Some(Encoding::Base64), "data") => self.base64_copy(item, ending),
                        (Some(Encoding::Text), "data") => self.value(item, ending),
                        (None, "body") => self.value(item, body_ending),
                        _ => self.value(item, Ending::Whole),
                    }
                    self.path.pop();
                }
            }
            Value::Null | Value::Bool(_) => {}
        }
    }

    /// A base64 body copy's data, redacted through its decoded bytes; as text when it
    /// does not decode.
    fn base64_copy(&mut self, data: &mut Value, ending: Ending) {
        let Value::String(text) = data else {
            return self.value(data, ending);
        };
        let Ok(decoded) = STANDARD.decode(text.as_bytes()) else {
            return self.value(data, ending);
        };
        let cleaned = self.patterns.redact_cut_bytes(&decoded, ending);
        // The text may also carry a secret's own base64 form at the copy's alignment.
        let encoded = self
            .patterns
            .redact_str(&STANDARD.encode(&cleaned), Ending::Whole);
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
/// and without padding, and inside longer base64 text at any alignment); URL-encoded
/// (percent-encoding of every character but letters and digits, or leaving what common
/// encoders leave, with a space as `%20` or `+`; any escape's hex matches in either case,
/// see [`Patterns::spans`]); and JSON-escaped inside a text body (the standard escapes,
/// with `/` as `\/`, and with characters as `\uXXXX` the ways common encoders write them).
fn encoded_forms(value: &str) -> BTreeSet<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut forms = BTreeSet::new();
    forms.insert(bytes.to_vec());
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        forms.insert(engine.encode(bytes).into_bytes());
    }
    forms.extend(base64_inside_longer_text(bytes));
    for set in [NON_ALPHANUMERIC, UNRESERVED, URI_COMPONENT, URL_PATH] {
        let percent = utf8_percent_encode(value, set).to_string();
        forms.insert(percent.replace("%20", "+").into_bytes());
        forms.insert(percent.into_bytes());
    }
    forms.insert(
        url::form_urlencoded::byte_serialize(bytes)
            .collect::<String>()
            .into_bytes(),
    );
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

/// The part of the base64 of `bytes`, standard and URL-safe, that stays the same
/// wherever they sit inside longer encoded data (`btoa("user:" + secret)`): encoded after
/// zero, one, or two other bytes, without the leading characters those bytes share and
/// the trailing one the next byte would. Parts shorter than [`MIN_SHIFTED_BASE64_CHARS`]
/// are left out.
fn base64_inside_longer_text(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut forms = Vec::new();
    for engine in [&STANDARD_NO_PAD, &URL_SAFE_NO_PAD] {
        // After `shift` other bytes, the first `shared` characters mix in theirs.
        for (shift, shared) in [(0, 0), (1, 2), (2, 3)] {
            let mut shifted = vec![0_u8; shift];
            shifted.extend_from_slice(bytes);
            let encoded = engine.encode(&shifted);
            // A last group short of three bytes ends in a character the next byte shares.
            let trailing = usize::from(shifted.len() % 3 != 0);
            let end = encoded.len().saturating_sub(trailing);
            if let Some(part) = encoded.get(shared..end)
                && part.len() >= MIN_SHIFTED_BASE64_CHARS
            {
                forms.push(part.as_bytes().to_vec());
            }
        }
    }
    forms
}

/// `bytes` with each `%XX` escape's hex digits in uppercase, when any was lowercase; the
/// same length, so a span of one is a span of the other.
fn uppercase_percent_hex(bytes: &[u8]) -> Option<Vec<u8>> {
    let is_escape = |at: usize| {
        bytes.get(at) == Some(&b'%')
            && bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(at + 2).is_some_and(u8::is_ascii_hexdigit)
    };
    let mut out: Option<Vec<u8>> = None;
    let mut at = 0;
    while at < bytes.len() {
        if !is_escape(at) {
            at += 1;
            continue;
        }
        for digit in [at + 1, at + 2] {
            if bytes.get(digit).is_some_and(u8::is_ascii_lowercase) {
                let out = out.get_or_insert_with(|| bytes.to_vec());
                if let Some(byte) = out.get_mut(digit) {
                    byte.make_ascii_uppercase();
                }
            }
        }
        at += 3;
    }
    out
}

/// Merges overlapping or adjacent spans, in order.
fn merge_spans(mut spans: Vec<Range<usize>>) -> Vec<Range<usize>> {
    spans.sort_by_key(|span| (span.start, span.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    merged
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

/// Every place `needle` occurs in `haystack`, overlapping ones included.
fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    // Each window starts at `at` and is `needle.len()` long, inside `haystack`.
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at..at + needle.len())
        .collect()
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
            assert!(find_all(&data, SECRET.as_bytes()).is_empty());
            assert!(
                !find_all(&data, REDACTED.as_bytes()).is_empty(),
                "pad {pad}"
            );
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
    fn overlapping_secrets_are_cut_as_one_span() {
        let known = KnownSecrets::default();
        known.add("AAAAAAAAXXXXXX");
        known.add("XXXXXXBBBBBBBBBBBB");
        assert_eq!(
            known.redact_text("pre AAAAAAAAXXXXXXBBBBBBBBBBBB post"),
            format!("pre {REDACTED} post")
        );
        // The same the other way round, the second secret starting first.
        assert_eq!(
            known.redact_text("XXXXXXBBBBBBBBBBBB AAAAAAAAXXXXXXBBBBBBBBBBBB"),
            format!("{REDACTED} {REDACTED}")
        );
    }

    #[test]
    fn a_secret_that_starts_another_is_cut_with_the_longer_one() {
        let known = KnownSecrets::default();
        known.add("abcdefgh");
        known.add("abcdefgh12345");
        known.add("12345zyxwvu");
        assert_eq!(
            known.redact_text("x abcdefgh12345zyxwvu y abcdefgh z"),
            format!("x {REDACTED} y {REDACTED} z")
        );
    }

    #[test]
    fn a_secret_base64_encoded_after_other_bytes_is_cut_at_every_alignment() {
        let known = known();
        // `btoa("user:" + secret)`, and the same after one, two, and three bytes, and
        // followed by more text.
        for prefix in ["user:", "u", "us", "usr", ""] {
            for suffix in ["", "\n", ":more"] {
                for engine in [&STANDARD, &URL_SAFE] {
                    let encoded = engine.encode(format!("{prefix}{SECRET}{suffix}"));
                    let text = format!("Basic {encoded}");
                    let redacted = known.redact_text(&text);
                    let rest = redacted.strip_prefix("Basic ").unwrap();
                    let (before, after) = rest
                        .split_once(REDACTED)
                        .unwrap_or_else(|| panic!("{prefix:?} {suffix:?}: {redacted}"));
                    // What is left mixes in the bytes around the secret: at most the
                    // prefix's own characters and one shared one, then the suffix's.
                    let prefix_chars = engine.encode(prefix).trim_end_matches('=').len();
                    assert!(before.len() <= prefix_chars, "{prefix:?}: {redacted}");
                    let suffix_chars = engine.encode(suffix).len() + 4;
                    assert!(after.len() <= suffix_chars, "{suffix:?}: {redacted}");
                }
            }
        }
    }

    #[test]
    fn percent_escapes_match_in_any_hex_case_and_under_encode_uri_component() {
        let known = known();
        for form in [
            "sk_live_9f8e7d%2b%2F%3d%3F%26",
            "sk_live_9f8e7d%2B%2f%3D%3f%26",
            "sk_live_9f8e7d%2b%2f%3d%3f%26",
        ] {
            assert_eq!(
                known.redact_text(&format!("q={form}&x=1")),
                format!("q={REDACTED}&x=1"),
                "{form}"
            );
        }
        // `encodeURIComponent` leaves `~ ! ' ( ) *` as they are.
        let known = KnownSecrets::default();
        known.add("tok~en!'(x)*/=1 ok");
        for form in [
            "tok~en!'(x)*%2F%3D1%20ok",
            "tok~en!'(x)*%2f%3d1%20ok",
            "tok%7Een%21%27%28x%29%2A%2F%3D1%20ok",
            "tok~en%21%27%28x%29%2A%2F%3D1+ok",
        ] {
            assert_eq!(
                known.redact_text(&format!("q={form}&x=1")),
                format!("q={REDACTED}&x=1"),
                "{form}"
            );
        }
    }

    #[test]
    fn short_values_are_not_treated_as_secrets() {
        let known = KnownSecrets::default();
        known.add("abc");
        known.add("");
        assert_eq!(known.redact_text("abc kind"), "abc kind");
    }

    #[test]
    fn a_secret_with_surrounding_whitespace_is_also_cut_trimmed() {
        let known = KnownSecrets::default();
        known.add("tok_abc123\n");
        assert_eq!(
            known.redact_text("using tok_abc123 now"),
            format!("using {REDACTED} now")
        );
        assert_eq!(
            known.redact_text("raw tok_abc123\n"),
            format!("raw {REDACTED}")
        );
        // A trimmed form shorter than the minimum is not learned on its own.
        known.add("  abc  ");
        assert_eq!(known.redact_text("abc"), "abc");
    }

    #[test]
    fn a_whitespace_only_value_is_not_treated_as_a_secret() {
        let known = KnownSecrets::default();
        known.add("      ");
        known.add(" \t\n \r\n  ");
        let text = "fn main() {\n      let x = 1;\n            y();\n}";
        assert_eq!(known.redact_text(text), text);
        let value = redact(&known, serde_json::json!({ "code": text }));
        assert_eq!(value["code"], text);
    }

    #[test]
    fn a_six_byte_secret_is_found_in_base64_only_at_alignment_zero() {
        let known = KnownSecrets::default();
        known.add("s3cr3t");
        let alone = STANDARD.encode("s3cr3t");
        assert_eq!(known.redact_text(&alone), REDACTED);
        let after_three = STANDARD.encode("abcs3cr3t");
        assert!(known.redact_text(&after_three).contains(REDACTED));
        // The documented gap: after one or two other bytes it is not found.
        let shifted = STANDARD.encode("u:s3cr3t");
        assert_eq!(known.redact_text(&shifted), shifted);
    }

    const MULTIBYTE: &str = "pässwörd🔑key";

    #[test]
    fn a_multibyte_secret_is_cut_from_text_and_next_to_other_matches() {
        let known = KnownSecrets::default();
        known.add(MULTIBYTE);
        known.add(SECRET);
        assert_eq!(
            known.redact_text(&format!("é{MULTIBYTE}é 🔑 ok")),
            format!("é{REDACTED}é 🔑 ok")
        );
        // Adjacent to another secret: one span covers both.
        assert_eq!(
            known.redact_text(&format!("ü{MULTIBYTE}{SECRET}ü")),
            format!("ü{REDACTED}ü")
        );
        assert_eq!(
            known.redact_text(&format!("{MULTIBYTE} {MULTIBYTE}")),
            format!("{REDACTED} {REDACTED}")
        );
        let value = redact(&known, serde_json::json!({ "s": format!("→{MULTIBYTE}←") }));
        assert_eq!(value["s"], format!("→{REDACTED}←"));
    }

    #[test]
    fn a_multibyte_secret_is_cut_in_lowercase_percent_and_json_escapes() {
        let known = KnownSecrets::default();
        known.add(MULTIBYTE);
        let mut forms = Vec::new();
        for set in [NON_ALPHANUMERIC, UNRESERVED, URI_COMPONENT, URL_PATH] {
            let percent = utf8_percent_encode(MULTIBYTE, set).to_string();
            assert!(percent.contains("%C3%A4"), "{percent}");
            forms.push(percent.to_ascii_lowercase());
            forms.push(percent);
        }
        // The emoji as a surrogate pair, in either hex case, as Python's `ensure_ascii`.
        forms.push("p\\u00e4ssw\\u00f6rd\\ud83d\\udd11key".to_owned());
        forms.push("p\\u00E4ssw\\u00F6rd\\uD83D\\uDD11key".to_owned());
        for form in forms {
            assert_eq!(
                known.redact_text(&format!("q={form}&x=1")),
                format!("q={REDACTED}&x=1"),
                "{form}"
            );
        }
    }

    #[test]
    fn a_context_cut_inside_a_multibyte_secret_loses_the_kept_start() {
        let known = KnownSecrets::default();
        known.add(MULTIBYTE);
        // Cut at every character boundary inside the secret, the emoji's too.
        for (at, _) in MULTIBYTE.char_indices().skip(1) {
            let kept = format!("é: {}", &MULTIBYTE[..at]);
            let capped = format!("{kept}…[truncated, {} of 400 bytes kept]", kept.len());
            let value = redact(&known, serde_json::json!({ "c": capped }));
            let redacted = value["c"].as_str().unwrap();
            if at >= MIN_SECRET_BYTES {
                assert!(
                    redacted.starts_with(&format!("é: {REDACTED}…")),
                    "{redacted}"
                );
            } else {
                assert_eq!(redacted, capped);
            }
        }
    }

    #[test]
    fn a_secret_that_is_itself_valid_base64_is_cut() {
        let secret = "QWxhZGRpbjpvcGVuU2VzYW1l";
        assert!(STANDARD.decode(secret).is_ok());
        let known = KnownSecrets::default();
        known.add(secret);
        assert_eq!(
            known.redact_text(&format!("auth {secret}")),
            format!("auth {REDACTED}")
        );
        // A base64 body copy whose data is the secret: its decoded bytes hold no secret,
        // but its text is one.
        let value = redact(
            &known,
            serde_json::json!({ "encoding": "base64", "data": secret }),
        );
        let data = value["data"].as_str().unwrap();
        assert!(!data.contains(secret), "{data}");
        // And the secret's own base64.
        assert_eq!(known.redact_text(&STANDARD.encode(secret)), REDACTED);
    }

    #[test]
    fn many_secrets_are_cut_with_the_automaton_and_without_it() {
        let known = KnownSecrets::default();
        let secrets: Vec<String> = (0..500).map(|n| format!("secret-{n:04}-é")).collect();
        known.add_all(&secrets);
        let text: String = secrets.iter().map(|secret| format!("[{secret}]")).collect();
        let expected = format!("[{REDACTED}]").repeat(secrets.len());
        assert!(known.inner.read().unwrap().matcher.is_some());
        assert_eq!(known.redact_text(&text), expected);
        let encoded = STANDARD.encode(&secrets[321]);
        assert_eq!(known.redact_text(&encoded), REDACTED);
        // The path taken when building the automaton failed: each form searched alone.
        known.inner.write().unwrap().matcher = None;
        assert_eq!(known.redact_text(&text), expected);
        assert_eq!(known.redact_text(&encoded), REDACTED);
        assert_eq!(known.redact_text("nothing here"), "nothing here");
    }
}
