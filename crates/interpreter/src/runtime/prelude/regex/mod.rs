//! The Rust port of the prelude's hand-written Wasm `RegExp` / `RegExpMatch` /
//! `RegExpConstructor` glue (`codegen/prelude/regex.rs`).
//!
//! Matching itself stays in [`crate::runtime::regex`] (the `regex` crate); this
//! module re-homes the *object* surface: `$regex` construction, `test`/`exec`
//! with the JS-faithful `lastIndex` read/writeback, the property getters, the
//! `$RegExpMatchBox` accessors, and the `String` regex-arm methods. The two
//! object-identity vtables (`regex_vtable`, `regex_match_box_vtable`) stay
//! Wasm-built and are read from the prelude instance via [`prelude_global`]; a
//! host-built `$regex`/`$RegExpMatchBox` shares them, so it hashes and serializes
//! identically to a guest-built one.
//!
//! `$regex` (a `$Object` subtype): `[0] $VTable, [1] (ref extern) ChargedRegex,
//! [2] (mut i32) lastIndex, [3] (ref $string) source, [4] (ref $string) flags,
//! [5] i32 flag-bitset (`g=1,i=2,m=4,s=8,u=16,y=32`)`.
//!
//! `$RegExpMatchBox`: `[0] $VTable, [1] (ref $string) match, [2] i32 index,
//! [3] (ref $string) input, [4] $regexCaptureArray numbered, [5]
//! $regexCaptureArray named (alternating name/value, value null when unmatched)`.

use crate::runtime::host::abi_arg;
pub(crate) mod engine;
pub(crate) mod input;
mod install;
mod output;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use wasmtime::{ArrayRef, ArrayRefPre, Caller, ExternRef, Rooted, StructRef, StructRefPre, Val};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    host_regex_match_box_vtable, host_regex_vtable, host_string_vtable, read_string_arg,
    write_submilli_array_struct, write_submilli_string, write_submilli_string_struct,
    write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};
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
        Val::I32(v) => Ok(v.max(0) as usize),
        other => wasmtime::bail!("RegExp: lastIndex is {other:?}, not i32"),
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
    input: &str,
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
    Ok(if sticky {
        raw.filter(|s| s.match_start == start)
    } else {
        raw
    })
}

fn find_at(
    caller: &mut Caller<'_, StoreData>,
    regex: &Rooted<StructRef>,
    input: &str,
    start: usize,
) -> wasmtime::Result<Option<(usize, usize)>> {
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
    Ok(found.filter(|(index, _)| !sticky || *index == start))
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
        st.set_field(&mut *caller, 2, Val::I32(next.unwrap_or(0) as i32))?;
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
            Val::I32(0),
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
    input: &str,
    input_value: &Val,
    snapshot: &ExecSnapshot,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;

    let matched = input
        .get(snapshot.match_start..snapshot.match_end)
        .ok_or_else(|| wasmtime::Error::msg("RegExp: invalid match span"))?;
    let match_str = write_submilli_string_struct(caller, matched)?;

    let mut numbered: Vec<Val> = Vec::new();
    numbered
        .try_reserve_exact(snapshot.numbered.len())
        .map_err(crate::runtime::host::fatal_host_error)?;
    for slot in &snapshot.numbered {
        numbered.push(capture_slot(caller, input, *slot)?);
    }
    let numbered_arr = capture_array(caller, &intr, &numbered)?;

    let mut named: Vec<Val> = Vec::new();
    let named_slots =
        snapshot.named.len().checked_mul(2).ok_or_else(|| {
            crate::runtime::host::fatal_host_error("RegExp: capture count overflow")
        })?;
    named
        .try_reserve_exact(named_slots)
        .map_err(crate::runtime::host::fatal_host_error)?;
    for (name, slot) in &snapshot.named {
        let raw = write_submilli_string(&mut *caller, name)?;
        named.push(Val::AnyRef(Some(raw.to_anyref())));
        named.push(capture_slot(caller, input, *slot)?);
    }
    let named_arr = capture_array(caller, &intr, &named)?;

    let vtable = host_regex_match_box_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, intr.regex_match_box.clone());
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(match_str.to_anyref())),
            Val::I32(
                i32::try_from(snapshot.match_start)
                    .map_err(|_| wasmtime::Error::msg("RegExp: match index exceeds i32"))?,
            ),
            *input_value,
            Val::AnyRef(Some(numbered_arr.to_anyref())),
            Val::AnyRef(Some(named_arr.to_anyref())),
            Val::I64(0),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// One capture-array slot: a raw `$rawString` for a participating group, `null`
/// for a group that didn't match.
fn capture_slot(
    caller: &mut Caller<'_, StoreData>,
    input: &str,
    slot: Option<(usize, usize)>,
) -> wasmtime::Result<Val> {
    match slot {
        Some((s, e)) => {
            let capture = input
                .get(s..e)
                .ok_or_else(|| wasmtime::Error::msg("RegExp: invalid capture span"))?;
            let raw = write_submilli_string(&mut *caller, capture)?;
            Ok(Val::AnyRef(Some(raw.to_anyref())))
        }
        None => Ok(Val::null_any_ref()),
    }
}

fn capture_array(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    elements: &[Val],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    fuel::charge(&mut *caller, fuel::ELEM, elements.len() as u64)?;
    let pre = ArrayRefPre::new(&mut *caller, intr.regex_capture_array.clone());
    ArrayRef::new_fixed(&mut *caller, &pre, elements)
}

/// Wrap a raw `$rawString` payload (a capture-array element) in a `$string`
/// object, reusing the shared string vtable.
fn wrap_raw_string(
    caller: &mut Caller<'_, StoreData>,
    raw: wasmtime::Rooted<wasmtime::AnyRef>,
) -> wasmtime::Result<Val> {
    let string_ty = intrinsic_types(&mut *caller)?.string.clone();
    let vtable = host_string_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, string_ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::AnyRef(Some(raw)), Val::I64(0)],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Read a `$RegExpMatchBox` capture-array field into its elements.
fn read_capture_array(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    field: usize,
) -> wasmtime::Result<Vec<Val>> {
    let arr = match st.field(&mut *caller, field)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => wasmtime::bail!("RegExpMatch: capture array is {other:?}"),
    };
    let len = arr.len(&mut *caller)?;
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(arr.get(&mut *caller, i)?);
    }
    Ok(out)
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
        snapshot.as_ref().map(|s| s.next_last_index),
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
    Ok(last_index(caller, &st)? as f64)
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

/// A direct `$RegExpMatchBox` field (`match` = 1, `input` = 3).
pub(super) fn match_field(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    field: usize,
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExpMatch property")?;
    st.field(&mut *caller, field)
}

pub(super) fn match_index(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExpMatch#index")?;
    match st.field(&mut *caller, 2)? {
        Val::I32(v) => Ok(f64::from(v)),
        other => wasmtime::bail!("RegExpMatch#index is {other:?}, not i32"),
    }
}

/// `RegExpMatch#groups` — a fresh `(string | null)[]` wrapping each numbered
/// capture.
pub(super) fn groups(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExpMatch#groups")?;
    let raw = read_capture_array(caller, &st, 4)?;
    let mut elements = Vec::with_capacity(raw.len());
    for elem in raw {
        elements.push(match elem {
            Val::AnyRef(Some(any)) => wrap_raw_string(caller, any)?,
            _ => Val::null_any_ref(),
        });
    }
    let arr = write_submilli_array_struct(caller, &elements)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

/// `RegExpMatch#namedGroups` — a `Map<string, string>` from the alternating
/// name/value capture array. Unmatched named captures (null value) are skipped
/// (the fix the Wasm body deferred).
pub(super) async fn named_groups(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, abi_arg(params, 0)?, "RegExpMatch#namedGroups")?;
    let raw = read_capture_array(caller, &st, 5)?;
    let map = super::map::construct(caller, &Val::null_any_ref()).await?;
    let mut i = 0;
    while i + 1 < raw.len() {
        if let (Val::AnyRef(Some(name)), Val::AnyRef(Some(value))) = (raw[i], raw[i + 1]) {
            let key = wrap_raw_string(caller, name)?;
            let val = wrap_raw_string(caller, value)?;
            super::map::set(caller, &map, &key, &val).await?;
        }
        i += 2;
    }
    Ok(map)
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

/// `String#match(re)` — no `g` semantics; equivalent to `re.exec(self)` with
/// `lastIndex` pinned at 0.
pub(super) fn string_match(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let params: &[Val; 2] = params
        .try_into()
        .map_err(|_| wasmtime::Error::msg("string_match: invalid argument count"))?;
    let input = read_string_arg(&mut *caller, abi_arg(params, 0)?, "String#match(input)")?;
    let st = as_struct(caller, abi_arg(params, 1)?, "String#match(regex)")?;
    match exec_at(caller, &st, &input, 0)? {
        Some(s) => build_match_box(caller, &input, abi_arg(params, 0)?, &s),
        None => Ok(Val::null_any_ref()),
    }
}

/// `String#search(re)` — index of the first match, or `-1`.
pub(super) fn string_search(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let input = input::read(caller, abi_arg(params, 0)?)?;
    let st = as_struct(caller, abi_arg(params, 1)?, "String#search(regex)")?;
    Ok(match find_at(caller, &st, &input, 0)? {
        Some((start, _)) => start as f64,
        None => -1.0,
    })
}

/// `String#matchAll(re)` — every match as a `RegExpMatch[]`. A zero-length match
/// advances the scan by one (ECMA-262).
pub(super) fn string_match_all(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let params: &[Val; 2] = params
        .try_into()
        .map_err(|_| wasmtime::Error::msg("string_match_all: invalid argument count"))?;
    let input = read_string_arg(&mut *caller, abi_arg(params, 0)?, "String#matchAll(input)")?;
    let st = as_struct(caller, abi_arg(params, 1)?, "String#matchAll(regex)")?;
    let mut boxes: Vec<Val> = Vec::new();
    let mut pos = 0usize;
    while let Some(s) = exec_at(caller, &st, &input, pos)? {
        let next = if s.next_last_index == pos {
            pos.checked_add(1)
                .ok_or_else(|| wasmtime::Error::msg("RegExp: match position overflow"))?
        } else {
            s.next_last_index
        };
        boxes.push(build_match_box(caller, &input, abi_arg(params, 0)?, &s)?);
        pos = next;
    }
    let arr = write_submilli_array_struct(caller, &boxes)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
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
    let _inputs = reserve_string_inputs(caller, &[*abi_arg(params, 0)?, *abi_arg(params, 2)?])?;
    let input = read_string_arg(&mut *caller, abi_arg(params, 0)?, "String#replace(input)")?;
    let repl = read_string_arg(
        &mut *caller,
        abi_arg(params, 2)?,
        "String#replace(replacement)",
    )?;
    fuel::charge(&mut *caller, fuel::REGEX, input.len() as u64)?;
    let regex = with_regex(caller, &st, |compiled| compiled.regex.clone())?;
    let out = replace_regex_bounded(caller, &regex, &input, &repl, all)?;
    let result = write_submilli_string_struct_units(caller, out.values())?;
    Ok(Val::AnyRef(Some(result.to_anyref())))
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
    let _inputs = reserve_string_inputs(caller, &[*abi_arg(params, 0)?, *abi_arg(params, 2)?])?;
    let input = read_string_arg(
        &mut *caller,
        abi_arg(params, 0)?,
        "String#replaceAll(input)",
    )?;
    let repl = read_string_arg(
        &mut *caller,
        abi_arg(params, 2)?,
        "String#replaceAll(replacement)",
    )?;
    fuel::charge(&mut *caller, fuel::REGEX, input.len() as u64)?;
    let regex = with_regex(caller, &st, |compiled| compiled.regex.clone())?;
    let out = replace_regex_bounded(caller, &regex, &input, &repl, true)?;
    let result = write_submilli_string_struct_units(caller, out.values())?;
    Ok(Val::AnyRef(Some(result.to_anyref())))
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
        for part in regex.split(&input).take(cap) {
            let part = write_submilli_string_struct(caller, part)?;
            elements.push(caller, Val::AnyRef(Some(part.to_anyref())))?;
        }
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

fn replace_regex_bounded(
    caller: &mut Caller<'_, StoreData>,
    regex: &regex::Regex,
    input: &str,
    replacement: &str,
    all: bool,
) -> wasmtime::Result<output::Buffer<u16>> {
    let mut output = output::Buffer::new();
    let mut previous = 0;
    let capture_slots = regex.captures_len() as u64;
    let _captures = crate::runtime::limits::HostBytes::new(
        &caller.data().tenant_limits,
        capture_slots.saturating_mul(128),
    )?;
    fuel::charge(&mut *caller, fuel::ELEM, capture_slots)?;
    for captures in regex.captures_iter(input) {
        let found = captures.get(0).ok_or_else(|| {
            crate::runtime::host::fatal_host_error("regex replacement omitted whole match")
        })?;
        let prefix = input.get(previous..found.start()).ok_or_else(|| {
            crate::runtime::host::fatal_host_error("invalid regex replacement prefix")
        })?;
        output.append_text(caller, prefix)?;
        fuel::charge(&mut *caller, fuel::SCAN, replacement.len() as u64)?;
        expand_regex(caller, &mut output, replacement, &captures)?;
        previous = found.end();
        if !all {
            break;
        }
        fuel::charge(&mut *caller, fuel::ELEM, capture_slots)?;
    }
    let suffix = input.get(previous..).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("invalid regex replacement suffix")
    })?;
    output.append_text(caller, suffix)?;
    Ok(output)
}

fn expand_regex(
    caller: &mut Caller<'_, StoreData>,
    output: &mut output::Buffer<u16>,
    mut replacement: &str,
    captures: &regex::Captures<'_>,
) -> wasmtime::Result<()> {
    let mut unclosed_brace = false;
    while let Some(dollar) = replacement.find('$') {
        let prefix = replacement
            .get(..dollar)
            .ok_or_else(|| crate::runtime::host::fatal_host_error("invalid replacement token"))?;
        output.append_text(caller, prefix)?;
        replacement = replacement
            .get(dollar..)
            .ok_or_else(|| crate::runtime::host::fatal_host_error("invalid replacement token"))?;
        if let Some(rest) = replacement.strip_prefix("$$") {
            output.append_text(caller, "$")?;
            replacement = rest;
            continue;
        }
        let reference = if unclosed_brace && replacement.starts_with("${") {
            None
        } else {
            capture_reference(replacement)
        };
        let Some((reference, end)) = reference else {
            if replacement.starts_with("${") {
                unclosed_brace = true;
            }
            output.append_text(caller, "$")?;
            replacement = replacement.get(1..).ok_or_else(|| {
                crate::runtime::host::fatal_host_error("invalid replacement token")
            })?;
            continue;
        };
        let capture = match reference.parse::<usize>() {
            Ok(index) => captures.get(index),
            Err(_) => captures.name(reference),
        };
        if let Some(capture) = capture {
            output.append_text(caller, capture.as_str())?;
        }
        replacement = replacement
            .get(end..)
            .ok_or_else(|| crate::runtime::host::fatal_host_error("invalid capture reference"))?;
    }
    output.append_text(caller, replacement)
}

/// Keep the regex crate's existing replacement syntax, including missing
/// captures becoming empty and greedily parsed unbraced names.
fn capture_reference(replacement: &str) -> Option<(&str, usize)> {
    let after_dollar = replacement.strip_prefix('$')?;
    if let Some(braced) = after_dollar.strip_prefix('{') {
        let end = braced.find('}')?;
        return Some((braced.get(..end)?, end + 3));
    }
    let length = after_dollar
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    if length == 0 {
        return None;
    }
    Some((after_dollar.get(..length)?, length + 1))
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
    let mut position = 0;
    while let Some(unit) = replacement.get(position) {
        let token = if *unit == b'$' as u16 {
            replacement.get(position + 1).copied()
        } else {
            None
        };
        let expanded = match token {
            Some(0x24) => Some(&[0x24_u16][..]),
            Some(0x26) => Some(checked_units(input, start, end)?),
            Some(0x60) => Some(checked_units(input, 0, start)?),
            Some(0x27) => Some(checked_units(input, end, input.len())?),
            _ => None,
        };
        if let Some(expanded) = expanded {
            output.append(caller, expanded)?;
            position += 2;
        } else {
            // Copy a whole literal run, retaining per-fragment COPY rounding.
            let literal_start = position;
            position += 1;
            while replacement
                .get(position)
                .is_some_and(|unit| *unit != b'$' as u16)
            {
                position += 1;
            }
            output.append(caller, checked_units(replacement, literal_start, position)?)?;
        }
    }
    Ok(())
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
