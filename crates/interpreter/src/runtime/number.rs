//! JavaScript-compatible number parsers and formatters.

use num_traits::FromPrimitive;

/// ECMAScript ToUint32: truncate then wrap, including non-finite inputs.
pub(crate) fn to_uint32(value: f64) -> u32 {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    value.trunc().rem_euclid(4_294_967_296.0) as u32
}

pub(crate) fn to_int32(value: f64) -> i32 {
    to_uint32(value) as i32
}

/// ECMAScript ToUint16, as `String.fromCharCode` applies it.
pub(crate) fn to_uint16(value: f64) -> u16 {
    to_uint32(value) as u16
}

/// ECMAScript ToUint8, as a `Uint8Array` byte write applies it.
pub(crate) fn to_uint8(value: f64) -> u8 {
    to_uint32(value) as u8
}

/// ECMAScript ToString for f64 — spells NaN/Infinity/-Infinity and
/// normalizes -0 to "0" (Rust's default prints "inf" and keeps the sign).
pub fn format_number_js(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if n == 0.0 {
        "0".to_string()
    } else if n.abs() >= 1e21 || n.abs() < 1e-6 {
        jsify_exponent(&format!("{n:e}"))
    } else {
        n.to_string()
    }
}

/// JS-spec `Number#toFixed(digits)`.
///
/// Exact-tie rounding follows Rust's binary-value formatting (half-to-even),
/// where JS rounds ties away from zero — visible only when the value sits
/// exactly on a representable midpoint, e.g. `(2.5).toFixed(0)`.
pub fn to_fixed_js(x: f64, digits: f64) -> Result<String, String> {
    let d = digits.trunc();
    if !(0.0..=100.0).contains(&d) || digits.is_nan() {
        return Err("toFixed digits must be between 0 and 100".to_string());
    }
    if !x.is_finite() || x.abs() >= 1e21 {
        return Ok(format_number_js(x));
    }
    Ok(format!("{:.*}", d as usize, x))
}

/// JS-spec `Number#toExponential(fractionDigits)`. A NaN `fractionDigits`
/// is the omitted-argument sentinel: use however many digits uniquely
/// identify the value.
pub fn to_exponential_js(x: f64, fraction_digits: f64) -> Result<String, String> {
    if !x.is_finite() {
        return Ok(format_number_js(x));
    }
    let mantissa_exp = if fraction_digits.is_nan() {
        format!("{x:e}")
    } else {
        let d = fraction_digits.trunc();
        if !(0.0..=100.0).contains(&d) {
            return Err("toExponential digits must be between 0 and 100".to_string());
        }
        format!("{:.*e}", d as usize, x)
    };
    Ok(jsify_exponent(&mantissa_exp))
}

/// JS-spec `Number#toPrecision(precision)`. A NaN `precision` is the
/// omitted-argument sentinel: behave like plain `toString`.
pub fn to_precision_js(x: f64, precision: f64) -> Result<String, String> {
    if precision.is_nan() {
        return Ok(format_number_js(x));
    }
    let p = precision.trunc();
    if !(1.0..=100.0).contains(&p) {
        return Err("toPrecision precision must be between 1 and 100".to_string());
    }
    let p = p as usize;
    if !x.is_finite() {
        return Ok(format_number_js(x));
    }
    if x == 0.0 {
        return Ok(if p == 1 {
            "0".to_string()
        } else {
            format!("{:.*}", p - 1, 0.0)
        });
    }
    // Round to p significant digits first; the decimal exponent decides
    // between fixed and exponential form (JS: e < -6 or e >= p → exponential).
    // Rust exponential formatting of finite nonzero f64 always has an integer
    // exponent (at most three decimal digits), even after rounding.
    let sci = format!("{:.*e}", p - 1, x);
    let (_, exponent) = sci
        .split_once('e')
        .expect("finite exponential formatting includes an exponent");
    let exp: i32 = exponent.parse().expect("an f64 decimal exponent fits i32");
    if exp < -6 || exp >= p as i32 {
        return Ok(jsify_exponent(&sci));
    }
    let fraction_digits = (p as i32 - 1 - exp).max(0) as usize;
    Ok(format!("{x:.fraction_digits$}"))
}

/// JS-style exponent spelling: `1.5e3` → `1.5e+3` (negative exponents
/// already carry their sign).
fn jsify_exponent(s: &str) -> String {
    match s.find('e') {
        Some(i) if !s[i + 1..].starts_with('-') => format!("{}e+{}", &s[..i], &s[i + 1..]),
        _ => s.to_string(),
    }
}

/// JS-spec `Number#toString(radix)` for radix 2–36.
///
/// The integer part is exact at any magnitude; the fraction part emits up
/// to 32 digits and stops (JS prints the shortest uniquely-identifying
/// fraction, which can be longer or shorter).
pub fn to_string_radix_js(x: f64, radix: f64) -> Result<String, String> {
    let r = radix.trunc();
    if !(2.0..=36.0).contains(&r) || radix.is_nan() {
        return Err("toString radix must be between 2 and 36".to_string());
    }
    let r = r as u32;
    if r == 10 || !x.is_finite() {
        return Ok(format_number_js(x));
    }
    if x == 0.0 {
        return Ok("0".to_string());
    }

    let negative = x < 0.0;
    let magnitude = x.abs();
    let integer = magnitude.trunc();
    let mut fraction = magnitude - integer;

    // f64 → BigInt is exact for any finite value, so arbitrarily large
    // integer parts convert without drift.
    let int_digits = num_bigint::BigInt::from_f64(integer)
        .expect("finite integer magnitude converts to BigInt")
        .to_str_radix(r);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&int_digits);

    if fraction > 0.0 {
        out.push('.');
        for _ in 0..32 {
            // fraction is in [0, 1). Even its largest f64 value times the
            // exact integer radix is below the midpoint between r and its previous
            // representable value, so rounding
            // cannot produce r. Subtracting trunc below preserves [0, 1).
            fraction *= f64::from(r);
            let digit = fraction.trunc() as u32;
            out.push(
                char::from_digit(digit, r).expect("fraction digit is below the validated radix"),
            );
            fraction -= fraction.trunc();
            if fraction == 0.0 {
                break;
            }
        }
    }
    Ok(out)
}

/// JS-spec `parseInt(s, radix)`.
///
/// `radix == 0` means auto-detect: `0x` / `0X` prefix → base 16,
/// otherwise base 10. `radix == 16` also accepts the `0x` prefix.
/// Other radices in `[2, 36]` are taken literally; out-of-range
/// radices return `NaN`.
pub fn parse_int_js(input: &str, radix: u32) -> f64 {
    let s = input.trim_start_matches(is_ascii_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }
    let (sign, rest) = strip_sign(s);
    if rest.is_empty() {
        return f64::NAN;
    }

    let (effective_radix, digits) = if radix == 0 || radix == 16 {
        if let Some(after_prefix) = strip_hex_prefix(rest) {
            (16u32, after_prefix)
        } else if radix == 0 {
            (10, rest)
        } else {
            (16, rest)
        }
    } else if (2..=36).contains(&radix) {
        (radix, rest)
    } else {
        return f64::NAN;
    };

    let mut consumed = 0usize;
    let mut value: f64 = 0.0;
    for ch in digits.chars() {
        let Some(digit) = ch.to_digit(effective_radix) else {
            break;
        };
        value = value * (effective_radix as f64) + (digit as f64);
        consumed += ch.len_utf8();
    }
    if consumed == 0 {
        return f64::NAN;
    }
    sign * value
}

pub fn parse_float_js(input: &str) -> f64 {
    let s = input.trim_start_matches(is_ascii_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }
    let (sign, rest) = strip_sign(s);
    if rest.is_empty() {
        return f64::NAN;
    }
    if let Some(remainder) = rest.strip_prefix("Infinity") {
        // prefix rule: trailing junk is fine, so "Infinity" prefix suffices
        let _ = remainder;
        return sign * f64::INFINITY;
    }

    let prefix = float_prefix(rest);
    if prefix.is_empty() {
        return f64::NAN;
    }
    if prefix == "." {
        return f64::NAN;
    }
    match prefix.parse::<f64>() {
        Ok(v) => sign * v,
        Err(_) => f64::NAN,
    }
}

/// JS-spec `Number(s)` / `ToNumber` on a string.
///
/// Unlike [`parse_float_js`] (prefix rule) the *entire* trimmed string must be
/// a valid `StringNumericLiteral`; trailing junk yields `NaN`. Empty/whitespace
/// is `0`. Handles the `0x`/`0o`/`0b` integer-literal prefixes — these take no
/// sign and no fractional/exponent part, per the grammar.
pub fn string_to_number_js(input: &str) -> f64 {
    let s = input.trim_matches(is_js_whitespace);
    if s.is_empty() {
        return 0.0;
    }

    if let Some(rest) = strip_radix_prefix(s, b'x') {
        return parse_radix_strict(rest, 16);
    }
    if let Some(rest) = strip_radix_prefix(s, b'o') {
        return parse_radix_strict(rest, 8);
    }
    if let Some(rest) = strip_radix_prefix(s, b'b') {
        return parse_radix_strict(rest, 2);
    }

    let (sign, rest) = strip_sign(s);
    if rest == "Infinity" {
        return sign * f64::INFINITY;
    }
    // Rust's `f64::from_str` also accepts `inf`/`infinity`/`nan`, which JS
    // rejects; gate on the decimal-literal charset to exclude them.
    if !rest.bytes().all(is_decimal_literal_byte) {
        return f64::NAN;
    }
    match rest.parse::<f64>() {
        Ok(v) => sign * v,
        Err(_) => f64::NAN,
    }
}

fn strip_radix_prefix(s: &str, marker: u8) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'0' && bytes[1] | 0x20 == marker {
        Some(&s[2..])
    } else {
        None
    }
}

fn parse_radix_strict(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut value = 0.0_f64;
    for ch in digits.chars() {
        let Some(d) = ch.to_digit(radix) else {
            return f64::NAN;
        };
        value = value * f64::from(radix) + f64::from(d);
    }
    value
}

fn is_decimal_literal_byte(b: u8) -> bool {
    b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')
}

fn is_ascii_whitespace(c: char) -> bool {
    c.is_ascii_whitespace()
}

fn strip_sign(s: &str) -> (f64, &str) {
    if let Some(rest) = s.strip_prefix('+') {
        (1.0, rest)
    } else if let Some(rest) = s.strip_prefix('-') {
        (-1.0, rest)
    } else {
        (1.0, s)
    }
}

fn strip_hex_prefix(s: &str) -> Option<&str> {
    let rest = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    if rest.is_empty() { None } else { Some(rest) }
}

fn float_prefix(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let n = bytes.len();

    let int_start = i;
    while i < n && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int_end = i;

    let mut frac_end = int_end;
    if i < n && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if frac_start == i && int_end == int_start {
            // Lone `.` with no digits on either side — not a float.
            return "";
        }
        frac_end = i;
    } else if int_end == int_start {
        return "";
    }

    // Only consume exponent if well-formed ([eE][+-]?digit+); otherwise roll back.
    let exp_start = i;
    if i < n && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < n && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let digits_start = j;
        while j < n && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > digits_start {
            i = j;
        } else {
            i = exp_start;
        }
    }

    let end = if i > frac_end { i } else { frac_end };
    &s[..end]
}

/// ECMAScript WhiteSpace and LineTerminator code points.
pub(crate) fn is_js_whitespace(value: char) -> bool {
    matches!(value, '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}'
        | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}'
        | '\u{3000}' | '\u{FEFF}')
}

pub(crate) fn pow_js(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        return f64::NAN;
    }
    base.powf(exponent)
}

#[cfg(test)]
mod tests {
    use super::{parse_float_js, parse_int_js, string_to_number_js};

    #[test]
    fn formatter_invariants_hold_at_float_boundaries() {
        let below_one = f64::from_bits(1.0_f64.to_bits() - 1);
        for radix in 2..=36 {
            for value in [below_one, 0.5, f64::MIN_POSITIVE, f64::from_bits(1)] {
                let text = super::to_string_radix_js(value, f64::from(radix)).unwrap();
                if radix != 10 {
                    assert!(text.chars().all(|c| c == '.' || c.is_digit(radix)));
                }
            }
            assert!(super::to_string_radix_js(f64::MAX, f64::from(radix)).is_ok());
        }
        assert_eq!(
            super::to_precision_js(f64::from_bits(1), 1.0).unwrap(),
            "5e-324"
        );
        assert_eq!(super::to_precision_js(f64::MAX, 1.0).unwrap(), "2e+308");
        assert_eq!(super::to_precision_js(-0.0, 3.0).unwrap(), "0.00");
    }

    #[test]
    fn parse_int_decimal_prefix() {
        assert_eq!(parse_int_js("42", 10), 42.0);
        assert_eq!(parse_int_js("42px", 10), 42.0);
        assert_eq!(parse_int_js("  42  ", 10), 42.0);
    }

    #[test]
    fn parse_int_negative_and_positive() {
        assert_eq!(parse_int_js("-42", 10), -42.0);
        assert_eq!(parse_int_js("+42", 10), 42.0);
    }

    #[test]
    fn parse_int_hex_with_explicit_radix_16() {
        assert_eq!(parse_int_js("0xff", 16), 255.0);
        assert_eq!(parse_int_js("0XFF", 16), 255.0);
        assert_eq!(parse_int_js("ff", 16), 255.0);
    }

    #[test]
    fn parse_int_radix_zero_auto_detects_hex() {
        assert_eq!(parse_int_js("0xff", 0), 255.0);
        assert_eq!(parse_int_js("010", 0), 10.0); // ES5+: NOT octal
        assert_eq!(parse_int_js("42", 0), 42.0);
    }

    #[test]
    fn parse_int_no_digits_is_nan() {
        assert!(parse_int_js("abc", 10).is_nan());
        assert!(parse_int_js("", 10).is_nan());
        assert!(parse_int_js("   ", 10).is_nan());
        assert!(parse_int_js("0xZZ", 16).is_nan());
    }

    #[test]
    fn parse_int_invalid_radix_is_nan() {
        assert!(parse_int_js("42", 1).is_nan());
        assert!(parse_int_js("42", 37).is_nan());
    }

    #[test]
    fn parse_float_basic() {
        assert_eq!(parse_float_js("1.5"), 1.5);
        assert_eq!(parse_float_js("1.5x"), 1.5);
        assert_eq!(parse_float_js(".5"), 0.5);
        assert_eq!(parse_float_js("42"), 42.0);
    }

    #[test]
    fn parse_float_negative_and_positive() {
        assert_eq!(parse_float_js("-1.5"), -1.5);
        assert_eq!(parse_float_js("+1.5"), 1.5);
    }

    #[test]
    fn parse_float_exponent() {
        assert_eq!(parse_float_js("1e3"), 1000.0);
        assert_eq!(parse_float_js("1.5e2"), 150.0);
        assert_eq!(parse_float_js("1.5e+2"), 150.0);
        assert_eq!(parse_float_js("1.5e-2"), 0.015);
    }

    #[test]
    fn parse_float_infinity() {
        assert_eq!(parse_float_js("Infinity"), f64::INFINITY);
        assert_eq!(parse_float_js("-Infinity"), f64::NEG_INFINITY);
    }

    #[test]
    fn parse_float_no_digits_is_nan() {
        assert!(parse_float_js("abc").is_nan());
        assert!(parse_float_js(".").is_nan());
        assert!(parse_float_js("").is_nan());
        assert!(parse_float_js("   ").is_nan());
    }

    #[test]
    fn parse_float_skips_whitespace() {
        assert_eq!(parse_float_js("  1.5  "), 1.5);
    }

    #[test]
    fn parse_float_partial_exponent_keeps_mantissa() {
        assert_eq!(parse_float_js("1.5e"), 1.5);
    }

    #[test]
    fn number_decimal_and_scientific() {
        assert_eq!(string_to_number_js("123"), 123.0);
        assert_eq!(string_to_number_js("3.5"), 3.5);
        assert_eq!(string_to_number_js("1e3"), 1000.0);
        assert_eq!(string_to_number_js(".5"), 0.5);
        assert_eq!(string_to_number_js("-42"), -42.0);
        assert_eq!(string_to_number_js("  7  "), 7.0);
    }

    #[test]
    fn number_radix_prefixes() {
        assert_eq!(string_to_number_js("0xff"), 255.0);
        assert_eq!(string_to_number_js("0XFF"), 255.0);
        assert_eq!(string_to_number_js("0o17"), 15.0);
        assert_eq!(string_to_number_js("0O17"), 15.0);
        assert_eq!(string_to_number_js("0b101"), 5.0);
        assert_eq!(string_to_number_js("0B101"), 5.0);
        assert_eq!(string_to_number_js("  0xff  "), 255.0);
    }

    #[test]
    fn number_empty_is_zero() {
        assert_eq!(string_to_number_js(""), 0.0);
        assert_eq!(string_to_number_js("   "), 0.0);
    }

    #[test]
    fn number_infinity() {
        assert_eq!(string_to_number_js("Infinity"), f64::INFINITY);
        assert_eq!(string_to_number_js("+Infinity"), f64::INFINITY);
        assert_eq!(string_to_number_js("-Infinity"), f64::NEG_INFINITY);
    }

    #[test]
    fn number_rejects_trailing_junk_and_bad_radix() {
        assert!(string_to_number_js("12px").is_nan());
        assert!(string_to_number_js("0xZZ").is_nan());
        assert!(string_to_number_js("0x").is_nan());
        assert!(string_to_number_js("0b12").is_nan());
        assert!(string_to_number_js("0xff.5").is_nan());
    }

    #[test]
    fn number_rejects_signed_radix_literals() {
        // Sign is not part of the non-decimal integer-literal grammar.
        assert!(string_to_number_js("-0xff").is_nan());
        assert!(string_to_number_js("+0b101").is_nan());
    }

    #[test]
    fn number_rejects_rust_only_float_spellings() {
        // `f64::from_str` accepts these; JS `Number` does not.
        assert!(string_to_number_js("inf").is_nan());
        assert!(string_to_number_js("infinity").is_nan());
        assert!(string_to_number_js("nan").is_nan());
        assert!(string_to_number_js("NaN").is_nan());
    }
}
