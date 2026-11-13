//! Prices a blueprint can declare for its models, and the money type they use.
//!
//! A price is US dollars per million tokens. Amounts are counted exactly, in
//! pico-dollars, so a run's total is the sum of its parts with no float drift
//! and the type can derive `Eq` like the rest of the blueprint.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Pico-dollars in one dollar.
const PICO_PER_USD: u128 = 1_000_000_000_000;

/// Decimal places a [`Usd`] keeps: one pico-dollar.
const FRACTION_DIGITS: usize = 12;

/// An exact, non-negative amount of US dollars, counted in pico-dollars.
///
/// Serializes as a decimal string (`"0.00421"`) so a JSON reader does not round
/// it through a float. Deserializes from that string or from a number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Usd(u128);

impl Usd {
    pub const ZERO: Self = Self(0);

    pub const fn from_pico(pico: u128) -> Self {
        Self(pico)
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Parse a plain decimal: digits, an optional point, at most twelve
    /// fractional digits. No sign, exponent or spaces.
    pub fn parse(text: &str) -> Result<Self, String> {
        let invalid =
            || format!("'{text}' is not a dollar amount; write a plain decimal such as 0.042");
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if whole.is_empty() && fraction.is_empty() || !digits(whole) || !digits(fraction) {
            return Err(invalid());
        }
        if fraction.len() > FRACTION_DIGITS {
            return Err(format!(
                "'{text}' is more precise than a pico-dollar (12 decimal places)"
            ));
        }
        let whole: u128 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| invalid())?
        };
        let fraction: u128 = format!("{fraction:0<FRACTION_DIGITS$}")
            .parse()
            .map_err(|_| invalid())?;
        whole
            .checked_mul(PICO_PER_USD)
            .and_then(|pico| pico.checked_add(fraction))
            .map(Self)
            .ok_or_else(invalid)
    }

    /// The cost of `tokens` at this price per million tokens, rounded to the
    /// nearest pico-dollar.
    pub fn per_million(self, tokens: u64) -> Self {
        let product = self.0.saturating_mul(u128::from(tokens));
        Self(product.saturating_add(500_000) / 1_000_000)
    }
}

impl fmt::Display for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / PICO_PER_USD;
        let fraction = self.0 % PICO_PER_USD;
        if fraction == 0 {
            return write!(f, "{whole}");
        }
        let fraction = format!("{fraction:0>FRACTION_DIGITS$}");
        write!(f, "{whole}.{}", fraction.trim_end_matches('0'))
    }
}

impl Serialize for Usd {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Usd {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UsdVisitor;

        impl Visitor<'_> for UsdVisitor {
            type Value = Usd;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a non-negative dollar amount")
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Usd, E> {
                Usd::parse(&value.to_string()).map_err(E::custom)
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Usd, E> {
                if value < 0 {
                    return Err(E::custom(format!("{value} is negative; a price cannot be")));
                }
                self.visit_u64(value.unsigned_abs())
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Usd, E> {
                if value < 0.0 {
                    return Err(E::custom(format!("{value} is negative; a price cannot be")));
                }
                // `Display` for f64 is the shortest text that reads back as the
                // same value and never uses an exponent, so 0.042 parses as
                // exactly 0.042; NaN, infinity and more than 12 places are refused.
                Usd::parse(&value.to_string()).map_err(E::custom)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Usd, E> {
                Usd::parse(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(UsdVisitor)
    }
}

/// What an LLM model costs, in dollars per million tokens.
///
/// A cache price left out charges those tokens at the `input` price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmPricing {
    /// Prompt tokens the cache did not serve.
    pub input: Usd,
    pub output: Usd,
    /// Prompt tokens read from the provider's prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<Usd>,
    /// Prompt tokens written to the provider's prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<Usd>,
}

/// What an embedding model costs, in dollars per million input tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingPricing {
    pub input: Usd,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_exact_decimals() {
        for (text, printed) in [
            ("0.042", "0.042"),
            ("3", "3"),
            ("3.00", "3"),
            (".5", "0.5"),
            ("0.000000000001", "0.000000000001"),
            ("15.75", "15.75"),
        ] {
            assert_eq!(Usd::parse(text).unwrap().to_string(), printed, "{text}");
        }
    }

    #[test]
    fn refuses_what_is_not_a_plain_decimal() {
        for text in [
            "",
            ".",
            "-1",
            "1e3",
            " 1",
            "1.2.3",
            "0.0000000000001",
            "abc",
        ] {
            assert!(Usd::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn prices_tokens_per_million() {
        let three = Usd::parse("3").unwrap();
        assert_eq!(three.per_million(1_000_000).to_string(), "3");
        assert_eq!(three.per_million(1).to_string(), "0.000003");
        let jev = Usd::parse("0.042").unwrap();
        assert_eq!(jev.per_million(10).to_string(), "0.00000042");
    }

    #[test]
    fn deserializes_from_yaml_numbers_and_strings() {
        let pricing: LlmPricing =
            serde_yml::from_str("input: 3\noutput: 15.0\ncache_read: '0.3'\n").unwrap();
        assert_eq!(pricing.input.to_string(), "3");
        assert_eq!(pricing.output.to_string(), "15");
        assert_eq!(pricing.cache_read.unwrap().to_string(), "0.3");
        assert!(serde_yml::from_str::<LlmPricing>("input: -1\noutput: 1\n").is_err());
        assert!(serde_yml::from_str::<LlmPricing>("input: 1\noutput: 1\nextra: 1\n").is_err());
        let exponent: LlmPricing = serde_yml::from_str("input: 1e-3\noutput: 2\n").unwrap();
        assert_eq!(exponent.input.to_string(), "0.001");
        for refused in ["0.0000000000001", ".inf", ".nan"] {
            let yaml = format!("input: {refused}\noutput: 1\n");
            assert!(
                serde_yml::from_str::<LlmPricing>(&yaml).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn serializes_as_a_string() {
        let json = serde_json::to_string(&Usd::parse("0.00421").unwrap()).unwrap();
        assert_eq!(json, "\"0.00421\"");
    }
}
