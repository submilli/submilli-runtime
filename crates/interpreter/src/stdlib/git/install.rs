//! Explicit Repository method registrations and Wasm value conversion.
use super::{GitCall, MODULE_NAME};
use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, read_string_arg, register_host_value_async, write_submilli_string_struct,
    write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::stdlib::session::value;
use serde::de::DeserializeOwned;
use serde_json::Value;
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Result, Val, ValType, bail};

pub fn install(linker: &mut Linker<StoreData>) -> Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    install_repository_reads(linker, &engine, &intr)?;
    install_repository_writes(linker, &engine, &intr)
}

fn install_repository_reads(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> Result<()> {
    let argument = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let receiver = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    let bytes = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.uint8_array.clone()),
    ));
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.array.clone()),
    ));
    let repository = crate::mangle::package_symbol(MODULE_NAME, "Repository");
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "status"),
        FuncType::new(engine, [receiver.clone()], [object.clone()]),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "status")?;
                let arguments = ();
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::status).await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "log"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                prepare_history_cache(caller)?;
                let call = GitCall::new(caller, "log")?;
                let arguments = read_argument(
                    caller,
                    abi_arg(params, 1)?,
                    Some(serde_json::json!({})),
                    &call,
                )
                .await?;
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::log).await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "diff"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "diff")?;
                let arguments = read_argument(
                    caller,
                    abi_arg(params, 1)?,
                    Some(serde_json::json!({})),
                    &call,
                )
                .await?;
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::diff).await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "show"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [bytes.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "show")?;
                let arguments = super::ShowArguments {
                    revision: read_argument(caller, abi_arg(params, 1)?, None, &call).await?,
                    path: read_argument(caller, abi_arg(params, 2)?, None, &call).await?,
                };
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::show).await?;
                Ok(Val::AnyRef(Some(
                    write_submilli_uint8array_struct(caller, &result.0)?.to_anyref(),
                )))
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "branches"),
        FuncType::new(engine, [receiver.clone()], [array.clone()]),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "branches")?;
                let arguments = ();
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call
                    .execute(caller, path, arguments, super::branches)
                    .await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "remotes"),
        FuncType::new(engine, [receiver.clone()], [array.clone()]),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "remotes")?;
                let arguments = ();
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call
                    .execute(caller, path, arguments, super::remotes)
                    .await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    Ok(())
}

fn install_repository_writes(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> Result<()> {
    let argument = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let receiver = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    // Class method slots carry a value even when their source return type is void.
    let completion = argument.clone();
    let repository = crate::mangle::package_symbol(MODULE_NAME, "Repository");
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "add"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [completion.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                invalidate_history_cache(caller)?;
                let call = GitCall::new(caller, "add")?;
                let arguments = read_argument(caller, abi_arg(params, 1)?, None, &call).await?;
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                call.execute(caller, path, arguments, super::add).await?;
                crate::runtime::prelude::undefined::value(caller)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "commit"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [ValType::Ref(RefType::new(
                false,
                intr.string.clone().into(),
            ))],
        ),
        |caller, params| {
            Box::pin(async move {
                invalidate_history_cache(caller)?;
                let call = GitCall::new(caller, "commit")?;
                let arguments = read_argument(caller, abi_arg(params, 1)?, None, &call).await?;
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::commit).await?;
                Ok(Val::AnyRef(Some(
                    write_submilli_string_struct(caller, &result)?.to_anyref(),
                )))
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "createBranch"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [completion.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                invalidate_history_cache(caller)?;
                let call = GitCall::new(caller, "createBranch")?;
                let arguments = super::BranchArguments {
                    name: read_argument(caller, abi_arg(params, 1)?, None, &call).await?,
                    start: read_argument(
                        caller,
                        abi_arg(params, 2)?,
                        Some(String::from("HEAD")),
                        &call,
                    )
                    .await?,
                };
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                call.execute(caller, path, arguments, super::create_branch)
                    .await?;
                crate::runtime::prelude::undefined::value(caller)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "switchBranch"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [completion.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                invalidate_history_cache(caller)?;
                let call = GitCall::new(caller, "switchBranch")?;
                let arguments = read_argument(caller, abi_arg(params, 1)?, None, &call).await?;
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                call.execute(caller, path, arguments, super::switch_branch)
                    .await?;
                crate::runtime::prelude::undefined::value(caller)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "addRemote"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [completion.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "addRemote")?;
                let arguments = super::RemoteArguments {
                    name: read_argument(caller, abi_arg(params, 1)?, None, &call).await?,
                    url: read_argument(caller, abi_arg(params, 2)?, None, &call).await?,
                };
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                call.execute(caller, path, arguments, super::add_remote)
                    .await?;
                crate::runtime::prelude::undefined::value(caller)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "setRemoteUrl"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [completion.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let call = GitCall::new(caller, "setRemoteUrl")?;
                let arguments = super::RemoteArguments {
                    name: read_argument(caller, abi_arg(params, 1)?, None, &call).await?,
                    url: read_argument(caller, abi_arg(params, 2)?, None, &call).await?,
                };
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                call.execute(caller, path, arguments, super::set_remote_url)
                    .await?;
                crate::runtime::prelude::undefined::value(caller)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "fetch"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                invalidate_history_cache(caller)?;
                let call = GitCall::new(caller, "fetch")?;
                let arguments = super::FetchArguments {
                    remote: read_argument(
                        caller,
                        abi_arg(params, 1)?,
                        Some(String::from("origin")),
                        &call,
                    )
                    .await?,
                    branch: read_argument(caller, abi_arg(params, 2)?, Some(String::new()), &call)
                        .await?,
                };
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::fetch).await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "pull"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                invalidate_history_cache(caller)?;
                let call = GitCall::new(caller, "pull")?;
                let arguments = super::FetchArguments {
                    remote: read_argument(
                        caller,
                        abi_arg(params, 1)?,
                        Some(String::from("origin")),
                        &call,
                    )
                    .await?,
                    branch: read_argument(caller, abi_arg(params, 2)?, Some(String::new()), &call)
                        .await?,
                };
                let path = repository_path(caller, abi_arg(params, 0)?)?;
                let result = call.execute(caller, path, arguments, super::pull).await?;
                encode_json(caller, &result, call.max_bytes)
            })
        },
    )?;
    Ok(())
}

pub(super) async fn open_repository(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> Result<Val> {
    let call = GitCall::new(caller, "open")?;
    let path: String = read_argument(caller, abi_arg(params, 0)?, None, &call).await?;
    let path = normalize_repository_path(caller, &path)?;
    call.execute(caller, path.clone(), (), super::open).await?;
    Ok(Val::AnyRef(Some(
        write_submilli_string_struct(caller, &path)?.to_anyref(),
    )))
}

pub(super) async fn init_repository(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> Result<Val> {
    invalidate_history_cache(caller)?;
    let call = GitCall::new(caller, "init")?;
    let path: String = read_argument(caller, abi_arg(params, 0)?, None, &call).await?;
    let options = read_argument(
        caller,
        abi_arg(params, 1)?,
        Some(serde_json::json!({})),
        &call,
    )
    .await?;
    let path = normalize_repository_path(caller, &path)?;
    call.execute(caller, path.clone(), options, super::init)
        .await?;
    Ok(Val::AnyRef(Some(
        write_submilli_string_struct(caller, &path)?.to_anyref(),
    )))
}

pub(super) async fn clone_repository(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> Result<Val> {
    invalidate_history_cache(caller)?;
    let call = GitCall::new(caller, "clone")?;
    let url = read_argument(caller, abi_arg(params, 0)?, None, &call).await?;
    let path: String = read_argument(caller, abi_arg(params, 1)?, None, &call).await?;
    let options = read_argument(
        caller,
        abi_arg(params, 2)?,
        Some(serde_json::json!({})),
        &call,
    )
    .await?;
    let path = normalize_repository_path(caller, &path)?;
    call.execute(
        caller,
        path.clone(),
        super::CloneArguments { url, options },
        super::clone_repository,
    )
    .await?;
    Ok(Val::AnyRef(Some(
        write_submilli_string_struct(caller, &path)?.to_anyref(),
    )))
}

async fn read_argument<T: DeserializeOwned>(
    caller: &mut Caller<'_, StoreData>,
    argument: &Val,
    default: Option<T>,
    call: &GitCall,
) -> Result<T> {
    if crate::runtime::prelude::undefined::is_undefined(caller, argument)? {
        return default.ok_or_else(|| {
            wasmtime::Error::msg(format!(
                "git: undefined is not accepted for this argument to {}",
                call.operation
            ))
        });
    }
    // Serializing a guest value may run callbacks. Keep that work outside the effect
    // and within the same deadline as the worker it will start.
    let units = super::before_deadline(call.deadline, value::serialize(caller, argument)).await??;
    if units.len() as u64 * 2 > call.max_bytes {
        bail!("git: argument size limit exceeded");
    }
    fuel::charge(&mut *caller, fuel::PARSE, units.len() as u64)?;
    let text = String::from_utf16(&units)
        .map_err(|_| wasmtime::Error::msg("git: arguments must be valid Unicode"))?;
    Ok(serde_json::from_str(&text)?)
}

fn repository_path(caller: &mut Caller<'_, StoreData>, receiver: &Val) -> Result<String> {
    let field = super::class::path(caller, receiver)?;
    let path = read_string_arg(caller, &field, "git repository")?;
    normalize_repository_path(caller, &path)
}

fn normalize_repository_path(caller: &Caller<'_, StoreData>, path: &str) -> Result<String> {
    let path = crate::runtime::fs::guest_normalize(caller.data().vfs.cwd(), path)?;
    if path
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git") || super::stage::is_reserved_stage_name(part))
    {
        bail!("git: repository path targets protected metadata");
    }
    Ok(path)
}

fn prepare_history_cache(caller: &mut Caller<'_, StoreData>) -> Result<()> {
    if caller.data().git.is_none() {
        bail!("submilli:git is disabled; configure the blueprint git block");
    }
    if caller.data().git_history.is_none() {
        let cache = super::log_cache::Cache::new(&caller.data().tenant_limits)?;
        caller.data_mut().git_history = Some(std::sync::Arc::new(cache));
    }
    Ok(())
}

fn invalidate_history_cache(caller: &Caller<'_, StoreData>) -> Result<()> {
    if let Some(cache) = &caller.data().git_history {
        cache.invalidate()?;
    }
    Ok(())
}

fn encode_json(caller: &mut Caller<'_, StoreData>, result: &Value, max_bytes: u64) -> Result<Val> {
    let text = serde_json::to_string(result)?;
    if text.len() as u64 > max_bytes {
        bail!("git: result limit exceeded");
    }
    fuel::charge(&mut *caller, fuel::PARSE, 2 * text.len() as u64)?;
    value::deserialize(caller, &text.encode_utf16().collect::<Vec<_>>())
}
