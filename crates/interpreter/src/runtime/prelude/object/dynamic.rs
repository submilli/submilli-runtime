//! Runtime string-key operations on the shared object carrier.
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use super::{
    ctor_key, field_array, field_is_present, field_is_private, is_accessor_slot, shape_arrays,
};
use crate::runtime::StoreData;
use crate::runtime::host::{register_host_fn, register_host_fn_async, write_submilli_array_struct};
use crate::runtime::prelude::collection::FIELD_NAME;
use crate::runtime::prelude::keep::KeptValues;
use crate::runtime::prelude::vtable::read_string_units;
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{PackageDeclaration, Param, Type};

struct Property {
    slot: u32,
    name: Val,
    value: Val,
}

fn find(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    key: &[u16],
    accessor: bool,
) -> wasmtime::Result<Option<Property>> {
    let Some((names, values)) = shape_arrays(caller, object)? else {
        return Err(crate::runtime::host::throw_host_error(
            caller,
            crate::runtime::host::type_error("property receiver must be an object"),
        ));
    };
    let object = super::super::iterator::as_struct(caller, object, "property receiver")?;
    let Some(slot) = super::index::lookup(caller, &object, key, accessor, true)? else {
        return Ok(None);
    };
    Ok(Some(Property {
        slot,
        name: names.get(&mut *caller, slot)?,
        value: values.get(&mut *caller, slot)?,
    }))
}

fn accessor_key(prefix: &str, key: &[u16]) -> wasmtime::Result<Vec<u16>> {
    let capacity = key
        .len()
        .checked_add(prefix.len())
        .ok_or_else(|| crate::runtime::host::fatal_host_error("property key size overflow"))?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(capacity)
        .map_err(crate::runtime::host::fatal_host_error)?;
    result.extend(prefix.encode_utf16());
    result.extend_from_slice(key);
    Ok(result)
}

async fn get(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    name: &Val,
) -> wasmtime::Result<Val> {
    let key = read_string_units(caller, name, FIELD_NAME)?;
    if let Some(property) = find(caller, object, &key, false)? {
        if !field_is_present(caller, &property.name, &property.value)? {
            return super::super::undefined::value(caller);
        }
        return checked_data_value(caller, object, property.slot, property.value).await;
    }
    if let Some(property) = find(caller, object, &accessor_key("get ", &key)?, true)? {
        return super::super::closure::read(caller, &property.value, "property getter")?
            .call_with_receiver(caller, *object, &[])
            .await;
    }
    super::super::undefined::value(caller)
}

/// Hidden payload rows retain concrete class validators across structural views.
/// Insertion preserves one guard per named slot plus the row's generic context.
async fn checked_data_value(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    slot: u32,
    mut value: Val,
) -> wasmtime::Result<Val> {
    let Some((names, fields)) = shape_arrays(caller, object)? else {
        return Err(crate::runtime::host::fatal_host_error(
            "internal error: class field guard lost its object carrier",
        ));
    };
    let named_len = names.len(&mut *caller)?;
    let total = fields.len(&mut *caller)?;
    let width = named_len.checked_add(1).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("class field guard row width overflow")
    })?;
    let hidden = total.checked_sub(named_len).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("class field payload is shorter than its names")
    })?;
    if slot >= named_len || hidden % width != 0 {
        return Err(crate::runtime::host::fatal_host_error(
            "malformed class field guard rows",
        ));
    }
    for row in (named_len..total).step_by(width as usize) {
        let guard = fields.get(&mut *caller, row + slot)?;
        if !matches!(guard, Val::AnyRef(None)) {
            value = super::super::closure::read(caller, &guard, "class field guard")?
                .call(caller, &[value])
                .await?;
        }
    }
    Ok(value)
}

async fn set(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    name: &Val,
    value: &Val,
) -> wasmtime::Result<()> {
    let key = read_string_units(caller, name, FIELD_NAME)?;
    if let Some(property) = find(caller, object, &key, false)? {
        let object = super::super::iterator::as_struct(caller, object, "property receiver")?;
        let values = field_array(caller, &object, 2)?;
        values.set(&mut *caller, property.slot, *value)?;
        let marker = super::super::iterator::as_struct(caller, &property.name, "field name")?;
        if marker.ty(&*caller)?.fields().count() > 3
            && matches!(marker.field(&mut *caller, 3)?, Val::I32(0))
        {
            marker.set_field(&mut *caller, 3, Val::I32(1))?;
        }
        return Ok(());
    }
    if let Some(property) = find(caller, object, &accessor_key("set ", &key)?, true)? {
        super::super::closure::read(caller, &property.value, "property setter")?
            .call_with_receiver(caller, *object, &[*value])
            .await?;
        return Ok(());
    }
    if find(caller, object, &accessor_key("get ", &key)?, true)?.is_some() {
        return Err(crate::runtime::host::throw_host_error(
            caller,
            crate::runtime::host::type_error("cannot write a getter-only property"),
        ));
    }
    super::insert_field(caller, object, name, value)
}

fn has(caller: &mut Caller<'_, StoreData>, object: &Val, name: &Val) -> wasmtime::Result<bool> {
    let key = read_string_units(caller, name, FIELD_NAME)?;
    if let Some(property) = find(caller, object, &key, false)? {
        return field_is_present(caller, &property.name, &property.value);
    }
    Ok(
        find(caller, object, &accessor_key("get ", &key)?, true)?.is_some()
            || find(caller, object, &accessor_key("set ", &key)?, true)?.is_some(),
    )
}

async fn values(caller: &mut Caller<'_, StoreData>, object: &Val) -> wasmtime::Result<Val> {
    let Some((names, fields)) = shape_arrays(caller, object)? else {
        return Ok(Val::AnyRef(None));
    };
    let count = super::field_count(caller, object)?;
    // A getter's result is held by nothing while the next getter runs.
    let mut result = KeptValues::with_capacity(caller, count as usize)?;
    for slot in 0..count {
        let name = names.get(&mut *caller, slot)?;
        let value = fields.get(&mut *caller, slot)?;
        if field_is_private(caller, &name)? {
            continue;
        }
        if is_accessor_slot(caller, &name)? {
            if read_string_units(caller, &name, FIELD_NAME)?.starts_with(&[103, 101, 116, 32]) {
                let got = super::super::closure::read(caller, &value, "record getter")?
                    .call_with_receiver(caller, *object, &[])
                    .await?;
                result.push(caller, got)?;
            }
        } else if field_is_present(caller, &name, &value)? {
            let checked = checked_data_value(caller, object, slot, value).await?;
            result.push(caller, checked)?;
        }
    }
    Ok(Val::AnyRef(Some(
        write_submilli_array_struct(caller, result.values())?.to_anyref(),
    )))
}

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?;
    let object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    for name in ["#getField", "#setField", "#recordValues"] {
        let mut params = vec![object.clone()];
        if name != "#recordValues" {
            params.push(string.clone());
        }
        if name == "#setField" {
            params.push(object.clone());
        }
        let results = if name == "#setField" {
            vec![]
        } else {
            vec![object.clone()]
        };
        register_host_fn_async(
            linker,
            MODULE_NAME,
            ctor_key(name),
            FuncType::new(&engine, params, results),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    match (name, params, results) {
                        ("#getField", [object, key], [result]) => {
                            *result = get(caller, object, key).await?;
                        }
                        ("#setField", [object, key, value], []) => {
                            set(caller, object, key, value).await?;
                        }
                        ("#recordValues", [object], [result]) => {
                            *result = values(caller, object).await?;
                        }
                        _ => {
                            return Err(crate::runtime::host::fatal_host_error(
                                "invalid dynamic object host ABI",
                            ));
                        }
                    }
                    Ok(())
                })
            },
        )?;
    }
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("#hasField"),
        FuncType::new(&engine, [object, string], [ValType::I32]),
        true,
        |caller, params, results| {
            let ([object, key], [result]) = (params, results) else {
                return Err(crate::runtime::host::fatal_host_error(
                    "invalid property presence host ABI",
                ));
            };
            *result = Val::I32(i32::from(has(caller, object, key)?));
            Ok(())
        },
    )
}

pub(super) fn declare(defs: &mut PackageDeclaration) {
    for name in ["#getField", "#setField", "#recordValues", "#hasField"] {
        let mut params = vec![Param::new("object", Type::Unknown)];
        if name != "#recordValues" {
            params.push(Param::new("key", Type::String));
        }
        if name == "#setField" {
            params.push(Param::new("value", Type::Unknown));
        }
        let ret = match name {
            "#setField" => Type::Void,
            "#hasField" => Type::Boolean,
            _ => Type::Unknown,
        };
        declare_method(defs, name, ctor_key(name), params, ret);
    }
}
