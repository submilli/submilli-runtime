//! Apply defaults and rest packing after a dynamic call resolves its target.
use super::member::box_result;
use crate::{
    DefaultValue,
    runtime::{StoreData, host, intrinsic_types::build_intrinsic_types},
};
use wasmtime::{
    Caller, FieldType, Finality, HeapType, RefType, StorageType, StructType, Val, ValType,
};

pub(super) type Parameters = Vec<(Option<DefaultValue>, bool)>;

pub(super) fn metadata_type(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    crate::runtime::gc_singleton::singleton_struct(
        engine,
        Finality::Final,
        None,
        vec![
            FieldType::new(
                wasmtime::Mutability::Const,
                StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any))),
            ),
            FieldType::new(
                wasmtime::Mutability::Const,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    HeapType::ConcreteStruct(intr.string),
                ))),
            ),
        ],
    )
}

pub(super) fn metadata(
    caller: &mut Caller<'_, StoreData>,
    env: &Val,
) -> wasmtime::Result<Option<Parameters>> {
    let Val::AnyRef(Some(reference)) = env else {
        return Ok(None);
    };
    let Some(object) = reference.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    if !object.matches_ty(&*caller, &metadata_type(caller.engine())?)? {
        return Ok(None);
    }
    let encoded = object.field(&mut *caller, 1)?;
    let encoded = host::read_string_arg(caller, &encoded, "call parameters")?;
    serde_json::from_str(&encoded)
        .map(Some)
        .map_err(|error| wasmtime::Error::msg(error.to_string()))
}

pub(super) fn bind(
    caller: &mut Caller<'_, StoreData>,
    params: &Parameters,
    args: &[Val],
) -> wasmtime::Result<Vec<Val>> {
    params
        .iter()
        .enumerate()
        .map(|(index, (default, rest))| {
            if *rest {
                return Ok(Val::AnyRef(Some(
                    host::write_submilli_array_struct(
                        caller,
                        args.get(index..).unwrap_or_default(),
                    )?
                    .to_anyref(),
                )));
            }
            match args.get(index) {
                Some(value) => Ok(*value),
                None => default_value(caller, default.as_ref()),
            }
        })
        .collect()
}

fn default_value(
    caller: &mut Caller<'_, StoreData>,
    default: Option<&DefaultValue>,
) -> wasmtime::Result<Val> {
    match default {
        Some(DefaultValue::Number(value)) => box_result(caller, Val::F64(value.to_bits())),
        Some(DefaultValue::Boolean(value)) => box_result(caller, Val::I32(*value as i32)),
        Some(DefaultValue::String(value)) => Ok(Val::AnyRef(Some(
            host::write_submilli_string_struct(caller, value)?.to_anyref(),
        ))),
        Some(DefaultValue::EmptyArray) => Ok(Val::AnyRef(Some(
            host::write_submilli_array_struct(caller, &[])?.to_anyref(),
        ))),
        Some(DefaultValue::EmptyObject) => empty_object(caller),
        Some(DefaultValue::EnumVariant { value, .. }) => match value {
            crate::EnumVariantValue::Number(value) => box_result(caller, Val::F64(value.to_bits())),
            crate::EnumVariantValue::String(value) => Ok(Val::AnyRef(Some(
                host::write_submilli_string_struct(caller, value)?.to_anyref(),
            ))),
        },
        Some(DefaultValue::GlobalConst(name)) => match caller.get_export(name.as_str()) {
            Some(wasmtime::Extern::Global(global)) => {
                let value = global.get(&mut *caller);
                box_result(caller, value)
            }
            _ => Err(host::type_error("Default argument constant is unavailable")),
        },
        None | Some(DefaultValue::Null) => Ok(Val::null_any_ref()),
    }
}

fn empty_object(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let intr = build_intrinsic_types(caller.engine())?;
    let names = wasmtime::ArrayRefPre::new(&mut *caller, intr.field_names);
    let fields = wasmtime::ArrayRefPre::new(&mut *caller, intr.object_fields);
    let object = wasmtime::StructRefPre::new(&mut *caller, intr.object_shape);
    let names = wasmtime::ArrayRef::new_fixed(&mut *caller, &names, &[])?;
    let fields = wasmtime::ArrayRef::new_fixed(&mut *caller, &fields, &[])?;
    let vtable = host::host_object_vtable(caller)?;
    Ok(Val::AnyRef(Some(
        wasmtime::StructRef::new(
            &mut *caller,
            &object,
            &[
                vtable,
                Val::AnyRef(Some(names.to_anyref())),
                Val::AnyRef(Some(fields.to_anyref())),
            ],
        )?
        .to_anyref(),
    )))
}
