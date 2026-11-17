//! Remove an own data property without changing shared shape names or slot indices.
use wasmtime::{ArrayRef, ArrayRefPre, Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use super::{ctor_key, field_array, index, is_method_slot, is_protocol_slot};
use crate::runtime::host::{fatal_host_error, register_host_fn};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::collection::FIELD_NAME;
use crate::runtime::prelude::iterator::as_struct;
use crate::runtime::prelude::vtable::read_string_units;
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::runtime::{StoreData, fuel};
use crate::{PackageDeclaration, Param, Type};

fn delete(caller: &mut Caller<'_, StoreData>, object: &Val, key: &Val) -> wasmtime::Result<()> {
    let key = read_string_units(caller, key, FIELD_NAME)?;
    let Some((names, values)) = super::shape_arrays(caller, object)? else {
        return delete_builtin(caller, object, &key);
    };
    let object = as_struct(caller, object, "delete receiver")?;
    let Some(slot) = index::lookup(caller, &object, &key, false, true)? else {
        return delete_builtin(caller, &Val::AnyRef(Some(object.to_anyref())), &key);
    };
    let name = names.get(&mut *caller, slot)?;
    // Prototype members survive deletion from the instance.
    if is_method_slot(caller, &name)? || is_protocol_slot(caller, &name)? {
        return Ok(());
    }
    let value = values.get(&mut *caller, slot)?;
    if !super::field_is_present(caller, &name, &value)? {
        return Ok(());
    }
    let table = index::cached(caller, &object)?
        .ok_or_else(|| fatal_host_error("delete lost the object field index"))?;
    let capacity = names.len(&mut *caller)?;
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(capacity) * 2)?;
    let (restored_name, restored_value) = match super::protocol::inherited(caller, &name)? {
        Some(inherited) => inherited,
        None => (
            super::optional_field_name(caller, &name, false)?,
            super::super::undefined::value(caller)?,
        ),
    };
    let intr = intrinsic_types(&mut *caller)?;
    let pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let detached = ArrayRef::new(&mut *caller, &pre, &restored_name, capacity)?;
    for index in 0..capacity {
        let name = names.get(&mut *caller, index)?;
        detached.set(&mut *caller, index, name)?;
    }
    detached.set(&mut *caller, slot, restored_name)?;
    // All allocation and fuel checks precede publication. The cached name index
    // remains valid: text, privacy and slot positions have not changed.
    values.set(&mut *caller, slot, restored_value)?;
    object.set_field(&mut *caller, 1, Val::AnyRef(Some(detached.to_anyref())))?;
    // A formerly required field is now absent: static equality/hash hooks must
    // use the live field set, just as they do after an insertion.
    table.set(&mut *caller, 1, Val::I32(1))?;
    Ok(())
}

fn delete_builtin(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    key: &[u16],
) -> wasmtime::Result<()> {
    use crate::runtime::host::{throw_host_error, type_error};
    use crate::runtime::prelude::collection::is_a;

    if super::super::undefined::is_nullish(caller, object)? {
        return Err(throw_host_error(
            caller,
            type_error("Cannot delete a property of null or undefined"),
        ));
    }
    // A structural optional view does not make a built-in's own fixed property
    // configurable. Other named members of these carriers live on prototypes.
    fuel::charge(&mut *caller, fuel::SCAN, key.len() as u64)?;
    let intr = intrinsic_types(&mut *caller)?;
    if is_a(caller, object, &intr.regex_match_box)? {
        for name in ["index", "input", "groups"] {
            if name.encode_utf16().eq(key.iter().copied()) {
                return super::super::regex::delete_match_metadata(caller, object, name);
            }
        }
    }
    let length = key.iter().copied().eq("length".encode_utf16());
    if length && super::super::value::is_callable(caller, object)? {
        return super::super::closure::delete_length(caller, object);
    }
    let last_index = key.iter().copied().eq("lastIndex".encode_utf16());
    let fixed = if is_a(caller, object, &intr.string)? {
        let object = as_struct(caller, object, "string delete receiver")?;
        let units = field_array(caller, &object, 1)?;
        let len = units.len(&mut *caller)?;
        length || super::integer_key(key).is_some_and(|index| index < len)
    } else {
        (length && is_a(caller, object, &intr.array)?)
            || (last_index && is_a(caller, object, &intr.regex)?)
    };
    if fixed {
        return Err(throw_host_error(
            caller,
            type_error("Cannot delete a non-configurable property"),
        ));
    }
    Ok(())
}

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?;
    let object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("#deleteField"),
        FuncType::new(&engine, [object, string], [ValType::I32]),
        |caller, params, results| {
            let ([object, key], [result]) = (params, results) else {
                return Err(fatal_host_error("invalid property deletion host ABI"));
            };
            delete(caller, object, key)?;
            *result = Val::I32(1);
            Ok(())
        },
    )
}

pub(super) fn declare(defs: &mut PackageDeclaration) {
    declare_method(
        defs,
        "#deleteField",
        ctor_key("#deleteField"),
        vec![
            Param::new("object", Type::Unknown),
            Param::new("key", Type::String),
        ],
        Type::Boolean,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs, install_runtime_async};
    use wasmtime::Func;

    #[tokio::test]
    async fn fuel_refusal_preserves_the_property_and_allows_a_later_delete() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(
                &engine,
                StoreData::with_vfs("delete-test".into(), Vfs::none()),
            )
            .unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let probe = Func::new(
            &mut store,
            FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                for budget in [0, 80] {
                    reset_fuel(&mut caller, 100_000);
                    let key = Val::AnyRef(Some(
                        crate::runtime::host::write_submilli_string_struct(&mut caller, "p")?
                            .to_anyref(),
                    ));
                    let object = super::super::object_from_fields(&mut caller, &[(key, key)])?;
                    let shape = as_struct(&mut caller, &object, "test object")?;
                    // Warm the cache so the second refusal is in deletion's own work.
                    index::lookup(&mut caller, &shape, &[u16::from(b'p')], false, true)?;
                    reset_fuel(&mut caller, budget);
                    let failure = delete(&mut caller, &object, &key).unwrap_err();
                    assert!(failure.is::<wasmtime::Trap>(), "{failure}");
                    reset_fuel(&mut caller, 100_000);
                    assert!(super::super::dynamic::has(&mut caller, &object, &key)?);
                    let values = field_array(&mut caller, &shape, 2)?;
                    let value = values.get(&mut caller, 0)?;
                    assert_eq!(
                        read_string_units(&mut caller, &value, "retained value")?,
                        [u16::from(b'p')]
                    );
                    delete(&mut caller, &object, &key)?;
                    assert!(!super::super::dynamic::has(&mut caller, &object, &key)?);
                }
                Ok(())
            },
        );
        probe.call_async(&mut store, &[], &mut []).await.unwrap();
    }

    fn reset_fuel(caller: &mut Caller<'_, StoreData>, fuel: u64) {
        caller.set_fuel(fuel).unwrap();
        caller.data_mut().host_fuel_pending = 0;
        caller.data_mut().host_fuel_applied_at = None;
    }
}
