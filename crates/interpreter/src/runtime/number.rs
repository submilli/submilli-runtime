//! JavaScript-compatible number parsers and formatters.

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
pub fn to_fixed_js(x: f64, digits: f64) -> Result<String, String> {
    let d = digits.trunc();
    if !(0.0..=100.0).contains(&d) || digits.is_nan() {
        return Err("toFixed digits must be between 0 and 100".to_string());
    }
    if !x.is_finite() || x.abs() >= 1e21 {
        return Ok(format_number_js(x));
    }
    let digits = d as usize;
    let magnitude = fixed_ties_away_from_zero(x.abs(), digits);
    // JS keeps the sign of a negative value that rounds to zero (`-0.0001`
    // gives "-0.00") but drops it for -0 itself.
    Ok(if x < 0.0 {
        format!("-{magnitude}")
    } else {
        magnitude
    })
}

/// Digits past the decimal point that spell any f64 exactly: the smallest
/// subnormal has 1074 of them.
const EXACT_FRACTION_DIGITS: usize = 1074;

/// The non-negative `x` with `digits` fraction digits, an exact tie rounded
/// up as JS's `toFixed` picks "the larger n". Rust's formatting rounds the
/// binary value correctly but breaks an exact tie to even, so
/// `(2.25).toFixed(1)` would be "2.2" where JS gives "2.3".
fn fixed_ties_away_from_zero(x: f64, digits: usize) -> String {
    let rounded = format!("{x:.digits$}");
    // Only a value whose next digit rounds to 5 can sit on a tie; checking
    // that first spares the exact spelling for nearly every call.
    let one_more = digits + 1;
    if !format!("{x:.one_more$}").ends_with('5') {
        return rounded;
    }
    let exact = format!("{x:.EXACT_FRACTION_DIGITS$}");
    let Some(point) = exact.find('.') else {
        return rounded;
    };
    let fraction_end = point + 1 + digits;
    let Some(rest) = exact.get(fraction_end..) else {
        return rounded;
    };
    let is_tie = rest
        .strip_prefix('5')
        .is_some_and(|tail| tail.bytes().all(|b| b == b'0'));
    if !is_tie {
        return rounded;
    }
    let kept_end = if digits == 0 { point } else { fraction_end };
    increment_last_digit(&exact[..kept_end])
}

/// `number`, a decimal spelled with ASCII digits and at most one point, plus
/// one unit in its last digit: "2.2" gives "2.3" and "9.99" gives "10.00".
fn increment_last_digit(number: &str) -> String {
    let mut digits = number.to_owned().into_bytes();
    for byte in digits.iter_mut().rev() {
        match *byte {
            b'.' => continue,
            b'9' => *byte = b'0',
            _ => {
                *byte += 1;
                return ascii_text(digits);
            }
        }
    }
    digits.insert(0, b'1');
    ascii_text(digits)
}

/// `bytes`, ASCII by construction, as a `String`; nothing is replaced.
fn ascii_text(bytes: Vec<u8>) -> String {
    bytes.into_iter().map(char::from).collect()
}

/// JS-spec `Number#toExponential(fractionDigits)`. A NaN `fractionDigits`
/// is the omitted-argument sentinel: use however many digits uniquely
/// identify the value.
pub fn to_exponential_js(x: f64, fraction_digits: f64) -> Result<String, String> {
    if !x.is_finite() {
        return Ok(format_number_js(x));
    }
    // JavaScript writes `-0` without its sign; `{:e}` would keep it.
    let x = if x == 0.0 { 0.0 } else { x };
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
    // A value that is not finite spells its name before the precision's
    // range is checked (ECMA-262 `Number.prototype.toPrecision`, step 4).
    if precision.is_nan() || !x.is_finite() {
        return Ok(format_number_js(x));
    }
    let p = precision.trunc();
    if !(1.0..=100.0).contains(&p) {
        return Err("toPrecision precision must be between 1 and 100".to_string());
    }
    let p = p as usize;
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
/// ECMA-262 leaves the digits for a radix other than 10 to the
/// implementation. This follows V8's `DoubleToRadixCString`, so the text is
/// what Node prints: the fraction stops once its digits tell the value from
/// its neighbors (within half the gap to the next double), and an integer
/// part at or above 2^53 writes zeros where the double has no digits.
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
    let digit_char =
        |digit: u32| char::from_digit(digit, r).expect("digit is below the validated radix");

    let value = x.abs();
    let (fraction_digits, carry) = radix_fraction_digits(value, r);
    let integer_digits = radix_integer_digits(value.floor() + if carry { 1.0 } else { 0.0 }, r);

    let mut out = String::with_capacity(integer_digits.len() + fraction_digits.len() + 2);
    if x < 0.0 {
        out.push('-');
    }
    out.extend(integer_digits.into_iter().map(digit_char));
    if !fraction_digits.is_empty() {
        out.push('.');
        out.extend(fraction_digits.into_iter().map(digit_char));
    }
    Ok(out)
}

/// The radix-`r` digits after the point of `value`, most significant first,
/// and whether rounding the last digit carried into the integer part.
///
/// The digits stop once the remainder is within `delta`, half the gap to the
/// next double: past that, they no longer separate the value from its
/// neighbor. The smallest subnormal bounds `delta` below.
fn radix_fraction_digits(value: f64, r: u32) -> (Vec<u32>, bool) {
    let base = f64::from(r);
    let mut fraction = value - value.floor();
    let mut delta = (0.5 * (value.next_up() - value)).max(f64::from_bits(1));
    let mut digits = Vec::new();
    while fraction >= delta {
        // `fraction` is in [0, 1). Even its largest f64 value times the exact
        // integer radix is below the midpoint between r and its previous
        // representable value, so the digit stays below r.
        fraction *= base;
        delta *= base;
        let digit = fraction.trunc() as u32;
        digits.push(digit);
        fraction -= f64::from(digit);
        // Round half to even on the last digit.
        if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
            let carry = round_up(&mut digits, r);
            return (digits, carry);
        }
    }
    (digits, false)
}

/// Adds one to the last of `digits`, dropping each trailing digit that
/// becomes `r`. Returns whether the carry ran past the first digit.
fn round_up(digits: &mut Vec<u32>, r: u32) -> bool {
    while let Some(last) = digits.pop() {
        if last + 1 < r {
            digits.push(last + 1);
            return false;
        }
    }
    true
}

/// The radix-`r` digits of the non-negative `integer`, most significant
/// first (collected least significant first, then reversed). While the
/// quotient is at or above 2^53 the double has no digit for the place, so it
/// reads as zero. Each remainder is in [0, r), and the
/// truncating cast takes its digit as V8 does.
fn radix_integer_digits(mut integer: f64, r: u32) -> Vec<u32> {
    let base = f64::from(r);
    let mut digits = Vec::new();
    while integer / base >= TWO_POW_53 {
        integer /= base;
        digits.push(0);
    }
    loop {
        let remainder = integer % base;
        digits.push(remainder as u32);
        integer = (integer - remainder) / base;
        if integer <= 0.0 {
            digits.reverse();
            return digits;
        }
    }
}

/// 2^53, where a double's integers stop being consecutive.
const TWO_POW_53: f64 = 9_007_199_254_740_992.0;

/// `parseInt(s, radix)` with the radix argument as JavaScript reads it:
/// `ToInt32`, where 0 (including `NaN` and an omitted radix) means "detect a
/// `0x` prefix, else base 10". Any other value outside 2–36 makes the result
/// `NaN`.
pub fn parse_int_with_radix(input: &str, radix: f64) -> f64 {
    match u32::try_from(to_int32(radix)) {
        Ok(radix @ (0 | 2..=36)) => parse_int_js(input, radix),
        _ => f64::NAN,
    }
}

/// JS-spec `parseInt(s, radix)` for an already-converted radix.
///
/// `radix == 0` means auto-detect: `0x` / `0X` prefix → base 16,
/// otherwise base 10. `radix == 16` also accepts the `0x` prefix.
/// Other radices in `[2, 36]` are taken literally; out-of-range
/// radices return `NaN`.
pub fn parse_int_js(input: &str, radix: u32) -> f64 {
    let s = input.trim_start_matches(is_js_whitespace);
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
    let s = input.trim_start_matches(is_js_whitespace);
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

fn strip_sign(s: &str) -> (f64, &str) {
    if let Some(rest) = s.strip_prefix('+') {
        (1.0, rest)
    } else if let Some(rest) = s.strip_prefix('-') {
        (-1.0, rest)
    } else {
        (1.0, s)
    }
}

/// The digits after a `0x`/`0X` prefix. A bare prefix leaves none, which
/// `parse_int_js` reports as `NaN`, as JavaScript does.
fn strip_hex_prefix(s: &str) -> Option<&str> {
    s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
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
    use super::{parse_float_js, parse_int_js, string_to_number_js, to_fixed_js, to_uint8};

    #[test]
    fn to_fixed_matches_node_including_exact_ties() {
        // Expected strings are Node 24's.
        let cases: [(f64, f64, &str); 22] = [
            (2.25, 1.0, "2.3"),
            (0.125, 2.0, "0.13"),
            (2.5, 0.0, "3"),
            (-2.25, 1.0, "-2.3"),
            (0.5, 0.0, "1"),
            (1.5, 0.0, "2"),
            (-0.5, 0.0, "-1"),
            (9.5, 0.0, "10"),
            (99.995, 2.0, "100.00"),
            (0.0625, 3.0, "0.063"),
            (1.005, 2.0, "1.00"),
            (1.45, 1.0, "1.4"),
            (8.345, 2.0, "8.35"),
            (-0.0001, 2.0, "-0.00"),
            (-0.0, 2.0, "0.00"),
            (0.0, 0.0, "0"),
            (123.456, 10.0, "123.4560000000"),
            (0.1, 20.0, "0.10000000000000000555"),
            (5e-324, 2.0, "0.00"),
            (100000000000000070000.0, 1.0, "100000000000000065536.0"),
            (0.15, 1.0, "0.1"),
            (1e-7, 8.0, "0.00000010"),
        ];
        for (x, digits, expected) in cases {
            assert_eq!(
                to_fixed_js(x, digits).unwrap(),
                expected,
                "({x}).toFixed({digits})"
            );
        }
    }

    #[test]
    fn to_uint8_wraps_modulo_256() {
        assert_eq!(to_uint8(7.0), 7);
        assert_eq!(to_uint8(256.0), 0);
        assert_eq!(to_uint8(257.9), 1);
        assert_eq!(to_uint8(-1.0), 255);
        assert_eq!(to_uint8(-1.5), 255);
        assert_eq!(to_uint8(-256.0), 0);
        assert_eq!(to_uint8(-0.5), 0);
        assert_eq!(to_uint8(f64::NAN), 0);
        assert_eq!(to_uint8(f64::INFINITY), 0);
        assert_eq!(to_uint8(f64::NEG_INFINITY), 0);
        assert_eq!(to_uint8(2f64.powi(60) + 512.0), 0);
    }

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
    fn radix_digits_match_node() {
        // Expected values from Node 24 (V8's `DoubleToRadixCString`).
        for (value, radix, expected) in [
            (
                0.1,
                2,
                "0.0001100110011001100110011001100110011001100110011001101",
            ),
            (0.1, 36, "0.3lllllllllm"),
            (1e21, 36, "5v1j4f4ds7c000"),
            (0.1, 3, "0.0022002200220022002200220022002201"),
            (-0.1, 8, "-0.0631463146314631464"),
            (255.0, 16, "ff"),
            (0.5, 2, "0.1"),
            (-0.0, 2, "0"),
            (
                f64::MAX,
                36,
                "1a1e4vngaiqo0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ),
        ] {
            assert_eq!(
                super::to_string_radix_js(value, radix as f64).unwrap(),
                expected
            );
        }
        let smallest = super::to_string_radix_js(f64::from_bits(1), 2.0).unwrap();
        assert_eq!(smallest.len(), 1076);
        assert!(smallest.ends_with('1'));
    }

    #[test]
    fn radix_digits_match_node_across_radixes() {
        // Node 24: values just below an integer or a carry, the subnormal
        // boundary, past 2^53, and repeating fractions.
        for (value, radix, expected) in [
            (
                0.9999999999999999,
                2,
                "0.11111111111111111111111111111111111111111111111111111",
            ),
            (0.9999999999999999, 3, "0.222222222222222222222222222222222"),
            (0.9999999999999999, 7, "0.6666666666666666666"),
            (0.9999999999999999, 16, "0.fffffffffffff8"),
            (0.9999999999999999, 36, "0.zzzzzzzzzza"),
            (
                4503599627370495.5,
                2,
                "1111111111111111111111111111111111111111111111111111.1",
            ),
            (4503599627370495.5, 3, "210212120220220211102011121012020.2"),
            (4503599627370495.5, 7, "2523420223060136361.4"),
            (4503599627370495.5, 16, "fffffffffffff.8"),
            (4503599627370495.5, 36, "18ce53un18f.i"),
            (
                35.99999999999999,
                2,
                "100011.11111111111111111111111111111111111111111111111",
            ),
            (35.99999999999999, 3, "1022.222222222222222222222222222222"),
            (35.99999999999999, 7, "50.66666666666666665"),
            (35.99999999999999, 16, "23.fffffffffffe"),
            (35.99999999999999, 36, "z.zzzzzzzzz"),
            (
                2.2250738585072014e-308,
                2,
                "0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001",
            ),
            (
                2.2250738585072014e-308,
                3,
                "0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000102002100002110201011001212212212",
            ),
            (
                2.2250738585072014e-308,
                7,
                "0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006300034234640101632",
            ),
            (
                2.2250738585072014e-308,
                16,
                "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004",
            ),
            (
                2.2250738585072014e-308,
                36,
                "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000034lmua2oev",
            ),
            (
                1e300,
                2,
                "1011111100100001111001000100000000000011101011001110000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                1e300,
                3,
                "20222221112102120111122202200001200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                1e300,
                7,
                "6562255533065302131000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                1e300,
                16,
                "17e43c8800759c00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                1e300,
                36,
                "fhgyjdfcg7s00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            ),
            (
                -1e-300,
                2,
                "-0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010101011011100001111111000010111110001111001101011001",
            ),
            (
                -1e-300,
                3,
                "-0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001021201112222120202101222201001001",
            ),
            (
                -1e-300,
                7,
                "-0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010105550636136256015",
            ),
            (
                -1e-300,
                16,
                "-0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ab70fe17c79ac8",
            ),
            (
                -1e-300,
                36,
                "-0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002box4rhe6qp",
            ),
            (
                9007199254740994.0,
                2,
                "100000000000000000000000000000000000000000000000000010",
            ),
            (9007199254740994.0, 3, "1121202011211211122211100012101111"),
            (9007199254740994.0, 7, "5350140446150306056"),
            (9007199254740994.0, 16, "20000000000002"),
            (9007199254740994.0, 36, "2gosa7pa2gy"),
            (
                123.456,
                2,
                "1111011.0111010010111100011010100111111011111001110111",
            ),
            (123.456, 3, "11120.11002210211000212022212010001"),
            (123.456, 7, "234.312256641535441"),
            (123.456, 16, "7b.74bc6a7ef9dc"),
            (123.456, 36, "3f.gez4w97ry"),
            (
                1.0 / 3.0,
                2,
                "0.010101010101010101010101010101010101010101010101010101",
            ),
            (1.0 / 3.0, 3, "0.1"),
            (1.0 / 3.0, 7, "0.2222222222222222222"),
            (1.0 / 3.0, 16, "0.55555555555554"),
            (1.0 / 3.0, 36, "0.c"),
            (
                std::f64::consts::PI,
                2,
                "11.001001000011111101101010100010001000010110100011",
            ),
            (std::f64::consts::PI, 3, "10.010211012222010211002111110222"),
            (std::f64::consts::PI, 7, "3.066365143203613411"),
            (std::f64::consts::PI, 16, "3.243f6a8885a3"),
            (std::f64::consts::PI, 36, "3.53i5ab8p5f"),
        ] {
            assert_eq!(
                super::to_string_radix_js(value, f64::from(radix)).unwrap(),
                expected,
                "{value} in radix {radix}"
            );
        }
    }

    #[test]
    fn exponential_and_precision_edges_match_node() {
        assert_eq!(super::to_exponential_js(-0.0, f64::NAN).unwrap(), "0e+0");
        assert_eq!(super::to_exponential_js(-0.0, 2.0).unwrap(), "0.00e+0");
        assert_eq!(
            super::to_precision_js(f64::INFINITY, 200.0).unwrap(),
            "Infinity"
        );
        assert_eq!(
            super::to_precision_js(f64::NEG_INFINITY, 0.0).unwrap(),
            "-Infinity"
        );
        assert_eq!(super::to_precision_js(f64::NAN, 101.0).unwrap(), "NaN");
        assert!(super::to_precision_js(1.0, 0.0).is_err());
        assert!(super::to_precision_js(1.0, 101.0).is_err());
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
