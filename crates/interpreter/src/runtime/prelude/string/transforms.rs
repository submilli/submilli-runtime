//! UTF-16 case mapping and ECMAScript whitespace trimming.
use std::sync::OnceLock;

use regex_syntax::hir::{Class, ClassUnicode, HirKind};

use crate::runtime::host::fatal_host_error;

/// The code units a transform reads, by index.
pub(super) trait Units {
    fn len(&self) -> usize;
    fn unit(&mut self, index: usize) -> wasmtime::Result<u16>;
}

/// A transform's input and the sink for its result. One value serves both
/// because the GC implementation reaches input and output through one caller.
pub(super) trait Text: Units {
    fn emit(&mut self, units: &[u16]) -> wasmtime::Result<()>;
}

pub(super) fn to_upper_case(text: &mut impl Text) -> wasmtime::Result<()> {
    let mut index = 0;
    while let Some((decoded, next)) = decode_at(text, index)? {
        match decoded {
            Decoded::Char(ch) => {
                for mapped in ch.to_uppercase() {
                    emit_char(text, mapped)?;
                }
            }
            Decoded::Lone(surrogate) => text.emit(&[surrogate])?,
        }
        index = next;
    }
    Ok(())
}

pub(super) fn to_lower_case(text: &mut impl Text) -> wasmtime::Result<()> {
    let properties = case_properties()?;
    let mut preceded_by_cased = false;
    let mut index = 0;
    while let Some((decoded, next)) = decode_at(text, index)? {
        index = next;
        let ch = match decoded {
            Decoded::Char(ch) => ch,
            Decoded::Lone(surrogate) => {
                text.emit(&[surrogate])?;
                preceded_by_cased = false;
                continue;
            }
        };
        if ch == 'Σ' && preceded_by_cased && !followed_by_cased(text, next, properties)? {
            text.emit(&['ς' as u16])?;
        } else {
            for mapped in ch.to_lowercase() {
                emit_char(text, mapped)?;
            }
        }
        // Case_Ignorable takes precedence when a character has both properties.
        if !contains(&properties.ignorable, ch) {
            preceded_by_cased = contains(&properties.cased, ch);
        }
    }
    Ok(())
}

/// What starts at an index of UTF-16 text.
enum Decoded {
    Char(char),
    Lone(u16),
}

/// The character or lone surrogate starting at `index`, with the index after
/// it. `None` at the end of the text.
fn decode_at(text: &mut impl Units, index: usize) -> wasmtime::Result<Option<(Decoded, usize)>> {
    if index >= text.len() {
        return Ok(None);
    }
    let first = text.unit(index)?;
    let second = if index + 1 < text.len() {
        Some(text.unit(index + 1)?)
    } else {
        None
    };
    let decoded = char::decode_utf16(std::iter::once(first).chain(second)).next();
    match decoded {
        Some(Ok(ch)) => Ok(Some((Decoded::Char(ch), index + ch.len_utf16()))),
        Some(Err(error)) => Ok(Some((Decoded::Lone(error.unpaired_surrogate()), index + 1))),
        None => Err(fatal_host_error("no code unit to decode")),
    }
}

fn followed_by_cased(
    text: &mut impl Units,
    mut index: usize,
    properties: &CaseProperties,
) -> wasmtime::Result<bool> {
    // Lookahead crosses only ignorable runs; distinct sigmas cannot rescan
    // the same run, so all lookaheads together visit at most the input length.
    while let Some((decoded, next)) = decode_at(text, index)? {
        let Decoded::Char(ch) = decoded else {
            return Ok(false);
        };
        if !contains(&properties.ignorable, ch) {
            return Ok(contains(&properties.cased, ch));
        }
        index = next;
    }
    Ok(false)
}

fn emit_char(text: &mut impl Text, ch: char) -> wasmtime::Result<()> {
    let mut units = [0_u16; 2];
    text.emit(ch.encode_utf16(&mut units))
}

struct CaseProperties {
    cased: ClassUnicode,
    ignorable: ClassUnicode,
}

pub(super) fn prepare_case_properties() -> wasmtime::Result<()> {
    case_properties().map(|_| ())
}

fn case_properties() -> wasmtime::Result<&'static CaseProperties> {
    static PROPERTIES: OnceLock<std::result::Result<CaseProperties, &'static str>> =
        OnceLock::new();
    PROPERTIES
        .get_or_init(|| {
            Ok(CaseProperties {
                cased: property_class(r"\p{Cased}")?,
                ignorable: property_class(r"\p{Case_Ignorable}")?,
            })
        })
        .as_ref()
        .map_err(|message| fatal_host_error(*message))
}

fn property_class(pattern: &str) -> std::result::Result<ClassUnicode, &'static str> {
    let hir = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|_| "could not parse built-in Unicode case property")?;
    match hir.into_kind() {
        HirKind::Class(Class::Unicode(class)) => Ok(class),
        _ => Err("built-in Unicode case property is not a character class"),
    }
}

fn contains(class: &ClassUnicode, ch: char) -> bool {
    let ranges = class.ranges();
    let index = ranges.partition_point(|range| range.end() < ch);
    ranges.get(index).is_some_and(|range| range.start() <= ch)
}

/// Which ends of the string a trim removes whitespace from.
#[derive(Clone, Copy)]
pub(super) enum TrimEnds {
    Both,
    Start,
    End,
}

/// The range of `units` a trim keeps.
pub(super) fn trim_range(
    units: &mut impl Units,
    ends: TrimEnds,
) -> wasmtime::Result<std::ops::Range<usize>> {
    let len = units.len();
    let mut start = 0;
    if matches!(ends, TrimEnds::Both | TrimEnds::Start) {
        while start < len && whitespace(units.unit(start)?) {
            start += 1;
        }
    }
    let mut end = len;
    if matches!(ends, TrimEnds::Both | TrimEnds::End) {
        while end > start && whitespace(units.unit(end - 1)?) {
            end -= 1;
        }
    }
    Ok(start..end)
}

fn whitespace(unit: u16) -> bool {
    matches!(unit, 0x0009..=0x000D | 0x0020 | 0x00A0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}
