//! `announceOpen` / `announceClosed`: a package tells the host which resources it holds open
//! for the session. Package attribution is checked at the host boundary, as for finalizers.
use super::{MODULE_NAME, read_key, trap};
use crate::runtime::effects;
use crate::runtime::host::{abi_arg, register_host_fn_async};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::session_announcements::ResourceState;
use crate::runtime::{StoreData, fuel};
use crate::stdlib::shared::{check_and_approve_call, running_package};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

const CAPABILITY: &str = "session.announce";

#[derive(serde::Serialize)]
struct Announcement {
    package: String,
    kind: Vec<u16>,
    key: Vec<u16>,
    state: ResourceState,
}

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    for (name, state) in [
        ("announceOpen", ResourceState::Open),
        ("announceClosed", ResourceState::Closed),
    ] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::package_symbol(MODULE_NAME, name),
            FuncType::new(&engine, [string.clone(), string.clone()], []),
            move |caller, params, _| {
                Box::pin(async move {
                    let package =
                        running_package(&*caller).map_err(|e| e.into_denial(CAPABILITY))?;
                    let kind = read_key(caller, abi_arg(params, 0)?, name)?;
                    let key = read_key(caller, abi_arg(params, 1)?, name)?;
                    check_and_approve_call(
                        &mut *caller,
                        CAPABILITY,
                        serde_json::json!({
                            "kind": String::from_utf16_lossy(&kind),
                            "key": String::from_utf16_lossy(&key),
                        }),
                    )
                    .await?;
                    let request = Announcement {
                        package,
                        kind,
                        key,
                        state,
                    };
                    effects::execute(
                        caller,
                        if state == ResourceState::Open {
                            "session.announceOpen"
                        } else {
                            "session.announceClosed"
                        },
                        request,
                        |caller, _, request| {
                            Box::pin(async move {
                                let Some(provider) = caller.data().session_announcements.clone()
                                else {
                                    return Ok(());
                                };
                                let size = request.kind.len().saturating_add(request.key.len());
                                fuel::charge(
                                    &mut *caller,
                                    fuel::IO,
                                    (size as u64).saturating_mul(2),
                                )?;
                                provider
                                    .announce(
                                        &request.package,
                                        &request.kind,
                                        &request.key,
                                        request.state,
                                    )
                                    .await
                                    .map_err(|e| trap(caller, None, &e))
                            })
                        },
                    )
                    .await?;
                    Ok(())
                })
            },
        )?;
    }
    Ok(())
}
