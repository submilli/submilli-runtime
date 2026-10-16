//! `String#replace` and `replaceAll` with a replacer function. As in
//! ECMA-262, every match is found first, then the function is called once per
//! match, in order, with the matched text, each capture (`undefined` when it
//! didn't participate), the match's offset, the whole input, and, when the
//! pattern names groups, an object of them.

use wasmtime::{Caller, Rooted, StructRef, Val};

use super::{FlagSet, as_struct, flag_bits, input, input_units, with_regex};
use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{write_boxed_number_struct, write_submilli_string_struct_units};
use crate::runtime::prelude::closure::Closure;
use crate::runtime::prelude::vtable::read_string_units;

/// One match, in the input's UTF-16 code units.
struct Found {
    start: usize,
    end: usize,
    captures: Vec<Option<Vec<u16>>>,
    named: Vec<(String, Option<Vec<u16>>)>,
}

/// Replace with `replacer`'s result for each match of `search`, a string or
/// a `RegExp`. `all` is `replaceAll`; `replace` follows the regex's `g` flag.
pub(super) async fn replace_with_callback(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    search: &Val,
    replacer: &Val,
    all: bool,
) -> wasmtime::Result<Val> {
    let label = if all {
        "String#replaceAll"
    } else {
        "String#replace"
    };
    let replacer = crate::runtime::prelude::closure::read_callback(caller, replacer, label)?;
    let units = read_string_units(caller, receiver, label)?;
    let (found, has_named) = if super::arg_is_string(caller, search)? {
        let needle = read_string_units(caller, search, label)?;
        (literal_matches(caller, &units, &needle, all)?, false)
    } else {
        let regex = as_struct(caller, search, label)?;
        regex_matches(caller, receiver, &regex, all)?
    };
    let mut output: Vec<u16> = Vec::new();
    let mut previous = 0;
    for hit in &found {
        output.extend_from_slice(units.get(previous..hit.start).unwrap_or_default());
        let replacement =
            call_replacer(caller, &replacer, receiver, &units, hit, has_named).await?;
        fuel::charge(&mut *caller, fuel::SCAN, replacement.len() as u64)?;
        output.extend_from_slice(&replacement);
        previous = hit.end;
    }
    output.extend_from_slice(units.get(previous..).unwrap_or_default());
    let st = write_submilli_string_struct_units(caller, &output)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

fn literal_matches(
    caller: &mut Caller<'_, StoreData>,
    units: &[u16],
    needle: &[u16],
    all: bool,
) -> wasmtime::Result<Vec<Found>> {
    fuel::charge(
        &mut *caller,
        fuel::SCAN,
        (units.len() + needle.len()) as u64,
    )?;
    let matcher = crate::runtime::prelude::string::search::Search::new(needle, false);
    let mut found = Vec::new();
    let mut position = 0;
    while position <= units.len() {
        let Some(start) = matcher.find(units, position) else {
            break;
        };
        found.push(Found {
            start,
            end: start + needle.len(),
            captures: Vec::new(),
            named: Vec::new(),
        });
        if !all {
            break;
        }
        // An empty needle matches between every pair of code units.
        position = start + needle.len().max(1);
    }
    Ok(found)
}

fn regex_matches(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    regex: &Rooted<StructRef>,
    all: bool,
) -> wasmtime::Result<(Vec<Found>, bool)> {
    let all = all || flag_bits(caller, regex)? & FlagSet::G.bits() as i32 != 0;
    let decoded = input::read(caller, receiver)?;
    fuel::charge(&mut *caller, fuel::REGEX, decoded.len() as u64)?;
    let (compiled, has_named) = with_regex(caller, regex, |compiled| {
        let regex = compiled.regex.clone();
        let has_named = regex.capture_names().flatten().next().is_some();
        (regex, has_named)
    })?;
    let mut found = Vec::new();
    let mut position = 0;
    while let Some(hit) = super::engine::exec_snapshot(&compiled, &decoded, position) {
        fuel::charge(&mut *caller, fuel::ELEM, compiled.captures_len() as u64)?;
        let mut captures = Vec::new();
        for slot in &hit.numbered {
            captures.push(capture_units(caller, &decoded, *slot)?);
        }
        let mut named = Vec::new();
        for (name, slot) in &hit.named {
            named.push((name.clone(), capture_units(caller, &decoded, *slot)?));
        }
        found.push(Found {
            start: decoded.byte_to_unit(hit.match_start)?,
            end: decoded.byte_to_unit(hit.match_end)?,
            captures,
            named,
        });
        if !all {
            break;
        }
        position = if hit.match_start == hit.match_end {
            super::next_char_boundary(&decoded, hit.match_end)
        } else {
            hit.match_end
        };
        if position > decoded.len() {
            break;
        }
    }
    Ok((found, has_named))
}

fn capture_units(
    caller: &mut Caller<'_, StoreData>,
    decoded: &input::DecodedInput,
    slot: Option<(usize, usize)>,
) -> wasmtime::Result<Option<Vec<u16>>> {
    slot.map(|(start, end)| input_units(caller, decoded, start..end))
        .transpose()
}

async fn call_replacer(
    caller: &mut Caller<'_, StoreData>,
    replacer: &Closure,
    receiver: &Val,
    units: &[u16],
    hit: &Found,
    has_named: bool,
) -> wasmtime::Result<Vec<u16>> {
    let mut args = Vec::new();
    let matched = units.get(hit.start..hit.end).unwrap_or_default();
    args.push(string_value(caller, matched)?);
    for capture in &hit.captures {
        args.push(optional_string_value(caller, capture.as_deref())?);
    }
    args.push(Val::AnyRef(Some(
        write_boxed_number_struct(caller, hit.start as f64)?.to_anyref(),
    )));
    args.push(*receiver);
    if has_named {
        let mut fields = Vec::new();
        for (name, value) in &hit.named {
            let name_units: Vec<u16> = name.encode_utf16().collect();
            let name = string_value(caller, &name_units)?;
            fields.push((name, optional_string_value(caller, value.as_deref())?));
        }
        args.push(crate::runtime::prelude::object::object_from_fields(
            caller, &fields,
        )?);
    }
    let result = replacer.call_dynamic(caller, &args).await?;
    read_string_units(caller, &result, "replacer result")
}

fn string_value(caller: &mut Caller<'_, StoreData>, units: &[u16]) -> wasmtime::Result<Val> {
    Ok(Val::AnyRef(Some(
        write_submilli_string_struct_units(caller, units)?.to_anyref(),
    )))
}

fn optional_string_value(
    caller: &mut Caller<'_, StoreData>,
    units: Option<&[u16]>,
) -> wasmtime::Result<Val> {
    match units {
        Some(units) => string_value(caller, units),
        None => crate::runtime::prelude::undefined::value(caller),
    }
}
