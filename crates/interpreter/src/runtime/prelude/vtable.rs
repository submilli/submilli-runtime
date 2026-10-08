//! Host-owned object vtables — the four object-identity slots (`toString`,
//! `toJson`, `equals`, `hash`) of `$VTable` built as Rust host `Func`s and owned
//! by the host under the `submilli:prelude_vtable` linker module.
//!
//! [`install_vtable_module`] is the **producer**: it assembles a `$VTable` struct
//! whose four fields are host `Func`s of the canonical slot types (recovered via
//! [`build_intrinsic_types`]) and defines it as a linker global. Guest modules
//! import these globals; a guest `call_ref` through a `$VTable` slot lands in
//! the host with no codegen change.
//!
//! [`dispatch_vtable_slot`] is the **consumer**: given a `(ref $Object)`, it reads
//! field 0 (`$VTable`), pulls the slot funcref and `call_ref`s it — the array slots
//! use it to re-enter each element's own slot, exactly as the Wasm bodies did.
//!
//! The slot algorithms mirror the Wasm bodies they replace (`prelude::string` and
//! `prelude::array`) byte-for-byte: the FNV-1a hash and JSON escaping must agree so
//! a host-built value hashes and serializes identically to a guest-built one.

use crate::runtime::host::{abi_arg, abi_result};
pub(crate) mod serialization;

use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, Caller, Func, Global, GlobalType, HeapType, Linker,
    Mutability, RefType, Rooted, Store, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    fatal_host_error, host_string_vtable, read_code_units, read_uint8_array_arg,
    write_submilli_string_struct, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types, intrinsic_types};
use crate::runtime::number::format_number_js;
use crate::runtime::prelude::keep::keep_all;

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

/// Direct handles to the three host-owned vtable globals, returned by
/// [`install_vtable_module`] so they can be cached in `HostAbi` — host fns read
/// them from the store rather than via the prelude instance's re-exports.
pub(crate) struct HostVtables {
    pub string: Global,
    pub array: Global,
    pub object: Global,
    pub boxed_number: Global,
    pub boxed_boolean: Global,
    pub uint8_array: Global,
    pub bigint: Global,
    pub closure: Global,
    pub regex: Global,
    pub regex_match_box: Global,
    /// The identity vtable for host-only backing structs (stdlib `URL`,
    /// `Response`, fs `Stat`, …): `toString` → "[object Object]", `toJson` →
    /// "{}", reference-identity equality, hash 0.
    pub opaque: Global,
}

/// Build the host-owned vtable globals and define them under
/// [`MODULE_NAME`](super::MODULE_NAME). Uses the install-time `Store`, which is
/// the same store the guest later runs in — so the host `Func`s and globals
/// stay valid for `call_ref` from guest Wasm. Returns the global handles for
/// caching in `HostAbi`.
pub fn install_vtable_module(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<HostVtables> {
    let intr = build_intrinsic_types(store.engine())?;
    let string_vt = build_string_vtable(store, &intr)?;
    let array_vt = build_array_vtable(store, &intr)?;
    let object_vt = build_object_vtable(store, &intr)?;
    let boxed_number_vt = build_boxed_number_vtable(store, &intr)?;
    let boxed_boolean_vt = build_boxed_boolean_vtable(store, &intr)?;
    let uint8_array_vt = build_uint8array_vtable(store, &intr)?;
    let bigint_vt = build_bigint_vtable(store, &intr)?;
    let closure_vt = build_closure_vtable(store, &intr)?;
    let regex_vt = build_regex_vtable(store, &intr)?;
    let regex_match_box_vt = build_regex_match_box_vtable(store, &intr)?;
    let opaque_vt = build_opaque_vtable(store, &intr)?;
    Ok(HostVtables {
        opaque: define_vtable(linker, store, &intr, "opaque_vtable", opaque_vt)?,
        string: define_vtable(linker, store, &intr, "string_vtable", string_vt)?,
        array: define_vtable(linker, store, &intr, "array_vtable", array_vt)?,
        object: define_vtable(linker, store, &intr, "object_vtable", object_vt)?,
        boxed_number: define_vtable(linker, store, &intr, "boxed_number_vtable", boxed_number_vt)?,
        boxed_boolean: define_vtable(
            linker,
            store,
            &intr,
            "boxed_boolean_vtable",
            boxed_boolean_vt,
        )?,
        uint8_array: define_vtable(linker, store, &intr, "uint8_array_vtable", uint8_array_vt)?,
        bigint: define_vtable(linker, store, &intr, "bigint_vtable", bigint_vt)?,
        closure: define_vtable(linker, store, &intr, "closure_vtable", closure_vt)?,
        regex: define_vtable(linker, store, &intr, "regex_vtable", regex_vt)?,
        regex_match_box: define_vtable(
            linker,
            store,
            &intr,
            "regex_match_box_vtable",
            regex_match_box_vt,
        )?,
    })
}

/// Assemble a `$VTable` struct from its four slot funcs, wrap it in an immutable
/// `(ref $VTable)` global, register it under `mangle::prelude(name)`, and return
/// the global handle.
fn define_vtable(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
    name: &str,
    slots: [Func; 4],
) -> wasmtime::Result<Global> {
    let pre = StructRefPre::new(&mut *store, intr.vtable.clone());
    let vtable = StructRef::new(
        &mut *store,
        &pre,
        &[
            Val::FuncRef(Some(slots[0])),
            Val::FuncRef(Some(slots[1])),
            Val::FuncRef(Some(slots[2])),
            Val::FuncRef(Some(slots[3])),
        ],
    )?;
    let gty = GlobalType::new(
        ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.vtable.clone()),
        )),
        Mutability::Const,
    );
    let global = Global::new(&mut *store, gty, Val::AnyRef(Some(vtable.to_anyref())))?;
    let field = crate::mangle::prelude(name);
    linker.define(&mut *store, super::MODULE_NAME, field.as_str(), global)?;
    Ok(global)
}

// ---------------------------------------------------------------------------
// Consumer
// ---------------------------------------------------------------------------

/// Re-enter `object`'s vtable `slot` via `call_ref`: read field 0 (`$VTable`),
/// pull the slot funcref, and call it with `object` as the leading argument
/// followed by `extra` (e.g. `equals`' second operand). Returns the slot's result.
pub(crate) async fn dispatch_vtable_slot(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    slot: usize,
    extra: &[Val],
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, object, "vtable dispatch receiver")?;
    let vtable = match st.field(&mut *caller, 0)? {
        Val::AnyRef(Some(any)) => any,
        other => wasmtime::bail!("vtable dispatch: object vtable is {other:?}"),
    };
    let vt = vtable
        .as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg("vtable dispatch: field 0 is not a $VTable"))?;
    let func = match vt.field(&mut *caller, slot)? {
        Val::FuncRef(Some(func)) => func,
        other => wasmtime::bail!("vtable dispatch: slot {slot} is {other:?}"),
    };
    let mut args = Vec::with_capacity(1 + extra.len());
    args.push(*object);
    args.extend_from_slice(extra);
    let mut out = [Val::null_any_ref()];
    enter_walk(caller)?;
    let call_result = func.call_async(&mut *caller, &args, &mut out).await;
    leave_walk(caller);
    call_result?;
    let [result] = out;
    Ok(result)
}

/// Count one level into the universal-vtable walk. These bodies are host
/// frames, so without a bound a cyclic graph recurses until the *native* stack
/// is gone — an abort rather than something the program can catch. See
/// [`MAX_VTABLE_WALK_DEPTH`](crate::runtime::MAX_VTABLE_WALK_DEPTH).
fn enter_walk(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<()> {
    let depth = caller.data().vtable_walk_depth;
    if depth >= crate::runtime::MAX_VTABLE_WALK_DEPTH {
        let err = crate::runtime::host::range_error(format!(
            "object graph is nested deeper than {} levels, so it cannot be compared, \
             hashed, or serialized — a cycle (an object reachable from itself) reaches \
             this bound too",
            crate::runtime::MAX_VTABLE_WALK_DEPTH,
        ));
        return Err(crate::runtime::host::throw_host_error(caller, err));
    }
    if caller.data().vtable_walk_nodes >= crate::runtime::MAX_STRUCTURAL_WALK_NODES {
        let err = crate::runtime::host::range_error(format!(
            "object graph exceeds {} structural visits; use a smaller value or reduce shared nesting",
            crate::runtime::MAX_STRUCTURAL_WALK_NODES,
        ));
        return Err(crate::runtime::host::throw_host_error(caller, err));
    }
    let data = caller.data_mut();
    data.vtable_walk_nodes += 1;
    data.vtable_walk_depth = depth + 1;
    Ok(())
}

/// Release one level entered by [`enter_walk`]. Runs on the error path too, so
/// a caught throw leaves the counter where it started.
fn leave_walk(caller: &mut Caller<'_, StoreData>) {
    let depth = caller.data().vtable_walk_depth.saturating_sub(1);
    let data = caller.data_mut();
    data.vtable_walk_depth = depth;
    if depth == 0 {
        data.vtable_walk_nodes = 0;
    }
}

/// Direct guest `call_ref` bypasses the dispatcher. Establish its outer scope;
/// nested host dispatches and generated structural bodies own their own entries.
fn host_vtable_func<F>(store: &mut Store<StoreData>, ty: wasmtime::FuncType, body: F) -> Func
where
    F: for<'a, 'b> Fn(
            &'a mut Caller<'b, StoreData>,
            &'a [Val],
            &'a mut [Val],
        )
            -> Box<dyn std::future::Future<Output = wasmtime::Result<()>> + Send + 'a>
        + Send
        + Sync
        + 'static,
{
    let body = std::sync::Arc::new(body);
    fuel::host_func_async(store, ty, move |mut caller, params, results| {
        let body = std::sync::Arc::clone(&body);
        Box::new(async move {
            let outer = caller.data().vtable_walk_depth == 0;
            if outer {
                enter_walk(&mut caller)?;
            }
            let result = std::pin::Pin::from(body(&mut caller, params, results)).await;
            if outer {
                leave_walk(&mut caller);
            }
            result
        })
    })
}

// ---------------------------------------------------------------------------
// $string vtable
// ---------------------------------------------------------------------------

fn build_string_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |_caller, params, results| {
            // `toString` of a string is identity — the receiver already *is* the
            // `$string`; hand it back unchanged.
            Box::new(async move {
                *abi_result(results, 0)? = *abi_arg(params, 0)?;
                Ok(())
            })
        },
    );

    let raw_string = intr.raw_string.clone();
    let string_ty = intr.string.clone();
    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        move |caller, params, results| {
            let raw_string = raw_string.clone();
            let string_ty = string_ty.clone();
            Box::new(async move {
                *abi_result(results, 0)? =
                    string_to_json(&mut *caller, abi_arg(params, 0)?, &raw_string, &string_ty)?;
                Ok(())
            })
        },
    );

    let string_ty = intr.string.clone();
    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        move |caller, params, results| {
            let string_ty = string_ty.clone();
            Box::new(async move {
                let eq = string_equals(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    &string_ty,
                )?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                *abi_result(results, 0)? =
                    Val::I32(string_hash(caller, abi_arg(params, 0)?)? as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

fn string_to_json(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    raw_string: &ArrayType,
    string_ty: &StructType,
) -> wasmtime::Result<Val> {
    let st = as_struct(caller, recv, "String#toJson")?;
    let vtable = st.field(&mut *caller, 0)?;
    let units = read_struct_units(caller, &st, "String#toJson")?;
    let escaped = serialization::quoted(caller, &units)?;
    build_string(caller, raw_string, string_ty, vtable, escaped.units())
}

pub(super) fn quote_string(
    caller: &mut Caller<'_, StoreData>,
    units: &[u16],
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let escaped = serialization::quoted(caller, units)?;
    let vtable = host_string_vtable(caller)?;
    build_string(
        caller,
        &intr.raw_string,
        &intr.string,
        vtable,
        escaped.units(),
    )
}

pub(super) fn string_hash(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<u32> {
    let string = as_struct(caller, value, "String#hash")?;
    let Val::I64(cached) = string.field(&mut *caller, 2)? else {
        return Err(crate::runtime::host::fatal_host_error(
            "String: invalid hash cache",
        ));
    };
    if cached != 0 {
        let hash = cached
            .checked_sub(1)
            .ok_or_else(|| crate::runtime::host::fatal_host_error("String: invalid hash cache"))?;
        return u32::try_from(hash).map_err(crate::runtime::host::fatal_host_error);
    }
    let units = read_struct_units(caller, &string, "String#hash")?;
    fuel::charge(&mut *caller, fuel::SCAN, units.len() as u64)?;
    let hash = fnv_hash_units(&units);
    string.set_field(&mut *caller, 2, Val::I64(i64::from(hash) + 1))?;
    Ok(hash)
}

pub(super) fn string_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
    string_ty: &StructType,
) -> wasmtime::Result<bool> {
    // Dispatched off a union value, `other` may be a non-string — a mismatched
    // type is unequal, not an error.
    let Some(other_st) = as_opt_struct(caller, other)? else {
        return Ok(false);
    };
    if !other_st.matches_ty(&*caller, string_ty)? {
        return Ok(false);
    }
    let recv_st = as_struct(caller, recv, "String#equals receiver")?;
    if Rooted::ref_eq(&*caller, &recv_st, &other_st)? {
        return Ok(true);
    }
    let length = |caller: &mut Caller<'_, StoreData>, string: &Rooted<StructRef>| match string
        .field(&mut *caller, 1)?
    {
        Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?.len(&mut *caller),
        _ => Err(crate::runtime::host::fatal_host_error(
            "String: invalid payload",
        )),
    };
    let len = length(caller, &recv_st)?;
    if len != length(caller, &other_st)? {
        return Ok(false);
    }
    let recv_units = read_struct_units(caller, &recv_st, "String#equals receiver")?;
    let other_units = read_struct_units(caller, &other_st, "String#equals other")?;
    fuel::charge(&mut *caller, fuel::SCAN, u64::from(len))?;
    Ok(recv_units == other_units)
}

// ---------------------------------------------------------------------------
// $Array vtable
// ---------------------------------------------------------------------------

fn build_array_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let raw_string = intr.raw_string.clone();
    let string_ty = intr.string.clone();
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        move |caller, params, results| {
            let raw_string = raw_string.clone();
            let string_ty = string_ty.clone();
            Box::new(async move {
                *abi_result(results, 0)? =
                    array_to_string(&mut *caller, abi_arg(params, 0)?, &raw_string, &string_ty)
                        .await?;
                Ok(())
            })
        },
    );

    let raw_string = intr.raw_string.clone();
    let string_ty = intr.string.clone();
    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        move |caller, params, results| {
            let raw_string = raw_string.clone();
            let string_ty = string_ty.clone();
            Box::new(async move {
                *abi_result(results, 0)? =
                    array_to_json(&mut *caller, abi_arg(params, 0)?, &raw_string, &string_ty)
                        .await?;
                Ok(())
            })
        },
    );

    let array_ty = intr.array.clone();
    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        move |caller, params, results| {
            let array_ty = array_ty.clone();
            Box::new(async move {
                let eq = array_equals(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    &array_ty,
                )
                .await?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let h = array_hash(&mut *caller, abi_arg(params, 0)?).await?;
                *abi_result(results, 0)? = Val::I32(h as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

/// `Array#toString` == `join(",")`: each element's `toString` slot, joined by `,`.
/// Matches `prelude::array::join`: a null element contributes the empty string.
async fn array_to_string(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    raw_string: &ArrayType,
    string_ty: &StructType,
) -> wasmtime::Result<Val> {
    let output = serialization::array(caller, recv, false).await?;
    let vtable = host_string_vtable(caller)?;
    build_string(caller, raw_string, string_ty, vtable, output.units())
}

/// `Array#toJson`: `[` + each element's `toJson` (null → `null`), joined by `,` + `]`.
async fn array_to_json(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    raw_string: &ArrayType,
    string_ty: &StructType,
) -> wasmtime::Result<Val> {
    let output = serialization::array(caller, recv, true).await?;
    let vtable = host_string_vtable(caller)?;
    build_string(caller, raw_string, string_ty, vtable, output.units())
}

/// Structural `Array#equals`: same length, element-wise via each element's
/// `equals` slot. Mirrors `prelude::array::equals_body`.
async fn array_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
    array_ty: &StructType,
) -> wasmtime::Result<bool> {
    let Some(other_st) = as_opt_struct(caller, other)? else {
        return Ok(false);
    };
    if !StructType::eq(&other_st.ty(&caller)?, array_ty) {
        return Ok(false);
    }
    if let (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) = (recv, other)
        && Rooted::ref_eq(&caller, a, b)?
    {
        return Ok(true);
    }
    let a = read_array_backing(caller, recv, "Array#equals receiver")?;
    let b = read_array_backing(caller, other, "Array#equals other")?;
    if a.len() != b.len() {
        return Ok(false);
    }
    for (ae, be) in a.iter().zip(b.iter()) {
        match (ae, be) {
            (Val::AnyRef(None), Val::AnyRef(None)) => continue,
            (Val::AnyRef(Some(_)), Val::AnyRef(Some(_))) => {
                let eq = dispatch_vtable_slot(caller, ae, 2, std::slice::from_ref(be)).await?;
                if eq.i32() != Some(1) {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// FNV-1a over element hashes (null element contributes 0). Mirrors
/// `prelude::array::hash_body`.
async fn array_hash(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<u32> {
    let elements = read_array_backing(caller, recv, "Array#hash")?;
    let mut hash = FNV_OFFSET;
    for elem in &elements {
        let e_hash = match elem {
            Val::AnyRef(Some(_)) => dispatch_vtable_slot(caller, elem, 3, &[])
                .await?
                .i32()
                .unwrap_or(0) as u32,
            _ => 0,
        };
        hash = fnv_combine(hash, e_hash);
    }
    Ok(hash)
}

// ---------------------------------------------------------------------------
// generic object vtable (host-built objects, e.g. iterators)
// ---------------------------------------------------------------------------

fn build_object_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                if let Some(value) =
                    object_override(&mut *caller, abi_arg(params, 0)?, "toString").await?
                {
                    *abi_result(results, 0)? = value;
                    return Ok(());
                }
                let tag = match collection_backing_kind(&mut *caller, abi_arg(params, 0)?)? {
                    Some(CollectionBacking::Map) => "[object Map]",
                    Some(CollectionBacking::Set) => "[object Set]",
                    None => "[object Object]",
                };
                let st = write_submilli_string_struct(&mut *caller, tag)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let raw_string = intr.raw_string.clone();
    let string_ty = intr.string.clone();
    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        move |caller, params, results| {
            let raw_string = raw_string.clone();
            let string_ty = string_ty.clone();
            Box::new(async move {
                *abi_result(results, 0)? =
                    object_to_json(&mut *caller, abi_arg(params, 0)?, &raw_string, &string_ty)
                        .await?;
                Ok(())
            })
        },
    );

    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let eq =
                    object_equals(&mut *caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let hash = object_hash(&mut *caller, abi_arg(params, 0)?).await?;
                *abi_result(results, 0)? = Val::I32(hash as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

async fn object_override(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    name: &str,
) -> wasmtime::Result<Option<Val>> {
    let intr = intrinsic_types(&mut *caller)?;
    if !super::collection::is_a(caller, recv, &intr.object_shape)? {
        return Ok(None);
    }
    let wanted: Vec<u16> = name.encode_utf16().collect();
    for (key, value) in read_object_entries(caller, recv, name)? {
        if key == wanted && is_function(caller, &value)? {
            let closure = super::closure::read(caller, &value, name)?;
            let result = closure.call(caller, &[]).await?;
            // A `Record` field can hold a conversion of any return type, and
            // every reader of the result takes its payload as code units.
            if !super::collection::is_a(caller, &result, &intr.string)? {
                let error =
                    crate::runtime::host::type_error(format!("{name} must return a string"));
                return Err(crate::runtime::host::throw_host_error(caller, error));
            }
            return Ok(Some(result));
        }
    }
    Ok(None)
}

/// `$ObjectShape#toJson`: serialize the dynamic field-name and field-value arrays
/// stored on host-built objects. Each value re-enters its own `toJson` slot.
pub(crate) async fn object_to_json(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    raw_string: &ArrayType,
    string_ty: &StructType,
) -> wasmtime::Result<Val> {
    // Typed JSON's dynamic fallback can call this without dispatching a slot.
    let outer = caller.data().vtable_walk_depth == 0;
    if outer {
        enter_walk(caller)?;
    }
    let result = object_to_json_fields(caller, recv, raw_string, string_ty).await;
    if outer {
        leave_walk(caller);
    }
    result
}

async fn object_to_json_fields(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    raw_string: &ArrayType,
    string_ty: &StructType,
) -> wasmtime::Result<Val> {
    if let Some(kind) = collection_backing_kind(caller, recv)? {
        // `$MapBacking` / `$SetBacking` carry the object vtable but not the
        // `$ObjectShape` field arrays this body reads.
        let err = crate::runtime::host::type_error(format!(
            "a `{kind}` has no JSON representation — convert it first: \
             `JSON.stringify(Array.from(x))` serializes the entries, or build a plain object"
        ));
        return Err(crate::runtime::host::throw_host_error(caller, err));
    }
    if let Some(value) = object_override(caller, recv, "toJson").await? {
        return Ok(value);
    }
    let output = serialization::object(caller, recv).await?;
    let vtable = host_string_vtable(caller)?;
    build_string(caller, raw_string, string_ty, vtable, output.units())
}

/// Snapshot keys, then read each value in canonical key order. Getter side
/// effects can affect later values, and getter exceptions abort serialization.
fn json_property_slots(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
) -> wasmtime::Result<Vec<(Vec<u16>, u32, bool)>> {
    let object = as_struct(caller, recv, "Object#toJson")?;
    let names = super::object::field_array(caller, &object, 1)?;
    let values = super::object::field_array(caller, &object, 2)?;
    let mut entries = Vec::new();
    let count = super::object::field_count(caller, recv)?;
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(count))?;
    entries
        .try_reserve_exact(count as usize)
        .map_err(fatal_host_error)?;
    let error_slots = super::error::non_enumerable_slots(caller, recv)?;
    for slot in 0..count {
        if error_slots
            .as_ref()
            .is_some_and(|hidden| hidden.hides(slot))
        {
            continue;
        }
        let name = names.get(&mut *caller, slot)?;
        if super::object::field_is_private(caller, &name)? {
            continue;
        }
        let getter = super::object::is_accessor_slot(caller, &name)?;
        let mut units = read_string_units(caller, &name, "JSON property name")?;
        if getter {
            let getter_prefix = [
                u16::from(b'g'),
                u16::from(b'e'),
                u16::from(b't'),
                u16::from(b' '),
            ];
            if !units.starts_with(&getter_prefix) {
                continue;
            }
            units.drain(..getter_prefix.len());
        } else {
            let value = values.get(&mut *caller, slot)?;
            if !super::object::field_is_present(caller, &name, &value)? {
                continue;
            }
        }
        entries.push((units, slot, getter));
    }
    let _sort_memory = super::array::sort::reserve_key_sort_memory(caller, entries.len())?;
    let mut indices = Vec::new();
    indices
        .try_reserve_exact(entries.len())
        .map_err(fatal_host_error)?;
    indices.extend(0..entries.len());
    super::array::sort::sort_key_indices(
        caller,
        |index| entries.get(index).map(|entry| entry.0.as_slice()),
        &mut indices,
    )?;
    let mut ranks = Vec::new();
    ranks
        .try_reserve_exact(entries.len())
        .map_err(fatal_host_error)?;
    ranks.resize(entries.len(), 0);
    for (rank, original) in indices.into_iter().enumerate() {
        let position = ranks
            .get_mut(original)
            .ok_or_else(|| fatal_host_error("invalid JSON key index"))?;
        *position = rank;
    }
    fuel::charge(&mut *caller, fuel::ELEM, entries.len() as u64)?;
    // Slots are visited in ascending order above; ranks map that order to the
    // UTF-16 ordering. Apply the permutation without copying key strings.
    for position in 0..entries.len() {
        loop {
            let target = *ranks
                .get(position)
                .ok_or_else(|| fatal_host_error("invalid JSON key rank"))?;
            if target == position {
                break;
            }
            if target >= entries.len() {
                return Err(fatal_host_error("invalid JSON key rank"));
            }
            entries.swap(position, target);
            ranks.swap(position, target);
        }
    }
    Ok(entries)
}

async fn object_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<bool> {
    let (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) = (recv, other) else {
        return Ok(matches!(
            (recv, other),
            (Val::AnyRef(None), Val::AnyRef(None))
        ));
    };
    if Rooted::ref_eq(&caller, a, b)? {
        return Ok(true);
    }
    // A `Map`/`Set` compares by reference, the way JS does: the structural
    // walk below would read its bucket arrays as an `$ObjectShape` payload.
    // The `ref.eq` above already answered the same-collection case.
    if is_collection_backing(caller, recv)? || is_collection_backing(caller, other)? {
        return Ok(false);
    }

    let intr = intrinsic_types(&mut *caller)?;
    if !super::collection::is_a(caller, other, &intr.object_shape)? {
        return Ok(false);
    }
    let lhs_object = as_struct(caller, recv, "Object#equals receiver")?;
    let rhs_object = as_struct(caller, other, "Object#equals other")?;
    for object in [lhs_object, rhs_object] {
        let vtable = object.field(&mut *caller, 0)?;
        if super::collection::is_a(caller, &vtable, &intr.class_vtable)? {
            return Ok(false);
        }
    }
    let left_count = super::object::field_count(caller, recv)?;
    let right_count = super::object::field_count(caller, other)?;
    fuel::charge(
        &mut *caller,
        fuel::ELEM,
        u64::from(left_count) + u64::from(right_count),
    )?;
    let lhs = read_object_entries(caller, recv, "Object#equals receiver")?;
    if lhs.len() != super::object::data_field_count(caller, other)? {
        return Ok(false);
    }
    let names = super::object::field_array(caller, &rhs_object, 1)?;
    let values = super::object::field_array(caller, &rhs_object, 2)?;
    for (name, lhs_value) in &lhs {
        let Some(slot) = super::object::find_data_slot(caller, other, name)? else {
            return Ok(false);
        };
        let rhs_name = names.get(&mut *caller, slot)?;
        let rhs_value = values.get(&mut *caller, slot)?;
        if !super::object::field_is_present(caller, &rhs_name, &rhs_value)?
            || !object_field_equals(caller, lhs_value, &rhs_value).await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn object_hash(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<u32> {
    if is_collection_backing(caller, recv)? {
        return identity_hash(caller, recv);
    }
    let count = super::object::field_count(caller, recv)?;
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(count))?;
    let entries = read_object_entries(caller, recv, "Object#hash")?;
    let mut hash = FNV_OFFSET;
    for (name, value) in &entries {
        let field_hash = object_field_hash(caller, value).await?;
        fuel::charge(&mut *caller, fuel::SCAN, name.len() as u64)?;
        hash = hash.wrapping_add(field_hash.rotate_left(13) ^ fnv_hash_units(name));
    }
    Ok(hash)
}

async fn object_field_equals(
    caller: &mut Caller<'_, StoreData>,
    lhs: &Val,
    rhs: &Val,
) -> wasmtime::Result<bool> {
    match (lhs, rhs) {
        (Val::AnyRef(None), Val::AnyRef(None)) => Ok(true),
        (Val::AnyRef(Some(_)), Val::AnyRef(Some(_))) => {
            Ok(
                dispatch_vtable_slot(caller, lhs, 2, std::slice::from_ref(rhs))
                    .await?
                    .i32()
                    == Some(1),
            )
        }
        _ => Ok(false),
    }
}

async fn object_field_hash(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<u32> {
    match value {
        Val::AnyRef(Some(_)) => Ok(dispatch_vtable_slot(caller, value, 3, &[])
            .await?
            .i32()
            .unwrap_or(0) as u32),
        _ => Ok(0),
    }
}

pub(crate) fn read_object_entries(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    name: &str,
) -> wasmtime::Result<Vec<(Vec<u16>, Val)>> {
    let st = as_struct(caller, recv, name)?;
    let names = match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => wasmtime::bail!("{name}: malformed field-name array {other:?}"),
    };
    let values = match st.field(&mut *caller, 2)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => wasmtime::bail!("{name}: malformed field-value array {other:?}"),
    };

    let name_count = super::object::field_count(caller, recv)?;
    let value_count = values.len(&mut *caller)?;
    // Class validator rows follow the named payload and are not properties.
    if name_count > value_count {
        wasmtime::bail!("{name}: field-name count {name_count} != field-value count {value_count}");
    }

    let mut entries = Vec::new();
    entries
        .try_reserve_exact(name_count as usize)
        .map_err(crate::runtime::host::fatal_host_error)?;
    for i in 0..name_count {
        let field_name = names.get(&mut *caller, i)?;
        let value = values.get(&mut *caller, i)?;
        if !super::object::field_is_present(caller, &field_name, &value)?
            || super::object::is_accessor_slot(caller, &field_name)?
        {
            continue;
        }
        let field_name = read_string_units(caller, &field_name, name)?;
        entries.push((field_name, value));
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// $boxed_number / $boxed_boolean vtables
// ---------------------------------------------------------------------------
//
// Boxed primitives carry an unboxed `f64` / `i32` in field 1. The slot
// algorithms mirror the Wasm bodies they replace (`prelude::boxed`): `equals`
// is bit-faithful `f64`/`i32` equality (so `NaN` is unequal and `+0`/`-0` are
// equal), and `hash` XOR-folds the `f64` bit pattern (distinguishing `±0`).

fn build_boxed_number_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let n = read_boxed_f64(&mut *caller, abi_arg(params, 0)?, "Number#toString")?;
                let st = write_submilli_string_struct(&mut *caller, &format_number_js(n))?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let n = read_boxed_f64(&mut *caller, abi_arg(params, 0)?, "Number#toJson")?;
                // NaN and ±Infinity are not valid JSON; both render as `null`.
                let out = if n.is_finite() {
                    format_number_js(n)
                } else {
                    "null".to_string()
                };
                let st = write_submilli_string_struct(&mut *caller, &out)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let boxed_number = intr.boxed_number.clone();
    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        move |caller, params, results| {
            let boxed_number = boxed_number.clone();
            Box::new(async move {
                let eq = boxed_number_equals(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    &boxed_number,
                )?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let n = read_boxed_f64(&mut *caller, abi_arg(params, 0)?, "Number#hash")?;
                *abi_result(results, 0)? = Val::I32(boxed_number_hash(n) as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

fn build_boxed_boolean_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move { boxed_boolean_spell(caller, params, results) })
        },
    );
    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        |caller, params, results| {
            Box::new(async move { boxed_boolean_spell(caller, params, results) })
        },
    );

    let boxed_boolean = intr.boxed_boolean.clone();
    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        move |caller, params, results| {
            let boxed_boolean = boxed_boolean.clone();
            Box::new(async move {
                let eq = boxed_boolean_equals(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    &boxed_boolean,
                )?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let v = read_boxed_i32(&mut *caller, abi_arg(params, 0)?, "Boolean#hash")?;
                *abi_result(results, 0)? = Val::I32(v);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

/// Both boolean slots spell the value (`toString` == `toJson` for a boolean).
fn boxed_boolean_spell(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
) -> wasmtime::Result<()> {
    let truthy = read_boxed_i32(&mut *caller, abi_arg(params, 0)?, "Boolean#toString")? != 0;
    let st = write_submilli_string_struct(&mut *caller, if truthy { "true" } else { "false" })?;
    *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
    Ok(())
}

// ---------------------------------------------------------------------------
// $bigint vtable
// ---------------------------------------------------------------------------
//
// `$bigint` carries `sign` (i32, field 1) and little-endian u64 `limbs`
// (`$rawBigInt`, field 2). The host stores a canonical num_bigint representation
// (sign ∈ {−1,0,1}, minimal limbs, empty for zero), so structural equality of
// sign and limbs is exactly value equality. The slot algorithms mirror the Wasm
// bodies they replace (`prelude::bigint`): `hash` XOR-folds the sign with each
// limb's low/high halves byte-for-byte, so a host-built bigint hashes identically
// to a guest-built one.

fn build_bigint_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                *abi_result(results, 0)? =
                    bigint_decimal_string(&mut *caller, abi_arg(params, 0)?, "BigInt#toString")?;
                Ok(())
            })
        },
    );

    // BigInt JSON has no native literal, so toJson renders the decimal text (== toString).
    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                *abi_result(results, 0)? =
                    bigint_decimal_string(&mut *caller, abi_arg(params, 0)?, "BigInt#toJson")?;
                Ok(())
            })
        },
    );

    let bigint_ty = intr.bigint.clone();
    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        move |caller, params, results| {
            let bigint_ty = bigint_ty.clone();
            Box::new(async move {
                let eq = bigint_equals(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    &bigint_ty,
                )?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let (sign, limbs) = crate::runtime::prelude::bigint::ops::read_bigint_struct(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    "BigInt#hash",
                )?;
                *abi_result(results, 0)? = Val::I32(bigint_hash(sign, &limbs) as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

/// Format a `$bigint` receiver as its canonical base-10 string.
fn bigint_decimal_string(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Val> {
    let (sign, limbs) =
        crate::runtime::prelude::bigint::ops::read_bigint_struct(caller, val, name)?;
    let value = crate::runtime::prelude::bigint::ops::limbs_to_bigint(sign, &limbs)?;
    let text = crate::runtime::prelude::bigint::ops::format_bigint(caller, &value, 10)?;
    let st = write_submilli_string_struct(caller, &text)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Value equality. A union value can dispatch this with a non-bigint `other` — a
/// mismatched type is unequal, not an error.
fn bigint_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
    bigint_ty: &StructType,
) -> wasmtime::Result<bool> {
    let Some(other_st) = as_opt_struct(caller, other)? else {
        return Ok(false);
    };
    if !StructType::eq(&other_st.ty(&caller)?, bigint_ty) {
        return Ok(false);
    }
    let (recv_sign, recv_limbs) =
        crate::runtime::prelude::bigint::ops::read_bigint_struct(caller, recv, "BigInt#equals")?;
    let (other_sign, other_limbs) =
        crate::runtime::prelude::bigint::ops::read_bigint_struct(caller, other, "BigInt#equals")?;
    Ok(recv_sign == other_sign && recv_limbs == other_limbs)
}

// ---------------------------------------------------------------------------
// $Uint8Array vtable
// ---------------------------------------------------------------------------
//
// A `$Uint8Array` carries a packed `$rawUint8Array` (field 1). The slot
// algorithms mirror the Wasm bodies they replace (`prelude::uint8array`):
// `toString` comma-joins the bytes' decimals, `toJson` wraps the standard
// (padded) base64 in quotes, `hash` is FNV-1a-32 over the raw unsigned bytes —
// byte-for-byte, so a host-built array hashes identically to a guest-built one.

fn build_uint8array_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let bytes =
                    read_uint8_array_arg(&mut *caller, abi_arg(params, 0)?, "Uint8Array#toString")?;
                let units = crate::runtime::prelude::uint8array::join(&bytes, &[u16::from(b',')]);
                let st = write_submilli_string_struct_units(&mut *caller, &units)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let bytes =
                    read_uint8_array_arg(&mut *caller, abi_arg(params, 0)?, "Uint8Array#toJson")?;
                let text = format!(
                    "\"{}\"",
                    crate::runtime::prelude::uint8array::to_base64_standard(&bytes)
                );
                let st = write_submilli_string_struct(&mut *caller, &text)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let uint8_ty = intr.uint8_array.clone();
    let equals = host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        move |caller, params, results| {
            let uint8_ty = uint8_ty.clone();
            Box::new(async move {
                let eq = uint8array_equals(
                    &mut *caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    &uint8_ty,
                )?;
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    );

    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let bytes =
                    read_uint8_array_arg(&mut *caller, abi_arg(params, 0)?, "Uint8Array#hash")?;
                *abi_result(results, 0)? = Val::I32(uint8array_hash(&bytes) as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

/// Structural byte equality. Dispatched off a union value, `other` may be a
/// non-`Uint8Array` — a mismatched type is unequal, not an error.
fn uint8array_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
    uint8_ty: &StructType,
) -> wasmtime::Result<bool> {
    let Some(other_st) = as_opt_struct(caller, other)? else {
        return Ok(false);
    };
    if !StructType::eq(&other_st.ty(&caller)?, uint8_ty) {
        return Ok(false);
    }
    let a = read_uint8_array_arg(caller, recv, "Uint8Array#equals")?;
    let b = read_uint8_array_arg(caller, other, "Uint8Array#equals")?;
    Ok(a == b)
}

// ---------------------------------------------------------------------------
// $closure / $regex / $RegExpMatchBox vtables
// ---------------------------------------------------------------------------
//
// These mirror the Wasm vtables they replace. Closures (host-built iterator
// closures) render `[object Object]` for toString and `null` for toJson;
// `equals` is reference identity, `hash` is 0. The regex
// types add a real `toString` (`/source/flags`, and a match box renders its
// match text) over the same stub `toJson` (`{}`) / `equals` / `hash`.

fn build_closure_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = object_object_slot(store, intr.to_string_fn.clone());
    let to_json = host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        |caller, _, results| {
            Box::new(async move {
                let value = write_submilli_string_struct(&mut *caller, "null")?;
                *abi_result(results, 0)? = Val::AnyRef(Some(value.to_anyref()));
                Ok(())
            })
        },
    );
    let equals = ref_identity_equals_slot(store, intr);
    let hash = host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let object = as_struct(caller, &params[0], "Closure#hash")?;
                if matches!(object.field(&mut *caller, 3)?, Val::I64(0)) {
                    let original = super::closure::original(caller, params[0])?;
                    identity_hash(caller, &original)?;
                    let original = as_struct(caller, &original, "Closure#hash original")?;
                    let id = original.field(&mut *caller, 3)?;
                    object.set_field(&mut *caller, 3, id)?;
                }
                results[0] = Val::I32(identity_hash(caller, &params[0])? as i32);
                Ok(())
            })
        },
    );
    Ok([to_string, to_json, equals, hash])
}

fn build_regex_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                *abi_result(results, 0)? = regex_to_string(&mut *caller, abi_arg(params, 0)?)?;
                Ok(())
            })
        },
    );
    let to_json = empty_object_json_slot(store, intr);
    let equals = ref_identity_equals_slot(store, intr);
    let hash = identity_hash_slot(store, intr);
    Ok([to_string, to_json, equals, hash])
}

fn build_regex_match_box_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_vtable_func(
        &mut *store,
        intr.to_string_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                // The match text ($string) sits in field 1 — hand it back as-is.
                let st = as_struct(&mut *caller, abi_arg(params, 0)?, "RegExpMatch#toString")?;
                *abi_result(results, 0)? = st.field(&mut *caller, 1)?;
                Ok(())
            })
        },
    );
    let to_json = empty_object_json_slot(store, intr);
    let equals = ref_identity_equals_slot(store, intr);
    let hash = identity_hash_slot(store, intr);
    Ok([to_string, to_json, equals, hash])
}

/// The identity vtable for host-only backing structs (stdlib `URL`,
/// `Response`, fs `Stat`, …): `[object Object]` / `{}` string slots over
/// reference-identity equality and hash 0.
fn build_opaque_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = object_object_slot(store, intr.to_string_fn.clone());
    let to_json = empty_object_json_slot(store, intr);
    let equals = ref_identity_equals_slot(store, intr);
    let hash = zero_hash_slot(store, intr);
    Ok([to_string, to_json, equals, hash])
}

/// `/source/flags` from a `$regex` receiver (source = field 3, flags = field 4).
fn regex_to_string(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    let st = as_struct(caller, recv, "RegExp#toString")?;
    let source = st.field(&mut *caller, 3)?;
    let flags = st.field(&mut *caller, 4)?;
    let mut out = vec![u16::from(b'/')];
    out.extend(read_string_units(
        caller,
        &source,
        "RegExp#toString source",
    )?);
    out.push(u16::from(b'/'));
    out.extend(read_string_units(caller, &flags, "RegExp#toString flags")?);
    let st = write_submilli_string_struct_units(caller, &out)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Slot returning the literal `[object Object]`.
fn object_object_slot(store: &mut Store<StoreData>, ty: wasmtime::FuncType) -> Func {
    host_vtable_func(&mut *store, ty, |caller, _params, results| {
        Box::new(async move {
            let st = write_submilli_string_struct(&mut *caller, "[object Object]")?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        })
    })
}

/// `toJson` slot returning the literal `{}`.
fn empty_object_json_slot(store: &mut Store<StoreData>, intr: &IntrinsicTypes) -> Func {
    host_vtable_func(
        &mut *store,
        intr.to_json_fn.clone(),
        |caller, _params, results| {
            Box::new(async move {
                let st = write_submilli_string_struct(&mut *caller, "{}")?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    )
}

/// `equals` slot with `ref.eq` semantics (null == null, otherwise identity).
fn ref_identity_equals_slot(store: &mut Store<StoreData>, intr: &IntrinsicTypes) -> Func {
    host_vtable_func(
        &mut *store,
        intr.equals_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                let eq = match (abi_arg(params, 0)?, abi_arg(params, 1)?) {
                    (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) => Rooted::ref_eq(&caller, a, b)?,
                    (Val::AnyRef(None), Val::AnyRef(None)) => true,
                    _ => false,
                };
                *abi_result(results, 0)? = Val::I32(eq as i32);
                Ok(())
            })
        },
    )
}

/// Fallback hash for opaque host values without identity metadata.
fn zero_hash_slot(store: &mut Store<StoreData>, intr: &IntrinsicTypes) -> Func {
    host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |_caller, _params, results| {
            Box::new(async move {
                *abi_result(results, 0)? = Val::I32(0);
                Ok(())
            })
        },
    )
}

fn identity_hash_slot(store: &mut Store<StoreData>, intr: &IntrinsicTypes) -> Func {
    host_vtable_func(
        &mut *store,
        intr.hash_fn.clone(),
        |caller, params, results| {
            Box::new(async move {
                results[0] = Val::I32(identity_hash(caller, &params[0])? as i32);
                Ok(())
            })
        },
    )
}

/// Identity-equal builtins reserve their last field for a store-local hash ID.
pub(super) fn identity_hash(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<u32> {
    let object = as_struct(caller, value, "identity hash")?;
    let field = object
        .ty(&*caller)?
        .fields()
        .count()
        .checked_sub(1)
        .ok_or_else(|| {
            crate::runtime::host::fatal_host_error("Identity object has no hash field")
        })?;
    let Val::I64(mut id) = object.field(&mut *caller, field)? else {
        return Err(crate::runtime::host::fatal_host_error(
            "Invalid identity hash field",
        ));
    };
    if id == 0 {
        let next = caller
            .data()
            .next_identity_hash
            .checked_add(1)
            .ok_or_else(|| crate::runtime::host::fatal_host_error("Identity hash IDs exhausted"))?;
        object.set_field(&mut *caller, field, Val::I64(next as i64))?;
        caller.data_mut().next_identity_hash = next;
        id = next as i64;
    }
    Ok(mix_hash_bits(id as u64))
}

fn read_boxed_f64(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<f64> {
    let st = as_struct(caller, val, name)?;
    match st.field(&mut *caller, 1)? {
        Val::F64(bits) => Ok(f64::from_bits(bits)),
        other => wasmtime::bail!("{name}: field 1 is {other:?}, not f64"),
    }
}

fn read_boxed_i32(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<i32> {
    let st = as_struct(caller, val, name)?;
    match st.field(&mut *caller, 1)? {
        Val::I32(v) => Ok(v),
        other => wasmtime::bail!("{name}: field 1 is {other:?}, not i32"),
    }
}

/// `f64`-faithful equality. A union value can dispatch this with a non-number
/// `other` — a mismatched type is unequal, not an error.
fn boxed_number_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
    boxed_number: &StructType,
) -> wasmtime::Result<bool> {
    let Some(other_st) = as_opt_struct(caller, other)? else {
        return Ok(false);
    };
    if !StructType::eq(&other_st.ty(&caller)?, boxed_number) {
        return Ok(false);
    }
    Ok(read_boxed_f64(caller, recv, "Number#equals")?
        == read_boxed_f64(caller, other, "Number#equals")?)
}

fn boxed_boolean_equals(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
    boxed_boolean: &StructType,
) -> wasmtime::Result<bool> {
    let Some(other_st) = as_opt_struct(caller, other)? else {
        return Ok(false);
    };
    if !StructType::eq(&other_st.ty(&caller)?, boxed_boolean) {
        return Ok(false);
    }
    Ok(read_boxed_i32(caller, recv, "Boolean#equals")?
        == read_boxed_i32(caller, other, "Boolean#equals")?)
}

// ---------------------------------------------------------------------------
// pure slot algorithms (unit-tested without a store)
// ---------------------------------------------------------------------------

/// Mix exponent and mantissa into the low bits used by power-of-two tables.
fn boxed_number_hash(n: f64) -> u32 {
    // Equality treats signed zeros alike, so their hashes must agree.
    let bits = if n == 0.0 { 0 } else { n.to_bits() };
    mix_hash_bits(bits)
}

pub(super) fn mix_hash_bits(bits: u64) -> u32 {
    let mut mixed = bits.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^= mixed >> 31;
    mixed as u32 ^ (mixed >> 32) as u32
}

/// XOR-fold a bigint's sign with each limb's low/high 32-bit halves. Mirrors
/// `prelude::bigint::hash_body`; must agree with `bigint_equals` so equal bigints
/// (canonical sign + limbs) hash alike.
fn bigint_hash(sign: i32, limbs: &[u64]) -> u32 {
    let mut acc = sign as u32;
    for &limb in limbs {
        acc ^= (limb as u32) ^ ((limb >> 32) as u32);
    }
    acc
}

/// FNV-1a-32 over raw unsigned bytes — the exact fold of
/// `prelude::uint8array::hash_body`, so a host-built `Uint8Array` hashes
/// identically to a guest-built one.
fn uint8array_hash(bytes: &[u8]) -> u32 {
    let mut hash = FNV_OFFSET;
    for &b in bytes {
        hash = (hash ^ u32::from(b)).wrapping_mul(FNV_PRIME);
    }
    hash
}

/// FNV-1a-32 over UTF-16 code units, low byte then high byte per unit — the exact
/// order of `prelude::string::vtable_hash_body`.
pub(super) fn fnv_hash_units(units: &[u16]) -> u32 {
    hash_utf16_units(units.iter().copied())
}

pub(crate) fn hash_utf16_units(units: impl IntoIterator<Item = u16>) -> u32 {
    let mut hash = FNV_OFFSET;
    for unit in units {
        hash = (hash ^ u32::from(unit & 0xff)).wrapping_mul(FNV_PRIME);
        hash = (hash ^ u32::from(unit >> 8)).wrapping_mul(FNV_PRIME);
    }
    hash
}

/// One array-hash step: `(acc ^ element_hash) * FNV_PRIME`.
fn fnv_combine(acc: u32, element_hash: u32) -> u32 {
    (acc ^ element_hash).wrapping_mul(FNV_PRIME)
}

/// JSON-escape UTF-16 code units into a quoted `"…"` unit sequence — the exact
/// escapes for controls, quotes, backslashes, and unpaired surrogates.
/// Valid surrogate pairs stay intact; lone halves use well-formed JSON escapes.
#[cfg(test)]
pub(super) fn json_escape_units(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    out.push(u16::from(b'"'));
    for (index, &unit) in units.iter().enumerate() {
        let (escaped, len) = escaped_json_unit(units, index, unit);
        out.extend(escaped.into_iter().take(len));
    }
    out.push(u16::from(b'"'));
    out
}

pub(super) fn escaped_json_unit(units: &[u16], index: usize, unit: u16) -> ([u16; 6], usize) {
    let escaped_letter = match unit {
        0x08 => Some(b'b'),
        0x09 => Some(b't'),
        0x0a => Some(b'n'),
        0x0c => Some(b'f'),
        0x0d => Some(b'r'),
        0x22 => Some(b'"'),
        0x5c => Some(b'\\'),
        _ => None,
    };
    if let Some(letter) = escaped_letter {
        return ([92, u16::from(letter), 0, 0, 0, 0], 2);
    }
    if unit < 0x20 || is_unpaired_surrogate(units, index, unit) {
        return (
            [
                92,
                117,
                hex_nibble((unit >> 12) & 15),
                hex_nibble((unit >> 8) & 15),
                hex_nibble((unit >> 4) & 15),
                hex_nibble(unit & 15),
            ],
            6,
        );
    }
    ([unit, 0, 0, 0, 0, 0], 1)
}

fn is_unpaired_surrogate(units: &[u16], index: usize, unit: u16) -> bool {
    match unit {
        0xd800..=0xdbff => !index
            .checked_add(1)
            .and_then(|next| units.get(next))
            .is_some_and(|next| (0xdc00..=0xdfff).contains(next)),
        0xdc00..=0xdfff => !index
            .checked_sub(1)
            .and_then(|previous| units.get(previous))
            .is_some_and(|previous| (0xd800..=0xdbff).contains(previous)),
        _ => false,
    }
}

fn hex_nibble(nibble: u16) -> u16 {
    if nibble < 10 {
        u16::from(b'0') + nibble
    } else {
        u16::from(b'a') + (nibble - 10)
    }
}

// ---------------------------------------------------------------------------
// shared marshalling helpers
// ---------------------------------------------------------------------------

/// Which collection backing `val` is, if it is one. They are `$Object`
/// subtypes sharing the object vtable, so the universal slots reach them but
/// their field layout is not `$ObjectShape`'s.
#[derive(Clone, Copy)]
enum CollectionBacking {
    Map,
    Set,
}

impl std::fmt::Display for CollectionBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CollectionBacking::Map => "Map",
            CollectionBacking::Set => "Set",
        })
    }
}

fn collection_backing_kind(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Option<CollectionBacking>> {
    let (map, set) = {
        let abi = caller
            .data()
            .host_abi
            .as_ref()
            .ok_or_else(|| wasmtime::Error::msg("host ABI not installed"))?;
        (abi.map_backing_type.clone(), abi.set_backing_type.clone())
    };
    if super::collection::is_a(caller, val, &map)? {
        return Ok(Some(CollectionBacking::Map));
    }
    if super::collection::is_a(caller, val, &set)? {
        return Ok(Some(CollectionBacking::Set));
    }
    Ok(None)
}

/// [`collection_backing_kind`] as the yes/no question the `equals` and `hash`
/// slots actually ask.
fn is_collection_backing(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<bool> {
    Ok(collection_backing_kind(caller, val)?.is_some())
}

pub(super) fn as_struct(
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

fn as_opt_struct(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Option<Rooted<StructRef>>> {
    match val {
        Val::AnyRef(Some(any)) => any.as_struct(&mut *caller),
        _ => Ok(None),
    }
}

/// Read a `$string`/`$Object` struct's field-1 `(array i16)` payload into code
/// units.
fn read_struct_units(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    name: &str,
) -> wasmtime::Result<Vec<u16>> {
    let payload = string_payload(caller, st, name)?;
    read_code_units(&mut *caller, payload, name)
}

/// A `$string` value's length in code units, without copying them.
pub(crate) fn string_length(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<usize> {
    let st = as_struct(caller, val, name)?;
    let payload = string_payload(caller, &st, name)?;
    usize::try_from(payload.len(&mut *caller)?).map_err(fatal_host_error)
}

/// A `$string` struct's field-1 `$rawString` payload.
fn string_payload(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    name: &str,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller),
        other => wasmtime::bail!("{name}: malformed payload {other:?}"),
    }
}

/// A `$string` value's code units. `name` labels a malformed value in the error.
pub(crate) fn read_string_units(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u16>> {
    let st = as_struct(caller, val, name)?;
    read_struct_units(caller, &st, name)
}

/// Build a `$string` from code units, reusing `vtable` (the shared `$string`
/// vtable singleton).
fn build_string(
    caller: &mut Caller<'_, StoreData>,
    raw_string: &ArrayType,
    string_ty: &StructType,
    vtable: Val,
    units: &[u16],
) -> wasmtime::Result<Val> {
    fuel::charge(&mut *caller, fuel::COPY, units.len() as u64)?;
    let pre = ArrayRefPre::new(&mut *caller, raw_string.clone());
    let raw = ArrayRef::new_from_i16_slice(&mut *caller, &pre, units)?;
    let pre = StructRefPre::new(&mut *caller, string_ty.clone());
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::AnyRef(Some(raw.to_anyref())), Val::I64(0)],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Read an `$Array`'s field-1 `$rawArray` backing into its `(ref null $Object)`
/// element values.
fn read_array_backing(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    _name: &str,
) -> wasmtime::Result<Vec<Val>> {
    crate::runtime::array_storage::ArrayStorage::read(caller, val)?.snapshot(caller)
}

/// Guest structural bodies share the host walk budget and unwind it on throws.
pub(crate) fn install_walk_guards(
    linker: &mut wasmtime::Linker<StoreData>,
) -> wasmtime::Result<()> {
    let ty = wasmtime::FuncType::new(linker.engine(), [], []);
    linker.func_new(
        super::MODULE_NAME,
        crate::mangle::prelude("vtable_walk_enter").as_str(),
        ty.clone(),
        |mut caller, _, _| enter_walk(&mut caller),
    )?;
    linker.func_new(
        super::MODULE_NAME,
        crate::mangle::prelude("vtable_walk_leave").as_str(),
        ty,
        |mut caller, _, _| {
            leave_walk(&mut caller);
            Ok(())
        },
    )?;
    Ok(())
}

pub(crate) fn declare_walk_guards(defs: &mut crate::PackageDeclaration) {
    for name in ["vtable_walk_enter", "vtable_walk_leave"] {
        super::declare_method(
            defs,
            name,
            crate::mangle::prelude(name),
            vec![],
            crate::Type::Void,
        );
    }
}

fn is_function(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<bool> {
    let closure = intrinsic_types(&mut *caller)?.closure.clone();
    super::collection::is_a(caller, value, &closure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_hash_matches_reference() {
        // FNV-1a-32 of "" is the offset basis.
        assert_eq!(fnv_hash_units(&[]), FNV_OFFSET);
        // "a" = 0x61: low byte 0x61 mixed in, then high byte 0x00 (a no-op xor,
        // still a prime multiply).
        let h = (FNV_OFFSET ^ 0x61)
            .wrapping_mul(FNV_PRIME)
            .wrapping_mul(FNV_PRIME);
        assert_eq!(fnv_hash_units(&[0x61]), h);
    }

    #[test]
    fn boxed_number_hash_distributes_small_integers_and_agrees_for_zero() {
        assert_eq!(boxed_number_hash(0.0), boxed_number_hash(-0.0));
        let buckets: std::collections::BTreeSet<_> = (0..256)
            .map(|n| boxed_number_hash(f64::from(n)) & 511)
            .collect();
        // The former bit fold put every one of these values in bucket zero.
        assert!(buckets.len() > 180, "{} distinct buckets", buckets.len());
    }

    #[test]
    fn bigint_hash_folds_sign_and_limbs() {
        // Zero is canonical sign 0 with no limbs → hash 0, matching the Wasm fold.
        assert_eq!(bigint_hash(0, &[]), 0);
        // Each limb XOR-folds its low/high 32 bits onto the accumulator (seeded
        // with the sign), exactly like `prelude::bigint::hash_body`.
        let limb = 0x0123_4567_89ab_cdef_u64;
        let expected = 1u32 ^ (limb as u32) ^ ((limb >> 32) as u32);
        assert_eq!(bigint_hash(1, &[limb]), expected);
        // Equal magnitudes with opposite signs hash differently (sign seeds the acc).
        assert_ne!(bigint_hash(1, &[7]), bigint_hash(-1, &[7]));
    }

    #[test]
    fn uint8array_hash_folds_bytes_fnv1a() {
        // Empty array hashes to the offset basis.
        assert_eq!(uint8array_hash(&[]), FNV_OFFSET);
        // Each byte mixes in low-byte-only (no high byte, unlike the UTF-16 fold).
        let expected = [0x01u8, 0x02, 0xff].iter().fold(FNV_OFFSET, |h, &b| {
            (h ^ u32::from(b)).wrapping_mul(FNV_PRIME)
        });
        assert_eq!(uint8array_hash(&[0x01, 0x02, 0xff]), expected);
    }

    #[test]
    fn json_escape_passes_through_and_quotes() {
        // Plain ASCII is wrapped in quotes, nothing else.
        assert_eq!(
            json_escape_units(&"hi".encode_utf16().collect::<Vec<_>>()),
            "\"hi\"".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn json_escape_named_control_and_quote_and_backslash() {
        // 0x0A -> \n, 0x22 (") -> \" , 0x5C (\) -> \\ , 0x1F -> .
        let input = [u16::from(b'a'), 0x0A, 0x22, 0x5C, 0x1F];
        let got = String::from_utf16(&json_escape_units(&input)).unwrap();
        let mut want = String::from("\"a");
        want.push_str("\\n");
        want.push_str("\\\"");
        want.push_str("\\\\");
        want.push_str("\\u001f");
        want.push('"');
        assert_eq!(got, want);
    }
}
