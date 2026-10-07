//! `JSON.parse`'s reader, over UTF-16 code units.
//!
//! A JavaScript string is a sequence of code units, and JSON text may hold a
//! lone surrogate either raw or as a `\uD800` escape. Both must survive into the
//! parsed string, as they do in JavaScript, so this reader never decodes to
//! Rust's UTF-8 `String`. Its grammar, recursion limit and error texts follow
//! `serde_json`, which the runtime used before and which the MCP and other host
//! paths still use for text that is always well-formed UTF-8.

use std::cmp::Ordering;
use std::collections::BTreeMap;

/// The nesting at which a document is refused, as `serde_json` refuses it: the
/// 128th open bracket fails. [`super::MAX_VTABLE_WALK_DEPTH`] is pinned to it.
const RECURSION_LIMIT: u32 = 128;

/// A parsed JSON value whose strings keep their code units.
#[derive(Debug, PartialEq)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    Array(Vec<JsonValue>),
    /// Fields in canonical order (by code point), the last of duplicate keys
    /// winning, as `serde_json`'s map orders them.
    Object(BTreeMap<JsonKey, JsonValue>),
}

/// An object key, ordered by code point as Rust orders a `String`; a lone
/// surrogate orders by its own value.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct JsonKey(pub(crate) Vec<u16>);

impl Ord for JsonKey {
    fn cmp(&self, other: &Self) -> Ordering {
        code_points(&self.0).cmp(code_points(&other.0))
    }
}

fn code_points(units: &[u16]) -> impl Iterator<Item = u32> + '_ {
    char::decode_utf16(units.iter().copied())
        .map(|c| c.map_or_else(|lone| u32::from(lone.unpaired_surrogate()), u32::from))
}

impl PartialOrd for JsonKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<&serde_json::Value> for JsonValue {
    fn from(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(*b),
            // `as_f64` is total for the default (non-arbitrary-precision) number.
            serde_json::Value::Number(n) => Self::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => Self::String(s.encode_utf16().collect()),
            serde_json::Value::Array(items) => Self::Array(items.iter().map(Self::from).collect()),
            serde_json::Value::Object(map) => Self::Object(
                map.iter()
                    .map(|(k, v)| (JsonKey(k.encode_utf16().collect()), Self::from(v)))
                    .collect(),
            ),
        }
    }
}

/// Why a document was refused. A number literal past the `f64` range is a
/// `RangeError` in the language; everything else is a `SyntaxError`.
#[derive(Debug, PartialEq)]
pub(crate) enum JsonError {
    Syntax(String),
    Range(String),
}

impl JsonError {
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Syntax(message) | Self::Range(message) => message,
        }
    }
}

/// Parses one JSON document.
pub(crate) fn parse(text: &[u16]) -> Result<JsonValue, JsonError> {
    let mut reader = Reader {
        text,
        pos: 0,
        remaining_depth: RECURSION_LIMIT,
    };
    reader.skip_whitespace();
    let value = reader.value()?;
    reader.skip_whitespace();
    if reader.pos < text.len() {
        reader.pos += 1;
        return Err(reader.syntax("trailing characters"));
    }
    Ok(value)
}

struct Reader<'a> {
    text: &'a [u16],
    pos: usize,
    remaining_depth: u32,
}

fn ascii(unit: u16) -> Option<u8> {
    u8::try_from(unit).ok().filter(u8::is_ascii)
}

impl Reader<'_> {
    fn peek(&self) -> Option<u16> {
        self.text.get(self.pos).copied()
    }

    fn peek_ascii(&self) -> Option<u8> {
        self.peek().and_then(ascii)
    }

    fn next(&mut self) -> Option<u16> {
        let unit = self.peek()?;
        self.pos += 1;
        Some(unit)
    }

    /// `message` at the line and column of the last unit read, as `serde_json`
    /// reports them.
    fn syntax(&self, message: &str) -> JsonError {
        JsonError::Syntax(self.positioned(message))
    }

    fn positioned(&self, message: &str) -> String {
        let read = &self.text[..self.pos.min(self.text.len())];
        let line = 1 + read.iter().filter(|&&u| u == u16::from(b'\n')).count();
        let column = read
            .iter()
            .rev()
            .take_while(|&&u| u != u16::from(b'\n'))
            .count();
        format!("{message} at line {line} column {column}")
    }

    fn eof(&self, what: &str) -> JsonError {
        self.syntax(&format!("EOF while parsing {what}"))
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_ascii(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<JsonValue, JsonError> {
        let Some(first) = self.peek_ascii() else {
            if self.peek().is_none() {
                return Err(self.eof("a value"));
            }
            self.pos += 1;
            return Err(self.syntax("expected value"));
        };
        match first {
            b'n' => self.keyword("null", JsonValue::Null),
            b't' => self.keyword("true", JsonValue::Bool(true)),
            b'f' => self.keyword("false", JsonValue::Bool(false)),
            b'"' => {
                self.pos += 1;
                Ok(JsonValue::String(self.string()?))
            }
            b'-' | b'0'..=b'9' => self.number(),
            b'[' => self.nested(Self::array),
            b'{' => self.nested(Self::object),
            _ => {
                self.pos += 1;
                Err(self.syntax("expected value"))
            }
        }
    }

    fn keyword(&mut self, word: &str, value: JsonValue) -> Result<JsonValue, JsonError> {
        for expected in word.bytes() {
            match self.next() {
                Some(unit) if unit == u16::from(expected) => {}
                Some(_) => return Err(self.syntax("expected ident")),
                None => return Err(self.eof("a value")),
            }
        }
        Ok(value)
    }

    fn nested(
        &mut self,
        parse: fn(&mut Self) -> Result<JsonValue, JsonError>,
    ) -> Result<JsonValue, JsonError> {
        self.pos += 1;
        self.remaining_depth -= 1;
        if self.remaining_depth == 0 {
            return Err(self.syntax("recursion limit exceeded"));
        }
        let value = parse(self);
        self.remaining_depth += 1;
        value
    }

    fn array(&mut self) -> Result<JsonValue, JsonError> {
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek_ascii() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            self.skip_whitespace();
            if self.peek().is_none() {
                return Err(self.eof("a list"));
            }
            items.push(self.value()?);
            self.skip_whitespace();
            match self.next().map(ascii) {
                Some(Some(b',')) => {
                    self.skip_whitespace();
                    if self.peek_ascii() == Some(b']') {
                        self.pos += 1;
                        return Err(self.syntax("trailing comma"));
                    }
                }
                Some(Some(b']')) => return Ok(JsonValue::Array(items)),
                Some(_) => return Err(self.syntax("expected `,` or `]`")),
                None => return Err(self.eof("a list")),
            }
        }
    }

    fn object(&mut self) -> Result<JsonValue, JsonError> {
        let mut fields = BTreeMap::new();
        self.skip_whitespace();
        if self.peek_ascii() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(fields));
        }
        loop {
            self.skip_whitespace();
            match self.next().map(ascii) {
                Some(Some(b'"')) => {}
                Some(_) => return Err(self.syntax("key must be a string")),
                None => return Err(self.eof("an object")),
            }
            let key = self.string()?;
            self.skip_whitespace();
            match self.next().map(ascii) {
                Some(Some(b':')) => {}
                Some(_) => return Err(self.syntax("expected `:`")),
                None => return Err(self.eof("an object")),
            }
            self.skip_whitespace();
            if self.peek().is_none() {
                return Err(self.eof("a value"));
            }
            let value = self.value()?;
            fields.insert(JsonKey(key), value);
            self.skip_whitespace();
            match self.next().map(ascii) {
                Some(Some(b',')) => {
                    self.skip_whitespace();
                    if self.peek_ascii() == Some(b'}') {
                        self.pos += 1;
                        return Err(self.syntax("trailing comma"));
                    }
                }
                Some(Some(b'}')) => return Ok(JsonValue::Object(fields)),
                Some(_) => return Err(self.syntax("expected `,` or `}`")),
                None => return Err(self.eof("an object")),
            }
        }
    }

    /// The rest of a string whose opening quote has been read. Units outside an
    /// escape, a lone surrogate included, are copied as they are.
    fn string(&mut self) -> Result<Vec<u16>, JsonError> {
        let mut out = Vec::new();
        loop {
            let Some(unit) = self.next() else {
                return Err(self.eof("a string"));
            };
            match ascii(unit) {
                Some(b'"') => return Ok(out),
                Some(b'\\') => out.push(self.escape()?),
                Some(0x00..=0x1F) => {
                    return Err(self.syntax(
                        "control character (\\u0000-\\u001F) found while parsing a string",
                    ));
                }
                _ => out.push(unit),
            }
        }
    }

    /// One escape after its backslash. A `\u` escape yields one code unit, so a
    /// surrogate pair arrives as two escapes and a lone surrogate stays lone.
    fn escape(&mut self) -> Result<u16, JsonError> {
        let Some(unit) = self.next() else {
            return Err(self.eof("a string"));
        };
        let decoded = match ascii(unit) {
            Some(b'"') => b'"',
            Some(b'\\') => b'\\',
            Some(b'/') => b'/',
            Some(b'b') => 0x08,
            Some(b'f') => 0x0C,
            Some(b'n') => b'\n',
            Some(b'r') => b'\r',
            Some(b't') => b'\t',
            Some(b'u') => return self.hex_escape(),
            _ => return Err(self.syntax("invalid escape")),
        };
        Ok(u16::from(decoded))
    }

    fn hex_escape(&mut self) -> Result<u16, JsonError> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let Some(unit) = self.next() else {
                return Err(self.eof("a string"));
            };
            let digit = ascii(unit)
                .and_then(|b| char::from(b).to_digit(16))
                .ok_or_else(|| self.syntax("invalid escape"))?;
            // Four hex digits fit a `u16`, and each digit is below 16.
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    /// A number, by the JSON grammar: an optional `-`, an integer part without
    /// leading zeros, then an optional fraction and exponent.
    fn number(&mut self) -> Result<JsonValue, JsonError> {
        let start = self.pos;
        if self.peek_ascii() == Some(b'-') {
            self.pos += 1;
        }
        match self.next().map(ascii) {
            Some(Some(b'0')) => {
                if matches!(self.peek_ascii(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                    return Err(self.syntax("invalid number"));
                }
            }
            Some(Some(b'1'..=b'9')) => self.digits(),
            Some(_) => return Err(self.syntax("invalid number")),
            None => return Err(self.eof("a value")),
        }
        if self.peek_ascii() == Some(b'.') {
            self.pos += 1;
            self.required_digits()?;
        }
        if matches!(self.peek_ascii(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek_ascii(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.required_digits()?;
        }
        // Every unit in the literal was checked to be ASCII above.
        let literal: String = self.text[start..self.pos]
            .iter()
            .filter_map(|&u| ascii(u).map(char::from))
            .collect();
        let value: f64 = literal.parse().map_err(|_| self.syntax("invalid number"))?;
        if value.is_infinite() {
            return Err(JsonError::Range(self.positioned("number out of range")));
        }
        Ok(JsonValue::Number(value))
    }

    fn digits(&mut self) {
        while matches!(self.peek_ascii(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn required_digits(&mut self) -> Result<(), JsonError> {
        match self.next().map(ascii) {
            Some(Some(b'0'..=b'9')) => {
                self.digits();
                Ok(())
            }
            Some(_) => Err(self.syntax("invalid number")),
            None => Err(self.eof("a value")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn parse_str(text: &str) -> Result<JsonValue, JsonError> {
        parse(&units(text))
    }

    #[test]
    fn a_lone_surrogate_escape_is_kept() {
        assert_eq!(
            parse_str(r#""\ud800x""#),
            Ok(JsonValue::String(vec![0xD800, u16::from(b'x')]))
        );
    }

    #[test]
    fn a_raw_lone_surrogate_is_kept() {
        let text = [u16::from(b'"'), 0xDC00, u16::from(b'"')];
        assert_eq!(parse(&text), Ok(JsonValue::String(vec![0xDC00])));
    }

    #[test]
    fn a_surrogate_pair_escape_is_two_units() {
        assert_eq!(
            parse_str(r#""😀""#),
            Ok(JsonValue::String(units("\u{1F600}")))
        );
    }

    #[test]
    fn negative_zero_keeps_its_sign() {
        let Ok(JsonValue::Number(n)) = parse_str("-0") else {
            panic!("-0 parses as a number");
        };
        assert!(n == 0.0 && n.is_sign_negative());
    }

    #[test]
    fn numbers_follow_the_json_grammar() {
        for bad in [
            "01", "1.", ".5", "+1", "-", "1e", "1e+", "0x10", "Infinity", "NaN",
        ] {
            assert!(
                matches!(parse_str(bad), Err(JsonError::Syntax(_))),
                "{bad} must be refused"
            );
        }
        assert_eq!(parse_str("1.5e2"), Ok(JsonValue::Number(150.0)));
        assert!(matches!(parse_str("1e400"), Err(JsonError::Range(_))));
    }

    #[test]
    fn duplicate_keys_keep_the_last_and_keys_sort_by_code_point() {
        let Ok(JsonValue::Object(fields)) = parse_str(r#"{"b":1,"a":2,"b":3}"#) else {
            panic!("an object");
        };
        let entries: Vec<_> = fields.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], (&JsonKey(units("a")), &JsonValue::Number(2.0)));
        assert_eq!(entries[1], (&JsonKey(units("b")), &JsonValue::Number(3.0)));
    }

    #[test]
    fn malformed_documents_are_syntax_errors_like_serde_json() {
        for (text, message) in [
            ("", "EOF while parsing a value"),
            ("{", "EOF while parsing an object"),
            ("[1,]", "trailing comma"),
            ("[1 2]", "expected `,` or `]`"),
            ("{1:2}", "key must be a string"),
            (r#"{"a" 1}"#, "expected `:`"),
            ("tru", "EOF while parsing a value"),
            ("nul!", "expected ident"),
            ("1 2", "trailing characters"),
            (r#""\x""#, "invalid escape"),
            ("\"a\nb\"", "control character"),
        ] {
            let Err(JsonError::Syntax(got)) = parse_str(text) else {
                panic!("{text:?} must be refused");
            };
            assert!(got.starts_with(message), "{text:?}: {got}");
        }
    }

    #[test]
    fn the_recursion_limit_matches_serde_json() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        assert!(parse_str(&nested(127)).is_ok());
        let Err(JsonError::Syntax(message)) = parse_str(&nested(128)) else {
            panic!("128 levels must be refused");
        };
        assert!(message.starts_with("recursion limit exceeded"));
        for depth in [127, 128] {
            assert_eq!(
                parse_str(&nested(depth)).is_ok(),
                serde_json::from_str::<serde_json::Value>(&nested(depth)).is_ok()
            );
        }
    }
}
