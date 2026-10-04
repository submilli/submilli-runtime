//! UTF-16 case mapping and ECMAScript whitespace trimming.
use std::sync::OnceLock;

use regex_syntax::hir::{Class, ClassUnicode, HirKind};

use super::{MAX_RESULT_UNITS, Str};
use crate::runtime::host::{fatal_host_error, range_error};

pub fn to_upper_case(input: &Str) -> wasmtime::Result<Str> {
    let mut output = output_buffer(input.len().saturating_mul(3).min(MAX_RESULT_UNITS))?;
    for decoded in char::decode_utf16(input.units().iter().copied()) {
        match decoded {
            Ok(ch) => {
                for mapped in ch.to_uppercase() {
                    push_char(&mut output, mapped)?;
                }
            }
            Err(surrogate) => push_units(&mut output, &[surrogate.unpaired_surrogate()])?,
        }
    }
    Ok(Str::from_units(output))
}

pub fn to_lower_case(input: &Str) -> wasmtime::Result<Str> {
    let properties = case_properties()?;
    let mut output = output_buffer(input.len().saturating_mul(3).min(MAX_RESULT_UNITS))?;
    let mut preceded_by_cased = false;
    let mut decoded = char::decode_utf16(input.units().iter().copied());
    while let Some(unit) = decoded.next() {
        let ch = match unit {
            Ok(ch) => ch,
            Err(surrogate) => {
                push_units(&mut output, &[surrogate.unpaired_surrogate()])?;
                preceded_by_cased = false;
                continue;
            }
        };
        if ch == 'Σ' && preceded_by_cased && !followed_by_cased(decoded.clone(), properties) {
            push_units(&mut output, &['ς' as u16])?;
        } else {
            for mapped in ch.to_lowercase() {
                push_char(&mut output, mapped)?;
            }
        }
        // Case_Ignorable takes precedence when a character has both properties.
        if !contains(&properties.ignorable, ch) {
            preceded_by_cased = contains(&properties.cased, ch);
        }
    }
    Ok(Str::from_units(output))
}

fn followed_by_cased(
    following: impl Iterator<Item = std::result::Result<char, std::char::DecodeUtf16Error>>,
    properties: &CaseProperties,
) -> bool {
    // Lookahead crosses only ignorable runs; distinct sigmas cannot rescan
    // the same run, so all lookaheads together visit at most the input length.
    for decoded in following {
        let Ok(ch) = decoded else {
            return false;
        };
        if !contains(&properties.ignorable, ch) {
            return contains(&properties.cased, ch);
        }
    }
    false
}

fn push_char(output: &mut Vec<u16>, ch: char) -> wasmtime::Result<()> {
    let mut units = [0_u16; 2];
    push_units(output, ch.encode_utf16(&mut units))
}

fn push_units(output: &mut Vec<u16>, units: &[u16]) -> wasmtime::Result<()> {
    if units.len() > output.capacity().saturating_sub(output.len()) {
        return Err(range_error("Invalid string length"));
    }
    output.extend_from_slice(units);
    Ok(())
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

pub fn trim(input: &Str) -> wasmtime::Result<Str> {
    trim_boundaries(input, true, true)
}

pub fn trim_start(input: &Str) -> wasmtime::Result<Str> {
    trim_boundaries(input, true, false)
}

pub fn trim_end(input: &Str) -> wasmtime::Result<Str> {
    trim_boundaries(input, false, true)
}

fn trim_boundaries(input: &Str, leading: bool, trailing: bool) -> wasmtime::Result<Str> {
    let units = input.units();
    let start = if leading {
        units
            .iter()
            .position(|unit| !whitespace(*unit))
            .unwrap_or(units.len())
    } else {
        0
    };
    let end = if trailing {
        units
            .get(start..)
            .and_then(|rest| rest.iter().rposition(|unit| !whitespace(*unit)))
            .map_or(start, |index| start + index + 1)
    } else {
        units.len()
    };
    let retained = units
        .get(start..end)
        .ok_or_else(|| fatal_host_error("invalid trim boundaries"))?;
    let mut output = output_buffer(retained.len())?;
    output.extend_from_slice(retained);
    Ok(Str::from_units(output))
}

fn whitespace(unit: u16) -> bool {
    matches!(unit, 0x0009..=0x000D | 0x0020 | 0x00A0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

fn output_buffer(capacity: usize) -> wasmtime::Result<Vec<u16>> {
    if capacity > MAX_RESULT_UNITS {
        return Err(range_error("Invalid string length"));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(fatal_host_error)?;
    Ok(output)
}
