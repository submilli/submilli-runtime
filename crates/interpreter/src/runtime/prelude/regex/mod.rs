//! RegExp construction, matching and String regex methods over the regex engine.
//!
//! `$regex` fields: vtable, ChargedRegex externref, mutable lastIndex, source,
//! flags and flag bits. Match arrays extend `$Array` with index, input and groups
//! at fields 3, 4 and 5. Captures live in the ordinary mutable array backing;
//! their array vtable provides the usual methods and serialization.

use crate::runtime::host::abi_arg;
pub(crate) mod engine;
pub(crate) mod input;
mod install;
mod match_all;
mod output;
mod replace_callback;

pub use install::{declare, install};
pub(crate) use install::{declare_types, replacement_type};

use wasmtime::{Caller, ExternRef, Rooted, StructRef, StructRefPre, Val};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    host_array_vtable, host_regex_vtable, read_string_arg, write_submilli_array_struct,
    write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::vtable::read_string_units;
use engine::{ChargedRegex, ExecSnapshot, FlagSet, compile_charged, exec_snapshot};

/// `g | y` — the two flags whose presence makes matching consult and advance
/// `lastIndex`.
const G_OR_Y: i32 = FlagSet::G.bits() as i32 | FlagSet::Y.bits() as i32;

// ---------------------------------------------------------------------------
// $regex field access
// ---------------------------------------------------------------------------

/// Cast an erased `(ref null $Object)` receiver to its backing struct.
fn as_struct(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    let Val::AnyRef(Some(any)) = val else {
        return Err(crate::runtime::host::type_error(format!(
            "{name}: receiver is null"
        )));
    };
    any.as_struct(&mut *caller)?.ok_or_else(|| {
        crate::runtime::host::type_error(format!("{name}: receiver is not a struct"))
    })
}

/// `$regex` field 5 — the JS flag bitset.
fn flag_bits(caller: &mut Caller<'_, StoreData>, st: &Rooted<StructRef>) -> wasmtime::Result<i32> {
    match st.field(&mut *caller, 5)? {
        Val::I32(bits) => Ok(bits),
        other => wasmtime::bail!("RegExp: flag bitset is {other:?}, not i32"),
    }
}

/// `$regex` field 2 — the current `lastIndex` (clamped non-negative).
fn last_index(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
) -> wasmtime::Result<usize> {
    match st.field(&mut *caller, 2)? {
        Val::F64(bits) => Ok(f64::from_bits(bits).clamp(0.0, 9_007_199_254_740_991.0) as usize),
        other => wasmtime::bail!("RegExp: lastIndex is {other:?}, not f64"),
    }
}

/// `g`/`y` set → matching consults and advances `lastIndex`; neither → ignore it.
fn uses_last_index(bits: i32) -> bool {
    bits & G_OR_Y != 0
}

/// Match `input` at `start`, honouring the sticky (`y`) anchoring rule. Returns an
/// owned snapshot so the borrow on the compiled `regex` ends before any GC
/// allocation.
fn exec_at(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    input: &input::DecodedInput,
    start: usize,
) -> wasmtime::Result<Option<ExecSnapshot>> {
    let extern_ref = match st.field(&mut *caller, 1)? {
        Val::ExternRef(Some(r)) => r,
        other => wasmtime::bail!("RegExp: field 1 is {other:?}, not a compiled-regex externref"),
    };
    let charged = extern_ref
        .data(&caller)?
        .ok_or_else(|| wasmtime::Error::msg("RegExp: compiled regex was reclaimed"))?
        .downcast_ref::<ChargedRegex>()
        .ok_or_else(|| wasmtime::Error::msg("RegExp: externref had unexpected payload type"))?;
    let requested = start;
    let unicode = charged.flags.has(FlagSet::U);
    let Some(start) = input.unit_to_byte(start, unicode) else {
        return Ok(None);
    };
    let raw = exec_snapshot(&charged.regex, input, start);
    let sticky = charged.flags.has(FlagSet::Y);
    // The bytes the engine scanned: up to the match on a hit, to the end on a
    // miss. Known only afterwards, so a global loop over k matches pays for
    // the input once, not k times.
    let scanned = raw.as_ref().map_or(input.len(), |hit| hit.match_end);
    fuel::charge(
        &mut *caller,
        fuel::REGEX,
        scanned.saturating_sub(start) as u64,
    )?;
    if let Some(hit) = &raw
        && sticky
        && (hit.match_start != start
            || (!unicode && input.byte_to_unit(hit.match_start)? != requested))
    {
        return Ok(None);
    }
    Ok(raw)
}

fn find_at(
    caller: &mut Caller<'_, StoreData>,
    regex: &Rooted<StructRef>,
    input: &input::DecodedInput,
    start: usize,
) -> wasmtime::Result<Option<(usize, usize)>> {
    let requested = start;
    let unicode = with_regex(caller, regex, |compiled| compiled.flags.has(FlagSet::U))?;
    let Some(start) = input.unit_to_byte(start, unicode) else {
        return Ok(None);
    };
    let (found, sticky) = with_regex(caller, regex, |compiled| {
        (
            engine::find(&compiled.regex, input, start),
            compiled.flags.has(FlagSet::Y),
        )
    })?;
    let scanned = found.map_or(input.len(), |(_, end)| end);
    fuel::charge(
        &mut *caller,
        fuel::REGEX,
        scanned.saturating_sub(start) as u64,
    )?;
    let Some((index, end)) = found else {
        return Ok(None);
    };
    let unit = input.byte_to_unit(index)?;
    if sticky && (index != start || (!unicode && unit != requested)) {
        return Ok(None);
    }
    Ok(Some((unit, input.byte_to_unit(end)?)))
}

/// Write `lastIndex` back after a match attempt: `g`/`y` set → post-match offset
/// on a hit, `0` on a miss; neither → leave it alone (ECMA-262).
fn write_last_index(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    bits: i32,
    next: Option<usize>,
) -> wasmtime::Result<()> {
    if uses_last_index(bits) {
        st.set_field(
            &mut *caller,
            2,
            Val::F64((next.unwrap_or(0) as f64).to_bits()),
        )?;
    }
    Ok(())
}

/// Read the compiled `ChargedRegex` from a `$regex` receiver and run `f` on it.
/// `f` returns an owned value so the borrow ends before the caller allocates.
fn with_regex<T>(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    f: impl FnOnce(&ChargedRegex) -> T,
) -> wasmtime::Result<T> {
    let extern_ref = match st.field(&mut *caller, 1)? {
        Val::ExternRef(Some(r)) => r,
        other => wasmtime::bail!("RegExp: field 1 is {other:?}, not a compiled-regex externref"),
    };
    let charged = extern_ref
        .data(&caller)?
        .ok_or_else(|| wasmtime::Error::msg("RegExp: compiled regex was reclaimed"))?
        .downcast_ref::<ChargedRegex>()
        .ok_or_else(|| wasmtime::Error::msg("RegExp: externref had unexpected payload type"))?;
    Ok(f(charged))
}

// ---------------------------------------------------------------------------
// construction
// ---------------------------------------------------------------------------

/// `RegExpConstructor#new(source, flags)`: compile against the tenant's memory
/// budget, then build the `$regex` object. The `$string` args are reused verbatim
/// as the cached `source`/`flags` fields.
pub(super) fn construct(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    flags: &Val,
) -> wasmtime::Result<Val> {
    let source_str = read_string_arg(&mut *caller, source, "new RegExp(source)")?;
    let flags_str = read_string_arg(&mut *caller, flags, "new RegExp(flags)")?;
    // Compiling is not linear in the source (counted repetition expands it),
    // so a flat surcharge covers what the program size cap allows.
    fuel::charge_host_fuel(
        &mut *caller,
        fuel::PARSE
            .cost(source_str.len() as u64)
            .saturating_add(fuel::REGEX_COMPILE),
    )?;
    let charged = {
        let limits = &caller.data().tenant_limits;
        // An invalid pattern or flag is a spec `SyntaxError`; a tenant
        // memory-cap breach keeps its type, which ends the run.
        compile_charged(limits, &source_str, &flags_str).map_err(|e| match e {
            engine::RegexCompileError::Syntax(syntax) => {
                crate::runtime::host::syntax_error(format!("RegExp: {syntax}"))
            }
            engine::RegexCompileError::Memory(cap) => wasmtime::Error::new(cap).context("RegExp"),
        })?
    };
    let bits = i32::from(charged.flag_bits());
    let extern_ref = ExternRef::new(&mut *caller, charged)?;

    let regex_ty = intrinsic_types(&mut *caller)?.regex.clone();
    let vtable = host_regex_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, regex_ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::ExternRef(Some(extern_ref)),
            Val::F64(0.0_f64.to_bits()),
            *source,
            *flags,
            Val::I32(bits),
            Val::I64(0),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Build a fresh match box, retaining the immutable input value without copying it.
fn build_match_box(
    caller: &mut Caller<'_, StoreData>,
    input: &input::DecodedInput,
    input_value: &Val,
    snapshot: &ExecSnapshot,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;

    let matched = input_units(caller, input, snapshot.match_start..snapshot.match_end)?;
    let match_str = write_submilli_string_struct_units(caller, &matched)?;

    let mut named = std::collections::BTreeMap::new();
    for (name, slot) in &snapshot.named {
        let value = capture_value(caller, input, *slot)?;
        named.insert(name.clone(), value);
    }
    let named = if named.is_empty() {
        super::undefined::value(caller)?
    } else {
        crate::stdlib::abi::new_object(caller, named)?
    };
    let mut elements = Vec::new();
    elements
        .try_reserve_exact(snapshot.numbered.len().saturating_add(1))
        .map_err(crate::runtime::host::fatal_host_error)?;
    elements.push(Val::AnyRef(Some(match_str.to_anyref())));
    for slot in &snapshot.numbered {
        elements.push(capture_value(caller, input, *slot)?);
    }
    let array = write_submilli_array_struct(caller, &elements)?;
    let backing = array.field(&mut *caller, 1)?;
    let length = array.field(&mut *caller, 2)?;
    let vtable = host_array_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, intr.regex_match_box.clone());
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            backing,
            length,
            Val::I32(
                i32::try_from(input.byte_to_unit(snapshot.match_start)?)
                    .map_err(|_| wasmtime::Error::msg("RegExp: match index exceeds i32"))?,
            ),
            *input_value,
            named,
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// A participating capture as a string, or undefined for an unmatched group.
fn capture_value(
    caller: &mut Caller<'_, StoreData>,
    input: &input::DecodedInput,
    slot: Option<(usize, usize)>,
) -> wasmtime::Result<Val> {
    match slot {
        Some((s, e)) => {
            let capture = input_units(caller, input, s..e)?;
            let raw = write_submilli_string_struct_units(caller, &capture)?;
            Ok(Val::AnyRef(Some(raw.to_anyref())))
        }
        None => super::undefined::value(caller),
    }
}

/// The input's code units between two byte offsets of its decoded text, which
/// keeps any lone surrogate the text shows as U+FFFD.
fn input_units(
    caller: &mut Caller<'_, StoreData>,
    input: &input::DecodedInput,
    bytes: std::ops::Range<usize>,
) -> wasmtime::Result<Vec<u16>> {
    fuel::charge(&mut *caller, fuel::SCAN, bytes.len() as u64)?;
    input.units(bytes)
}

// ---------------------------------------------------------------------------
// RegExp instance methods
// ---------------------------------------------------------------------------

pub(super) fn test(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<bool> {
    let params: &[Val; 2] = params.try_into().map_err(|_| {
        crate::runtime::host::fatal_host_error("RegExp#test: invalid argument count")
    })?;
    let st = as_struct(caller, &params[0], "RegExp#test")?;
    let bits = flag_bits(caller, &st)?;
    let input = input::read(caller, &params[1])?;
    let start = if uses_last_index(bits) {
        last_index(caller, &st)?
    } else {
        0
    };
    let snapshot = find_at(caller, &st, &input, start)?;
    write_last_index(caller, &st, bits, snapshot.map(|(_, end)| end))?;
    Ok(snapshot.is_some())
}

pub(super) fn exec(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    let params: &[Val; 2] = params
        .try_into()
        .map_err(|_| wasmtime::Error::msg("exec: invalid argument count"))?;
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExp#exec")?;
    let bits = flag_bits(caller, &st)?;
    let input = input::read(caller, &params[1])?;
    let start = if uses_last_index(bits) {
        last_index(caller, &st)?
    } else {
        0
    };
    let snapshot = exec_at(caller, &st, &input, start)?;
    write_last_index(
        caller,
        &st,
        bits,
        snapshot
            .as_ref()
            .map(|s| input.byte_to_unit(s.next_last_index))
            .transpose()?,
    )?;
    match snapshot {
        Some(s) => build_match_box(caller, &input, abi_arg(params, 1)?, &s),
        None => Ok(Val::null_any_ref()),
    }
}

/// A `(ref $string)` property getter (`source` = field 3, `flags` = field 4).
pub(super) fn string_field(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    field: usize,
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExp property")?;
    st.field(&mut *caller, field)
}

pub(super) fn last_index_getter(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExp#lastIndex")?;
    match st.field(&mut *caller, 2)? {
        Val::F64(bits) => Ok(f64::from_bits(bits)),
        other => wasmtime::bail!("RegExp: lastIndex is {other:?}, not f64"),
    }
}

/// Store the assigned number unchanged; matching applies ToLength when reading it.
pub(super) fn last_index_setter(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<()> {
    let Val::F64(bits) = *abi_arg(params, 1)? else {
        wasmtime::bail!("RegExp#lastIndex: value is not f64");
    };
    set_last_index(caller, abi_arg(params, 0)?, f64::from_bits(bits))
}

/// `re.lastIndex = value`, for the host setter and for a write through a
/// structural view of the regex.
pub(crate) fn set_last_index(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    value: f64,
) -> wasmtime::Result<()> {
    let st = as_struct(caller, receiver, "RegExp#lastIndex")?;
    st.set_field(&mut *caller, 2, Val::F64(value.to_bits()))
}

/// `RegExp.escape(s)` (ES2025): `s` with every character that has a meaning
/// in a pattern escaped, so the result matches `s` literally. A leading
/// letter or digit is escaped too, so the result can follow `\0` or `\c`.
pub(super) fn escape_units(input: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(input.len());
    for (index, c) in char::decode_utf16(input.iter().copied()).enumerate() {
        let push = |out: &mut Vec<u16>, text: &str| out.extend(text.encode_utf16());
        match c {
            Err(lone) => push(&mut out, &format!("\\u{:04x}", lone.unpaired_surrogate())),
            Ok(c) if index == 0 && c.is_ascii_alphanumeric() => {
                push(&mut out, &format!("\\x{:02x}", u32::from(c)));
            }
            Ok(c) => push(&mut out, &escape_char(c)),
        }
    }
    out
}

fn escape_char(c: char) -> String {
    const SYNTAX: &str = "^$\\.*+?()[]{}|/";
    const OTHER_PUNCTUATORS: &str = ",-=<>#&!%:;@~'`\"";
    if SYNTAX.contains(c) {
        return format!("\\{c}");
    }
    let control = match c {
        '\t' => Some('t'),
        '\n' => Some('n'),
        '\u{b}' => Some('v'),
        '\u{c}' => Some('f'),
        '\r' => Some('r'),
        _ => None,
    };
    if let Some(letter) = control {
        return format!("\\{letter}");
    }
    if !(OTHER_PUNCTUATORS.contains(c) || is_js_whitespace_or_line_terminator(c)) {
        return c.to_string();
    }
    if u32::from(c) <= 0xFF {
        return format!("\\x{:02x}", u32::from(c));
    }
    let mut units = [0u16; 2];
    c.encode_utf16(&mut units)
        .iter()
        .map(|unit| format!("\\u{unit:04x}"))
        .collect()
}

/// ECMAScript `WhiteSpace` or `LineTerminator`: the Zs category plus TAB, VT,
/// FF, ZWNBSP, LF, CR, LS and PS.
fn is_js_whitespace_or_line_terminator(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// A boolean flag getter — `flag_mask & bitset != 0`.
pub(super) fn flag(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    mask: i32,
) -> wasmtime::Result<bool> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExp flag")?;
    Ok(flag_bits(caller, &st)? & mask != 0)
}

// ---------------------------------------------------------------------------
// RegExpMatch accessors
// ---------------------------------------------------------------------------

/// Metadata exists only on non-global matches; a global match is a plain Array.
pub(super) fn match_metadata(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    name: &str,
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExp match array")?;
    let intr = intrinsic_types(&mut *caller)?;
    if !st.matches_ty(&*caller, &intr.regex_match_box)? {
        return super::undefined::value(caller);
    }
    match name {
        "index" => {
            let Val::I32(index) = st.field(&mut *caller, 3)? else {
                return Err(crate::runtime::host::fatal_host_error(
                    "invalid regex match index",
                ));
            };
            crate::runtime::json::JsonUnknownAllocator::new(&mut *caller)?.allocate(
                &mut *caller,
                &crate::runtime::json_text::JsonValue::Number(f64::from(index)),
            )
        }
        "input" => st.field(&mut *caller, 4),
        "groups" => st.field(&mut *caller, 5),
        _ => Err(crate::runtime::host::fatal_host_error(
            "unknown regex metadata property",
        )),
    }
}

// ---------------------------------------------------------------------------
// String regex-arm methods
// ---------------------------------------------------------------------------

/// Distinguish the `string | RegExp` arg: a `$string` receiver takes the literal
/// arm; anything else is a `$regex`.
fn arg_is_string(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<bool> {
    let string_ty = intrinsic_types(&mut *caller)?.string.clone();
    match val {
        Val::AnyRef(Some(any)) => match any.as_struct(&mut *caller)? {
            Some(st) => Ok(wasmtime::StructType::eq(&st.ty(&caller)?, &string_ty)),
            None => Ok(false),
        },
        _ => Ok(false),
    }
}

/// Global matching collects full matches; non-global matching follows exec,
/// including the sticky expression's lastIndex.
pub(super) fn string_match(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let input_value = *abi_arg(params, 0)?;
    let st = pattern_arg(caller, abi_arg(params, 1)?, "String#match(regex)")?;
    let bits = flag_bits(caller, &st)?;
    if bits & FlagSet::G.bits() as i32 == 0 {
        return exec(caller, &[Val::AnyRef(Some(st.to_anyref())), input_value]);
    }
    st.set_field(&mut *caller, 2, Val::F64(0.0_f64.to_bits()))?;
    let input = input::read(caller, &input_value)?;
    let mut matches = output::Buffer::<Val>::new();
    let mut position = 0;
    while let Some(found) = exec_at(caller, &st, &input, position)? {
        let units = input_units(caller, &input, found.match_start..found.match_end)?;
        let value = write_submilli_string_struct_units(caller, &units)?;
        matches.push(caller, Val::AnyRef(Some(value.to_anyref())))?;
        let end = input.byte_to_unit(found.next_last_index)?;
        position = if found.match_start == found.match_end {
            input.advance(end, bits & FlagSet::U.bits() as i32 != 0)?
        } else {
            end
        };
    }
    if matches.values().is_empty() {
        return Ok(Val::null_any_ref());
    }
    let array =
        crate::runtime::host::write_submilli_array_struct_precharged(caller, matches.values())?;
    Ok(Val::AnyRef(Some(array.to_anyref())))
}

/// The regex `match` or `search` runs: a string argument is compiled as a
/// pattern with no flags, as JavaScript's `new RegExp(s)` does.
fn pattern_arg(
    caller: &mut Caller<'_, StoreData>,
    arg: &Val,
    name: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    if !arg_is_string(caller, arg)? {
        return as_struct(caller, arg, name);
    }
    let no_flags = Val::AnyRef(Some(
        crate::runtime::host::write_submilli_string_struct(caller, "")?.to_anyref(),
    ));
    let regex = construct(caller, arg, &no_flags)?;
    as_struct(caller, &regex, name)
}

/// `String#search(re)` — index of the first match, or `-1`.
pub(super) fn string_search(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let input = input::read(caller, abi_arg(params, 0)?)?;
    let st = pattern_arg(caller, abi_arg(params, 1)?, "String#search(regex)")?;
    Ok(match find_at(caller, &st, &input, 0)? {
        Some((start, _)) => start as f64,
        None => -1.0,
    })
}

pub(super) fn string_match_all(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    match_all::create(caller, params)
}

/// `String#replace(search, replacement)` — literal arm applies JS replacement
/// patterns to the first match; regex arm forwards to the crate (replacing all
/// when the pattern is `g`).
pub(super) fn string_replace(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    if arg_is_string(caller, abi_arg(params, 1)?)? {
        let _inputs = reserve_string_inputs(caller, params)?;
        let input = read_string_units(caller, abi_arg(params, 0)?, "String#replace(input)")?;
        let search = read_string_units(caller, abi_arg(params, 1)?, "String#replace(search)")?;
        let repl = read_string_units(caller, abi_arg(params, 2)?, "String#replace(replacement)")?;
        fuel::charge(
            &mut *caller,
            fuel::SCAN,
            (input.len() + search.len()) as u64,
        )?;
        let out = replace_literal_bounded(caller, &input, &search, &repl, false)?;
        let st = write_submilli_string_struct_units(caller, out.values())?;
        return Ok(Val::AnyRef(Some(st.to_anyref())));
    }
    let st = as_struct(caller, abi_arg(params, 1)?, "String#replace(regex)")?;
    let all = flag_bits(caller, &st)? & FlagSet::G.bits() as i32 != 0;
    replace_regex(caller, params, &st, all, "String#replace(replacement)")
}

/// `String#replace` or `replaceAll` whose replacement may be a function.
pub(super) async fn string_replace_any(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    all: bool,
) -> wasmtime::Result<Val> {
    if arg_is_string(caller, abi_arg(params, 2)?)? {
        return if all {
            string_replace_all(caller, params)
        } else {
            string_replace(caller, params)
        };
    }
    replace_callback::replace_with_callback(
        caller,
        abi_arg(params, 0)?,
        abi_arg(params, 1)?,
        abi_arg(params, 2)?,
        all,
    )
    .await
}

/// `String#replaceAll(search, replacement)` — always all occurrences (the regex
/// arm does not require `g`, a deliberate JS divergence kept from the Wasm body).
pub(super) fn string_replace_all(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    if arg_is_string(caller, abi_arg(params, 1)?)? {
        let _inputs = reserve_string_inputs(caller, params)?;
        let input = read_string_units(caller, abi_arg(params, 0)?, "String#replaceAll(input)")?;
        let search = read_string_units(caller, abi_arg(params, 1)?, "String#replaceAll(search)")?;
        let repl = read_string_units(
            caller,
            abi_arg(params, 2)?,
            "String#replaceAll(replacement)",
        )?;
        fuel::charge(
            &mut *caller,
            fuel::SCAN,
            (input.len() + search.len()) as u64,
        )?;
        let out = replace_literal_bounded(caller, &input, &search, &repl, true)?;
        let st = write_submilli_string_struct_units(caller, out.values())?;
        return Ok(Val::AnyRef(Some(st.to_anyref())));
    }
    let st = as_struct(caller, abi_arg(params, 1)?, "String#replaceAll(regex)")?;
    replace_regex(caller, params, &st, true, "String#replaceAll(replacement)")
}

/// `String#split(separator, limit)` — literal arm splits on code units; regex arm
/// splits via the crate. `limit` saturates to `i32`; negative means uncapped.
pub(super) fn string_split(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let limit = match *abi_arg(params, 2)? {
        Val::F64(bits) => f64::from_bits(bits) as i32,
        ref other => {
            return Err(crate::runtime::host::type_error(format!(
                "String#split(limit) is {other:?}, not f64"
            )));
        }
    };
    let mut elements = output::Buffer::<Val>::new();
    let cap = if limit < 0 {
        usize::MAX
    } else {
        limit as usize
    };
    if cap == 0 {
        let empty = write_submilli_array_struct(caller, &[])?;
        return Ok(Val::AnyRef(Some(empty.to_anyref())));
    }
    if arg_is_string(caller, abi_arg(params, 1)?)? {
        let _inputs = reserve_string_inputs(
            caller,
            params.get(..2).ok_or_else(|| {
                crate::runtime::host::fatal_host_error("String#split: invalid argument count")
            })?,
        )?;
        let input = read_string_units(caller, abi_arg(params, 0)?, "String#split(input)")?;
        let separator = read_string_units(caller, abi_arg(params, 1)?, "String#split(separator)")?;
        fuel::charge(
            &mut *caller,
            fuel::SCAN,
            (input.len() + separator.len()) as u64,
        )?;
        split_literal_bounded(caller, &mut elements, &input, &separator, cap)?;
    } else {
        let regex = as_struct(caller, abi_arg(params, 1)?, "String#split(regex)")?;
        let input = input::read(caller, abi_arg(params, 0)?)?;
        fuel::charge(&mut *caller, fuel::REGEX, input.len() as u64)?;
        let regex = with_regex(caller, &regex, |compiled| compiled.regex.clone())?;
        split_regex_bounded(caller, &mut elements, &regex, &input, cap)?;
    }
    let arr =
        crate::runtime::host::write_submilli_array_struct_precharged(caller, elements.values())?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

fn reserve_string_inputs(
    caller: &mut Caller<'_, StoreData>,
    values: &[Val],
) -> wasmtime::Result<crate::runtime::limits::HostBytes> {
    let mut units = 0u64;
    for value in values {
        let string = as_struct(caller, value, "String operation input")?;
        let payload = match string.field(&mut *caller, 1)? {
            Val::AnyRef(Some(payload)) => payload.unwrap_array(&mut *caller)?,
            _ => {
                return Err(crate::runtime::host::fatal_host_error(
                    "String operation has malformed payload",
                ));
            }
        };
        units = units.saturating_add(u64::from(payload.len(&mut *caller)?));
    }
    // Both the copied UTF-16 and decoded UTF-8 can coexist on regex paths.
    Ok(crate::runtime::limits::HostBytes::new(
        &caller.data().tenant_limits,
        units.saturating_mul(8),
    )?)
}

/// The regex arm of `replace` and `replaceAll`: the first match, or every match
/// when `all`, replaced by the expansion of the replacement pattern.
fn replace_regex(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    regex: &Rooted<StructRef>,
    all: bool,
    replacement_label: &str,
) -> wasmtime::Result<Val> {
    let _inputs = reserve_string_inputs(caller, &[*abi_arg(params, 0)?, *abi_arg(params, 2)?])?;
    let input = input::read(caller, abi_arg(params, 0)?)?;
    let replacement = read_string_units(caller, abi_arg(params, 2)?, replacement_label)?;
    fuel::charge(&mut *caller, fuel::REGEX, input.len() as u64)?;
    let (regex, has_named) = with_regex(caller, regex, |compiled| {
        let regex = compiled.regex.clone();
        let has_named = regex.capture_names().flatten().next().is_some();
        (regex, has_named)
    })?;
    let out = replace_regex_bounded(caller, &regex, &input, &replacement, all, has_named)?;
    let result = write_submilli_string_struct_units(caller, out.values())?;
    Ok(Val::AnyRef(Some(result.to_anyref())))
}

fn replace_regex_bounded(
    caller: &mut Caller<'_, StoreData>,
    regex: &regex::Regex,
    input: &input::DecodedInput,
    replacement: &[u16],
    all: bool,
    has_named: bool,
) -> wasmtime::Result<output::Buffer<u16>> {
    let mut output = output::Buffer::new();
    let mut previous = 0;
    let capture_slots = regex.captures_len() as u64;
    let _captures = crate::runtime::limits::HostBytes::new(
        &caller.data().tenant_limits,
        capture_slots.saturating_mul(128),
    )?;
    let mut position = 0;
    while let Some(hit) = engine::exec_snapshot(regex, input, position) {
        fuel::charge(&mut *caller, fuel::ELEM, capture_slots)?;
        let prefix = input_units(caller, input, previous..hit.match_start)?;
        output.append(caller, &prefix)?;
        fuel::charge(&mut *caller, fuel::SCAN, replacement.len() as u64)?;
        expand_replacement(
            caller,
            &mut output,
            &ReplacedInput::Decoded(input),
            replacement,
            &hit,
            has_named,
        )?;
        previous = hit.match_end;
        if !all {
            break;
        }
        position = if hit.match_start == hit.match_end {
            next_char_boundary(input, hit.match_end)
        } else {
            hit.match_end
        };
    }
    let suffix = input_units(caller, input, previous..input.len())?;
    output.append(caller, &suffix)?;
    Ok(output)
}

const DOLLAR: u16 = b'$' as u16;
const AMPERSAND: u16 = b'&' as u16;
const BACKTICK: u16 = b'`' as u16;
const APOSTROPHE: u16 = b'\'' as u16;
const LESS_THAN: u16 = b'<' as u16;
const GREATER_THAN: u16 = b'>' as u16;

/// What a `$` in a replacement pattern stands for (ECMA-262 GetSubstitution).
enum Substitution {
    /// A literal `$`.
    Dollar { consumed: usize },
    /// The input between two offsets, or nothing for a group that didn't
    /// participate.
    Input {
        span: Option<(usize, usize)>,
        consumed: usize,
    },
}

/// The input a replacement copies from: the regex arm's decoded input, whose
/// offsets are bytes of its text, or the string arm's code units.
enum ReplacedInput<'a> {
    Decoded(&'a input::DecodedInput),
    Units(&'a [u16]),
}

impl ReplacedInput<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Decoded(input) => input.len(),
            Self::Units(units) => units.len(),
        }
    }

    fn append_span(
        &self,
        caller: &mut Caller<'_, StoreData>,
        output: &mut output::Buffer<u16>,
        start: usize,
        end: usize,
    ) -> wasmtime::Result<()> {
        match self {
            Self::Decoded(input) => {
                let units = input_units(caller, input, start..end)?;
                output.append(caller, &units)
            }
            Self::Units(units) => output.append(caller, checked_units(units, start, end)?),
        }
    }
}

/// Append `replacement` with each `$` pattern expanded against `hit`. Both
/// arms of `replace` and `replaceAll` expand through it; a string search has
/// no groups, so `$1` and `$<name>` stay literal there.
fn expand_replacement(
    caller: &mut Caller<'_, StoreData>,
    output: &mut output::Buffer<u16>,
    input: &ReplacedInput<'_>,
    replacement: &[u16],
    hit: &ExecSnapshot,
    has_named: bool,
) -> wasmtime::Result<()> {
    let mut position = 0;
    while let Some(dollar) = next_dollar(replacement, position) {
        output.append(caller, checked_units(replacement, position, dollar)?)?;
        let pattern = checked_units(replacement, dollar, replacement.len())?;
        let consumed = match substitution(pattern, hit, input.len(), has_named) {
            Substitution::Dollar { consumed } => {
                output.append(caller, &[DOLLAR])?;
                consumed
            }
            Substitution::Input { span, consumed } => {
                if let Some((start, end)) = span {
                    input.append_span(caller, output, start, end)?;
                }
                consumed
            }
        };
        position = dollar + consumed;
    }
    output.append(
        caller,
        checked_units(replacement, position, replacement.len())?,
    )
}

fn next_dollar(replacement: &[u16], from: usize) -> Option<usize> {
    let offset = replacement.get(from..)?.iter().position(|u| *u == DOLLAR)?;
    Some(from + offset)
}

/// The substitution for the `$` that starts `pattern`, given the match's
/// offsets in an input of `input_len`.
fn substitution(
    pattern: &[u16],
    hit: &ExecSnapshot,
    input_len: usize,
    has_named: bool,
) -> Substitution {
    let digit = |index: usize| {
        pattern
            .get(index)
            .and_then(|unit| char::from_u32(u32::from(*unit))?.to_digit(10))
            .map(|digit| digit as usize)
    };
    let groups = hit.numbered.len();
    let group = |number: usize, consumed: usize| Substitution::Input {
        span: hit.numbered.get(number - 1).copied().flatten(),
        consumed,
    };
    let between = |start: usize, end: usize| Substitution::Input {
        span: Some((start, end)),
        consumed: 2,
    };
    match pattern.get(1).copied() {
        Some(DOLLAR) => Substitution::Dollar { consumed: 2 },
        Some(AMPERSAND) => between(hit.match_start, hit.match_end),
        Some(BACKTICK) => between(0, hit.match_start),
        Some(APOSTROPHE) => between(hit.match_end, input_len),
        Some(LESS_THAN) if has_named => named_substitution(pattern, hit),
        _ => match (digit(1), digit(2)) {
            (Some(tens), Some(ones)) if (1..=groups).contains(&(tens * 10 + ones)) => {
                group(tens * 10 + ones, 3)
            }
            (Some(number), _) if (1..=groups).contains(&number) => group(number, 2),
            _ => Substitution::Dollar { consumed: 1 },
        },
    }
}

/// `$<name>`: the named group's text, or nothing when no group has that name.
/// Without a closing `>` the `$` is literal.
fn named_substitution(pattern: &[u16], hit: &ExecSnapshot) -> Substitution {
    let Some(close) = pattern.iter().position(|unit| *unit == GREATER_THAN) else {
        return Substitution::Dollar { consumed: 1 };
    };
    let name = String::from_utf16_lossy(pattern.get(2..close).unwrap_or_default());
    let span = hit
        .named
        .iter()
        .find(|(group, _)| *group == name)
        .and_then(|(_, span)| *span);
    Substitution::Input {
        span,
        consumed: close + 1,
    }
}

/// The regex arm of `split` (ECMA-262 `RegExp.prototype[@@split]`): an empty
/// match where the previous part ended doesn't split, and each split adds the
/// match's captures, `undefined` for a group that didn't participate.
fn split_regex_bounded(
    caller: &mut Caller<'_, StoreData>,
    elements: &mut output::Buffer<Val>,
    regex: &regex::Regex,
    input: &input::DecodedInput,
    cap: usize,
) -> wasmtime::Result<()> {
    let size = input.len();
    if size == 0 {
        if engine::find(regex, input, 0).is_none() {
            push_split_part(caller, elements, &[])?;
        }
        return Ok(());
    }
    let mut part_start = 0;
    let mut position = 0;
    while position < size {
        let Some(hit) = engine::exec_snapshot(regex, input, position) else {
            break;
        };
        if hit.match_start >= size {
            break;
        }
        if hit.match_end == part_start {
            position = next_char_boundary(input, hit.match_start);
            continue;
        }
        let part = input_units(caller, input, part_start..hit.match_start)?;
        push_split_part(caller, elements, &part)?;
        for group in &hit.numbered {
            if elements.values().len() == cap {
                return Ok(());
            }
            let value = match *group {
                Some((start, end)) => {
                    let units = input_units(caller, input, start..end)?;
                    let string = write_submilli_string_struct_units(caller, &units)?;
                    Val::AnyRef(Some(string.to_anyref()))
                }
                None => super::undefined::value(caller)?,
            };
            elements.push(caller, value)?;
        }
        if elements.values().len() == cap {
            return Ok(());
        }
        part_start = hit.match_end;
        position = part_start;
    }
    let rest = input_units(caller, input, part_start..size)?;
    push_split_part(caller, elements, &rest)
}

/// The byte offset of the character after the one at `byte`.
fn next_char_boundary(text: &str, byte: usize) -> usize {
    text.get(byte..)
        .and_then(|rest| rest.chars().next())
        .map_or(byte + 1, |c| byte + c.len_utf8())
}

fn split_literal_bounded(
    caller: &mut Caller<'_, StoreData>,
    elements: &mut output::Buffer<Val>,
    input: &[u16],
    separator: &[u16],
    cap: usize,
) -> wasmtime::Result<()> {
    if separator.is_empty() {
        for unit in input.iter().take(cap) {
            push_split_part(caller, elements, &[*unit])?;
        }
        return Ok(());
    }
    let matcher = super::string::search::Search::new(separator, false);
    let mut position = 0;
    while let Some(found) = matcher.find(input, position) {
        if elements.values().len() == cap {
            return Ok(());
        }
        push_split_part(caller, elements, checked_units(input, position, found)?)?;
        position = found
            .checked_add(separator.len())
            .ok_or_else(|| crate::runtime::host::fatal_host_error("split position overflow"))?;
    }
    if elements.values().len() < cap {
        push_split_part(
            caller,
            elements,
            checked_units(input, position, input.len())?,
        )?;
    }
    Ok(())
}

fn push_split_part(
    caller: &mut Caller<'_, StoreData>,
    elements: &mut output::Buffer<Val>,
    units: &[u16],
) -> wasmtime::Result<()> {
    let string = write_submilli_string_struct_units(caller, units)?;
    elements.push(caller, Val::AnyRef(Some(string.to_anyref())))
}

fn replace_literal_bounded(
    caller: &mut Caller<'_, StoreData>,
    input: &[u16],
    search: &[u16],
    replacement: &[u16],
    all: bool,
) -> wasmtime::Result<output::Buffer<u16>> {
    let matcher = super::string::search::Search::new(search, false);
    let mut output = output::Buffer::new();
    let mut position = 0;
    while let Some(found) = matcher.find(input, position) {
        output.append(caller, checked_units(input, position, found)?)?;
        let end = found.checked_add(search.len()).ok_or_else(|| {
            crate::runtime::host::fatal_host_error("replacement match offset overflow")
        })?;
        fuel::charge(&mut *caller, fuel::SCAN, replacement.len() as u64)?;
        expand_literal(caller, &mut output, input, replacement, found, end)?;
        position = end;
        if !all {
            break;
        }
        if search.is_empty() {
            if let Some(unit) = input.get(found) {
                output.append(caller, &[*unit])?;
            }
            position = found.checked_add(1).ok_or_else(|| {
                crate::runtime::host::fatal_host_error("replacement match offset overflow")
            })?;
            if position > input.len() {
                break;
            }
        }
    }
    if position <= input.len() {
        output.append(caller, checked_units(input, position, input.len())?)?;
    }
    Ok(output)
}

fn expand_literal(
    caller: &mut Caller<'_, StoreData>,
    output: &mut output::Buffer<u16>,
    input: &[u16],
    replacement: &[u16],
    start: usize,
    end: usize,
) -> wasmtime::Result<()> {
    let hit = ExecSnapshot {
        match_start: start,
        match_end: end,
        next_last_index: end,
        numbered: Vec::new(),
        named: Vec::new(),
    };
    let input = ReplacedInput::Units(input);
    expand_replacement(caller, output, &input, replacement, &hit, false)
}

fn checked_units(input: &[u16], start: usize, end: usize) -> wasmtime::Result<&[u16]> {
    input
        .get(start..end)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("invalid string operation span"))
}

// ---------------------------------------------------------------------------
// pure UTF-16 string algorithms (literal arm) — unit-tested without a store
// ---------------------------------------------------------------------------

/// First index at/after `from` where `needle` occurs in `hay`. An empty needle
/// matches at `from` (JS treats `""` as present everywhere).
#[cfg(test)]
fn find(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    super::string::search::Search::new(needle, false).find(hay, from)
}

/// Expand JS replacement-pattern tokens against a single match: `$$`→`$`,
/// `$&`→match, `` $` ``→prefix, `$'`→suffix. `$n`/`$<name>` and a trailing `$`
/// pass through literally (string-arm replace has no captures).
#[cfg(test)]
fn apply_replacement(repl: &[u16], src: &[u16], start: usize, end: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(repl.len());
    let mut i = 0;
    while i < repl.len() {
        let ch = repl[i];
        if ch == 0x24 && i + 1 < repl.len() {
            match repl[i + 1] {
                0x24 => out.push(0x24),
                0x26 => out.extend_from_slice(&src[start..end]),
                0x60 => out.extend_from_slice(&src[..start]),
                0x27 => out.extend_from_slice(&src[end..]),
                _ => {
                    out.push(0x24);
                    i += 1;
                    continue;
                }
            }
            i += 2;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// Replace the first (or every, when `all`) literal occurrence of `search`,
/// applying JS replacement patterns.
#[cfg(test)]
fn replace_literal(input: &[u16], search: &[u16], repl: &[u16], all: bool) -> Vec<u16> {
    let matcher = super::string::search::Search::new(search, false);
    if !all {
        return match matcher.find(input, 0) {
            Some(idx) => {
                let end = idx + search.len();
                let mut out = input[..idx].to_vec();
                out.extend(apply_replacement(repl, input, idx, end));
                out.extend_from_slice(&input[end..]);
                out
            }
            None => input.to_vec(),
        };
    }

    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(idx) = matcher.find(input, pos) {
        out.extend_from_slice(&input[pos..idx]);
        out.extend(apply_replacement(repl, input, idx, idx + search.len()));
        if search.is_empty() {
            if idx < input.len() {
                out.push(input[idx]);
            }
            pos = idx + 1;
        } else {
            pos = idx + search.len();
        }
        if pos > input.len() {
            break;
        }
    }
    if pos <= input.len() {
        out.extend_from_slice(&input[pos..]);
    }
    out
}

/// JS `String#split` on a literal separator. `limit == 0` → empty; `limit < 0` →
/// uncapped; an empty separator yields individual code units.
#[cfg(test)]
fn split_literal(input: &[u16], sep: &[u16], limit: i32) -> Vec<Vec<u16>> {
    if limit == 0 {
        return Vec::new();
    }
    let cap = if limit < 0 {
        usize::MAX
    } else {
        limit as usize
    };

    if sep.is_empty() {
        if input.is_empty() {
            return Vec::new();
        }
        return input.iter().take(cap).map(|&u| vec![u]).collect();
    }

    let mut out = Vec::new();
    let mut start = 0;
    let matcher = super::string::search::Search::new(sep, false);
    while let Some(pos) = matcher.find(input, start) {
        if out.len() == cap {
            return out;
        }
        out.push(input[start..pos].to_vec());
        start = pos + sep.len();
    }
    if out.len() == cap {
        return out;
    }
    out.push(input[start..].to_vec());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn text(units: &[u16]) -> String {
        String::from_utf16(units).unwrap()
    }

    #[test]
    fn find_handles_empty_and_missing() {
        assert_eq!(find(&u16s("abc"), &u16s(""), 0), Some(0));
        assert_eq!(find(&u16s("abc"), &u16s(""), 3), Some(3));
        assert_eq!(find(&u16s("abc"), &u16s(""), 4), None);
        assert_eq!(find(&u16s("abc"), &u16s("b"), 0), Some(1));
        assert_eq!(find(&u16s("abc"), &u16s("z"), 0), None);
        assert_eq!(find(&u16s("ab"), &u16s("abc"), 0), None);
    }

    #[test]
    fn apply_replacement_expands_dollar_tokens() {
        // src = "hello world!", match "world" at [6,11]: prefix "hello ", suffix "!".
        let src = u16s("hello world!");
        let got = apply_replacement(&u16s("[$&|$`|$'|$$]"), &src, 6, 11);
        assert_eq!(text(&got), "[world|hello |!|$]");
    }

    #[test]
    fn apply_replacement_passes_through_unknown_and_trailing() {
        let src = u16s("abc");
        assert_eq!(text(&apply_replacement(&u16s("$1"), &src, 0, 1)), "$1");
        assert_eq!(text(&apply_replacement(&u16s("x$"), &src, 0, 1)), "x$");
    }

    #[test]
    fn replace_first_and_all_literal() {
        assert_eq!(
            text(&replace_literal(
                &u16s("a.b.c"),
                &u16s("."),
                &u16s("-"),
                false
            )),
            "a-b.c"
        );
        assert_eq!(
            text(&replace_literal(
                &u16s("a.b.c"),
                &u16s("."),
                &u16s("-"),
                true
            )),
            "a-b-c"
        );
    }

    #[test]
    fn replace_empty_search_inserts_around_units() {
        assert_eq!(
            text(&replace_literal(&u16s("abc"), &u16s(""), &u16s("-"), false)),
            "-abc"
        );
        assert_eq!(
            text(&replace_literal(&u16s("abc"), &u16s(""), &u16s("-"), true)),
            "-a-b-c-"
        );
    }

    #[test]
    fn replace_applies_pattern_to_match() {
        // Replace "b" with "[$&]" → wraps the matched text.
        assert_eq!(
            text(&replace_literal(
                &u16s("abc"),
                &u16s("b"),
                &u16s("[$&]"),
                false
            )),
            "a[b]c"
        );
    }

    #[test]
    fn split_literal_semantics() {
        let parts = |i: &str, s: &str, l: i32| {
            split_literal(&u16s(i), &u16s(s), l)
                .iter()
                .map(|p| text(p))
                .collect::<Vec<_>>()
        };
        assert_eq!(parts("a,b,c,d", ",", -1), vec!["a", "b", "c", "d"]);
        assert_eq!(parts("a,b,c,d", ",", 2), vec!["a", "b"]);
        assert_eq!(parts("abc", ",", -1), vec!["abc"]);
        assert_eq!(parts("", ",", -1), vec![""]);
        assert_eq!(parts("abc", "", -1), vec!["a", "b", "c"]);
        assert!(parts("", "", -1).is_empty());
        assert!(parts("a,b", ",", 0).is_empty());
    }
}
