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

pub(crate) mod engine;
mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use wasmtime::{ArrayRef, ArrayRefPre, Caller, ExternRef, Rooted, StructRef, StructRefPre, Val};

use crate::runtime::StoreData;
use crate::runtime::host::{
    host_regex_match_box_vtable, host_regex_vtable, host_string_vtable, read_string_arg,
    write_submilli_array_struct, write_submilli_string, write_submilli_string_struct,
    write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
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
    Ok(if charged.flags.has(FlagSet::Y) {
        raw.filter(|s| s.match_start == start)
    } else {
        raw
    })
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
    let charged = {
        let limits = &caller.data().tenant_limits;
        compile_charged(limits, &source_str, &flags_str).map_err(|e| {
            let msg = format!("RegExp: {e}");
            // An invalid pattern or flag is a spec `SyntaxError`; a tenant
            // memory-cap breach is a resource failure and stays a base `Error`.
            match e {
                engine::RegexCompileError::Syntax(_) => crate::runtime::host::syntax_error(msg),
                engine::RegexCompileError::Memory(_) => wasmtime::Error::msg(msg),
            }
        })?
    };
    let bits = i32::from(charged.flag_bits());
    let extern_ref = ExternRef::new(&mut *caller, charged)?;

    let regex_ty = build_intrinsic_types(caller.engine())?.regex;
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
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Build a fresh `$RegExpMatchBox` from a match snapshot over `input`.
fn build_match_box(
    caller: &mut Caller<'_, StoreData>,
    input: &str,
    snapshot: &ExecSnapshot,
) -> wasmtime::Result<Val> {
    let intr = build_intrinsic_types(caller.engine())?;

    let match_str =
        write_submilli_string_struct(caller, &input[snapshot.match_start..snapshot.match_end])?;
    let input_str = write_submilli_string_struct(caller, input)?;

    let mut numbered: Vec<Val> = Vec::with_capacity(snapshot.numbered.len());
    for slot in &snapshot.numbered {
        numbered.push(capture_slot(caller, input, *slot)?);
    }
    let numbered_arr = capture_array(caller, &intr, &numbered)?;

    let mut named: Vec<Val> = Vec::with_capacity(snapshot.named.len() * 2);
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
            Val::I32(snapshot.match_start as i32),
            Val::AnyRef(Some(input_str.to_anyref())),
            Val::AnyRef(Some(numbered_arr.to_anyref())),
            Val::AnyRef(Some(named_arr.to_anyref())),
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
            let raw = write_submilli_string(&mut *caller, &input[s..e])?;
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
    let pre = ArrayRefPre::new(&mut *caller, intr.regex_capture_array.clone());
    ArrayRef::new_fixed(&mut *caller, &pre, elements)
}

/// Wrap a raw `$rawString` payload (a capture-array element) in a `$string`
/// object, reusing the shared string vtable.
fn wrap_raw_string(
    caller: &mut Caller<'_, StoreData>,
    raw: wasmtime::Rooted<wasmtime::AnyRef>,
) -> wasmtime::Result<Val> {
    let string_ty = build_intrinsic_types(caller.engine())?.string;
    let vtable = host_string_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, string_ty);
    let st = StructRef::new(&mut *caller, &pre, &[vtable, Val::AnyRef(Some(raw))])?;
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
    let st = as_struct(caller, &params[0], "RegExp#test")?;
    let bits = flag_bits(caller, &st)?;
    let input = read_string_arg(&mut *caller, &params[1], "RegExp#test(input)")?;
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
    Ok(snapshot.is_some())
}

pub(super) fn exec(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    let st = as_struct(caller, &params[0], "RegExp#exec")?;
    let bits = flag_bits(caller, &st)?;
    let input = read_string_arg(&mut *caller, &params[1], "RegExp#exec(input)")?;
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
        Some(s) => build_match_box(caller, &input, &s),
        None => Ok(Val::null_any_ref()),
    }
}

/// A `(ref $string)` property getter (`source` = field 3, `flags` = field 4).
pub(super) fn string_field(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    field: usize,
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, &params[0], "RegExp property")?;
    st.field(&mut *caller, field)
}

pub(super) fn last_index_getter(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let st = as_struct(caller, &params[0], "RegExp#lastIndex")?;
    Ok(last_index(caller, &st)? as f64)
}

/// A boolean flag getter — `flag_mask & bitset != 0`.
pub(super) fn flag(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    mask: i32,
) -> wasmtime::Result<bool> {
    let st = as_struct(caller, &params[0], "RegExp flag")?;
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
    let st = as_struct(caller, &params[0], "RegExpMatch property")?;
    st.field(&mut *caller, field)
}

pub(super) fn match_index(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let st = as_struct(caller, &params[0], "RegExpMatch#index")?;
    match st.field(&mut *caller, 2)? {
        Val::I32(v) => Ok(f64::from(v)),
        other => wasmtime::bail!("RegExpMatch#index is {other:?}, not i32"),
    }
}

/// `RegExpMatch#groups` — a fresh `(string | null)[]` wrapping each numbered
/// capture.
pub(super) fn groups(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    let st = as_struct(caller, &params[0], "RegExpMatch#groups")?;
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
    let st = as_struct(caller, &params[0], "RegExpMatch#namedGroups")?;
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
    let string_ty = build_intrinsic_types(caller.engine())?.string;
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
    let input = read_string_arg(&mut *caller, &params[0], "String#match(input)")?;
    let st = as_struct(caller, &params[1], "String#match(regex)")?;
    match exec_at(caller, &st, &input, 0)? {
        Some(s) => build_match_box(caller, &input, &s),
        None => Ok(Val::null_any_ref()),
    }
}

/// `String#search(re)` — index of the first match, or `-1`.
pub(super) fn string_search(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<f64> {
    let input = read_string_arg(&mut *caller, &params[0], "String#search(input)")?;
    let st = as_struct(caller, &params[1], "String#search(regex)")?;
    Ok(match exec_at(caller, &st, &input, 0)? {
        Some(s) => s.match_start as f64,
        None => -1.0,
    })
}

/// `String#matchAll(re)` — every match as a `RegExpMatch[]`. A zero-length match
/// advances the scan by one (ECMA-262).
pub(super) fn string_match_all(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let input = read_string_arg(&mut *caller, &params[0], "String#matchAll(input)")?;
    let st = as_struct(caller, &params[1], "String#matchAll(regex)")?;
    let mut boxes: Vec<Val> = Vec::new();
    let mut pos = 0usize;
    while let Some(s) = exec_at(caller, &st, &input, pos)? {
        let next = if s.next_last_index == pos {
            pos + 1
        } else {
            s.next_last_index
        };
        boxes.push(build_match_box(caller, &input, &s)?);
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
    if arg_is_string(caller, &params[1])? {
        let input = read_units(caller, &params[0], "String#replace(input)")?;
        let search = read_units(caller, &params[1], "String#replace(search)")?;
        let repl = read_units(caller, &params[2], "String#replace(replacement)")?;
        let out = replace_literal(&input, &search, &repl, false);
        let st = write_submilli_string_struct_units(caller, &out)?;
        return Ok(Val::AnyRef(Some(st.to_anyref())));
    }
    let st = as_struct(caller, &params[1], "String#replace(regex)")?;
    let all = flag_bits(caller, &st)? & FlagSet::G.bits() as i32 != 0;
    let input = read_string_arg(&mut *caller, &params[0], "String#replace(input)")?;
    let repl = read_string_arg(&mut *caller, &params[2], "String#replace(replacement)")?;
    let out = with_regex(caller, &st, |c| {
        if all {
            c.regex.replace_all(&input, repl.as_str()).into_owned()
        } else {
            c.regex.replace(&input, repl.as_str()).into_owned()
        }
    })?;
    let result = write_submilli_string_struct(caller, &out)?;
    Ok(Val::AnyRef(Some(result.to_anyref())))
}

/// `String#replaceAll(search, replacement)` — always all occurrences (the regex
/// arm does not require `g`, a deliberate JS divergence kept from the Wasm body).
pub(super) fn string_replace_all(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    if arg_is_string(caller, &params[1])? {
        let input = read_units(caller, &params[0], "String#replaceAll(input)")?;
        let search = read_units(caller, &params[1], "String#replaceAll(search)")?;
        let repl = read_units(caller, &params[2], "String#replaceAll(replacement)")?;
        let out = replace_literal(&input, &search, &repl, true);
        let st = write_submilli_string_struct_units(caller, &out)?;
        return Ok(Val::AnyRef(Some(st.to_anyref())));
    }
    let st = as_struct(caller, &params[1], "String#replaceAll(regex)")?;
    let input = read_string_arg(&mut *caller, &params[0], "String#replaceAll(input)")?;
    let repl = read_string_arg(&mut *caller, &params[2], "String#replaceAll(replacement)")?;
    let out = with_regex(caller, &st, |c| {
        c.regex.replace_all(&input, repl.as_str()).into_owned()
    })?;
    let result = write_submilli_string_struct(caller, &out)?;
    Ok(Val::AnyRef(Some(result.to_anyref())))
}

/// `String#split(separator, limit)` — literal arm splits on code units; regex arm
/// splits via the crate. `limit` saturates to `i32`; negative means uncapped.
pub(super) fn string_split(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let limit = match params[2] {
        Val::F64(bits) => f64::from_bits(bits) as i32,
        ref other => {
            return Err(crate::runtime::host::type_error(format!(
                "String#split(limit) is {other:?}, not f64"
            )));
        }
    };
    let parts: Vec<Vec<u16>> = if arg_is_string(caller, &params[1])? {
        let input = read_units(caller, &params[0], "String#split(input)")?;
        let sep = read_units(caller, &params[1], "String#split(separator)")?;
        split_literal(&input, &sep, limit)
    } else {
        let st = as_struct(caller, &params[1], "String#split(regex)")?;
        let input = read_string_arg(&mut *caller, &params[0], "String#split(input)")?;
        with_regex(caller, &st, |c| regex_split(&c.regex, &input, limit))?
            .into_iter()
            .map(|s| s.encode_utf16().collect())
            .collect()
    };
    let mut elements = Vec::with_capacity(parts.len());
    for part in &parts {
        let st = write_submilli_string_struct_units(caller, part)?;
        elements.push(Val::AnyRef(Some(st.to_anyref())));
    }
    let arr = write_submilli_array_struct(caller, &elements)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

/// Read a `$string` receiver/arg into its UTF-16 code units (the literal string
/// arm works in code-unit space; the regex arm works on the UTF-8 decode).
fn read_units(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u16>> {
    let st = as_struct(caller, val, name)?;
    let raw = match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => wasmtime::bail!("{name}: malformed $string payload {other:?}"),
    };
    let len = raw.len(&mut *caller)?;
    let mut units = Vec::with_capacity(len as usize);
    for i in 0..len {
        match raw.get(&mut *caller, i)? {
            Val::I32(u) => units.push(u as u16),
            other => wasmtime::bail!("{name}: code unit {i} is {other:?}"),
        }
    }
    Ok(units)
}

// ---------------------------------------------------------------------------
// pure UTF-16 string algorithms (literal arm) — unit-tested without a store
// ---------------------------------------------------------------------------

/// First index at/after `from` where `needle` occurs in `hay`. An empty needle
/// matches at `from` (JS treats `""` as present everywhere).
fn find(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Expand JS replacement-pattern tokens against a single match: `$$`→`$`,
/// `$&`→match, `` $` ``→prefix, `$'`→suffix. `$n`/`$<name>` and a trailing `$`
/// pass through literally (string-arm replace has no captures).
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
fn replace_literal(input: &[u16], search: &[u16], repl: &[u16], all: bool) -> Vec<u16> {
    if !all {
        return match find(input, search, 0) {
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
    while let Some(idx) = find(input, search, pos) {
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
    let mut pos = 0;
    while pos + sep.len() <= input.len() {
        if &input[pos..pos + sep.len()] == sep {
            if out.len() == cap {
                return out;
            }
            out.push(input[start..pos].to_vec());
            pos += sep.len();
            start = pos;
        } else {
            pos += 1;
        }
    }
    if out.len() == cap {
        return out;
    }
    out.push(input[start..].to_vec());
    out
}

/// Regex-arm split, mirroring `submilli:regex.split`: JS-faithful strict
/// truncation (drop the tail past `limit`, not pack it into the last element).
fn regex_split(regex: &regex::Regex, input: &str, limit: i32) -> Vec<String> {
    if limit < 0 {
        regex.split(input).map(str::to_string).collect()
    } else if limit == 0 {
        Vec::new()
    } else {
        regex
            .split(input)
            .take(limit as usize)
            .map(str::to_string)
            .collect()
    }
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
