//! The submilli string and its operations — the Rust port of the prelude's
//! `String` methods.
//!
//! Everything here works in UTF-16 *code-unit* space (`[u16]`), the
//! representation `$string` stores and the one JS string semantics are defined
//! over. Operations never decode to a Rust `String`: that would round-trip
//! through UTF-8 (lossy on lone surrogates, and an allocation on every call).
//! The `$string` ↔ [`Str`] marshalling and host-fn registration live in
//! [`install`]; this file is just the strings.

mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use unicode_normalization::UnicodeNormalization;

/// A submilli string: its UTF-16 code units. Cheap to slice and concatenate, and
/// faithful to JS — indices, `length`, and surrogate handling are all code-unit
/// based.
pub struct Str(Vec<u16>);

impl Str {
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A string operation rejected its input the way JS would (a `RangeError`).
/// [`install`] raises it as a catchable guest `RangeError`.
pub struct RangeError(&'static str);

impl RangeError {
    pub fn message(&self) -> &'static str {
        self.0
    }
}

pub type Result<T> = std::result::Result<T, RangeError>;

/// Caps a built result so a ported op can't allocate an unbounded host buffer.
/// Rust-side allocations bypass the Wasm GC limiter the prelude's `array.new`
/// answers to, so every builder must bound itself; 32Mi code units sits well
/// above any realistic string yet clear of a memory-exhaustion risk.
const MAX_RESULT_UNITS: usize = 32 * 1024 * 1024;

/// `String.prototype.repeat`. `ToInteger(count)` truncates toward zero, leaving
/// three bands — and the guard order below encodes them, so `NaN` must fall
/// through the negative check to the zero case:
///   - `count <= -1` → throws (negative repeat)
///   - `-1 < count < 1`, or `NaN` → `""` (zero repeats)
///   - `count >= 1` → repeat, capped so an overflowing or `+Infinity` count
///     throws instead of allocating (see [`MAX_RESULT_UNITS`]).
pub fn repeat(s: &Str, count: f64) -> Result<Str> {
    let n = count.trunc();
    if n <= -1.0 {
        return Err(RangeError("Invalid count value"));
    }
    if n.is_nan() || n < 1.0 || s.is_empty() {
        return Ok(Str::from_units(Vec::new()));
    }
    let n = n as usize; // saturates `+Infinity` to `usize::MAX`
    let total = s
        .len()
        .checked_mul(n)
        .filter(|&t| t <= MAX_RESULT_UNITS)
        .ok_or(RangeError("Invalid count value"))?;
    let mut units = Vec::with_capacity(total);
    for _ in 0..n {
        units.extend_from_slice(s.units());
    }
    Ok(Str::from_units(units))
}

/// Trunc-sat `f64`→`i32`, matching the prelude's `i32.trunc_sat_f64_s`: `NaN`→0,
/// out-of-range saturates to `i32::MIN`/`MAX`. Rust's `as` cast is already
/// saturating, so the index methods agree with the Wasm bodies they replace.
fn trunc_sat_i32(x: f64) -> i32 {
    x as i32
}

/// Rejects a builder result that would exceed [`MAX_RESULT_UNITS`]. JS throws a
/// `RangeError` ("Invalid string length") here; the Wasm bodies trapped through
/// the GC limiter, which a Rust `Vec` would bypass — so host builders bound
/// themselves explicitly.
fn checked_len(n: usize) -> Result<usize> {
    if n > MAX_RESULT_UNITS {
        Err(RangeError("Invalid string length"))
    } else {
        Ok(n)
    }
}

/// Clamp an index into `[0, len]` (negatives to 0, overshoot to `len`).
fn clamp_to_len(x: i32, len: usize) -> usize {
    if x < 0 { 0 } else { (x as usize).min(len) }
}

/// JS slice-style index normalization: a negative index counts from the end,
/// then the result is clamped into `[0, len]`.
fn normalize_slice_index(x: i32, len: usize) -> usize {
    if x < 0 {
        let from_end = x as i64 + len as i64;
        if from_end < 0 {
            0
        } else {
            (from_end as usize).min(len)
        }
    } else {
        (x as usize).min(len)
    }
}

/// First index `>= from` at which `needle` occurs in `haystack`. An empty
/// `needle` matches at `min(from, len)` — JS `indexOf`/`includes` search order.
fn raw_index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(haystack.len()));
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

/// `charAt`: the single code unit at `index`, or `""` out of range.
pub fn char_at(s: &Str, index: f64) -> Str {
    let units = s.units();
    let i = trunc_sat_i32(index);
    if i < 0 || i as usize >= units.len() {
        return Str::from_units(Vec::new());
    }
    Str::from_units(vec![units[i as usize]])
}

/// `at`: like `charAt`, but a negative `index` counts from the end. `None` on
/// OOB — `Array#at` and `Uint8Array#at` both answer `null` there, and `""` is
/// indistinguishable from a legitimate empty read.
pub fn at(s: &Str, index: f64) -> Option<Str> {
    let units = s.units();
    let len = units.len() as i64;
    let mut i = trunc_sat_i32(index) as i64;
    if i < 0 {
        i += len;
    }
    if i < 0 || i >= len {
        return None;
    }
    Some(Str::from_units(vec![units[i as usize]]))
}

/// `charCodeAt`: the UTF-16 code unit at `index` as `f64`, or `NaN` out of range.
pub fn char_code_at(s: &Str, index: f64) -> f64 {
    let units = s.units();
    let i = trunc_sat_i32(index);
    if i < 0 || i as usize >= units.len() {
        return f64::NAN;
    }
    units[i as usize] as f64
}

/// `codePointAt`: like `charCodeAt`, but decodes a surrogate pair into the full
/// code point. `NaN` out of range.
pub fn code_point_at(s: &Str, index: f64) -> f64 {
    let units = s.units();
    let i = trunc_sat_i32(index);
    if i < 0 || i as usize >= units.len() {
        return f64::NAN;
    }
    let i = i as usize;
    let hi = units[i];
    if (0xD800..=0xDBFF).contains(&hi) && i + 1 < units.len() {
        let lo = units[i + 1];
        if (0xDC00..=0xDFFF).contains(&lo) {
            let cp = 0x10000 + (((hi as u32) - 0xD800) << 10) + ((lo as u32) - 0xDC00);
            return cp as f64;
        }
    }
    hi as f64
}

/// `slice`: negatives count from the end; empty when `start >= end` after
/// normalization.
pub fn slice(s: &Str, start: f64, end: f64) -> Str {
    let units = s.units();
    let len = units.len();
    let si = normalize_slice_index(trunc_sat_i32(start), len);
    let ei = normalize_slice_index(trunc_sat_i32(end), len);
    if ei <= si {
        return Str::from_units(Vec::new());
    }
    Str::from_units(units[si..ei].to_vec())
}

/// `substring`: negatives clamp to 0 (not from the end), and the arguments swap
/// when `start > end`.
pub fn substring(s: &Str, start: f64, end: f64) -> Str {
    let units = s.units();
    let len = units.len();
    let a = clamp_to_len(trunc_sat_i32(start), len);
    let b = clamp_to_len(trunc_sat_i32(end), len);
    let (si, ei) = if a > b { (b, a) } else { (a, b) };
    Str::from_units(units[si..ei].to_vec())
}

/// `indexOf`: first match at or after `fromIndex`, else `-1`.
pub fn index_of(s: &Str, search: &Str, from: f64) -> f64 {
    let haystack = s.units();
    let from = clamp_to_len(trunc_sat_i32(from), haystack.len());
    match raw_index_of(haystack, search.units(), from) {
        Some(i) => i as f64,
        None => -1.0,
    }
}

/// `lastIndexOf`: last match at or before `fromIndex` (default end), else `-1`.
pub fn last_index_of(s: &Str, search: &Str, from: f64) -> f64 {
    let haystack = s.units();
    let needle = search.units();
    if needle.len() > haystack.len() {
        return -1.0;
    }
    let max_start = haystack.len() - needle.len();
    let from = clamp_to_len(trunc_sat_i32(from), max_start);
    if needle.is_empty() {
        return from as f64;
    }
    (0..=from)
        .rev()
        .find(|&i| &haystack[i..i + needle.len()] == needle)
        .map_or(-1.0, |i| i as f64)
}

/// `includes`: whether `search` occurs at or after `fromIndex`.
pub fn includes(s: &Str, search: &Str, from: f64) -> bool {
    let haystack = s.units();
    let from = clamp_to_len(trunc_sat_i32(from), haystack.len());
    raw_index_of(haystack, search.units(), from).is_some()
}

/// `startsWith`: whether `search` sits at `position`.
pub fn starts_with(s: &Str, search: &Str, position: f64) -> bool {
    let haystack = s.units();
    let needle = search.units();
    let pos = clamp_to_len(trunc_sat_i32(position), haystack.len());
    pos + needle.len() <= haystack.len() && &haystack[pos..pos + needle.len()] == needle
}

/// `endsWith`: whether `search` ends at `endPosition` (default end).
pub fn ends_with(s: &Str, search: &Str, end_position: f64) -> bool {
    let haystack = s.units();
    let needle = search.units();
    let end = clamp_to_len(trunc_sat_i32(end_position), haystack.len());
    match end.checked_sub(needle.len()) {
        Some(start) => &haystack[start..end] == needle,
        None => false,
    }
}

/// Structural equality over code units — the `===`/`==` operator primitive.
pub fn eq(a: &Str, b: &Str) -> bool {
    a.units() == b.units()
}

/// Lexicographic compare over code units — the relational-operator primitive.
/// Returns a sign: the first differing units' difference, else the length
/// difference (consumers test the sign only).
pub fn cmp(a: &Str, b: &Str) -> i32 {
    for (&au, &bu) in a.units().iter().zip(b.units()) {
        let diff = i32::from(au) - i32::from(bu);
        if diff != 0 {
            return diff;
        }
    }
    a.len() as i32 - b.len() as i32
}

/// `concat`: `self` followed by `other`.
pub fn concat(s: &Str, other: &Str) -> Str {
    let mut units = Vec::with_capacity(s.len() + other.len());
    units.extend_from_slice(s.units());
    units.extend_from_slice(other.units());
    Str::from_units(units)
}

/// `padStart`: prepend repetitions of `pad` until the string reaches
/// `target_length`. Returns `self` when already long enough or `pad` is empty.
pub fn pad_start(s: &Str, target_length: f64, pad: &Str) -> Result<Str> {
    let units = s.units();
    let pad_units = pad.units();
    let target = trunc_sat_i32(target_length);
    if target <= units.len() as i32 || pad_units.is_empty() {
        return Ok(Str::from_units(units.to_vec()));
    }
    let target = checked_len(target as usize)?;
    let needed = target - units.len();
    let mut out = Vec::with_capacity(target);
    while out.len() < needed {
        let chunk = (needed - out.len()).min(pad_units.len());
        out.extend_from_slice(&pad_units[..chunk]);
    }
    out.extend_from_slice(units);
    Ok(Str::from_units(out))
}

/// `padEnd`: append repetitions of `pad` until the string reaches
/// `target_length`. Returns `self` when already long enough or `pad` is empty.
pub fn pad_end(s: &Str, target_length: f64, pad: &Str) -> Result<Str> {
    let units = s.units();
    let pad_units = pad.units();
    let target = trunc_sat_i32(target_length);
    if target <= units.len() as i32 || pad_units.is_empty() {
        return Ok(Str::from_units(units.to_vec()));
    }
    let target = checked_len(target as usize)?;
    let mut out = Vec::with_capacity(target);
    out.extend_from_slice(units);
    while out.len() < target {
        let chunk = (target - out.len()).min(pad_units.len());
        out.extend_from_slice(&pad_units[..chunk]);
    }
    Ok(Str::from_units(out))
}

/// `isWellFormed`: `true` when the string contains no lone surrogate.
pub fn is_well_formed(s: &Str) -> bool {
    let u = s.units();
    let mut i = 0;
    while i < u.len() {
        let c = u[i];
        if (c & 0xFC00) == 0xD800 {
            if i + 1 < u.len() && (u[i + 1] & 0xFC00) == 0xDC00 {
                i += 2;
            } else {
                return false;
            }
        } else if (c & 0xFC00) == 0xDC00 {
            return false;
        } else {
            i += 1;
        }
    }
    true
}

/// `toWellFormed`: a copy with every lone surrogate replaced by U+FFFD.
pub fn to_well_formed(s: &Str) -> Str {
    let mut out = s.units().to_vec();
    let len = out.len();
    let mut i = 0;
    while i < len {
        let c = out[i];
        if (c & 0xFC00) == 0xD800 {
            if i + 1 < len && (out[i + 1] & 0xFC00) == 0xDC00 {
                i += 2;
            } else {
                out[i] = 0xFFFD;
                i += 1;
            }
        } else if (c & 0xFC00) == 0xDC00 {
            out[i] = 0xFFFD;
            i += 1;
        } else {
            i += 1;
        }
    }
    Str::from_units(out)
}

/// Decode to a Rust `String` for the Unicode-crate operations (case folding,
/// normalization). This is the one sanctioned UTF-8 round-trip — those crates
/// work on `char`s — and matches the prior `submilli:string` decode
/// (`String::from_utf16_lossy`, so a lone surrogate becomes U+FFFD).
fn decode(s: &Str) -> String {
    String::from_utf16_lossy(s.units())
}

fn encode(s: String) -> Str {
    Str::from_units(s.encode_utf16().collect())
}

/// `toUpperCase`: Unicode-correct upper-casing.
pub fn to_upper_case(s: &Str) -> Str {
    encode(decode(s).to_uppercase())
}

/// `toLowerCase`: Unicode-correct lower-casing.
pub fn to_lower_case(s: &Str) -> Str {
    encode(decode(s).to_lowercase())
}

/// `trim`: strip leading and trailing whitespace.
pub fn trim(s: &Str) -> Str {
    encode(decode(s).trim().to_string())
}

/// `trimStart`: strip leading whitespace.
pub fn trim_start(s: &Str) -> Str {
    encode(decode(s).trim_start().to_string())
}

/// `trimEnd`: strip trailing whitespace.
pub fn trim_end(s: &Str) -> Str {
    encode(decode(s).trim_end().to_string())
}

/// `normalize`: Unicode normalization. `form` must be `"NFC"`/`"NFD"`/`"NFKC"`/
/// `"NFKD"` (default `"NFC"`); any other value throws.
pub fn normalize(s: &Str, form: &Str) -> Result<Str> {
    let text = decode(s);
    let out: String = match decode(form).as_str() {
        "NFC" => text.nfc().collect(),
        "NFD" => text.nfd().collect(),
        "NFKC" => text.nfkc().collect(),
        "NFKD" => text.nfkd().collect(),
        _ => {
            return Err(RangeError(
                "String.normalize: invalid form — must be 'NFC', 'NFD', 'NFKC', or 'NFKD'",
            ));
        }
    };
    Ok(encode(out))
}

/// `String.fromCharCode(...codes)`: one UTF-16 code unit per argument. Values
/// trunc-sat to unsigned then mask to 16 bits, so negatives saturate to 0
/// (matching the `Uint8Array` constructor's divergence from JS's modulo wrap).
pub fn from_char_code(codes: &[f64]) -> Str {
    Str::from_units(
        codes
            .iter()
            .map(|&x| ((x as u32) & 0xFFFF) as u16)
            .collect(),
    )
}

/// `String.fromCodePoint(...codePoints)`: astral code points encode as surrogate
/// pairs; a negative, non-integer, or `> U+10FFFF` value throws.
pub fn from_code_point(codes: &[f64]) -> Result<Str> {
    let mut out = Vec::with_capacity(codes.len());
    for &cpf in codes {
        let cp = trunc_sat_i32(cpf);
        if !(0..=0x10FFFF).contains(&cp) || cpf != cp as f64 {
            return Err(RangeError("invalid code point"));
        }
        if cp < 0x10000 {
            out.push(cp as u16);
        } else {
            let v = (cp - 0x10000) as u32;
            out.push((0xD800 + (v >> 10)) as u16);
            out.push((0xDC00 + (v & 0x3FF)) as u16);
        }
    }
    Ok(Str::from_units(out))
}
