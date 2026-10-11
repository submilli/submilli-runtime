//! Apply defaults and rest packing after a dynamic call resolves its target.
use super::member::box_result;
use crate::runtime::fuel;
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::{
    DefaultValue,
    runtime::{StoreData, host},
};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wasmtime::{
    Caller, FieldType, Finality, HeapType, RefType, StorageType, StructType, Val, ValType,
};

pub(super) type Parameters = Vec<(Option<DefaultValue>, bool)>;

#[derive(Default)]
pub(crate) struct ParameterCache {
    next: i64,
    entries: HashMap<i64, Arc<CachedParameters>>,
}

pub(crate) struct CachedParameters {
    parameters: Parameters,
    _bytes: ParameterBytes,
}
impl std::ops::Deref for CachedParameters {
    type Target = Parameters;
    fn deref(&self) -> &Parameters {
        &self.parameters
    }
}

struct ParameterBytes {
    counter: Arc<AtomicU64>,
    bytes: u64,
}
impl Drop for ParameterBytes {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

pub(super) fn metadata_type(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<StructType> {
    if let Some(ty) = &caller.data().call_metadata_type {
        return Ok(ty.clone());
    }
    let string = intrinsic_types(&mut *caller)?.string.clone();
    let ty = build_metadata_type(caller.engine(), string)?;
    caller.data_mut().call_metadata_type = Some(ty.clone());
    Ok(ty)
}

fn build_metadata_type(
    engine: &wasmtime::Engine,
    string: StructType,
) -> wasmtime::Result<StructType> {
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
                    HeapType::ConcreteStruct(string),
                ))),
            ),
            FieldType::new(
                wasmtime::Mutability::Var,
                StorageType::ValType(ValType::I64),
            ),
        ],
    )
}

pub(super) fn metadata(
    caller: &mut Caller<'_, StoreData>,
    env: &Val,
) -> wasmtime::Result<Option<Arc<CachedParameters>>> {
    let mut env = *env;
    let mut adapters = 0;
    let object = loop {
        let Val::AnyRef(Some(reference)) = env else {
            return Ok(None);
        };
        let Some(object) = reference.as_struct(&mut *caller)? else {
            return Ok(None);
        };
        let receiver_type = super::closure::receiver_type(caller)?;
        if !object.matches_ty(&*caller, &receiver_type)? {
            break object;
        }
        if adapters == 128 {
            return Err(host::range_error(
                "Call metadata receiver nesting limit exceeded",
            ));
        }
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        adapters += 1;
        env = object.field(&mut *caller, 0)?;
    };
    let metadata_type = metadata_type(caller)?;
    if !object.matches_ty(&*caller, &metadata_type)? {
        return Ok(None);
    }
    let Val::I64(id) = object.field(&mut *caller, 2)? else {
        return Err(host::fatal_host_error("Invalid call metadata cache ID"));
    };
    if id != 0 {
        return caller
            .data()
            .parameter_cache
            .entries
            .get(&id)
            .cloned()
            .map(Some)
            .ok_or_else(|| host::fatal_host_error("Missing cached call metadata"));
    }
    let encoded = object.field(&mut *caller, 1)?;
    let string = super::iterator::as_struct(caller, &encoded, "call parameters")
        .map_err(host::fatal_host_error)?;
    let raw = match string.field(&mut *caller, 1)? {
        Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
        _ => return Err(host::fatal_host_error("Invalid call metadata string")),
    };
    let units = raw.len(&mut *caller)? as u64;
    // JSON strings and enum/default records are compiler-produced. Admit
    // retained parameter data and transient decoding before either allocation.
    let bytes = units
        .checked_mul(32)
        .and_then(|size| size.checked_add(512))
        .ok_or_else(|| host::fatal_host_error("Call metadata size overflow"))?;
    let limits = &caller.data().tenant_limits;
    limits.charge_host_bytes(bytes)?;
    let reservation = ParameterBytes {
        counter: limits.host_attached_counter(),
        bytes,
    };
    let encoded = host::read_string_arg(caller, &encoded, "call parameters")?;
    fuel::charge(&mut *caller, fuel::PARSE, units)?;
    let parameters: Parameters = serde_json::from_str(&encoded).map_err(host::fatal_host_error)?;
    let cached = Arc::new(CachedParameters {
        parameters,
        _bytes: reservation,
    });
    let cache = &mut caller.data_mut().parameter_cache;
    let id = cache
        .next
        .checked_add(1)
        .ok_or_else(|| host::fatal_host_error("Call metadata IDs exhausted"))?;
    cache
        .entries
        .try_reserve(1)
        .map_err(host::fatal_host_error)?;
    object.set_field(&mut *caller, 2, Val::I64(id))?;
    let cache = &mut caller.data_mut().parameter_cache;
    cache.next = id;
    cache.entries.insert(id, Arc::clone(&cached));
    Ok(Some(cached))
}

pub(super) fn bind(
    caller: &mut Caller<'_, StoreData>,
    params: &Parameters,
    args: &[Val],
) -> wasmtime::Result<Vec<Val>> {
    let mut bound = Vec::new();
    bound
        .try_reserve_exact(params.len())
        .map_err(host::fatal_host_error)?;
    for (index, (default, rest)) in params.iter().enumerate() {
        let value = if *rest {
            Val::AnyRef(Some(
                host::write_submilli_array_struct(caller, args.get(index..).unwrap_or_default())?
                    .to_anyref(),
            ))
        } else {
            match args.get(index) {
                Some(value)
                    if default.is_none() || !super::undefined::is_undefined(caller, value)? =>
                {
                    *value
                }
                Some(_) => match default {
                    Some(DefaultValue::OmittedNumber { undefined, .. }) => {
                        box_result(caller, Val::F64(undefined.to_bits()))?
                    }
                    _ => default_value(caller, default.as_ref())?,
                },
                None => default_value(caller, default.as_ref())?,
            }
        };
        bound.push(value);
    }
    Ok(bound)
}

fn default_value(
    caller: &mut Caller<'_, StoreData>,
    default: Option<&DefaultValue>,
) -> wasmtime::Result<Val> {
    match default {
        Some(DefaultValue::Number(value) | DefaultValue::OmittedNumber { omitted: value, .. }) => {
            box_result(caller, Val::F64(value.to_bits()))
        }
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
            _ => Err(host::fatal_host_error(
                "Default argument constant is unavailable",
            )),
        },
        Some(DefaultValue::Null) => Ok(Val::null_any_ref()),
        None | Some(DefaultValue::Undefined) => super::undefined::value(caller),
    }
}

fn empty_object(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let names = wasmtime::ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let fields = wasmtime::ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let object = wasmtime::StructRefPre::new(&mut *caller, intr.object_shape.clone());
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
                Val::AnyRef(None),
            ],
        )?
        .to_anyref(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs, install_runtime_async};
    use wasmtime::{Func, FuncType, Linker, StructRef, StructRefPre};

    #[tokio::test]
    async fn unavailable_default_constant_is_fatal_and_store_remains_usable() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let name = crate::mangle::host("test:arguments", "default");
        host::register_host_fn(
            &mut linker,
            "test:arguments",
            name.clone(),
            FuncType::new(&engine, [ValType::I32], [ValType::ANYREF]),
            true,
            |caller, args, results| {
                let default = if args[0].i32() == Some(0) {
                    DefaultValue::GlobalConst(crate::mangle::host(
                        "test:arguments",
                        "missing_default_export",
                    ))
                } else {
                    DefaultValue::Null
                };
                results[0] = default_value(caller, Some(&default))?;
                Ok(())
            },
        )
        .unwrap();
        let wasmtime::Extern::Func(probe) = linker
            .get(&mut store, "test:arguments", name.as_str())
            .unwrap()
        else {
            panic!("default probe is not a function");
        };
        let mut result = [Val::null_any_ref()];
        let error = probe
            .call_async(&mut store, &[Val::I32(0)], &mut result)
            .await
            .unwrap_err();
        assert!(error.is::<host::FatalHostError>(), "{error:#}");
        assert!(
            error
                .to_string()
                .contains("Default argument constant is unavailable")
        );
        probe
            .call_async(&mut store, &[Val::I32(1)], &mut result)
            .await
            .unwrap();
        assert!(matches!(result[0], Val::AnyRef(None)));
    }

    #[tokio::test]
    async fn parsed_metadata_is_reused_when_calls_and_input_double() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let costs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let measured = Arc::clone(&costs);
        let probe = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::I32], []),
            move |mut caller, args, _| {
                let count = args[0].i32().unwrap() as usize;
                let encoded = serde_json::to_string(&vec![(
                    Some(DefaultValue::String("x".repeat(count * 32))),
                    false,
                )])
                .unwrap();
                let string = host::write_submilli_string_struct(&mut caller, &encoded)?;
                let ty = metadata_type(&mut caller)?;
                let pre = StructRefPre::new(&mut caller, ty);
                let wrapper = StructRef::new(
                    &mut caller,
                    &pre,
                    &[
                        Val::null_any_ref(),
                        Val::AnyRef(Some(string.to_anyref())),
                        Val::I64(0),
                    ],
                )?;
                let env = Val::AnyRef(Some(wrapper.to_anyref()));
                let before = caller.data().host_fuel;
                let time = std::time::Instant::now();
                for _ in 0..count {
                    assert_eq!(metadata(&mut caller, &env)?.unwrap().len(), 1);
                }
                let work = caller.data().host_fuel - before;
                eprintln!(
                    "metadata calls/units {count}: {work} fuel/{:?}",
                    time.elapsed()
                );
                measured.lock().unwrap().push(work);
                let defaults = vec![
                    (Some(DefaultValue::EmptyArray), false),
                    (Some(DefaultValue::EmptyObject), false),
                ];
                let first = bind(&mut caller, &defaults, &[])?;
                let second = bind(&mut caller, &defaults, &[])?;
                for (first, second) in first.iter().zip(&second) {
                    let Val::AnyRef(Some(first)) = first else {
                        panic!("missing default object")
                    };
                    let Val::AnyRef(Some(second)) = second else {
                        panic!("missing second default object")
                    };
                    assert!(!wasmtime::Rooted::ref_eq(&caller, first, second)?);
                }
                wrapper.set_field(&mut caller, 2, Val::I64(-1))?;
                let Err(error) = metadata(&mut caller, &env) else {
                    panic!("accepted missing metadata cache ID");
                };
                assert!(error.to_string().contains("Missing cached call metadata"));
                wrapper.set_field(&mut caller, 2, Val::I64(0))?;
                assert_eq!(metadata(&mut caller, &env)?.unwrap().len(), 1);
                Ok(())
            },
        );
        for count in [128, 256] {
            probe
                .call_async(&mut store, &[Val::I32(count)], &mut [])
                .await
                .unwrap();
        }
        let costs = costs.lock().unwrap();
        assert!(costs[1] < costs[0] * 5 / 2);
        for (actual, old) in costs.iter().zip([593_152, 2_365_952]) {
            assert!(*actual < old / 2);
        }
    }
}
