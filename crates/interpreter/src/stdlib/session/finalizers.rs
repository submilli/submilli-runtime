//! Package attribution is checked at the host boundary, including indirect calls.
use super::{MODULE_NAME, read_key, trap, value};
use crate::runtime::host::{abi_arg, fatal_host_error, register_host_fn_async};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::{StoreData, fuel};
use crate::stdlib::shared::{check_security, running_package};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    let unknown = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    for (name, registering) in [("registerFinalizer", true), ("unregisterFinalizer", false)] {
        let params = if registering {
            vec![string.clone(), unknown.clone()]
        } else {
            vec![string.clone()]
        };
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::package_symbol(MODULE_NAME, name),
            FuncType::new(&engine, params, []),
            false,
            move |caller, params, _| {
                Box::pin(async move {
                    let package =
                        running_package(&*caller).map_err(|e| e.into_denial("session.finalize"))?;
                    let key = read_key(caller, abi_arg(params, 0)?, name)?;
                    check_security(
                        &mut *caller,
                        "session.finalize",
                        serde_json::json!({"key": String::from_utf16_lossy(&key)}),
                    )?;
                    if caller.data().finalizing {
                        return Err(fatal_host_error(
                            "finalizers cannot change finalizer registrations",
                        ));
                    }
                    let provider = caller.data().session_finalizers.clone().ok_or_else(|| {
                        fatal_host_error("no durable session finalizer provider configured")
                    })?;
                    if registering {
                        let bytes = provider.registration_bytes(&package);
                        fuel::charge(&mut *caller, fuel::COPY, bytes)?;
                        fuel::charge(&mut *caller, fuel::IO, bytes)?;
                        let payload = value::serialize(caller, abi_arg(params, 1)?).await?;
                        fuel::charge(
                            &mut *caller,
                            fuel::IO,
                            (payload.len() as u64).saturating_mul(2),
                        )?;
                        provider
                            .register(&package, &key, &payload)
                            .await
                            .map_err(|e| trap(caller, None, &e))?;
                    } else {
                        provider
                            .unregister(&package, &key)
                            .await
                            .map_err(|e| trap(caller, None, &e))?;
                    }
                    Ok(())
                })
            },
        )?;
    }
    Ok(())
}
