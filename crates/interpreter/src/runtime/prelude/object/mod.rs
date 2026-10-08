//! The Rust port of the prelude's `ObjectConstructor` statics —
//! `Object.keys`/`values`/`entries`/`hasOwn`/`is` — reading the `$ObjectShape`
//! field-name/field-value arrays directly. Every per-arity object subtype
//! subtypes `$ObjectShape`, so one `matches_ty` test covers them all; any other
//! value takes the "no fields" path (`[]`/`false`), and a null receiver throws
//! a catchable `Error`, matching the Wasm bodies in
//! `codegen/prelude/object_shape.rs`.

use crate::runtime::host::{abi_arg, abi_result};
mod dynamic;
mod index;
mod static_value;

use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, FuncType, HeapType, Linker, RefType, Rooted, StructRef,
    StructRefPre, StructType, Val, ValType,
};

use crate::MangledName;
use crate::runtime::StoreData;
use crate::runtime::host::{
    host_object_vtable, intrinsic_array_type, intrinsic_string_type, register_host_fn,
    register_host_fn_async, write_submilli_array_struct,
};
use crate::runtime::intrinsic_types::{build_intrinsic_types, intrinsic_types};
use crate::runtime::prelude::collection::{FIELD_NAME, is_a};
use crate::runtime::prelude::iterator::as_struct;
use crate::runtime::prelude::vtable::{dispatch_vtable_slot, read_string_units};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{PackageDeclaration, Param, Type};

fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("ObjectConstructor"), method)
}

#[derive(Clone, Copy)]
enum Enumerate {
    Keys,
    Values,
    Entries,
}

impl Enumerate {
    fn method(self) -> &'static str {
        match self {
            Enumerate::Keys => "keys",
            Enumerate::Values => "values",
            Enumerate::Entries => "entries",
        }
    }
}

pub(super) fn find_data_slot(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    key: &[u16],
) -> wasmtime::Result<Option<u32>> {
    let object = as_struct(caller, object, "object equality receiver")?;
    index::lookup(caller, &object, key, false, false)
}

pub(super) fn find_field_slot(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
    key: &[u16],
    accessor: bool,
) -> wasmtime::Result<Option<u32>> {
    index::lookup(caller, object, key, accessor, false)
}

pub(crate) fn data_field_count(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
) -> wasmtime::Result<usize> {
    let Some((names, values)) = shape_arrays(caller, object)? else {
        return Ok(0);
    };
    let count = field_count(caller, object)?;
    let mut present = 0;
    for slot in 0..count {
        let name = names.get(&mut *caller, slot)?;
        let value = values.get(&mut *caller, slot)?;
        if field_is_present(caller, &name, &value)? && !is_accessor_slot(caller, &name)? {
            present += 1;
        }
    }
    Ok(present)
}

pub(crate) fn field_count(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
) -> wasmtime::Result<u32> {
    let object = as_struct(caller, object, "object field count")?;
    index::len(caller, &object)
}

/// The `$ObjectShape` field arrays `(field_names, object_fields)` of `obj`, or
/// `None` when `obj` is any other (non-null) value — the "no fields" path.
fn shape_arrays(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
) -> wasmtime::Result<Option<(Rooted<ArrayRef>, Rooted<ArrayRef>)>> {
    let Val::AnyRef(Some(any)) = obj else {
        return Ok(None);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let shape = intrinsic_types(&mut *caller)?.object_shape.clone();
    if !st.matches_ty(&*caller, &shape)? {
        return Ok(None);
    }
    let names = field_array(caller, &st, 1)?;
    let values = field_array(caller, &st, 2)?;
    Ok(Some((names, values)))
}

pub(super) fn field_array(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    match st.field(&mut *caller, idx)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "$ObjectShape field {idx} is not an array: {other:?}"
        ))),
    }
}

/// A nullable optional slot has a separate presence flag on its field name.
pub(crate) fn field_is_present(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
    value: &Val,
) -> wasmtime::Result<bool> {
    if !matches!(value, Val::AnyRef(None)) {
        return Ok(true);
    }
    let name = as_struct(caller, name, "field name")?;
    let string = intrinsic_types(&mut *caller)?.string.clone();
    if StructType::eq(&name.ty(&*caller)?, &string) {
        return Ok(true);
    }
    Ok(matches!(name.field(&mut *caller, 3)?, Val::I32(value) if value != 0))
}

fn enumerate(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    kind: Enumerate,
) -> wasmtime::Result<Val> {
    if matches!(obj, Val::AnyRef(None)) {
        return Err(wasmtime::Error::msg(format!(
            "Object.{} called on null",
            kind.method()
        )));
    }
    let mut elems = Vec::new();
    if let Some((names, values)) = shape_arrays(caller, obj)? {
        let len = field_count(caller, obj)?;
        elems.reserve(len as usize);
        let error_slots = super::error::non_enumerable_slots(caller, obj)?;
        for i in 0..len {
            if error_slots.as_ref().is_some_and(|hidden| hidden.hides(i)) {
                continue;
            }
            let name = names.get(&mut *caller, i)?;
            let value = values.get(&mut *caller, i)?;
            if !field_is_present(caller, &name, &value)? || is_accessor_slot(caller, &name)? {
                continue;
            }
            let elem = match kind {
                Enumerate::Keys => names.get(&mut *caller, i)?,
                Enumerate::Values => values.get(&mut *caller, i)?,
                Enumerate::Entries => {
                    let name = names.get(&mut *caller, i)?;
                    let value = values.get(&mut *caller, i)?;
                    let pair = write_submilli_array_struct(caller, &[name, value])?;
                    Val::AnyRef(Some(pair.to_anyref()))
                }
            };
            elems.push(elem);
        }
    }
    let arr = write_submilli_array_struct(caller, &elems)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

/// A marked name identifies an internal accessor slot independently of its
/// spelling or current value; user data named `get x` remains ordinary data.
pub(crate) fn is_accessor_slot(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
) -> wasmtime::Result<bool> {
    let name = as_struct(caller, name, "field name")?;
    let string = intrinsic_types(&mut *caller)?.string.clone();
    if StructType::eq(&name.ty(&*caller)?, &string) {
        return Ok(false);
    }
    Ok(matches!(name.field(&mut *caller, 3)?, Val::I32(-1)))
}

/// Visibility is carried only by compiler-created marked names. Host-created
/// names predate that metadata and describe public data properties.
pub(crate) fn field_is_private(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
) -> wasmtime::Result<bool> {
    let name = as_struct(caller, name, "field name")?;
    if name.ty(&*caller)?.fields().count() < 5 {
        return Ok(false);
    }
    Ok(matches!(name.field(&mut *caller, 4)?, Val::I32(1)))
}

/// Copy present own fields while preserving UTF-16 names and boxed values.
/// Each call snapshots its source before the next literal member is evaluated.
fn spread(
    caller: &mut Caller<'_, StoreData>,
    target: &Val,
    source: &Val,
    shape: &Val,
    mask: &Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let mut entries = std::collections::BTreeMap::new();
    let omitted = spread_omitted_fields(caller, mask)?;
    for (source_index, object) in [target, source].into_iter().enumerate() {
        let Some((names, values)) = shape_arrays(caller, object)? else {
            continue;
        };
        for index in 0..field_count(caller, object)? {
            let name = names.get(&mut *caller, index)?;
            let value = values.get(&mut *caller, index)?;
            if !field_is_present(caller, &name, &value)? || is_accessor_slot(caller, &name)? {
                continue;
            }
            let units = read_string_units(caller, &name, FIELD_NAME)?;
            if source_index == 1 && omitted.contains(&units) {
                continue;
            }
            entries.insert(units, (copy_field_name(caller, name, true)?, value));
        }
    }
    if let Some((names, _)) = shape_arrays(caller, shape)? {
        for index in 0..field_count(caller, shape)? {
            let name = names.get(&mut *caller, index)?;
            let units = read_string_units(caller, &name, FIELD_NAME)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = entries.entry(units) {
                entry.insert((copy_field_name(caller, name, false)?, Val::AnyRef(None)));
            }
        }
    }
    let (names, values): (Vec<_>, Vec<_>) = entries.into_values().unzip();
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let values_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let shape_pre = StructRefPre::new(&mut *caller, intr.object_shape.clone());
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &names)?;
    let values = ArrayRef::new_fixed(&mut *caller, &values_pre, &values)?;
    let vtable = host_object_vtable(caller)?;
    let object = StructRef::new(
        &mut *caller,
        &shape_pre,
        &[
            vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(values.to_anyref())),
            Val::AnyRef(None),
        ],
    )?;
    Ok(Val::AnyRef(Some(object.to_anyref())))
}

fn copy_field_name(
    caller: &mut Caller<'_, StoreData>,
    name: Val,
    present: bool,
) -> wasmtime::Result<Val> {
    let object = as_struct(caller, &name, "field name")?;
    let string = intrinsic_types(&mut *caller)?.string.clone();
    if StructType::eq(&object.ty(&*caller)?, &string) {
        return Ok(name);
    }
    let ty = if present {
        string
    } else {
        object.ty(&*caller)?
    };
    let mut fields = vec![
        object.field(&mut *caller, 0)?,
        object.field(&mut *caller, 1)?,
        object.field(&mut *caller, 2)?,
    ];
    if !present {
        fields.push(Val::I32(0));
        if ty.fields().count() > 4 {
            fields.push(object.field(&mut *caller, 4)?);
        }
    }
    let pre = StructRefPre::new(&mut *caller, ty);
    Ok(Val::AnyRef(Some(
        StructRef::new(&mut *caller, &pre, &fields)?.to_anyref(),
    )))
}

/// Append without replacing the receiver, so every alias sees the new slot.
/// Existing indices (including class payload and guard slots) stay unchanged.
fn insert_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &Val,
    value: &Val,
) -> wasmtime::Result<()> {
    let object = as_struct(caller, obj, "field insertion receiver")?;
    let old_names = field_array(caller, &object, 1)?;
    let old_values = field_array(caller, &object, 2)?;
    let count = index::len(caller, &object)?;
    let capacity = old_names.len(&mut *caller)?;
    let new_count = count
        .checked_add(1)
        .filter(|n| *n <= i32::MAX as u32)
        .ok_or_else(|| crate::runtime::host::range_error("object field count limit exceeded"))?;
    let name = inserted_field_name(caller, name)?;
    let (names, values, table) = if count == capacity {
        let (names, values) = grow_fields(caller, &old_names, &old_values, &name)?;
        let new_capacity = names.len(&mut *caller)?;
        let table = index::build(caller, &names, count, new_capacity)?;
        (names, values, table)
    } else {
        let table = match index::cached(caller, &object)? {
            Some(table) => table,
            None => index::build(caller, &old_names, count, capacity)?,
        };
        (old_names, old_values, table)
    };
    let bucket = index::empty_bucket(caller, &table, &name)?;
    // All fuel and allocation checks precede publication. Guard rows use the
    // backing capacity as their stride, so spare named slots need no reshuffle.
    names.set(&mut *caller, count, name)?;
    values.set(&mut *caller, count, *value)?;
    table.set(&mut *caller, bucket, Val::I32(new_count as i32))?;
    table.set(&mut *caller, 0, Val::I32(new_count as i32))?;
    table.set(&mut *caller, 1, Val::I32(1))?;
    object.set_field(&mut *caller, 1, Val::AnyRef(Some(names.to_anyref())))?;
    object.set_field(&mut *caller, 2, Val::AnyRef(Some(values.to_anyref())))?;
    object.set_field(
        &mut *caller,
        index::INDEX_FIELD,
        Val::AnyRef(Some(table.to_anyref())),
    )?;
    Ok(())
}

fn grow_fields(
    caller: &mut Caller<'_, StoreData>,
    names: &Rooted<ArrayRef>,
    values: &Rooted<ArrayRef>,
    filler: &Val,
) -> wasmtime::Result<(Rooted<ArrayRef>, Rooted<ArrayRef>)> {
    let capacity = names.len(&mut *caller)?;
    let value_len = values.len(&mut *caller)?;
    let width = capacity
        .checked_add(1)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("object guard row width overflow"))?;
    let hidden = value_len
        .checked_sub(capacity)
        .filter(|n| n % width == 0)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("malformed object guard rows"))?;
    let rows = hidden / width;
    let new_capacity = capacity
        .checked_mul(2)
        .map(|n| n.max(8))
        .filter(|n| *n < i32::MAX as u32)
        .ok_or_else(|| crate::runtime::host::range_error("object field capacity limit exceeded"))?;
    let new_width = new_capacity + 1;
    let new_value_len = rows
        .checked_mul(new_width)
        .and_then(|n| n.checked_add(new_capacity))
        .ok_or_else(|| {
            crate::runtime::host::range_error("object payload capacity limit exceeded")
        })?;
    crate::runtime::fuel::charge(
        &mut *caller,
        crate::runtime::fuel::ELEM,
        u64::from(new_capacity)
            + u64::from(new_value_len)
            + u64::from(capacity)
            + u64::from(value_len),
    )?;
    let intr = intrinsic_types(&mut *caller)?;
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let values_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let new_names = ArrayRef::new(&mut *caller, &names_pre, filler, new_capacity)?;
    let new_values = ArrayRef::new(&mut *caller, &values_pre, &Val::AnyRef(None), new_value_len)?;
    for slot in 0..capacity {
        let name = names.get(&mut *caller, slot)?;
        let value = values.get(&mut *caller, slot)?;
        new_names.set(&mut *caller, slot, name)?;
        new_values.set(&mut *caller, slot, value)?;
    }
    for row in 0..rows {
        let old_start = capacity + row * width;
        let new_start = new_capacity + row * new_width;
        for slot in 0..capacity {
            let guard = values.get(&mut *caller, old_start + slot)?;
            new_values.set(&mut *caller, new_start + slot, guard)?;
        }
        let context = values.get(&mut *caller, old_start + capacity)?;
        new_values.set(&mut *caller, new_start + new_capacity, context)?;
    }
    Ok((new_names, new_values))
}

/// A present inserted name also tells typed serializers that the original
/// static shape no longer describes all of this object's fields.
fn inserted_field_name(caller: &mut Caller<'_, StoreData>, name: &Val) -> wasmtime::Result<Val> {
    use wasmtime::{FieldType, Finality, Mutability, StorageType};
    let intr = intrinsic_types(&mut *caller)?;
    let mut fields: Vec<_> = intr.string.fields().collect();
    fields.push(FieldType::new(
        Mutability::Var,
        StorageType::ValType(ValType::I32),
    ));
    let ty = crate::runtime::gc_singleton::singleton_struct(
        caller.engine(),
        Finality::Final,
        Some(intr.string.clone()),
        fields,
    )?;
    let name = as_struct(caller, name, "inserted field name")?;
    let values = [
        name.field(&mut *caller, 0)?,
        name.field(&mut *caller, 1)?,
        name.field(&mut *caller, 2)?,
        Val::I32(2),
    ];
    let pre = StructRefPre::new(&mut *caller, ty);
    Ok(Val::AnyRef(Some(
        StructRef::new(&mut *caller, &pre, &values)?.to_anyref(),
    )))
}

pub(crate) fn field_was_inserted(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
) -> wasmtime::Result<bool> {
    let name = as_struct(caller, name, "field name")?;
    let string = intrinsic_types(&mut *caller)?.string.clone();
    if StructType::eq(&name.ty(&*caller)?, &string) {
        return Ok(false);
    }
    Ok(matches!(name.field(&mut *caller, 3)?, Val::I32(2)))
}

/// The compiler marks rejected known fields with non-null mask slots.
fn spread_omitted_fields(
    caller: &mut Caller<'_, StoreData>,
    mask: &Val,
) -> wasmtime::Result<std::collections::BTreeSet<Vec<u16>>> {
    let mut omitted = std::collections::BTreeSet::new();
    if let Some((names, values)) = shape_arrays(caller, mask)? {
        for index in 0..field_count(caller, mask)? {
            if !matches!(values.get(&mut *caller, index)?, Val::AnyRef(None)) {
                let name = names.get(&mut *caller, index)?;
                omitted.insert(read_string_units(caller, &name, FIELD_NAME)?);
            }
        }
    }
    Ok(omitted)
}

fn has_own(caller: &mut Caller<'_, StoreData>, obj: &Val, key: &Val) -> wasmtime::Result<bool> {
    if matches!(obj, Val::AnyRef(None)) {
        return Err(wasmtime::Error::msg("Object.hasOwn called on null"));
    }
    let Some((names, values)) = shape_arrays(caller, obj)? else {
        return Ok(false);
    };
    let target = read_string_units(caller, key, FIELD_NAME)?;
    let Some(slot) = find_data_slot(caller, obj, &target)? else {
        return Ok(false);
    };
    let name = names.get(&mut *caller, slot)?;
    let value = values.get(&mut *caller, slot)?;
    field_is_present(caller, &name, &value)
}

/// SameValue on two `f64` bit patterns: any NaN equals any NaN (Wasm arithmetic
/// varies sign/payload bits), otherwise exact bit equality — which separates
/// `+0` from `-0` and compares ordinary values exactly.
fn same_value_bits(a: u64, b: u64) -> bool {
    (f64::from_bits(a).is_nan() && f64::from_bits(b).is_nan()) || a == b
}

fn boxed_number_bits(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<u64> {
    let st = as_struct(caller, val, "Object.is operand")?;
    match st.field(&mut *caller, 1)? {
        Val::F64(bits) => Ok(bits),
        other => Err(wasmtime::Error::msg(format!(
            "Object.is: boxed number payload is {other:?}, not f64"
        ))),
    }
}

async fn same_value(
    caller: &mut Caller<'_, StoreData>,
    a: &Val,
    b: &Val,
) -> wasmtime::Result<bool> {
    match (a, b) {
        (Val::AnyRef(None), Val::AnyRef(None)) => return Ok(true),
        (Val::AnyRef(None), _) | (_, Val::AnyRef(None)) => return Ok(false),
        _ => {}
    }
    // The number arm must run before vtable dispatch: the boxed-number `equals`
    // slot is `===` (`0 === -0`, `NaN !== NaN`), while SameValue distinguishes
    // both.
    let boxed_number = intrinsic_types(&mut *caller)?.boxed_number.clone();
    if is_a(caller, a, &boxed_number)? && is_a(caller, b, &boxed_number)? {
        let (a_bits, b_bits) = (boxed_number_bits(caller, a)?, boxed_number_bits(caller, b)?);
        return Ok(same_value_bits(a_bits, b_bits));
    }
    match dispatch_vtable_slot(caller, a, 2, std::slice::from_ref(b)).await? {
        Val::I32(v) => Ok(v != 0),
        other => Err(wasmtime::Error::msg(format!(
            "Object.is: equals returned {other:?}, expected i32"
        ))),
    }
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    dynamic::install(linker)?;
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let obj = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_string_type(&engine)?),
    ));
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_array_type(&engine)?),
    ));
    let boolean = ValType::I32;
    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    for kind in [Enumerate::Keys, Enumerate::Values, Enumerate::Entries] {
        register_host_fn(
            linker,
            MODULE_NAME,
            ctor_key(kind.method()),
            ft(vec![obj.clone()], vec![array.clone()]),
            true,
            move |caller, params, results| {
                *abi_result(results, 0)? = enumerate(caller, abi_arg(params, 0)?, kind)?;
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("hasOwn"),
        ft(vec![obj.clone(), string.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = Val::I32(i32::from(has_own(
                caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
            )?));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("#insertField"),
        ft(vec![obj.clone(), string, obj.clone()], vec![]),
        true,
        |caller, params, _| {
            insert_field(
                caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                abi_arg(params, 2)?,
            )
        },
    )?;

    let shape = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("#spread"),
        ft(
            vec![obj.clone(), obj.clone(), obj.clone(), obj.clone()],
            vec![shape],
        ),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = spread(
                caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                abi_arg(params, 2)?,
                abi_arg(params, 3)?,
            )?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        ctor_key("#toJson"),
        ft(
            vec![obj.clone()],
            vec![ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(intr.string.clone()),
            ))],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let intr = intrinsic_types(&mut *caller)?;
                *abi_result(results, 0)? = super::vtable::object_to_json(
                    caller,
                    abi_arg(params, 0)?,
                    &intr.raw_string,
                    &intr.string,
                )
                .await?;
                Ok(())
            })
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("#staticValue"),
        ft(
            vec![ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(intr.string.clone()),
            ))],
            vec![obj.clone()],
        ),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = static_value::static_value(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        ctor_key("is"),
        ft(vec![obj.clone(), obj], vec![boolean]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = Val::I32(i32::from(
                    same_value(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?,
                ));
                Ok(())
            })
        },
    )
}

pub fn declare(defs: &mut PackageDeclaration) {
    dynamic::declare(defs);
    declare_method(
        defs,
        "#toJson",
        ctor_key("#toJson"),
        vec![Param::new("object", Type::Unknown)],
        Type::String,
    );
    // Compiler-only helper: the value a static-dispatch binding reads as.
    declare_method(
        defs,
        "#staticValue",
        ctor_key("#staticValue"),
        vec![Param::new("tag", Type::String)],
        Type::Unknown,
    );
    declare_method(
        defs,
        "#insertField",
        ctor_key("#insertField"),
        vec![
            Param::new("object", Type::Unknown),
            Param::new("name", Type::String),
            Param::new("value", Type::Unknown),
        ],
        Type::Void,
    );
    // Compiler-only helper: deliberately absent from ObjectConstructor's public surface.
    declare_method(
        defs,
        "#spread",
        ctor_key("#spread"),
        vec![
            Param::new("target", Type::Unknown),
            Param::new("source", Type::Unknown),
            Param::new("shape", Type::Unknown),
            Param::new("mask", Type::Unknown),
        ],
        Type::Object {
            index: None,
            fields: Default::default(),
        },
    );
    // `Dispatch::Static` drops the constructor receiver, so no receiver param.
    // Types mirror the prelude MethodSigs: `Type::Array`/`Type::Tuple` lower to
    // `(ref $Array)`, matching the registered FuncTypes above.
    let obj = || Param::new("obj", Type::Unknown);
    declare_method(
        defs,
        "keys",
        ctor_key("keys"),
        vec![obj()],
        Type::Array(Box::new(Type::String)),
    );
    declare_method(
        defs,
        "values",
        ctor_key("values"),
        vec![obj()],
        Type::Array(Box::new(Type::Unknown)),
    );
    declare_method(
        defs,
        "entries",
        ctor_key("entries"),
        vec![obj()],
        Type::Array(Box::new(Type::Tuple(vec![Type::String, Type::Unknown]))),
    );
    declare_method(
        defs,
        "hasOwn",
        ctor_key("hasOwn"),
        vec![obj(), Param::new("key", Type::String)],
        Type::Boolean,
    );
    declare_method(
        defs,
        "is",
        ctor_key("is"),
        vec![
            Param::new("a", Type::Unknown),
            Param::new("b", Type::Unknown),
        ],
        Type::Boolean,
    );
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{
        Dispatch, MethodSig, Param, Span, Type, TypeKind, TypeSymbol, ValueKind, ValueSymbol,
    };
    use std::collections::BTreeMap;
    defs.types.insert(
        "Object".to_string(),
        TypeSymbol {
            name: "Object".to_string(),
            mangled_name: crate::mangle::prelude("Object"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns `\"[object Object]\"`. */"),
                        },
                    ),
                    (
                        "toJson".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns the JSON representation of this object — `\"{\"` + `\"key\":value-json` pairs joined with `\",\"` + `\"}\"`. Field iteration order matches the type's struct layout. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                // Object opts OUT of direct dispatch — `obj.toString()`
                // and `obj.toJson()` route through the per-shape vtable
                // (slots 0 / 1). Each user-object shape generates its
                // own per-type body.
                dispatch: Dispatch::VTable,
                doc: doc("/** Universal base type for every object shape. */"),
            },
        },
    );
    defs.types.insert(
        "ObjectConstructor".to_string(),
        TypeSymbol {
            name: "ObjectConstructor".to_string(),
            mangled_name: crate::mangle::prelude("ObjectConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "keys".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("obj", Type::Unknown)],
                            ret: Type::Array(Box::new(Type::String)),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the object's field names in canonical sorted order (the same order JSON output uses).\n * Optional fields are included even when they currently hold `null`. Non-object values yield `[]`; `null` throws a catchable `Error`.\n * @param obj The object to enumerate.\n */",
                            ),
                        },
                    ),
                    (
                        "values".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("obj", Type::Unknown)],
                            ret: Type::Array(Box::new(Type::Unknown)),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the object's field values, aligned with `Object.keys` order. Element types are erased to `unknown` — narrow with `typeof` / `as`.\n * Non-object values yield `[]`; `null` throws a catchable `Error`.\n * @param obj The object to enumerate.\n */",
                            ),
                        },
                    ),
                    (
                        "entries".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("obj", Type::Unknown)],
                            ret: Type::Array(Box::new(Type::Tuple(vec![
                                Type::String,
                                Type::Unknown,
                            ]))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `[name, value]` pairs in canonical sorted key order. Values are erased to `unknown` — narrow with `typeof` / `as`.\n * Non-object values yield `[]`; `null` throws a catchable `Error`.\n * @param obj The object to enumerate.\n */",
                            ),
                        },
                    ),
                    (
                        "hasOwn".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("obj", Type::Unknown),
                                Param::new("key", Type::String),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` when `obj` declares a field named `key`. Non-object values have no fields; `null` throws a catchable `Error`.\n * @param obj The object to test.\n * @param key The field name.\n */",
                            ),
                        },
                    ),
                    (
                        "is".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("a", Type::Unknown),
                                Param::new("b", Type::Unknown),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * SameValue comparison: like `===` but `Object.is(NaN, NaN)` is `true` and `Object.is(0, -0)` is `false`.\n * Object comparison follows the language's structural `===`, not JS reference identity.\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Object` — the enumeration statics (`keys`/`values`/`entries`/`hasOwn`) and `is`. Accessed via the global `Object` binding. */",
                ),
            },
        },
    );

    defs.values.insert(
        "Object".to_string(),
        ValueSymbol {
            name: "Object".to_string(),
            mangled_name: crate::mangle::prelude("Object"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("ObjectConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `Object` namespace — `Object.keys(o)`, `Object.values(o)`, `Object.entries(o)`, `Object.hasOwn(o, k)`, `Object.is(a, b)`. */",
                ),
            },
        },
    );
}

#[cfg(test)]
mod tests {
    use super::same_value_bits;

    #[test]
    fn same_value_bit_rules() {
        assert!(same_value_bits(f64::NAN.to_bits(), (-f64::NAN).to_bits()));
        assert!(!same_value_bits(0f64.to_bits(), (-0f64).to_bits()));
        assert!(same_value_bits((-0f64).to_bits(), (-0f64).to_bits()));
        assert!(same_value_bits(1.5f64.to_bits(), 1.5f64.to_bits()));
        assert!(!same_value_bits(1f64.to_bits(), 2f64.to_bits()));
    }
}
