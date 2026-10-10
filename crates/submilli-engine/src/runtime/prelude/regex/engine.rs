//! `submilli:regex` host module — JS-style regexes via the `regex` crate.
//!
//! `$RegExpMatch` sits outside the `$Object` rec group: wasmtime's host API
//! can't build rec-group types; consumer-side declarations unify via WasmGC
//! structural canonicalization at instantiation time.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use regex::{Regex, RegexBuilder};

use crate::runtime::limits::MemoryCapExceeded;

// Per-pattern NFA size cap; prevents a single tenant from exhausting memory with one regex.
const REGEX_SIZE_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    Lookahead,
    Lookbehind,
    Backreference,
    InvalidFlag(char),
    DuplicateFlag(char),
    InvalidPattern(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lookahead => f.write_str(
                "lookahead (`(?=...)` / `(?!...)`) is not supported by the Submilli regex engine",
            ),
            Self::Lookbehind => f.write_str(
                "lookbehind (`(?<=...)` / `(?<!...)`) is not supported by the Submilli regex engine",
            ),
            Self::Backreference => f.write_str(
                "backreferences (`\\1`, `\\2`, `\\k<name>`) are not supported by the Submilli regex engine",
            ),
            Self::InvalidFlag(c) => write!(
                f,
                "unknown regex flag `{c}` — Submilli accepts only `g`, `i`, `m`, `s`, `u`, `y`",
            ),
            Self::DuplicateFlag(c) => write!(f, "duplicate regex flag `{c}` — each flag may appear at most once"),
            Self::InvalidPattern(msg) => write!(f, "invalid regex pattern: {msg}"),
        }
    }
}

impl std::error::Error for TranslateError {}

/// Bitset of JS regex flags. Bit values (`g=1, i=2, m=4, s=8, u=16, y=32`)
/// must match the `$regex` Wasm struct's flag field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagSet(u8);

impl FlagSet {
    pub const G: Self = Self(1 << 0);
    pub const I: Self = Self(1 << 1);
    pub const M: Self = Self(1 << 2);
    pub const S: Self = Self(1 << 3);
    pub const U: Self = Self(1 << 4);
    pub const Y: Self = Self(1 << 5);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub fn has(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    fn merge(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedRegex {
    pub pattern: String,
    pub flags: FlagSet,
}

pub fn translate_js_pattern(pattern: &str, flags: &str) -> Result<TranslatedRegex, TranslateError> {
    let flag_set = parse_flags(flags)?;
    let mut translated = String::with_capacity(pattern.len());

    let mut chars = pattern.chars();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(escaped) = chars.next() else {
                    translated.push(c);
                    break;
                };
                if !in_class && ((escaped.is_ascii_digit() && escaped != '0') || escaped == 'k') {
                    return Err(TranslateError::Backreference);
                }
                if let Some(replacement) =
                    shorthand_class(escaped).or_else(|| word_boundary(escaped, in_class))
                {
                    translated.push_str(replacement);
                    continue;
                }
                if escaped.is_ascii() || flag_set.has(FlagSet::U) {
                    translated.push('\\');
                }
                translated.push(escaped);
            }
            '[' if !in_class => {
                in_class = true;
                translated.push(c);
            }
            ']' if in_class => {
                in_class = false;
                translated.push(c);
            }
            '(' if !in_class => {
                let rest = chars.as_str();
                if rest.starts_with("?=") || rest.starts_with("?!") {
                    return Err(TranslateError::Lookahead);
                }
                if rest.starts_with("?<=") || rest.starts_with("?<!") {
                    return Err(TranslateError::Lookbehind);
                }
                translated.push(c);
            }
            _ => translated.push(c),
        }
    }

    Ok(TranslatedRegex {
        pattern: translated,
        flags: flag_set,
    })
}

/// The JS WhiteSpace and LineTerminator code points, as `regex` class items.
macro_rules! js_space {
    () => {
        r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}"
    };
}

/// The `regex` crate spelling of a JS shorthand class escape, or `None` if
/// `escaped` isn't one. JS keeps `\d`/`\w` ASCII even under `u`, and its `\s`
/// is WhiteSpace plus LineTerminator, which differs from Unicode `White_Space`
/// (JS adds U+FEFF and leaves out U+0085). The crate keeps Unicode on so that
/// `.` matches whole characters, so each class is spelled out. Each one is
/// bracketed, which the crate reads as a nested class inside `[...]`; that keeps
/// a negated one like `[a\S]` a union rather than a class with a literal `^`.
fn shorthand_class(escaped: char) -> Option<&'static str> {
    Some(match escaped {
        'd' => "[0-9]",
        'D' => "[^0-9]",
        'w' => "[A-Za-z0-9_]",
        'W' => "[^A-Za-z0-9_]",
        's' => concat!("[", js_space!(), "]"),
        'S' => concat!("[^", js_space!(), "]"),
        _ => return None,
    })
}

/// JS `\b`/`\B` test ASCII word characters, as `\w` does. Inside a class `\b`
/// is a backspace instead.
fn word_boundary(escaped: char, in_class: bool) -> Option<&'static str> {
    match (escaped, in_class) {
        ('b', false) => Some(r"(?-u:\b)"),
        ('B', false) => Some(r"(?-u:\B)"),
        ('b', true) => Some(r"\x08"),
        _ => None,
    }
}

fn parse_flags(flags: &str) -> Result<FlagSet, TranslateError> {
    let mut set = FlagSet(0);
    for c in flags.chars() {
        let bit = match c {
            'g' => FlagSet::G,
            'i' => FlagSet::I,
            'm' => FlagSet::M,
            's' => FlagSet::S,
            'u' => FlagSet::U,
            'y' => FlagSet::Y,
            _ => return Err(TranslateError::InvalidFlag(c)),
        };
        if set.has(bit) {
            return Err(TranslateError::DuplicateFlag(c));
        }
        set = set.merge(bit);
    }
    Ok(set)
}

pub fn build_regex(pattern: &str, flags: &str) -> Result<(Regex, FlagSet), TranslateError> {
    let translated = translate_js_pattern(pattern, flags)?;
    let mut builder = RegexBuilder::new(&translated.pattern);
    builder
        .case_insensitive(translated.flags.has(FlagSet::I))
        .multi_line(translated.flags.has(FlagSet::M))
        .dot_matches_new_line(translated.flags.has(FlagSet::S))
        .size_limit(REGEX_SIZE_LIMIT);
    let regex = builder
        .build()
        .map_err(|e| TranslateError::InvalidPattern(e.to_string()))?;
    Ok((regex, translated.flags))
}

/// Externref payload for a compiled regex. Charges memory against `TenantLimits`
/// at creation; `Drop` releases the bytes when wasmtime GC reclaims the externref.
#[derive(Debug)]
pub struct ChargedRegex {
    pub regex: Regex,
    pub flags: FlagSet,
    bytes: u64,
    host_attached: Arc<AtomicU64>,
}

impl ChargedRegex {
    fn new(regex: Regex, flags: FlagSet, bytes: u64, host_attached: Arc<AtomicU64>) -> Self {
        Self {
            regex,
            flags,
            bytes,
            host_attached,
        }
    }

    pub fn flag_bits(&self) -> u8 {
        self.flags.bits()
    }
}

impl Drop for ChargedRegex {
    fn drop(&mut self) {
        let _ = self
            .host_attached
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(self.bytes))
            });
    }
}

/// On failure, no bytes remain charged.
pub fn compile_charged(
    limits: &crate::runtime::limits::TenantLimits,
    source: &str,
    flags: &str,
) -> Result<ChargedRegex, RegexCompileError> {
    let (regex, flag_set) = build_regex(source, flags).map_err(RegexCompileError::Syntax)?;
    let bytes = estimate_regex_bytes(source);
    limits
        .charge_host_bytes(bytes)
        .map_err(RegexCompileError::Memory)?;
    Ok(ChargedRegex::new(
        regex,
        flag_set,
        bytes,
        limits.host_attached_counter(),
    ))
}

// `regex` doesn't expose `Regex::estimated_size`; heuristic is intentionally
// generous — better to reject early than miss a cap breach.
fn estimate_regex_bytes(pattern: &str) -> u64 {
    const BASE: u64 = 16 * 1024;
    const PER_BYTE: u64 = 256;
    BASE.saturating_add((pattern.len() as u64).saturating_mul(PER_BYTE))
}

#[derive(Debug)]
pub enum RegexCompileError {
    Syntax(TranslateError),
    Memory(MemoryCapExceeded),
}

impl std::fmt::Display for RegexCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(e) => write!(f, "{e}"),
            Self::Memory(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RegexCompileError {}

/// Match boundaries only: boolean tests/search do not materialize captures.
pub(crate) fn find(regex: &Regex, input: &str, start: usize) -> Option<(usize, usize)> {
    if start > input.len() || !input.is_char_boundary(start) {
        return None;
    }
    regex
        .find_at(input, start)
        .map(|found| (found.start(), found.end()))
}

/// Run a single match at `last_index`, returning an owned snapshot so the borrow
/// on `regex` ends before any GC-mutating allocation. Shared by the Wasm-era
/// `submilli:regex.exec` host fn and the Rust `prelude::regex` methods.
pub(crate) fn exec_snapshot(regex: &Regex, input: &str, last_index: usize) -> Option<ExecSnapshot> {
    if last_index > input.len() || !input.is_char_boundary(last_index) {
        return None;
    }
    let m = regex.captures_at(input, last_index)?;
    let full = m.get(0)?;
    let numbered: Vec<Option<(usize, usize)>> = m
        .iter()
        .skip(1)
        .map(|g| g.map(|g| (g.start(), g.end())))
        .collect();
    let named: Vec<(String, Option<(usize, usize)>)> = regex
        .capture_names()
        .flatten()
        .map(|name| {
            let value = m.name(name).map(|g| (g.start(), g.end()));
            (name.to_string(), value)
        })
        .collect();
    Some(ExecSnapshot {
        match_start: full.start(),
        match_end: full.end(),
        next_last_index: full.end(),
        numbered,
        named,
    })
}

pub(crate) struct ExecSnapshot {
    pub match_start: usize,
    pub match_end: usize,
    pub next_last_index: usize,
    pub numbered: Vec<Option<(usize, usize)>>,
    /// Name is always present; `None` value = unmatched capture.
    pub named: Vec<(String, Option<(usize, usize)>)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::limits::TenantLimits;

    #[test]
    fn boolean_matches_skip_capture_materialization() {
        let mut work = Vec::new();
        for count in [128, 256] {
            let groups = (0..count)
                .map(|index| format!("(?<p{index}>a)"))
                .collect::<Vec<_>>()
                .join("|");
            let regex = Regex::new(&format!("|{groups}")).unwrap();
            let before = std::time::Instant::now();
            for _ in 0..count {
                assert_eq!(find(&regex, "", 0), Some((0, 0)));
            }
            eprintln!(
                "boolean matches {count}: {count} boundary results, {:?}; old {} capture slots",
                before.elapsed(),
                2 * count * count
            );
            work.push(count);
        }
        assert_eq!(work[1], 2 * work[0]);
    }

    #[test]
    fn translate_passes_through_basic_pattern() {
        let t = translate_js_pattern("ab+c", "").expect("translates");
        assert_eq!(t.pattern, "ab+c");
        assert_eq!(t.flags.bits(), 0);
    }

    #[test]
    fn translate_substitutes_digit_class_with_or_without_u() {
        for flags in ["", "u"] {
            let t = translate_js_pattern("\\d+", flags).expect("translates");
            assert_eq!(t.pattern, "[0-9]+");
        }
    }

    #[test]
    fn translate_brackets_a_shorthand_class_inside_a_class() {
        let t = translate_js_pattern("[a-z\\d]", "").expect("translates");
        assert_eq!(t.pattern, "[a-z[0-9]]");
    }

    #[test]
    fn translate_reads_backspace_inside_a_class() {
        let t = translate_js_pattern("[\\b]", "").expect("translates");
        assert_eq!(t.pattern, "[\\x08]");
    }

    /// The JS WhiteSpace and LineTerminator characters, and two that look
    /// like spaces but aren't in the set.
    const JS_SPACES: [char; 25] = [
        '\t', '\n', '\u{B}', '\u{C}', '\r', ' ', '\u{A0}', '\u{1680}', '\u{2000}', '\u{2001}',
        '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}',
        '\u{2009}', '\u{200A}', '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}',
        '\u{FEFF}',
    ];
    const NOT_JS_SPACES: [char; 2] = ['\u{85}', '\u{200B}'];

    #[test]
    fn build_regex_space_class_matches_the_js_set() {
        for flags in ["", "u"] {
            for pattern in ["^\\s$", "^[\\s]$", "^[a\\s]$"] {
                let (re, _) = build_regex(pattern, flags).expect("builds");
                for c in JS_SPACES {
                    assert!(re.is_match(&c.to_string()), "{pattern}/{flags} on {c:?}");
                }
                for c in NOT_JS_SPACES {
                    assert!(!re.is_match(&c.to_string()), "{pattern}/{flags} on {c:?}");
                }
            }
        }
    }

    #[test]
    fn build_regex_negated_space_class_is_the_complement() {
        let (re, _) = build_regex("^[a\\S]$", "").expect("builds");
        assert!(re.is_match("a"));
        assert!(re.is_match("\u{85}"));
        assert!(!re.is_match(" "));
        assert!(!re.is_match("\u{3000}"));
    }

    #[test]
    fn build_regex_word_boundary_is_ascii() {
        let (re, _) = build_regex("\\bx", "u").expect("builds");
        assert!(re.is_match("\u{E9}x"));
        assert!(!re.is_match("ax"));
    }

    #[test]
    fn translate_rejects_lookahead() {
        assert_eq!(
            translate_js_pattern("(?=foo)bar", ""),
            Err(TranslateError::Lookahead)
        );
    }

    #[test]
    fn translate_rejects_negative_lookahead() {
        assert_eq!(
            translate_js_pattern("(?!foo)", ""),
            Err(TranslateError::Lookahead)
        );
    }

    #[test]
    fn translate_rejects_lookbehind() {
        assert_eq!(
            translate_js_pattern("(?<=foo)bar", ""),
            Err(TranslateError::Lookbehind)
        );
    }

    #[test]
    fn translate_rejects_negative_lookbehind() {
        assert_eq!(
            translate_js_pattern("(?<!foo)", ""),
            Err(TranslateError::Lookbehind)
        );
    }

    #[test]
    fn translate_rejects_numeric_backref() {
        assert_eq!(
            translate_js_pattern("(a)\\1", ""),
            Err(TranslateError::Backreference)
        );
    }

    #[test]
    fn translate_rejects_named_backref() {
        assert_eq!(
            translate_js_pattern("(?<a>x)\\k<a>", ""),
            Err(TranslateError::Backreference)
        );
    }

    #[test]
    fn translate_passes_through_backref_inside_char_class() {
        let t = translate_js_pattern("[\\1]", "").expect("translates");
        assert_eq!(t.pattern, "[\\1]");
    }

    #[test]
    fn translate_passes_through_named_capture() {
        let t = translate_js_pattern("(?<year>\\d{4})", "u").expect("translates");
        assert_eq!(t.pattern, "(?<year>[0-9]{4})");
    }

    #[test]
    fn translate_passes_through_non_capturing_group() {
        let t = translate_js_pattern("(?:foo)", "").expect("translates");
        assert_eq!(t.pattern, "(?:foo)");
    }

    #[test]
    fn translate_flag_set_parses_each_letter() {
        let t = translate_js_pattern("x", "gimsuy").expect("translates");
        assert!(t.flags.has(FlagSet::G));
        assert!(t.flags.has(FlagSet::I));
        assert!(t.flags.has(FlagSet::M));
        assert!(t.flags.has(FlagSet::S));
        assert!(t.flags.has(FlagSet::U));
        assert!(t.flags.has(FlagSet::Y));
    }

    #[test]
    fn translate_rejects_unknown_flag() {
        assert_eq!(
            translate_js_pattern("x", "q"),
            Err(TranslateError::InvalidFlag('q'))
        );
    }

    #[test]
    fn translate_rejects_duplicate_flag() {
        assert_eq!(
            translate_js_pattern("x", "gg"),
            Err(TranslateError::DuplicateFlag('g'))
        );
    }

    #[test]
    fn build_regex_rejects_invalid_pattern() {
        let err = build_regex("(", "").expect_err("unbalanced paren rejects");
        match err {
            TranslateError::InvalidPattern(_) => {}
            other => panic!("expected InvalidPattern, got {other:?}"),
        }
    }

    #[test]
    fn build_regex_passes_through_case_insensitive() {
        let (re, flags) = build_regex("foo", "i").expect("builds");
        assert!(flags.has(FlagSet::I));
        assert!(re.is_match("FOO"));
        assert!(re.is_match("foo"));
    }

    #[test]
    fn build_regex_digit_class_is_ascii_even_with_u() {
        for flags in ["", "u"] {
            let (re, _) = build_regex("\\d+", flags).expect("builds");
            assert!(!re.is_match("\u{0660}"));
            assert!(re.is_match("7"));
        }
    }

    #[test]
    fn compile_charged_recompiles_without_cache() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let c1 = compile_charged(&limits, "abc", "").expect("compiles");
        let bytes_one = limits.host_attached_bytes();
        let _c2 = compile_charged(&limits, "abc", "").expect("compiles");
        assert_eq!(limits.host_attached_bytes(), bytes_one * 2);
        drop(c1);
    }

    #[test]
    fn compile_charged_charges_distinct_patterns_independently() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let _c1 = compile_charged(&limits, "abc", "").expect("compiles");
        let _c2 = compile_charged(&limits, "xyz", "").expect("compiles");
        assert!(limits.host_attached_bytes() >= 2 * estimate_regex_bytes("abc"));
    }

    #[test]
    fn drop_releases_bytes_back_to_limits() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let charged = compile_charged(&limits, "hello", "").expect("compiles");
        let before = limits.host_attached_bytes();
        assert!(before > 0, "compile should charge bytes");
        drop(charged);
        assert_eq!(
            limits.host_attached_bytes(),
            0,
            "ChargedRegex::drop must release the charged bytes",
        );
    }

    #[test]
    fn drop_releases_one_of_many_proportionally() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let c1 = compile_charged(&limits, "abc", "").expect("compiles");
        let c2 = compile_charged(&limits, "xyz", "").expect("compiles");
        let total = limits.host_attached_bytes();
        let c1_bytes = estimate_regex_bytes("abc");
        drop(c1);
        assert_eq!(limits.host_attached_bytes(), total - c1_bytes);
        drop(c2);
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn compile_rejects_over_cap() {
        let limits = TenantLimits::new(16);
        let err = compile_charged(&limits, "abc", "").expect_err("over cap");
        match err {
            RegexCompileError::Memory(_) => {}
            other => panic!("expected Memory, got {other}"),
        }
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn translate_keeps_non_ascii_characters_whole() {
        let t = translate_js_pattern("café[éè]—", "").expect("translates");
        assert_eq!(t.pattern, "café[éè]—");
    }

    #[test]
    fn translate_reads_an_escaped_non_ascii_character_as_itself() {
        let t = translate_js_pattern("\\é[\\è]", "").expect("translates");
        assert_eq!(t.pattern, "é[è]");
        // With `u` the escape is kept, and compiling it fails as JS's parse does.
        let t = translate_js_pattern("\\é", "u").expect("translates");
        assert_eq!(t.pattern, "\\é");
    }

    #[test]
    fn compile_propagates_syntax_error() {
        let limits = TenantLimits::new(10 * 1024 * 1024);
        let err = compile_charged(&limits, "(?=foo)", "").expect_err("lookahead rejects");
        match err {
            RegexCompileError::Syntax(TranslateError::Lookahead) => {}
            other => panic!("expected Syntax(Lookahead), got {other}"),
        }
        assert_eq!(limits.host_attached_bytes(), 0);
    }
}
