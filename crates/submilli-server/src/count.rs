//! Decimal counts for fuel and token settings, shared by all configuration sources.

use std::fmt;

use serde::{Deserialize, Deserializer};

#[derive(Debug)]
pub(crate) struct ParseCountError;

impl fmt::Display for ParseCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expected a whole count up to {}, optionally followed by K, M, B, or T (decimal, case-insensitive); underscores may separate digits, e.g. 10_000 or 20M",
            u64::MAX
        )
    }
}

impl std::error::Error for ParseCountError {}

pub(crate) fn parse_count(raw: &str) -> Result<u64, ParseCountError> {
    let (digits, multiplier) = match raw.as_bytes().last() {
        Some(b'k' | b'K') => (raw.strip_suffix(['k', 'K']), 1_000),
        Some(b'm' | b'M') => (raw.strip_suffix(['m', 'M']), 1_000_000),
        Some(b'b' | b'B') => (raw.strip_suffix(['b', 'B']), 1_000_000_000),
        Some(b't' | b'T') => (raw.strip_suffix(['t', 'T']), 1_000_000_000_000),
        _ => (Some(raw), 1),
    };
    let digits = digits.ok_or(ParseCountError)?;
    let mut value = 0_u64;
    let mut previous_was_digit = false;
    for byte in digits.bytes() {
        match byte {
            b'0'..=b'9' => {
                value = value
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                    .ok_or(ParseCountError)?;
                previous_was_digit = true;
            }
            b'_' if previous_was_digit => previous_was_digit = false,
            _ => return Err(ParseCountError),
        }
    }
    if !previous_was_digit {
        return Err(ParseCountError);
    }
    value.checked_mul(multiplier).ok_or(ParseCountError)
}

pub(crate) fn deserialize_optional_count<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CountValue {
        Integer(u64),
        Text(String),
    }

    Option::<CountValue>::deserialize(deserializer)?
        .map(|value| match value {
            CountValue::Integer(value) => Ok(value),
            CountValue::Text(raw) => parse_count(&raw).map_err(serde::de::Error::custom),
        })
        .transpose()
}
