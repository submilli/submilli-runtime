//! Host-side iterator construction and the shared index-cursor adapter.
//!
//! An iterator is an ordinary `$ObjectShape` whose `next` field holds a
//! `$closure_0_value` (`{ vtable, funcref, env }`); `for…of`'s structural
//! dispatch finds that closure by name and `call_ref`s it. Here the funcref is a
//! *host* `Func`, so the whole protocol — iterator object, `next` step, and the
//! `{ done, value }` results — is produced in Rust.
//!
//! [`iter_yield`] / [`iter_done`] build the `IteratorYieldResult` /
//! `IteratorReturnResult` objects directly, carrying the generic host
//! `object_vtable` (identity slots: `toString` → "[object Object]", `toJson`
//! → "{}", reference equality, hash 0).
//!
//! [`make_index_iterator`] is the reusable adapter for index-addressable
//! collections (`keys` / `values` / `entries`): the collection supplies a `step`
//! that reads the key/value at a position, plus a `payload` carrying its state
//! (a live ref it re-reads each step, or a snapshot captured up front).

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, FieldType, Finality, Func, FuncType, Global, GlobalType,
    HeapType, Mutability, RefType, Rooted, StorageType, StructRef, StructRefPre, StructType, Val,
    ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fuel::host_func;
use crate::runtime::gc_singleton::{singleton_func, singleton_struct};
use crate::runtime::host::{
    host_boxed_boolean_vtable, host_closure_vtable, host_object_vtable,
    write_submilli_array_struct, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};

/// The `$closure_0_value` func + struct types: `next: () => IteratorResult<T>`
/// under the closure ABI — funcref `(ref any) -> (ref null $object)`, struct a
/// `$closure` subtype `{ vtable, funcref, env }`. Declared to canonicalize with
/// `codegen::closures`, so the guest's `ref.cast` to its own `$closure_0_value`
/// during structural dispatch succeeds. The [`next_closure_matches_codegen`] test
/// pins the agreement; the `array_values` fixture is the runtime backstop.
pub(crate) fn next_closure_type(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<(FuncType, StructType)> {
    let imm = Mutability::Const;
    let func = singleton_func(
        engine,
        vec![ValType::Ref(RefType::new(false, HeapType::Any))],
        vec![ValType::Ref(RefType::new(true, intr.object.clone().into()))],
    )?;
    let st = singleton_struct(
        engine,
        Finality::NonFinal,
        Some(intr.closure.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.vtable.clone().into(),
                ))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, func.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, HeapType::Any))),
            ),
            FieldType::new(Mutability::Var, StorageType::ValType(ValType::I64)),
        ],
    )?;
    Ok((func, st))
}

/// The `$closure_0_void` func + struct types: `close: () => void` under the
/// closure ABI — funcref `(ref any) -> ()`, struct a `$closure` subtype
/// `{ vtable, funcref, env }`. Declared to canonicalize with
/// `codegen::closures`, so the guest's `ref.cast` when invoking an iterator's
/// `close` field succeeds. [`void_closure_matches_codegen`] pins the agreement.
pub(crate) fn void_closure_type(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<(FuncType, StructType)> {
    let imm = Mutability::Const;
    let func = singleton_func(
        engine,
        vec![ValType::Ref(RefType::new(false, HeapType::Any))],
        Vec::new(),
    )?;
    let st = singleton_struct(
        engine,
        Finality::NonFinal,
        Some(intr.closure.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.vtable.clone().into(),
                ))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, func.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, HeapType::Any))),
            ),
            FieldType::new(Mutability::Var, StorageType::ValType(ValType::I64)),
        ],
    )?;
    Ok((func, st))
}

/// The `IteratorYieldResult` / `IteratorReturnResult` struct type: a non-final
/// `$ObjectShape` subtype `{ vtable, field_names, object_fields }`. Codegen
/// declares the arity-1 (`done`) and arity-2 (`done`, `value`) shapes separately,
/// but their layouts coincide and WasmGC canonicalization unifies them — and with
/// this host handle — so a single type backs both results and matches the
/// `ref.cast` `for…of` does on `next()`'s result. [`iterator_result_matches_codegen`]
/// pins the agreement.
pub(crate) fn iterator_result_struct(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<StructType> {
    let imm = Mutability::Const;
    singleton_struct(
        engine,
        Finality::NonFinal,
        Some(intr.object_shape.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.vtable.clone().into(),
                ))),
            ),
            FieldType::new(
                Mutability::Var,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.field_names.clone().into(),
                ))),
            ),
            FieldType::new(
                Mutability::Var,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.object_fields.clone().into(),
                ))),
            ),
            FieldType::new(
                Mutability::Var,
                StorageType::ValType(ValType::Ref(RefType::ANYREF)),
            ),
        ],
    )
}

/// Build `{ done: false, value }` (an `IteratorYieldResult`).
pub(crate) fn iter_yield(caller: &mut Caller<'_, StoreData>, value: Val) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let vtable = host_object_vtable(caller)?;
    let done = box_boolean(caller, false)?;
    let names = field_names_array(caller, &intr, &["done", "value"])?;
    let fields = object_fields_array(caller, &intr, &[done, value])?;
    build_result(caller, &intr, vtable, names, fields)
}

/// Build `{ done: true }` (an `IteratorReturnResult`).
pub(crate) fn iter_done(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let vtable = host_object_vtable(caller)?;
    let done = box_boolean(caller, true)?;
    let names = field_names_array(caller, &intr, &["done"])?;
    let fields = object_fields_array(caller, &intr, &[done])?;
    build_result(caller, &intr, vtable, names, fields)
}

/// Box a `bool` into a `$boxed_boolean` object (the `done` field's value).
fn box_boolean(caller: &mut Caller<'_, StoreData>, b: bool) -> wasmtime::Result<Val> {
    let index = if b { 5 } else { 4 };
    if let Some(root) = caller
        .data()
        .iterator_constants
        .get(index)
        .copied()
        .flatten()
    {
        return Ok(root.get(&mut *caller));
    }
    let boxed = intrinsic_types(&mut *caller)?.boxed_boolean.clone();
    let vtable = host_boxed_boolean_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, boxed);
    let st = StructRef::new(&mut *caller, &pre, &[vtable, Val::I32(b as i32)])?;
    retain_constant(caller, index, Val::AnyRef(Some(st.to_anyref())))
}

fn field_names_array(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    names: &[&str],
) -> wasmtime::Result<Val> {
    let mut values = [Val::null_any_ref(); 2];
    let vals = values
        .get_mut(..names.len())
        .ok_or_else(|| crate::runtime::host::fatal_host_error("too many iterator field names"))?;
    for (slot, name) in vals.iter_mut().zip(names) {
        *slot = iterator_name(caller, name)?;
    }
    let pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let arr = ArrayRef::new_fixed(&mut *caller, &pre, vals)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

fn object_fields_array(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    fields: &[Val],
) -> wasmtime::Result<Val> {
    let pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let arr = ArrayRef::new_fixed(&mut *caller, &pre, fields)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

fn build_result(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    vtable: Val,
    names: Val,
    fields: Val,
) -> wasmtime::Result<Val> {
    let ty = iterator_result_struct(caller.engine(), intr)?;
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, names, fields, Val::AnyRef(None)],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Assemble the iterator object: `$ObjectShape { object_vtable, ["next"], [next_closure] }`
/// where `next_closure = $closure_0_value { closure_vtable, next_fn, env }`.
///
/// The wrapper carries the host-owned generic `object_vtable`; the inner `next`
/// closure carries `closure_vtable`. `for…of` only reads the `next`/`done`/`value`
/// fields, so the wrapper's identity slots are never invoked.
pub(crate) fn build_iterator(
    caller: &mut Caller<'_, StoreData>,
    next_struct_ty: StructType,
    next_fn: Func,
    env: Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let closure_vtable = host_closure_vtable(caller)?;
    let object_vtable = host_object_vtable(caller)?;

    let closure_pre = StructRefPre::new(&mut *caller, next_struct_ty);
    let closure = StructRef::new(
        &mut *caller,
        &closure_pre,
        &[
            closure_vtable,
            Val::FuncRef(Some(next_fn)),
            env,
            Val::I64(0),
        ],
    )?;

    let next_name = iterator_name(caller, "next")?;
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &[next_name])?;

    let fields_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let fields = ArrayRef::new_fixed(
        &mut *caller,
        &fields_pre,
        &[Val::AnyRef(Some(closure.to_anyref()))],
    )?;

    // The refined `$ObjectShape` subtype (non-null field arrays), not the base:
    // stdlib-shim cursor consumers `ref.cast` the iterator object to their
    // canonically-equal local subtype before extracting `next`.
    let obj_ty = iterator_result_struct(caller.engine(), &intr)?;
    let obj_pre = StructRefPre::new(&mut *caller, obj_ty);
    let obj = StructRef::new(
        &mut *caller,
        &obj_pre,
        &[
            object_vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(fields.to_anyref())),
            Val::AnyRef(None),
        ],
    )?;
    Ok(Val::AnyRef(Some(obj.to_anyref())))
}

/// Assemble a *closable* iterator:
/// `$ObjectShape { object_vtable, ["close", "next"], [close_closure, next_closure] }`.
///
/// The variant host-backed streams need — `for…of` invokes `close` on every
/// loop exit (normal exhaustion, `break`, or throw) so the OS handle in `env`
/// is released eagerly rather than at GC time. Both closures share `env`.
pub(crate) fn build_closable_iterator(
    caller: &mut Caller<'_, StoreData>,
    next_fn: Func,
    close_fn: Func,
    env: Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let closure_vtable = host_closure_vtable(caller)?;
    let object_vtable = host_object_vtable(caller)?;

    let (_, next_struct_ty) = next_closure_type(caller.engine(), &intr)?;
    let next_pre = StructRefPre::new(&mut *caller, next_struct_ty);
    let next_closure = StructRef::new(
        &mut *caller,
        &next_pre,
        &[
            closure_vtable,
            Val::FuncRef(Some(next_fn)),
            env,
            Val::I64(0),
        ],
    )?;

    let (_, close_struct_ty) = void_closure_type(caller.engine(), &intr)?;
    let close_pre = StructRefPre::new(&mut *caller, close_struct_ty);
    let close_closure = StructRef::new(
        &mut *caller,
        &close_pre,
        &[
            closure_vtable,
            Val::FuncRef(Some(close_fn)),
            env,
            Val::I64(0),
        ],
    )?;

    let close_name = iterator_name(caller, "close")?;
    let next_name = iterator_name(caller, "next")?;
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &[close_name, next_name])?;

    let fields_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let fields = ArrayRef::new_fixed(
        &mut *caller,
        &fields_pre,
        &[
            Val::AnyRef(Some(close_closure.to_anyref())),
            Val::AnyRef(Some(next_closure.to_anyref())),
        ],
    )?;

    let obj_ty = iterator_result_struct(caller.engine(), &intr)?;
    let obj_pre = StructRefPre::new(&mut *caller, obj_ty);
    let obj = StructRef::new(
        &mut *caller,
        &obj_pre,
        &[
            object_vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(fields.to_anyref())),
            Val::AnyRef(None),
        ],
    )?;
    Ok(Val::AnyRef(Some(obj.to_anyref())))
}

fn iterator_name(caller: &mut Caller<'_, StoreData>, name: &str) -> wasmtime::Result<Val> {
    let index = match name {
        "done" => 0,
        "value" => 1,
        "next" => 2,
        "close" => 3,
        _ => {
            return Err(crate::runtime::host::fatal_host_error(
                "unknown iterator field name",
            ));
        }
    };
    if let Some(root) = caller
        .data()
        .iterator_constants
        .get(index)
        .copied()
        .flatten()
    {
        return Ok(root.get(&mut *caller));
    }
    let name = write_submilli_string_struct(caller, name)?;
    retain_constant(caller, index, Val::AnyRef(Some(name.to_anyref())))
}

fn retain_constant(
    caller: &mut Caller<'_, StoreData>,
    index: usize,
    value: Val,
) -> wasmtime::Result<Val> {
    let ty = GlobalType::new(
        ValType::Ref(RefType::new(true, HeapType::Any)),
        Mutability::Const,
    );
    let root = Global::new(&mut *caller, ty, value)?;
    let slot = caller
        .data_mut()
        .iterator_constants
        .get_mut(index)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("invalid iterator constant index"))?;
    *slot = Some(root);
    Ok(value)
}

pub(crate) enum IteratorSource {
    Array,
    Map,
    Set,
    String,
}

pub(crate) fn shared_next(
    caller: &mut Caller<'_, StoreData>,
    source: IteratorSource,
    kind: IterKind,
    ty: FuncType,
    implementation: impl Fn(&mut Caller<'_, StoreData>, &[Val], &mut [Val]) -> wasmtime::Result<()>
    + Send
    + Sync
    + 'static,
) -> wasmtime::Result<Func> {
    let offset = match source {
        IteratorSource::Array => 0,
        IteratorSource::Map => 3,
        IteratorSource::Set => 6,
        IteratorSource::String => 9,
    };
    let kind = match kind {
        IterKind::Keys => 0,
        IterKind::Values => 1,
        IterKind::Entries => 2,
    };
    let index = if offset == 9 { 9 } else { offset + kind };
    if let Some(function) = caller
        .data()
        .iterator_functions
        .get(index)
        .copied()
        .flatten()
    {
        return Ok(function);
    }
    let function = host_func(&mut *caller, ty, implementation);
    let slot = caller
        .data_mut()
        .iterator_functions
        .get_mut(index)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("invalid iterator function index"))?;
    *slot = Some(function);
    Ok(function)
}

// ---------------------------------------------------------------------------
// Shared index-cursor adapter (keys / values / entries)
// ---------------------------------------------------------------------------

/// What an index-addressable iterator yields per position.
#[derive(Clone, Copy)]
pub(crate) enum IterKind {
    Keys,
    Values,
    Entries,
}

/// Reads the `(key, value)` at `pos` from the iterator's `payload`, or `None`
/// once exhausted. Pure reads only — no guest re-entry — so it's synchronous.
/// `payload` may be a *live* collection ref (re-read each call, tracking
/// mutation) or a *snapshot* captured at iterator-construction time.
type IndexStep = fn(&mut Caller<'_, StoreData>, &Val, i32) -> wasmtime::Result<Option<(Val, Val)>>;

/// Build a `keys`/`values`/`entries` iterator over an index-addressable
/// collection. `payload` carries the collection state through the cursor's
/// `(ref any)` slot; `step` reads position `i`. The driver applies the
/// [`IterKind`] projection (`Entries` boxes the pair into a 2-element `$Array`),
/// advances the position, and wraps the result with [`iter_yield`]/[`iter_done`].
pub(crate) fn make_index_iterator(
    caller: &mut Caller<'_, StoreData>,
    payload: Val,
    kind: IterKind,
    step: IndexStep,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let cursor = make_cursor(caller, payload)?;
    let (next_ty, next_struct) = next_closure_type(caller.engine(), &intr)?;
    let next = shared_next(
        caller,
        IteratorSource::Array,
        kind,
        next_ty,
        move |caller, params, results| index_step(caller, params, results, kind, step),
    )?;
    build_iterator(caller, next_struct, next, cursor)
}

/// Build the `String#iterator` cursor: yields one `$string` per CODE POINT — a
/// surrogate pair arrives as one two-unit string, a lone surrogate as itself.
/// The payload is the receiver `$string`; each step reads its backing directly.
pub(crate) fn make_string_iterator(
    caller: &mut Caller<'_, StoreData>,
    string: Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let cursor = make_cursor(caller, string)?;
    let (next_ty, next_struct) = next_closure_type(caller.engine(), &intr)?;
    let next = shared_next(
        caller,
        IteratorSource::String,
        IterKind::Values,
        next_ty,
        string_step,
    )?;
    build_iterator(caller, next_struct, next, cursor)
}

/// The `String#iterator` step: read the code point at the cursor position from
/// the payload `$string`'s backing, yield it as a fresh 1–2 unit `$string`, and
/// advance by however many units it spanned.
fn string_step(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let cursor = as_struct(caller, abi_arg(params, 0)?, "string iterator env")?;
    let Val::I32(pos) = cursor.field(&mut *caller, 0)? else {
        return Err(wasmtime::Error::msg(
            "string iterator: position is not an i32",
        ));
    };
    let payload = cursor.field(&mut *caller, 1)?;
    let st = as_struct(caller, &payload, "string iterator payload")?;
    let backing = match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => wasmtime::bail!("string iterator: malformed payload {other:?}"),
    };
    let len = backing.len(&mut *caller)? as i32;
    if pos >= len {
        *abi_result(results, 0)? = iter_done(caller)?;
        return Ok(());
    }
    let unit_at = |caller: &mut Caller<'_, StoreData>, i: i32| -> wasmtime::Result<u16> {
        match backing.get(&mut *caller, i as u32)? {
            Val::I32(unit) => Ok(unit as u16),
            other => wasmtime::bail!("string iterator: code unit {i} is {other:?}"),
        }
    };
    let lead = unit_at(caller, pos)?;
    let mut units = vec![lead];
    if (0xD800..=0xDBFF).contains(&lead) && pos + 1 < len {
        let trail = unit_at(caller, pos + 1)?;
        if (0xDC00..=0xDFFF).contains(&trail) {
            units.push(trail);
        }
    }
    let advance = units.len() as i32;
    let st = crate::runtime::host::write_submilli_string_struct_units(caller, &units)?;
    cursor.set_field(&mut *caller, 0, Val::I32(pos + advance))?;
    *abi_result(results, 0)? = iter_yield(caller, Val::AnyRef(Some(st.to_anyref())))?;
    Ok(())
}

/// `(struct (mut i32) (ref null any))` — the host-private cursor: a mutable
/// position and the collection payload. Only `make_index_iterator`/`index_step`
/// read it, so its shape need not match any codegen type; it flows through the
/// `next` closure's `(ref any)` env slot.
fn make_cursor(caller: &mut Caller<'_, StoreData>, payload: Val) -> wasmtime::Result<Val> {
    let cursor_ty = singleton_struct(
        caller.engine(),
        Finality::Final,
        None,
        vec![
            FieldType::new(Mutability::Var, StorageType::ValType(ValType::I32)),
            FieldType::new(
                Mutability::Const,
                StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any))),
            ),
        ],
    )?;
    let pre = StructRefPre::new(&mut *caller, cursor_ty);
    let st = StructRef::new(&mut *caller, &pre, &[Val::I32(0), payload])?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// The `next` step under the closure ABI: `*abi_arg(params, 0)?` is the cursor. Reads the
/// pair at the current position via `step`, projects it for `kind`, advances,
/// and yields — or returns `{done:true}` at the end.
fn index_step(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
    kind: IterKind,
    step: IndexStep,
) -> wasmtime::Result<()> {
    let cursor = as_struct(caller, abi_arg(params, 0)?, "index iterator env")?;
    let Val::I32(pos) = cursor.field(&mut *caller, 0)? else {
        return Err(wasmtime::Error::msg(
            "index iterator: position is not an i32",
        ));
    };
    let payload = cursor.field(&mut *caller, 1)?;
    let Some((key, value)) = step(caller, &payload, pos)? else {
        *abi_result(results, 0)? = iter_done(caller)?;
        return Ok(());
    };
    let yielded = match kind {
        IterKind::Keys => key,
        IterKind::Values => value,
        IterKind::Entries => {
            let pair = write_submilli_array_struct(caller, &[key, value])?;
            Val::AnyRef(Some(pair.to_anyref()))
        }
    };
    cursor.set_field(&mut *caller, 0, Val::I32(pos + 1))?;
    *abi_result(results, 0)? = iter_yield(caller, yielded)?;
    Ok(())
}

/// Unwrap a `Val::AnyRef` to a `Rooted<StructRef>`, tagging errors with `name`.
pub(crate) fn as_struct(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    let Val::AnyRef(Some(any)) = val else {
        return Err(wasmtime::Error::msg(format!(
            "{name}: expected a struct ref"
        )));
    };
    any.as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg(format!("{name}: not a struct")))
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{Dispatch, MethodSig, PropertySig, Span, Type, TypeKind, TypeSymbol};
    use std::collections::BTreeMap;
    let yield_result_body = || -> Type {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "done".to_string(),
            crate::ObjectField::required(Type::Boolean),
        );
        fields.insert(
            "value".to_string(),
            crate::ObjectField::required(Type::TypeVar("T".to_string())),
        );
        Type::Object {
            index: None,
            fields,
        }
    };
    defs.types.insert(
        "IteratorYieldResult".to_string(),
        TypeSymbol {
            name: "IteratorYieldResult".to_string(),
            mangled_name: crate::mangle::prelude("IteratorYieldResult"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Alias {
                generics: vec!["T".to_string()],
                ty: yield_result_body(),
                doc: doc(
                    "/** A single yielded element from an `Iterator<T>`: `{ done: false; value: T }`. The non-terminating arm of `IteratorResult<T>`. */",
                ),
            },
        },
    );

    let return_result_body = || -> Type {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "done".to_string(),
            crate::ObjectField::required(Type::Boolean),
        );
        Type::Object {
            index: None,
            fields,
        }
    };
    defs.types.insert(
        "IteratorReturnResult".to_string(),
        TypeSymbol {
            name: "IteratorReturnResult".to_string(),
            mangled_name: crate::mangle::prelude("IteratorReturnResult"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Alias {
                generics: Vec::new(),
                ty: return_result_body(),
                doc: doc(
                    "/** End-of-stream marker: `{ done: true }`. Returned by `Iterator<T>.next()` after the iterator is exhausted; subsequent calls keep returning this same shape. */",
                ),
            },
        },
    );

    let iterator_result_body = || -> Type {
        Type::Union(vec![
            Type::Alias {
                mangled: crate::mangle::prelude("IteratorYieldResult"),
                package: crate::Package::prelude(),
                name: "IteratorYieldResult".to_string(),
                args: vec![Type::TypeVar("T".to_string())],
                ty: Box::new(yield_result_body()),
            },
            Type::Alias {
                mangled: crate::mangle::prelude("IteratorReturnResult"),
                package: crate::Package::prelude(),
                name: "IteratorReturnResult".to_string(),
                args: Vec::new(),
                ty: Box::new(return_result_body()),
            },
        ])
    };
    defs.types.insert(
        "IteratorResult".to_string(),
        TypeSymbol {
            name: "IteratorResult".to_string(),
            mangled_name: crate::mangle::prelude("IteratorResult"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Alias {
                generics: vec!["T".to_string()],
                ty: iterator_result_body(),
                doc: doc(
                    "/** Discriminated-union result of `Iterator<T>.next()`. Discriminate on the `done` boolean: `false` → element in `value`; `true` → end-of-stream. Designed to compose with `for-of`, which handles the discrimination automatically. */",
                ),
            },
        },
    );

    defs.types.insert(
        "Iterator".to_string(),
        TypeSymbol {
            name: "Iterator".to_string(),
            mangled_name: crate::mangle::prelude("Iterator"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: vec!["T".to_string()],
                methods: BTreeMap::from([(
                    "next".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: Vec::new(),
                        ret: Type::Alias { mangled: crate::mangle::prelude("IteratorResult"), package: crate::Package::prelude(),
                            name: "IteratorResult".to_string(),
                            args: vec![Type::TypeVar("T".to_string())],
                            ty: Box::new(iterator_result_body()),
                        },
                        predicate: None,
                        doc: doc(
                            "/** Returns the next element wrapped in an `IteratorResult<T>`. Yield: `{ done: false; value: <element> }`. End-of-stream: `{ done: true }`. Subsequent calls after exhaustion keep returning the end-of-stream shape. */",
                        ),
                    },
                )]),
                properties: BTreeMap::from([(
                    "close".to_string(),
                    PropertySig {
                        ty: Type::Function {
                            params: Vec::new(),
                            ret: Box::new(Type::Void),
                            predicate: None,
                            has_rest: false,
                        },
                        readonly: true,
                        optional: true,
                        intrinsic: false,
                        doc: doc(
                            "/** Optional cleanup hook. Called once on every `for-of` exit (normal exhaustion, `break` / `return` / `continue`, or thrown exception) so host-backed iterators can release OS handles. Pure-data iterators omit it. */",
                        ),
                    },
                )]),
                dispatch: Dispatch::VTable,
                doc: doc(
                    "/** A consumed-once cursor producing values of type `T`. `for-of` walks it via `next()` until the result is `{ done: true }`. `T` may include `null` — the `done` field is the discriminator, not the element value. */",
                ),
            },
        },
    );
    defs.types.insert(
        "Iterable".to_string(),
        TypeSymbol {
            name: "Iterable".to_string(),
            mangled_name: crate::mangle::prelude("Iterable"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: vec!["T".to_string()],
                methods: BTreeMap::from([(
                    "iterator".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: Vec::new(),
                        ret: Type::prelude_interface("Iterator".to_string(), vec![Type::TypeVar("T".to_string())]),
                        predicate: None,
                        doc: doc(
                            "/** Returns a fresh `Iterator<T>` over this collection's elements. Multiple `for-of` loops over the same `Iterable` walk independently. */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::VTable,
                doc: doc(
                    "/** A re-iterable collection. `for-of` calls `iterator()` once at the loop head; each loop gets its own cursor. */",
                ),
            },
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::closures::{ClosureSig, emit_arity_closures};
    use crate::codegen::intrinsics::declare_intrinsic_types;
    use crate::runtime::intrinsic_types::build_intrinsic_types;
    use wasm_encoder::{
        CompositeInnerType, CompositeType, ConstExpr, ExportKind, ExportSection,
        FieldType as EncFieldType, GlobalSection, GlobalType, HeapType as EncHeapType, Module,
        RefType as EncRefType, StorageType as EncStorageType, StructType as EncStructType, SubType,
        TypeSection, ValType as EncValType,
    };
    use wasmtime::{Config, Engine, StructType};

    /// The codegen-side arity-2 `$ObjectShape` subtype (`{vtable, field_names,
    /// object_fields}`) the pin below compares the host type against.
    fn arity_2_object_subtype(
        object_shape_type_idx: u32,
        vtable_type_idx: u32,
        field_names_type_idx: u32,
        object_fields_type_idx: u32,
    ) -> SubType {
        let mk = |idx: u32| EncFieldType {
            element_type: EncStorageType::Val(EncValType::Ref(EncRefType {
                nullable: false,
                heap_type: EncHeapType::Concrete(idx),
            })),
            mutable: false,
        };
        SubType {
            is_final: false,
            supertype_idx: Some(object_shape_type_idx),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(EncStructType {
                    fields: vec![
                        mk(vtable_type_idx),
                        EncFieldType {
                            mutable: true,
                            ..mk(field_names_type_idx)
                        },
                        EncFieldType {
                            mutable: true,
                            ..mk(object_fields_type_idx)
                        },
                        EncFieldType {
                            mutable: true,
                            element_type: EncStorageType::Val(EncValType::Ref(EncRefType::ANYREF)),
                        },
                    ]
                    .into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        }
    }

    /// The `$closure_0_value` struct `next_closure_type` declares must be
    /// canonically equal to the one `codegen::closures` emits for `ClosureSig {
    /// arity: 0, is_void: false }` — otherwise the guest's structural-dispatch
    /// `ref.cast` of an iterator's `next` field would trap. Mirrors the
    /// `unary_void_closure_matches_codegen` backstop in the array port.
    #[test]
    fn next_closure_matches_codegen() {
        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = Engine::new(&config).unwrap();

        let intr = build_intrinsic_types(&engine).unwrap();
        let (_func, host) = next_closure_type(&engine, &intr).unwrap();

        let sig = ClosureSig {
            arity: 0,
            is_void: false,
        };
        let mut module = Module::new();
        let mut types = TypeSection::new();
        let intrinsics = declare_intrinsic_types(&mut types);
        let mut next_idx = crate::codegen::intrinsics::INTRINSIC_TYPE_COUNT;
        let assigned = emit_arity_closures([sig], &mut types, intrinsics, &mut next_idx).unwrap();
        let (_fn_idx, struct_idx) = assigned[&sig];
        module.section(&types);

        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: EncValType::Ref(EncRefType {
                    nullable: true,
                    heap_type: EncHeapType::Concrete(struct_idx),
                }),
                mutable: false,
                shared: false,
            },
            &ConstExpr::ref_null(EncHeapType::Concrete(struct_idx)),
        );
        let mut exports = ExportSection::new();
        exports.export("closure", ExportKind::Global, 0);
        module.section(&globals);
        module.section(&exports);

        let module = wasmtime::Module::new(&engine, module.finish()).unwrap();
        let recovered = module
            .get_export("closure")
            .unwrap()
            .global()
            .unwrap()
            .content()
            .as_ref()
            .map(|r| r.heap_type().clone())
            .unwrap()
            .as_concrete_struct()
            .unwrap()
            .clone();
        assert!(StructType::eq(&host, &recovered));
    }

    /// Same pin for `void_closure_type` against `ClosureSig { arity: 0,
    /// is_void: true }` — the `close` field a closable iterator carries.
    #[test]
    fn void_closure_matches_codegen() {
        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = Engine::new(&config).unwrap();

        let intr = build_intrinsic_types(&engine).unwrap();
        let (_func, host) = void_closure_type(&engine, &intr).unwrap();

        let sig = ClosureSig {
            arity: 0,
            is_void: true,
        };
        let mut module = Module::new();
        let mut types = TypeSection::new();
        let intrinsics = declare_intrinsic_types(&mut types);
        let mut next_idx = crate::codegen::intrinsics::INTRINSIC_TYPE_COUNT;
        let assigned = emit_arity_closures([sig], &mut types, intrinsics, &mut next_idx).unwrap();
        let (_fn_idx, struct_idx) = assigned[&sig];
        module.section(&types);

        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: EncValType::Ref(EncRefType {
                    nullable: true,
                    heap_type: EncHeapType::Concrete(struct_idx),
                }),
                mutable: false,
                shared: false,
            },
            &ConstExpr::ref_null(EncHeapType::Concrete(struct_idx)),
        );
        let mut exports = ExportSection::new();
        exports.export("closure", ExportKind::Global, 0);
        module.section(&globals);
        module.section(&exports);

        let module = wasmtime::Module::new(&engine, module.finish()).unwrap();
        let recovered = module
            .get_export("closure")
            .unwrap()
            .global()
            .unwrap()
            .content()
            .as_ref()
            .map(|r| r.heap_type().clone())
            .unwrap()
            .as_concrete_struct()
            .unwrap()
            .clone();
        assert!(StructType::eq(&host, &recovered));
    }

    /// The host `iterator_result_struct` must canonically equal the
    /// `$ObjectShape` subtype codegen emits for `IteratorYieldResult` — otherwise
    /// `for…of`'s `ref.cast` of `next()`'s result would trap. (The arity-1
    /// `IteratorReturnResult` shares the same canonical type.)
    #[test]
    fn iterator_result_matches_codegen() {
        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = Engine::new(&config).unwrap();

        let intr = build_intrinsic_types(&engine).unwrap();
        let host = iterator_result_struct(&engine, &intr).unwrap();

        let mut module = Module::new();
        let mut types = TypeSection::new();
        let idx = declare_intrinsic_types(&mut types);
        types.ty().subtype(&arity_2_object_subtype(
            idx.object_shape,
            idx.vtable,
            idx.field_names,
            idx.object_fields,
        ));
        let result_idx = crate::codegen::intrinsics::INTRINSIC_TYPE_COUNT;
        module.section(&types);

        let mut globals = GlobalSection::new();
        globals.global(
            GlobalType {
                val_type: EncValType::Ref(EncRefType {
                    nullable: true,
                    heap_type: EncHeapType::Concrete(result_idx),
                }),
                mutable: false,
                shared: false,
            },
            &ConstExpr::ref_null(EncHeapType::Concrete(result_idx)),
        );
        let mut exports = ExportSection::new();
        exports.export("result", ExportKind::Global, 0);
        module.section(&globals);
        module.section(&exports);

        let module = wasmtime::Module::new(&engine, module.finish()).unwrap();
        let recovered = module
            .get_export("result")
            .unwrap()
            .global()
            .unwrap()
            .content()
            .as_ref()
            .map(|r| r.heap_type().clone())
            .unwrap()
            .as_concrete_struct()
            .unwrap()
            .clone();
        assert!(StructType::eq(&host, &recovered));
    }
}
