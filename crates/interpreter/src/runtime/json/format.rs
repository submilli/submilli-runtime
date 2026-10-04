//! Validate and indent JSON tokens without decoding strings or building a tree.

use wasmtime::{ArrayRef, Caller, Rooted, Val};

use crate::runtime::StoreData;
use crate::runtime::host::{fatal_host_error, range_error, syntax_error};
use crate::runtime::limits::HostBytes;
use crate::runtime::prelude::vtable::serialization::Output;
use crate::runtime::{fuel, host};

pub(super) fn pretty(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    indent: &[u16],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let raw = match value {
        Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
        _ => return Err(fatal_host_error("JSON.stringify: invalid JSON payload")),
    };
    let len = raw.len(&mut *caller)?;
    let _input = HostBytes::new(&caller.data().tenant_limits, u64::from(len) * 2)?;
    let units = host::read_code_units(&mut *caller, raw, "JSON.stringify pretty input")?;
    fuel::charge(&mut *caller, fuel::PARSE, u64::from(len))?;
    let output = format(caller, &units, indent)?;
    host::write_code_units(caller, output.units())
}

#[derive(Clone, Copy)]
enum State {
    Value,
    End,
    Key { first: bool },
    Colon,
    ObjectEnd,
    Element { first: bool },
    ArrayEnd,
}

fn format(
    caller: &mut Caller<'_, StoreData>,
    units: &[u16],
    indent: &[u16],
) -> wasmtime::Result<Output> {
    let _stack = HostBytes::new(&caller.data().tenant_limits, 1024)?;
    let mut states = Vec::new();
    states.try_reserve_exact(129).map_err(fatal_host_error)?;
    states.push(State::Value);
    let mut output = Output::new(caller);
    let mut cursor = 0;
    let mut remaining = crate::runtime::MAX_STRUCTURAL_WALK_NODES;
    while let Some(state) = states.last().copied() {
        skip_space(units, &mut cursor);
        let next = units.get(cursor).copied();
        let depth = states.len() - 1;
        match state {
            State::Value => {
                super::charge_json_visit(caller, &mut remaining)?;
                set_state(&mut states, State::End)?;
                append_value(caller, &mut output, units, &mut cursor, &mut states)?;
            }
            State::End => {
                states.pop();
                if states.is_empty() && cursor != units.len() {
                    return Err(invalid_json(cursor));
                }
            }
            State::Key { first } => {
                if first && next == Some(125) {
                    close(caller, &mut output, &mut states, &mut cursor, 125)?;
                    continue;
                }
                if next != Some(34) {
                    return Err(invalid_json(cursor));
                }
                newline(caller, &mut output, indent, depth)?;
                let start = cursor;
                string_end(units, &mut cursor)?;
                append_range(caller, &mut output, units, start, cursor)?;
                set_state(&mut states, State::Colon)?;
            }
            State::Colon => {
                take(units, &mut cursor, 58)?;
                output.append(caller, &[58])?;
                if !indent.is_empty() {
                    output.append(caller, &[32])?;
                }
                set_state(&mut states, State::ObjectEnd)?;
                super::charge_json_visit(caller, &mut remaining)?;
                skip_space(units, &mut cursor);
                append_value(caller, &mut output, units, &mut cursor, &mut states)?;
            }
            State::Element { first } => {
                if first && next == Some(93) {
                    close(caller, &mut output, &mut states, &mut cursor, 93)?;
                    continue;
                }
                newline(caller, &mut output, indent, depth)?;
                set_state(&mut states, State::ArrayEnd)?;
                super::charge_json_visit(caller, &mut remaining)?;
                append_value(caller, &mut output, units, &mut cursor, &mut states)?;
            }
            State::ObjectEnd | State::ArrayEnd => {
                let closing = if matches!(state, State::ObjectEnd) {
                    125
                } else {
                    93
                };
                if next == Some(closing) {
                    newline(caller, &mut output, indent, depth - 1)?;
                    close(caller, &mut output, &mut states, &mut cursor, closing)?;
                } else {
                    take(units, &mut cursor, 44)?;
                    output.append(caller, &[44])?;
                    set_state(
                        &mut states,
                        if closing == 125 {
                            State::Key { first: false }
                        } else {
                            State::Element { first: false }
                        },
                    )?;
                }
            }
        }
    }
    Ok(output)
}

fn append_value(
    caller: &mut Caller<'_, StoreData>,
    output: &mut Output,
    units: &[u16],
    cursor: &mut usize,
    states: &mut Vec<State>,
) -> wasmtime::Result<()> {
    let start = *cursor;
    match units.get(*cursor).copied() {
        Some(open @ (123 | 91)) => {
            if states.len() > crate::runtime::MAX_VTABLE_WALK_DEPTH as usize {
                return Err(range_error("JSON.stringify exceeds 128 levels"));
            }
            output.append(caller, &[open])?;
            *cursor += 1;
            states.push(if open == 123 {
                State::Key { first: true }
            } else {
                State::Element { first: true }
            });
            return Ok(());
        }
        Some(34) => string_end(units, cursor)?,
        Some(116) => literal(units, cursor, &[116, 114, 117, 101])?,
        Some(102) => literal(units, cursor, &[102, 97, 108, 115, 101])?,
        Some(110) => literal(units, cursor, &[110, 117, 108, 108])?,
        Some(45 | 48..=57) => number_end(units, cursor)?,
        _ => return Err(invalid_json(*cursor)),
    }
    append_range(caller, output, units, start, *cursor)
}

fn string_end(units: &[u16], cursor: &mut usize) -> wasmtime::Result<()> {
    take(units, cursor, 34)?;
    while let Some(unit) = units.get(*cursor).copied() {
        *cursor += 1;
        match unit {
            34 => return Ok(()),
            0..=31 => return Err(invalid_json(*cursor - 1)),
            92 => match units.get(*cursor).copied() {
                Some(117) => {
                    *cursor += 1;
                    for _ in 0..4 {
                        if !matches!(units.get(*cursor), Some(48..=57 | 65..=70 | 97..=102)) {
                            return Err(invalid_json(*cursor));
                        }
                        *cursor += 1;
                    }
                }
                Some(34 | 92 | 47 | 98 | 102 | 110 | 114 | 116) => *cursor += 1,
                _ => return Err(invalid_json(*cursor)),
            },
            _ => {}
        }
    }
    Err(invalid_json(*cursor))
}

fn number_end(units: &[u16], cursor: &mut usize) -> wasmtime::Result<()> {
    if units.get(*cursor) == Some(&45) {
        *cursor += 1;
    }
    if units.get(*cursor) == Some(&48) {
        *cursor += 1;
    } else {
        if !matches!(units.get(*cursor), Some(49..=57)) {
            return Err(invalid_json(*cursor));
        }
        digits(units, cursor)?;
    }
    if units.get(*cursor) == Some(&46) {
        *cursor += 1;
        digits(units, cursor)?;
    }
    if matches!(units.get(*cursor), Some(69 | 101)) {
        *cursor += 1;
        if matches!(units.get(*cursor), Some(43 | 45)) {
            *cursor += 1;
        }
        digits(units, cursor)?;
    }
    Ok(())
}

fn digits(units: &[u16], cursor: &mut usize) -> wasmtime::Result<()> {
    let start = *cursor;
    while matches!(units.get(*cursor), Some(48..=57)) {
        *cursor += 1;
    }
    if start == *cursor {
        return Err(invalid_json(*cursor));
    }
    Ok(())
}

fn literal(units: &[u16], cursor: &mut usize, value: &[u16]) -> wasmtime::Result<()> {
    for unit in value {
        take(units, cursor, *unit)?;
    }
    Ok(())
}

fn take(units: &[u16], cursor: &mut usize, unit: u16) -> wasmtime::Result<()> {
    if units.get(*cursor) != Some(&unit) {
        return Err(invalid_json(*cursor));
    }
    *cursor += 1;
    Ok(())
}

fn skip_space(units: &[u16], cursor: &mut usize) {
    while matches!(units.get(*cursor), Some(9 | 10 | 13 | 32)) {
        *cursor += 1;
    }
}

fn newline(
    caller: &mut Caller<'_, StoreData>,
    output: &mut Output,
    indent: &[u16],
    depth: usize,
) -> wasmtime::Result<()> {
    if indent.is_empty() {
        return Ok(());
    }
    output.append(caller, &[10])?;
    for _ in 0..depth {
        output.append(caller, indent)?;
    }
    Ok(())
}

fn close(
    caller: &mut Caller<'_, StoreData>,
    output: &mut Output,
    states: &mut Vec<State>,
    cursor: &mut usize,
    unit: u16,
) -> wasmtime::Result<()> {
    output.append(caller, &[unit])?;
    *cursor += 1;
    states.pop();
    Ok(())
}

fn set_state(states: &mut [State], state: State) -> wasmtime::Result<()> {
    *states
        .last_mut()
        .ok_or_else(|| fatal_host_error("JSON formatter lost its state"))? = state;
    Ok(())
}

fn append_range(
    caller: &mut Caller<'_, StoreData>,
    output: &mut Output,
    units: &[u16],
    start: usize,
    end: usize,
) -> wasmtime::Result<()> {
    output.append(
        caller,
        units
            .get(start..end)
            .ok_or_else(|| fatal_host_error("JSON formatter range is invalid"))?,
    )
}

fn invalid_json(cursor: usize) -> wasmtime::Error {
    syntax_error(format!(
        "JSON.stringify: invalid JSON at code unit {cursor}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs};
    use wasmtime::{Func, FuncType};

    #[tokio::test]
    async fn formatter_validates_tokens_and_bounds_without_losing_utf16() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let callback = Func::new(
            &mut store,
            FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                for (input, expected) in [
                    (
                        "{\"z\":[true,null,{},[]],\"a\":-1.25e+3}",
                        "{\n  \"z\": [\n    true,\n    null,\n    {},\n    []\n  ],\n  \"a\": -1.25e+3\n}",
                    ),
                    ("\"\\ud800\"", "\"\\ud800\""),
                    (
                        " { \"text\" : \"a\\\\b\\\"c\" } ",
                        "{\n  \"text\": \"a\\\\b\\\"c\"\n}",
                    ),
                ] {
                    let units = input.encode_utf16().collect::<Vec<_>>();
                    let output = format(&mut caller, &units, &[32, 32])?;
                    assert_eq!(String::from_utf16(output.units()).unwrap(), expected);
                }
                for input in [
                    "",
                    "[1,]",
                    "{\"x\":}",
                    "{\"x\":1,}",
                    "\"\\u123\"",
                    "\"\\q\"",
                    "\"\n\"",
                    "01",
                    "1.",
                    "1e+",
                    "+1",
                    "true false",
                    "[",
                    "{]",
                ] {
                    let units = input.encode_utf16().collect::<Vec<_>>();
                    let error = format(&mut caller, &units, &[]).err().unwrap();
                    assert!(
                        error.to_string().contains("invalid JSON"),
                        "{input}: {error}"
                    );
                }
                let units = "{\"x\":1}".encode_utf16().collect::<Vec<_>>();
                let output = format(&mut caller, &units, &[0xd800])?;
                assert!(output.units().contains(&0xd800));
                drop(output);
                let deep = format!("{}0{}", "[".repeat(129), "]".repeat(129))
                    .encode_utf16()
                    .collect::<Vec<_>>();
                assert!(
                    format(&mut caller, &deep, &[])
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("128 levels")
                );
                let shared = format!("[{}0]", "0,".repeat(100000))
                    .encode_utf16()
                    .collect::<Vec<_>>();
                assert!(
                    format(&mut caller, &shared, &[])
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("structural visits")
                );
                let output = format(&mut caller, &units, &[])?;
                assert_eq!(output.units(), units);
                drop(output);
                assert_eq!(caller.data().tenant_limits.host_attached_bytes(), 0);
                Ok(())
            },
        );
        store.set_fuel(100000000).unwrap();
        callback.call_async(&mut store, &[], &mut []).await.unwrap();
    }
}
