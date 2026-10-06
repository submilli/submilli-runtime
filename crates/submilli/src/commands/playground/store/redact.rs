//! KTD19's redaction: every known secret value is cut out of what the store writes,
//! verbatim and in its base64 and URL-encoded forms.
//!
//! The runtime already keeps `secrets.get` results out of call records and masks
//! credential-bearing headers before it copies a payload. What it cannot do is find a
//! secret's value inside a body, a decision's context, or console output, because it
//! does not know the values. The playground does: it learns every value a run could
//! read (the secret store's answers, the harness secrets a request supplied, and the
//! development values `bind` will hold), and this pass replaces each occurrence before
//! anything reaches disk.

use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError, RwLock};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
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
    /// Every form of every value, longest first, so a longer form is cut before a
    /// shorter one inside it.
    forms: Vec<Vec<u8>>,
}

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
        redact_str(&patterns.forms, text)
    }

    /// Redacts every string in `value` in place, object keys included. A base64 body
    /// copy (`{"encoding": "base64", "data": ...}`) is decoded, redacted as bytes, and
    /// encoded again, since a secret inside binary data does not appear in its base64
    /// text at a fixed alignment.
    pub(crate) fn redact_value(&self, value: &mut Value) {
        let patterns = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        if patterns.forms.is_empty() {
            return;
        }
        redact_tree(&patterns.forms, value);
    }
}

/// The forms a secret can take in a record: as is, base64 (standard and URL-safe, with
/// and without padding), and URL-encoded (strict percent-encoding and form encoding).
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

fn encoded_forms(value: &str) -> BTreeSet<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut forms = BTreeSet::new();
    forms.insert(bytes.to_vec());
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        forms.insert(engine.encode(bytes).into_bytes());
    }
    forms.insert(
        utf8_percent_encode(value, NON_ALPHANUMERIC)
            .to_string()
            .into_bytes(),
    );
    forms.insert(
        url::form_urlencoded::byte_serialize(bytes)
            .collect::<String>()
            .into_bytes(),
    );
    forms
}

fn redact_tree(forms: &[Vec<u8>], value: &mut Value) {
    match value {
        Value::String(text) => {
            if contains_any(forms, text.as_bytes()) {
                *text = redact_str(forms, text);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_tree(forms, item);
            }
        }
        Value::Object(fields) => {
            if redact_base64_copy(forms, fields) {
                return;
            }
            let keys: Vec<String> = fields
                .keys()
                .filter(|key| contains_any(forms, key.as_bytes()))
                .cloned()
                .collect();
            for key in keys {
                if let Some(item) = fields.remove(&key) {
                    fields.insert(redact_str(forms, &key), item);
                }
            }
            for item in fields.values_mut() {
                redact_tree(forms, item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// A base64 body copy, redacted through its decoded bytes. `false` when `fields` is not
/// one, or its data does not decode (it is then redacted as text like any string).
fn redact_base64_copy(forms: &[Vec<u8>], fields: &mut serde_json::Map<String, Value>) -> bool {
    if fields.len() != 2 || fields.get("encoding").and_then(Value::as_str) != Some("base64") {
        return false;
    }
    let Some(Value::String(data)) = fields.get_mut("data") else {
        return false;
    };
    let Ok(decoded) = STANDARD.decode(data.as_bytes()) else {
        return false;
    };
    let cleaned = redact_bytes(forms, &decoded);
    let encoded = STANDARD.encode(&cleaned);
    // The text may also carry a secret's own base64 form at the copy's alignment.
    *data = redact_str(forms, &encoded);
    true
}

fn contains_any(forms: &[Vec<u8>], haystack: &[u8]) -> bool {
    forms.iter().any(|form| find(haystack, form).is_some())
}

fn redact_str(forms: &[Vec<u8>], text: &str) -> String {
    if !contains_any(forms, text.as_bytes()) {
        return text.to_owned();
    }
    let cleaned = redact_bytes(forms, text.as_bytes());
    // Every form is cut whole and the marker is ASCII, but a non-UTF-8 form (none is
    // today) could split a character; the lossy conversion keeps the result a string.
    String::from_utf8(cleaned)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

fn redact_bytes(forms: &[Vec<u8>], bytes: &[u8]) -> Vec<u8> {
    let mut current = bytes.to_vec();
    for form in forms {
        if form.is_empty() || find(&current, form).is_none() {
            continue;
        }
        let mut out = Vec::with_capacity(current.len());
        let mut rest = current.as_slice();
        while let Some(at) = find(rest, form) {
            out.extend_from_slice(rest.get(..at).unwrap_or_default());
            out.extend_from_slice(REDACTED.as_bytes());
            rest = rest.get(at + form.len()..).unwrap_or_default();
        }
        out.extend_from_slice(rest);
        current = out;
    }
    current
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
    use super::*;

    const SECRET: &str = "sk_live_9f8e7d+/=?&";

    fn known() -> KnownSecrets {
        let known = KnownSecrets::default();
        known.add(SECRET);
        known
    }

    #[test]
    fn every_form_of_a_secret_is_cut_from_text() {
        let known = known();
        let forms = [
            SECRET.to_owned(),
            STANDARD.encode(SECRET),
            STANDARD_NO_PAD.encode(SECRET),
            URL_SAFE_NO_PAD.encode(SECRET),
            utf8_percent_encode(SECRET, NON_ALPHANUMERIC).to_string(),
            url::form_urlencoded::byte_serialize(SECRET.as_bytes()).collect(),
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
            let mut value = serde_json::json!({
                "encoding": "base64",
                "data": STANDARD.encode(&body),
            });
            known.redact_value(&mut value);
            let data = STANDARD.decode(value["data"].as_str().unwrap()).unwrap();
            assert!(find(&data, SECRET.as_bytes()).is_none());
            assert!(find(&data, REDACTED.as_bytes()).is_some(), "pad {pad}");
        }
    }

    #[test]
    fn keys_and_nested_values_are_redacted_and_numbers_left_alone() {
        let known = known();
        let mut value = serde_json::json!({
            SECRET: [1, {"inner": format!("x{SECRET}y")}],
            "n": 5,
        });
        known.redact_value(&mut value);
        assert_eq!(
            value,
            serde_json::json!({ REDACTED: [1, {"inner": format!("x{REDACTED}y")}], "n": 5 })
        );
    }

    #[test]
    fn short_values_are_not_treated_as_secrets() {
        let known = KnownSecrets::default();
        known.add("abc");
        known.add("");
        assert_eq!(known.redact_text("abc kind"), "abc kind");
    }
}
