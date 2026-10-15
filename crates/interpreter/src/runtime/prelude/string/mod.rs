//! The submilli string and its operations — the Rust port of the prelude's
//! `String` methods.
//!
//! Everything here works in UTF-16 *code-unit* space (`[u16]`), the
//! representation `$string` stores and the one JS string semantics are defined
//! over. Operations never decode to a Rust `String`: that would round-trip
//! through UTF-8 (lossy on lone surrogates, and an allocation on every call).
//! The `$string` ↔ [`Str`] marshalling and host-fn registration live in
//! [`install`]; this file is just the strings.

mod gc_text;
mod install;
pub(crate) mod search;
mod transforms;

pub use install::{declare, install};
pub(crate) use install::{declare_types, string_ctor_call};

use unicode_normalization::UnicodeNormalization;

use crate::runtime::host::fatal_host_error;

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
#[derive(Debug)]
pub struct RangeError(&'static str);

impl RangeError {
    pub fn message(&self) -> &'static str {
        self.0
    }
}

pub type Result<T> = std::result::Result<T, RangeError>;

/// Caps the result of `repeat`, the pads and the case maps, whose length a
/// small input can multiply. This language-level ceiling complements the memory cap; the result
/// is still allocated under the GC limiter.
const MAX_RESULT_UNITS: usize = 32 * 1024 * 1024;

/// `String.prototype.repeat`. `ToInteger(count)` truncates toward zero, leaving
/// three bands — and the guard order below encodes them, so `NaN` must fall
/// through the negative check to the zero case:
///   - `count <= -1` → throws (negative repeat)
///   - `-1 < count < 1`, or `NaN` → `""` (zero repeats)
///   - `count >= 1` → repeat, capped so an overflowing or `+Infinity` count
///     throws instead of allocating (see [`MAX_RESULT_UNITS`]).
///
/// Returns the result's length in units: the receiver repeated that many times.
pub(crate) fn repeat_length(len: usize, count: f64) -> Result<usize> {
    let n = count.trunc();
    if n <= -1.0 {
        return Err(RangeError("Invalid count value"));
    }
    if n.is_nan() || n < 1.0 || len == 0 {
        return Ok(0);
    }
    // The saturating cast makes positive infinity fail the checked bound.
    len.checked_mul(n as usize)
        .filter(|&total| total <= MAX_RESULT_UNITS)
        .ok_or(RangeError("Invalid count value"))
}

/// Trunc-sat `f64`→`i32`, matching the prelude's `i32.trunc_sat_f64_s`: `NaN`→0,
/// out-of-range saturates to `i32::MIN`/`MAX`. Rust's `as` cast is already
/// saturating, so the index methods agree with the Wasm bodies they replace.
fn trunc_sat_i32(x: f64) -> i32 {
    x as i32
}

/// Rejects a builder result that would exceed [`MAX_RESULT_UNITS`], with the
/// `RangeError` ("Invalid string length") JS throws.
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
    search::Search::new(needle, false).find(haystack, from.min(haystack.len()))
}

/// `charAt`: the single code unit at `index`, or `""` out of range.
pub fn char_at(s: &Str, index: f64) -> Str {
    let units = s.units();
    match unit_index(units.len(), index) {
        Some(i) => Str::from_units(vec![units[i]]),
        None => Str::from_units(Vec::new()),
    }
}

/// `at`: like `charAt`, but a negative `index` counts from the end. `None` on
/// OOB — `Array#at` and `Uint8Array#at` both answer `undefined` there, and `""` is
/// indistinguishable from a legitimate empty read.
pub fn at(s: &Str, index: f64) -> Option<Str> {
    let units = s.units();
    at_index(units.len(), index).map(|i| Str::from_units(vec![units[i]]))
}

/// `charCodeAt`: the UTF-16 code unit at `index` as `f64`, or `NaN` out of range.
pub fn char_code_at(s: &Str, index: f64) -> f64 {
    let units = s.units();
    unit_index(units.len(), index).map_or(f64::NAN, |i| units[i] as f64)
}

/// `slice`: negatives count from the end; empty when `start >= end` after
/// normalization.
pub fn slice(s: &Str, start: f64, end: f64) -> Str {
    let units = s.units();
    Str::from_units(units[slice_range(units.len(), start, end)].to_vec())
}

/// `substring`: negatives clamp to 0 (not from the end), and the arguments swap
/// when `start > end`.
pub fn substring(s: &Str, start: f64, end: f64) -> Str {
    let units = s.units();
    Str::from_units(units[substring_range(units.len(), start, end)].to_vec())
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
    let end = from + needle.len();
    search::Search::new(needle, true)
        .find(&haystack[..end], 0)
        .map_or(-1.0, |i| (end - i - needle.len()) as f64)
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
    starts_with_window(haystack.len(), needle.len(), position)
        .is_some_and(|window| &haystack[window] == needle)
}

/// `endsWith`: whether `search` ends at `endPosition` (default end).
pub fn ends_with(s: &Str, search: &Str, end_position: f64) -> bool {
    let haystack = s.units();
    let needle = search.units();
    ends_with_window(haystack.len(), needle.len(), end_position)
        .is_some_and(|window| &haystack[window] == needle)
}

/// The unit `charAt`/`charCodeAt` read for `index`, if it is in range.
pub(super) fn unit_index(len: usize, index: f64) -> Option<usize> {
    let i = trunc_sat_i32(index);
    (i >= 0 && (i as usize) < len).then_some(i as usize)
}

/// The unit `at` reads for `index`, a negative one counting from the end.
pub(super) fn at_index(len: usize, index: f64) -> Option<usize> {
    let len = len as i64;
    let mut i = trunc_sat_i32(index) as i64;
    if i < 0 {
        i += len;
    }
    (i >= 0 && i < len).then_some(i as usize)
}

/// The code point `codePointAt` answers for the unit at an index and the one
/// after it, when a surrogate pair starts there.
pub(super) fn code_point(hi: u16, next: Option<u16>) -> f64 {
    match next {
        Some(lo) if (0xD800..=0xDBFF).contains(&hi) && (0xDC00..=0xDFFF).contains(&lo) => {
            (0x10000 + (((hi as u32) - 0xD800) << 10) + ((lo as u32) - 0xDC00)) as f64
        }
        _ => hi as f64,
    }
}

/// The units `slice(start, end)` keeps.
pub(super) fn slice_range(len: usize, start: f64, end: f64) -> std::ops::Range<usize> {
    let si = normalize_slice_index(trunc_sat_i32(start), len);
    let ei = normalize_slice_index(trunc_sat_i32(end), len);
    si..ei.max(si)
}

/// The units `substring(start, end)` keeps: negatives clamp to 0 and the
/// bounds swap when `start > end`.
pub(super) fn substring_range(len: usize, start: f64, end: f64) -> std::ops::Range<usize> {
    let a = clamp_to_len(trunc_sat_i32(start), len);
    let b = clamp_to_len(trunc_sat_i32(end), len);
    a.min(b)..a.max(b)
}

/// The window of a haystack `startsWith(needle, position)` compares, if the
/// needle fits there.
pub(super) fn starts_with_window(
    len: usize,
    needle_len: usize,
    position: f64,
) -> Option<std::ops::Range<usize>> {
    let pos = clamp_to_len(trunc_sat_i32(position), len);
    (pos + needle_len <= len).then_some(pos..pos + needle_len)
}

/// The window of a haystack `endsWith(needle, endPosition)` compares, if the
/// needle fits there.
pub(super) fn ends_with_window(
    len: usize,
    needle_len: usize,
    end_position: f64,
) -> Option<std::ops::Range<usize>> {
    let end = clamp_to_len(trunc_sat_i32(end_position), len);
    end.checked_sub(needle_len).map(|start| start..end)
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

/// `concat`: the length of `left` followed by `right`.
pub(crate) fn concat_length(left: usize, right: usize) -> wasmtime::Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| fatal_host_error("string length overflow"))
}

/// `padStart` and `padEnd`: the padded length. Repetitions of `pad` fill the
/// string up to `target_length`; it keeps its length when already long enough
/// or when `pad` is empty.
pub(crate) fn pad_length(len: usize, target_length: f64, pad_len: usize) -> Result<usize> {
    let target = trunc_sat_i32(target_length);
    if target <= 0 || target as usize <= len || pad_len == 0 {
        return Ok(len);
    }
    checked_len(target as usize)
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

/// `normalize`: Unicode normalization. `form` must be `"NFC"`/`"NFD"`/`"NFKC"`/
/// `"NFKD"` (default `"NFC"`); any other value throws. A lone surrogate is
/// kept: it neither decomposes nor composes, so the text on each side of it
/// normalizes on its own.
pub fn normalize(s: &Str, form: &Str) -> Result<Str> {
    let form: fn(&str) -> String = match String::from_utf16_lossy(form.units()).as_str() {
        "NFC" => |text| text.nfc().collect(),
        "NFD" => |text| text.nfd().collect(),
        "NFKC" => |text| text.nfkc().collect(),
        "NFKD" => |text| text.nfkd().collect(),
        _ => {
            return Err(RangeError(
                "String.normalize: invalid form — must be 'NFC', 'NFD', 'NFKC', or 'NFKD'",
            ));
        }
    };
    let mut out = Vec::with_capacity(s.units().len());
    let mut run = String::new();
    for scalar in char::decode_utf16(s.units().iter().copied()) {
        match scalar {
            Ok(c) => run.push(c),
            Err(lone) => {
                out.extend(form(&run).encode_utf16());
                run.clear();
                out.push(lone.unpaired_surrogate());
            }
        }
    }
    out.extend(form(&run).encode_utf16());
    Ok(Str::from_units(out))
}

/// `String.fromCharCode(...codes)`: one UTF-16 code unit per argument, through
/// JavaScript's `ToUint16`, so `-1` is U+FFFF.
pub fn from_char_code(codes: &[f64]) -> Str {
    Str::from_units(
        codes
            .iter()
            .map(|&x| crate::runtime::number::to_uint16(x))
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

#[cfg(test)]
mod length_tests {
    use super::*;

    #[test]
    fn length_validation_preserves_range_boundaries() {
        assert_eq!(
            repeat_length(1, MAX_RESULT_UNITS as f64).unwrap(),
            MAX_RESULT_UNITS
        );
        for count in [
            -1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::MAX,
            (MAX_RESULT_UNITS + 1) as f64,
        ] {
            assert!(repeat_length(1, count).is_err());
        }
        for count in [-0.5, 0.0, 0.5, f64::NAN] {
            assert_eq!(repeat_length(1, count).unwrap(), 0);
        }
        assert_eq!(repeat_length(2, 2.9).unwrap(), 4);
        assert_eq!(repeat_length(0, f64::INFINITY).unwrap(), 0);
        assert!(repeat_length(usize::MAX, 2.0).is_err());
        assert_eq!(
            pad_length(1, MAX_RESULT_UNITS as f64, 1).unwrap(),
            MAX_RESULT_UNITS
        );
        assert!(pad_length(1, (MAX_RESULT_UNITS + 1) as f64, 1).is_err());
        assert_eq!(pad_length(2, f64::INFINITY, 0).unwrap(), 2);
        assert_eq!(pad_length(2, f64::NAN, 1).unwrap(), 2);
        assert_eq!(pad_length(2, -1.0, 1).unwrap(), 2);
        assert_eq!(pad_length(2, 4.9, 1).unwrap(), 4);
    }

    #[test]
    fn concat_length_overflow_is_fatal() {
        assert_eq!(concat_length(2, 3).unwrap(), 5);
        assert!(concat_length(usize::MAX, 1).is_err());
    }
}
