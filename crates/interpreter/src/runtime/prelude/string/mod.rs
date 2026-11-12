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

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{Collator, CollatorBorrowed};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::{
    canonical_combining_class, decompose_canonical, decompose_compatible,
};

use crate::runtime::fuel;
use crate::runtime::limits::HostBytes;

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
/// four bands — and the guard order below encodes them, so `NaN` must fall
/// through the throwing checks to the zero case:
///   - `count <= -1` or `+Infinity` → throws, whatever the receiver, since
///     JavaScript checks the count before it looks at the string
///   - `-1 < count < 1`, or `NaN` → `""` (zero repeats)
///   - an empty receiver → `""`
///   - otherwise repeat, capped so an overflowing count throws instead of
///     allocating (see [`MAX_RESULT_UNITS`]).
///
/// Returns the result's length in units: the receiver repeated that many times.
pub(crate) fn repeat_length(len: usize, count: f64) -> Result<usize> {
    let n = count.trunc();
    if n <= -1.0 || n == f64::INFINITY {
        return Err(RangeError("Invalid count value"));
    }
    if n.is_nan() || n < 1.0 || len == 0 {
        return Ok(0);
    }
    // A finite count past `usize::MAX` saturates and fails the bound.
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

/// The unit `s[index]` reads, on the terms `arr[index]` has: a non-negative
/// integer below the length (`-0` reads unit 0). Anything else throws.
pub(super) fn string_index_unit(len: usize, index: f64) -> Option<usize> {
    (index >= 0.0 && index < len as f64 && index == index.floor()).then_some(index as usize)
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

/// The units `substr(start, length)` keeps: a negative start counts from the
/// end, and the length clamps into what remains.
pub(super) fn substr_range(len: usize, start: f64, length: f64) -> std::ops::Range<usize> {
    let from = normalize_slice_index(trunc_sat_i32(start), len);
    let count = clamp_to_len(trunc_sat_i32(length), len - from);
    from..from + count
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

/// The Unicode Collation Algorithm's root collation with the defaults
/// `Intl.Collator` uses for `localeCompare` (tertiary strength, punctuation
/// not ignored), and the same collation at primary strength. Both borrow
/// compiled-in data and hold no per-call state, so one pair serves every store.
struct RootCollators {
    primary: CollatorBorrowed<'static>,
    tertiary: CollatorBorrowed<'static>,
}

static ROOT_COLLATORS: std::sync::LazyLock<std::result::Result<RootCollators, String>> =
    std::sync::LazyLock::new(|| {
        let mut primary_only = CollatorOptions::default();
        primary_only.strength = Some(Strength::Primary);
        let primary = Collator::try_new(Default::default(), primary_only);
        let tertiary = Collator::try_new(Default::default(), Default::default());
        match (primary, tertiary) {
            (Ok(primary), Ok(tertiary)) => Ok(RootCollators { primary, tertiary }),
            (Err(error), _) | (_, Err(error)) => Err(error.to_string()),
        }
    });

/// Strings at most this long on both sides are compared by ICU4X directly.
/// Its direct comparison steps back over a shared prefix one character at a
/// time, moving the work done so far, and buffers every collation element
/// while the primary weights agree; at this length both stay small. Longer
/// strings are compared by sort keys, which do neither.
const DIRECT_COMPARE_UNITS: usize = 256;

/// Native bytes a direct comparison may buffer per decomposed character, on
/// either side. Measured peaks reach 112 (U+1F19C, seven collation elements).
const DIRECT_BYTES_PER_CHARACTER: u64 = 128;

/// Native bytes ICU4X buffers per combining mark in a run while it writes a
/// sort key. Measured peaks reach 37 (U+1DD4).
const RUN_BYTES_PER_MARK: u64 = 64;

/// Native bytes ICU4X buffers for the secondary, tertiary and case levels of a
/// sort key, per byte of the string's primary key plus per decomposed
/// character. Measured peaks reach 3 per primary byte (U+1F19C), and for
/// characters with no primary weight 6 per character just past a doubling of
/// the buffer (U+0A03) and 7 for some pairs (U+0A03 U+0897).
const LEVEL_BYTES_PER_PRIMARY_BYTE: u64 = 6;
const LEVEL_BYTES_PER_CHARACTER: u64 = 8;

/// Which sort key [`sort_key`] writes: the collator it uses, and so which
/// buffers ICU4X fills.
#[derive(Clone, Copy)]
enum KeyLevel {
    /// Primary weights only: ICU4X buffers nothing beyond runs of marks.
    Primary,
    /// All levels, for strings whose primary keys of this length agree.
    Tertiary { primary_key_bytes: u64 },
}

/// Fuel and native memory for [`locale_compare`], each charged or admitted
/// before the step that spends it.
pub(crate) trait CollationBudget {
    fn charge(&mut self, rate: fuel::Rate, units: u64) -> wasmtime::Result<()>;
    fn admit(&mut self, bytes: u64) -> wasmtime::Result<HostBytes>;
}

/// `String.prototype.localeCompare`: `-1`, `0` or `1` by root collation, the
/// order `Intl.Collator` gives for the root or an English locale. It is the
/// same on every machine, unlike a host whose locale comes from its
/// environment.
///
/// Canonically equivalent strings compare equal; an unpaired surrogate
/// compares as U+FFFD. Long strings compare by their primary sort keys first
/// and by tertiary keys only when those agree, which gives the same order as
/// ICU4X's direct comparison.
pub(crate) fn locale_compare(
    a: &Str,
    b: &Str,
    budget: &mut impl CollationBudget,
) -> wasmtime::Result<i32> {
    let collators = ROOT_COLLATORS
        .as_ref()
        .map_err(|error| fatal_host_error(format!("root collation data: {error}")))?;
    budget.charge(fuel::SCAN, a.len() as u64 + b.len() as u64)?;
    let shapes = (CollationShape::of(a.units()), CollationShape::of(b.units()));
    let ordering = if a.len().max(b.len()) <= DIRECT_COMPARE_UNITS {
        compare_directly(&collators.tertiary, a, b, &shapes, budget)?
    } else {
        compare_by_sort_keys(collators, a, b, &shapes, budget)?
    };
    // `Ordering` is -1, 0 or 1.
    Ok(ordering as i32)
}

/// ICU4X's direct comparison of two short strings. Besides a pass over each,
/// it steps back over their shared prefix one character at a time, moving the
/// work done so far: at most the prefix's length squared, which is small here.
fn compare_directly(
    collator: &CollatorBorrowed<'static>,
    a: &Str,
    b: &Str,
    (a_shape, b_shape): &(CollationShape, CollationShape),
    budget: &mut impl CollationBudget,
) -> wasmtime::Result<std::cmp::Ordering> {
    let shared = a
        .units()
        .iter()
        .zip(b.units())
        .take_while(|(x, y)| x == y)
        .count() as u64;
    a_shape.charge_pass(budget)?;
    b_shape.charge_pass(budget)?;
    budget.charge(fuel::SCAN, shared * shared)?;
    let _buffers = budget.admit(
        (a_shape.decomposed + b_shape.decomposed).saturating_mul(DIRECT_BYTES_PER_CHARACTER),
    )?;
    Ok(collator.compare_utf16(a.units(), b.units()))
}

/// Compares two strings by their primary sort keys, and by their tertiary
/// keys only when those agree: the order of ICU4X's direct comparison,
/// without its quadratic step back over a shared prefix or its buffering of
/// every collation element. The primary keys stay admitted while the
/// tertiary keys are written.
fn compare_by_sort_keys(
    collators: &RootCollators,
    a: &Str,
    b: &Str,
    (a_shape, b_shape): &(CollationShape, CollationShape),
    budget: &mut impl CollationBudget,
) -> wasmtime::Result<std::cmp::Ordering> {
    let (a_primary, _a_primary_memory) =
        sort_key(collators, a, a_shape, KeyLevel::Primary, budget)?;
    let (b_primary, _b_primary_memory) =
        sort_key(collators, b, b_shape, KeyLevel::Primary, budget)?;
    let primary = a_primary.cmp(&b_primary);
    if primary != std::cmp::Ordering::Equal {
        return Ok(primary);
    }
    // The keys are equal, so either one sizes both strings' level buffers.
    let tertiary = KeyLevel::Tertiary {
        primary_key_bytes: a_primary.len() as u64,
    };
    let (a_tertiary, _a_tertiary_memory) = sort_key(collators, a, a_shape, tertiary, budget)?;
    let (b_tertiary, _b_tertiary_memory) = sort_key(collators, b, b_shape, tertiary, budget)?;
    Ok(a_tertiary.cmp(&b_tertiary))
}

/// The sort key of `s` at `level`, with its admitted memory. ICU4X
/// writes it twice: once into nothing to learn its length, then into a buffer
/// of exactly that length. Both passes admit what ICU4X buffers on the way:
/// its longest run of marks, and for a tertiary key its level buffers.
fn sort_key(
    collators: &RootCollators,
    s: &Str,
    shape: &CollationShape,
    level: KeyLevel,
    budget: &mut impl CollationBudget,
) -> wasmtime::Result<(Vec<u8>, HostBytes)> {
    let (collator, levels) = match level {
        KeyLevel::Primary => (&collators.primary, 0),
        KeyLevel::Tertiary { primary_key_bytes } => (
            &collators.tertiary,
            shape.level_buffer_bytes(primary_key_bytes),
        ),
    };
    shape.charge_pass(budget)?;
    let _internal = budget.admit(shape.mark_buffer_bytes().saturating_add(levels))?;
    let length = match collator.write_sort_key_utf16_to(s.units(), &mut [][..]) {
        Ok(length) => length,
        Err(too_small) => too_small.length,
    };
    shape.charge_pass(budget)?;
    let memory = budget.admit(length as u64)?;
    let mut key = vec![0; length];
    match collator.write_sort_key_utf16_to(s.units(), &mut key[..]) {
        Ok(written) if written == length => Ok((key, memory)),
        _ => Err(fatal_host_error(
            "sort key changed length between two passes",
        )),
    }
}

/// What one ICU4X pass over a string costs, measured in one linear pass.
#[derive(Debug, PartialEq, Eq)]
struct CollationShape {
    /// Characters in the string's compatibility decomposition (U+FDFA is 18):
    /// about one collation element each.
    decomposed: u64,
    /// The sum of the squared lengths of its runs of combining marks, after
    /// canonical decomposition (U+0F73 is two marks). ICU4X reorders a run
    /// and matches contractions across it in time quadratic in its length.
    run_work: u64,
    /// The length of its longest run of combining marks, which ICU4X buffers.
    longest_run: u64,
}

impl CollationShape {
    /// Each mark adds `2n + 1` to a run of `n`, since `(n + 1)² - n² = 2n + 1`.
    /// A starter or an unpaired surrogate ends a run.
    fn of(units: &[u16]) -> Self {
        let mut shape = CollationShape {
            decomposed: 0,
            run_work: 0,
            longest_run: 0,
        };
        let mut run = 0u64;
        for scalar in char::decode_utf16(units.iter().copied()) {
            let Ok(c) = scalar else {
                shape.decomposed += 1;
                run = 0;
                continue;
            };
            decompose_compatible(c, |_| shape.decomposed += 1);
            decompose_canonical(c, |part| {
                if canonical_combining_class(part) == 0 {
                    run = 0;
                } else {
                    shape.run_work = shape.run_work.saturating_add(2 * run + 1);
                    run += 1;
                    shape.longest_run = shape.longest_run.max(run);
                }
            });
        }
        shape
    }

    /// Charges one ICU4X pass over the string.
    fn charge_pass(&self, budget: &mut impl CollationBudget) -> wasmtime::Result<()> {
        budget.charge(fuel::PARSE, self.decomposed)?;
        budget.charge(fuel::SCAN, self.run_work)
    }

    /// What ICU4X buffers for the longest run of marks while it writes a key.
    fn mark_buffer_bytes(&self) -> u64 {
        self.longest_run.saturating_mul(RUN_BYTES_PER_MARK)
    }

    /// What ICU4X buffers for the levels past the primary one while it writes
    /// a tertiary key, given the string's primary key length.
    fn level_buffer_bytes(&self, primary_key_bytes: u64) -> u64 {
        primary_key_bytes
            .saturating_mul(LEVEL_BYTES_PER_PRIMARY_BYTE)
            .saturating_add(self.decomposed.saturating_mul(LEVEL_BYTES_PER_CHARACTER))
    }
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
        // The count is checked before the receiver: an infinite or negative
        // count throws even for the empty string, a large finite one does not.
        assert!(repeat_length(0, f64::INFINITY).is_err());
        assert!(repeat_length(0, -1.0).is_err());
        assert_eq!(repeat_length(0, f64::MAX).unwrap(), 0);
        assert_eq!(repeat_length(0, 5.0).unwrap(), 0);
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

#[cfg(test)]
mod collation_tests {
    use super::*;

    /// Charges nothing and admits everything, recording the fuel and the
    /// total of the admissions.
    #[derive(Default)]
    struct Unlimited {
        fuel: u64,
        bytes: u64,
    }

    impl CollationBudget for Unlimited {
        fn charge(&mut self, rate: fuel::Rate, units: u64) -> wasmtime::Result<()> {
            self.fuel += rate.cost(units);
            Ok(())
        }
        fn admit(&mut self, bytes: u64) -> wasmtime::Result<HostBytes> {
            self.bytes += bytes;
            let limits = crate::runtime::limits::TenantLimits::new(u64::MAX);
            Ok(HostBytes::new(&limits, 0)?)
        }
    }

    fn units(s: &str) -> Str {
        Str::from_units(s.encode_utf16().collect())
    }

    fn compare(a: &Str, b: &Str) -> i32 {
        locale_compare(a, b, &mut Unlimited::default()).expect("root collation")
    }

    #[test]
    fn sort_keys_order_long_strings_as_the_direct_comparison_does() {
        let direct = &ROOT_COLLATORS
            .as_ref()
            .expect("root collation data")
            .tertiary;
        let pool: Vec<char> = "aAbBeE\u{e9}\u{c9}zZ\u{df}sS\u{e6}\u{c6} \u{301}\u{308}-.1\u{4e2d}\u{d55c}\u{30ab}\u{f900}\u{1d6}\u{b7}l\u{e40}\u{f73}\u{fdfa}\u{1f19c}\u{ad}\u{200d}\u{1f600}"
            .chars()
            .collect();
        let mut seed: u64 = 7;
        let mut next = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        let shared: String = (0..DIRECT_COMPARE_UNITS).map(|_| 'x').collect();
        for _ in 0..2_000 {
            let mut random = |length: usize| -> String {
                (0..length).map(|_| pool[next() % pool.len()]).collect()
            };
            let (head_a, head_b, tail) = (random(4), random(4), random(4));
            // Past the direct-comparison length, so sort keys decide. Some
            // carry an unpaired surrogate, which both compare as U+FFFD.
            let with_lone = |text: String, lone: bool| {
                let mut units: Vec<u16> = text.encode_utf16().collect();
                if lone {
                    units.insert(1, 0xD800);
                }
                Str::from_units(units)
            };
            let a = with_lone(head_a + &shared + &tail, next() % 5 == 0);
            let b = with_lone(head_b + &shared + &tail, next() % 5 == 0);
            let expected = direct.compare_utf16(a.units(), b.units()) as i32;
            assert_eq!(
                compare(&a, &b),
                expected,
                "{:?} {:?}",
                String::from_utf16_lossy(a.units()),
                String::from_utf16_lossy(b.units())
            );
        }
    }

    #[test]
    fn the_shape_counts_decomposition_and_runs_of_marks() {
        let shape = |s: &str| CollationShape::of(units(s).units());
        assert_eq!(shape("plain").decomposed, 5);
        // U+FDFA decomposes to 18 characters.
        assert_eq!(shape("\u{fdfa}").decomposed, 18);
        assert_eq!(shape("e\u{301}").run_work, 1);
        let two_runs = shape("a\u{328}\u{301}b\u{301}");
        assert_eq!((two_runs.run_work, two_runs.longest_run), (2 * 2 + 1, 2));
        assert_eq!(shape(&"\u{301}".repeat(1000)).run_work, 1_000_000);
        // U+0F73 is a starter whose decomposition is two marks.
        assert_eq!(shape("x\u{f73}\u{f73}").run_work, 4 * 4);
        // A lone surrogate ends a run and counts as one character.
        let lone = CollationShape::of(&[0x301, 0x301, 0xD800, 0x301]);
        assert_eq!((lone.decomposed, lone.run_work), (4, 2 * 2 + 1));
    }

    #[test]
    fn a_shared_prefix_costs_one_pass_not_its_square() {
        let run = "\u{f900}".repeat(10_000);
        let (a, b) = (units(&(run.clone() + "a")), units(&(run + "b")));
        let mut budget = Unlimited::default();
        assert_eq!(
            locale_compare(&a, &b, &mut budget).expect("root collation"),
            -1
        );
        // Two passes per key, two keys, at PARSE, plus the first SCAN.
        assert!(budget.fuel < 300_000, "{}", budget.fuel);
    }

    #[test]
    fn short_strings_pay_for_their_shared_prefix_and_buffers() {
        let run = "\u{f900}".repeat(100);
        let (a, b) = (units(&(run.clone() + "a")), units(&(run + "b")));
        let mut budget = Unlimited::default();
        assert_eq!(
            locale_compare(&a, &b, &mut budget).expect("root collation"),
            -1
        );
        assert!(budget.fuel >= 100 * 100, "{}", budget.fuel);
        assert_eq!(budget.bytes, 202 * DIRECT_BYTES_PER_CHARACTER);
    }

    #[test]
    fn short_and_long_spellings_compare_across_the_boundary() {
        // 256 units composed, 257 decomposed: one direct-sized side, keys decide.
        let composed = units(&("\u{e9}".to_string() + &"x".repeat(DIRECT_COMPARE_UNITS - 1)));
        let decomposed = units(&("e\u{301}".to_string() + &"x".repeat(DIRECT_COMPARE_UNITS - 1)));
        assert_eq!(compare(&composed, &decomposed), 0);
        let mut lone = vec![0xD800];
        lone.extend(std::iter::repeat_n(u16::from(b'x'), DIRECT_COMPARE_UNITS));
        let lone = Str::from_units(lone);
        let replaced = units(&("\u{fffd}".to_string() + &"x".repeat(DIRECT_COMPARE_UNITS)));
        assert_eq!(compare(&lone, &replaced), 0);
    }

    #[test]
    fn tertiary_keys_admit_their_level_buffers_without_primary_weights() {
        // U+0A03 has no primary weight, so the primary key is empty and only
        // the per-character term sizes the level buffers.
        let a = units(&"\u{a03}".repeat(DIRECT_COMPARE_UNITS + 1));
        let shape = CollationShape::of(a.units());
        let tertiary_key = {
            let collator = &ROOT_COLLATORS
                .as_ref()
                .expect("root collation data")
                .tertiary;
            match collator.write_sort_key_utf16_to(a.units(), &mut [][..]) {
                Ok(length) => length as u64,
                Err(too_small) => too_small.length as u64,
            }
        };
        let mut budget = Unlimited::default();
        assert_eq!(
            locale_compare(&a, &a, &mut budget).expect("root collation"),
            0
        );
        // 257 characters are just past a doubling of ICU4X's buffer, where it
        // holds 6 bytes per character (measured).
        let levels = shape.level_buffer_bytes(0);
        assert!(levels >= 6 * shape.decomposed, "{levels}");
        // U+0A03 is a starter, so no run of marks is admitted. Per string: an
        // empty primary key, then its level buffers and its tertiary key.
        assert_eq!(shape.longest_run, 0);
        assert_eq!(budget.bytes, 2 * (levels + tertiary_key));
    }

    #[test]
    fn keys_differing_only_in_case_take_the_tertiary_level() {
        let run = "x".repeat(DIRECT_COMPARE_UNITS);
        let (a, b) = (
            units(&("a".to_string() + &run)),
            units(&("A".to_string() + &run)),
        );
        assert_eq!(compare(&a, &b), -1);
        assert_eq!(compare(&a, &a), 0);
    }
}
