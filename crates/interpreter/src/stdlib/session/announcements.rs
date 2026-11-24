//! `announceOpen` / `announceClosed`: a package tells the host which resources it holds open
//! for the session. Package attribution is checked at the host boundary, as for finalizers.
use super::{MODULE_NAME, read_key, trap};
use crate::runtime::effects;
use crate::runtime::host::{abi_arg, range_error, register_host_fn_async, type_error};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::session_announcements::ResourceState;
use crate::runtime::session_kv::SessionKvError;
use crate::runtime::{StoreData, fuel};
use crate::stdlib::shared::{check_and_approve_call, running_package};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

const CAPABILITY: &str = "session.announce";
/// The longest kind or key, in UTF-16 units: the bound finalizer keys have.
pub const MAX_NAME_UNITS: usize = 256;

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
                    check_name(name, "kind", &kind)?;
                    check_name(name, "key", &key)?;
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
                                    .map_err(|e| refusal(caller, &e))
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

/// A kind or key is 1 to [`MAX_NAME_UNITS`] units, checked here so every host refuses the same
/// names with a catchable error, including one that records nothing.
fn check_name(operation: &str, part: &str, units: &[u16]) -> wasmtime::Result<()> {
    if units.is_empty() {
        return Err(type_error(format!(
            "session.{operation}: the {part} must not be empty"
        )));
    }
    if units.len() > MAX_NAME_UNITS {
        return Err(range_error(format!(
            "session.{operation}: the {part} is longer than {MAX_NAME_UNITS} UTF-16 units"
        )));
    }
    Ok(())
}

/// A host's refusal (a name it does not take, or too many resources) is catchable: nothing was
/// recorded. A closed session or a storage failure ends the run, as for session state.
fn refusal(caller: &wasmtime::Caller<'_, StoreData>, error: &SessionKvError) -> wasmtime::Error {
    match error {
        SessionKvError::InvalidKey { .. } => type_error(error.to_string()),
        SessionKvError::LimitExceeded { .. } => range_error(error.to_string()),
        SessionKvError::SessionClosed { .. } | SessionKvError::Backend { .. } => {
            trap(caller, None, error)
        }
    }
}
