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

    // Without `u`, JS `\d`/`\w`/`\s` are ASCII but `.` is Unicode. The `regex` crate
    // can't express this combo (disabling Unicode makes `.` byte-mode, rejected for str
    // input), so we keep Unicode on and rewrite the shorthand classes to ASCII equivalents.
    let escape_rewrites: &[(u8, &str)] = if flag_set.has(FlagSet::U) {
        &[]
    } else {
        &[
            (b'd', "[0-9]"),
            (b'D', "[^0-9]"),
            (b'w', "[A-Za-z0-9_]"),
            (b'W', "[^A-Za-z0-9_]"),
            (b's', "[ \\t\\r\\n\\x0B\\x0C]"),
            (b'S', "[^ \\t\\r\\n\\x0B\\x0C]"),
        ]
    };

    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut in_class = false;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\\' if i + 1 < bytes.len() => {
                let next = bytes[i + 1];
                if !in_class && next.is_ascii_digit() && next != b'0' {
                    // Inside a character class `\1` is a literal in both JS and regex.
                    return Err(TranslateError::Backreference);
                }
                if !in_class && next == b'k' {
                    return Err(TranslateError::Backreference);
                }
                if let Some((_, repl)) = escape_rewrites.iter().find(|(c, _)| *c == next) {
                    if in_class {
                        // Strip outer `[` / `]` from the replacement
                        // so we contribute to the surrounding class.
                        let inner = &repl[1..repl.len() - 1];
                        translated.push_str(inner);
                    } else {
                        translated.push_str(repl);
                    }
                    i += 2;
                    continue;
                }
                let escaped = char_at(pattern, i + 1);
                // Without `u`, JS reads `\é` as `é` (an identity escape); the `regex`
                // crate rejects escaping a non-ASCII character, so drop the backslash.
                // With `u`, JS rejects it too, and the crate's error stands.
                let is_identity_escape = !escaped.is_ascii() && !flag_set.has(FlagSet::U);
                if !is_identity_escape {
                    translated.push('\\');
                }
                translated.push(escaped);
                i += 1 + escaped.len_utf8();
                continue;
            }
            b'[' if !in_class => {
                in_class = true;
                translated.push('[');
                i += 1;
                continue;
            }
            b']' if in_class => {
                in_class = false;
                translated.push(']');
                i += 1;
                continue;
            }
            b'(' if !in_class && i + 1 < bytes.len() && bytes[i + 1] == b'?' => {
                let third = bytes.get(i + 2).copied();
                match third {
                    Some(b'=' | b'!') => return Err(TranslateError::Lookahead),
                    Some(b'<') => {
                        let fourth = bytes.get(i + 3).copied();
                        if fourth == Some(b'=') || fourth == Some(b'!') {
                            return Err(TranslateError::Lookbehind);
                        }
                        translated.push('(');
                        i += 1;
                        continue;
                    }
                    _ => {
                        translated.push('(');
                        i += 1;
                        continue;
                    }
                }
            }
            // Every byte the cases above inspect is ASCII; anything else is copied as
            // the whole character it starts, so `/é/` stays `é` rather than two
            // Latin-1 characters built from its UTF-8 bytes.
            _ => {
                let c = char_at(pattern, i);
                translated.push(c);
                i += c.len_utf8();
            }
        }
    }

    Ok(TranslatedRegex {
        pattern: translated,
        flags: flag_set,
    })
}

/// The character starting at byte `i`, which the scan in `translate_js_pattern`
/// only ever positions on a character boundary.
fn char_at(pattern: &str, i: usize) -> char {
    pattern[i..]
        .chars()
        .next()
        .expect("index is inside the pattern")
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
        let current = self.host_attached.load(Ordering::Relaxed);
        self.host_attached
            .store(current.saturating_sub(self.bytes), Ordering::Relaxed);
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

/// Run a single match at `last_index`, returning an owned snapshot so the borrow
/// on `regex` ends before any GC-mutating allocation. Shared by the Wasm-era
/// `submilli:regex.exec` host fn and the Rust `prelude::regex` methods.
pub(crate) fn exec_snapshot(regex: &Regex, input: &str, last_index: usize) -> Option<ExecSnapshot> {
    if last_index > input.len() {
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
    fn translate_passes_through_basic_pattern() {
        let t = translate_js_pattern("ab+c", "").expect("translates");
        assert_eq!(t.pattern, "ab+c");
        assert_eq!(t.flags.bits(), 0);
    }

    #[test]
    fn translate_u_flag_passes_pattern_through_unchanged() {
        let t = translate_js_pattern("\\d+", "u").expect("translates");
        assert_eq!(t.pattern, "\\d+");
        assert!(t.flags.has(FlagSet::U));
    }

    #[test]
    fn translate_no_u_flag_substitutes_digit_class() {
        let t = translate_js_pattern("\\d+", "").expect("translates");
        assert_eq!(t.pattern, "[0-9]+");
        assert!(!t.flags.has(FlagSet::U));
    }

    #[test]
    fn translate_no_u_flag_substitutes_word_and_space_classes() {
        let t = translate_js_pattern("\\w\\s\\W\\S", "").expect("translates");
        assert_eq!(
            t.pattern,
            "[A-Za-z0-9_][ \\t\\r\\n\\x0B\\x0C][^A-Za-z0-9_][^ \\t\\r\\n\\x0B\\x0C]"
        );
    }

    #[test]
    fn translate_substitutes_inside_char_class_without_brackets() {
        let t = translate_js_pattern("[a-z\\d]", "").expect("translates");
        assert_eq!(t.pattern, "[a-z0-9]");
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
        assert_eq!(t.pattern, "(?<year>\\d{4})");
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
    fn build_regex_no_u_flag_makes_digit_ascii_only() {
        let (re_u, _) = build_regex("\\d+", "u").expect("builds u");
        let (re_no_u, _) = build_regex("\\d+", "").expect("builds no-u");
        assert!(re_u.is_match("\u{0660}"));
        assert!(!re_no_u.is_match("\u{0660}"));
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
