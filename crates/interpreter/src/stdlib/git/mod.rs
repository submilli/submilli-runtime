//! Capability-controlled Git operations. No subprocesses or guest-visible credentials.
pub(crate) mod class;
pub mod declaration;
mod history;
mod index_limits;
mod location;
mod lock;
mod metadata_scan;
mod operations;
mod pack_index_check;
mod pack_limits;
mod stage;
mod storage;
mod transport;
mod worker;

use crate::runtime::fuel;
use crate::runtime::host::{
    permission_denied, read_string_arg, register_host_fn_async, write_submilli_string_struct,
    write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::security::CheckOutcome;
use crate::runtime::{HttpClient, SecretProvider, SecurityCheck, StoreData};
use crate::stdlib::{session::value, shared::running_package};
use serde_json::Value;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Result, Val, ValType, bail};

static WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

pub const MODULE_NAME: &str = "submilli:git";
pub use declaration::package_declaration;
pub(crate) use transport::canonical_url;

#[derive(Clone, Debug)]
pub struct GitConfig {
    pub name: String,
    pub email: String,
    pub username: Option<String>,
}

#[derive(Clone)]
struct Job {
    op: String,
    path: String,
    caller: String,
    config: GitConfig,
    security: Arc<dyn SecurityCheck>,
    secrets: Arc<dyn SecretProvider>,
    http: Arc<dyn HttpClient>,
    runtime: tokio::runtime::Handle,
    cancelled: Arc<AtomicBool>,
    max_bytes: u64,
    transferred: Arc<AtomicU64>,
    denial: Arc<Mutex<Option<(String, String)>>>,
}
impl Job {
    fn remote_capability(&self) -> &'static str {
        if self.op == "clone" {
            "git.clone"
        } else {
            "git.fetch"
        }
    }

    fn check(&self, capability: &str, context: Value) -> Result<()> {
        self.check_cancelled()?;
        match self.security.check(&self.caller, capability, &context) {
            CheckOutcome::Allow => Ok(()),
            CheckOutcome::Deny { reason } => {
                if let Ok(mut denial) = self.denial.lock() {
                    *denial = Some((capability.to_owned(), reason.clone()));
                }
                Err(permission_denied(&self.caller, capability, reason))
            }
        }
    }
    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        Ok(())
    }
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

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
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "status"),
        FuncType::new(engine, [receiver.clone()], [object.clone()]),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "status", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "log"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [object.clone()],
        ),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "log", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "diff"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone()],
            [object.clone()],
        ),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "diff", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "show"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [bytes.clone()],
        ),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "show", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "branches"),
        FuncType::new(engine, [receiver.clone()], [array.clone()]),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "branches", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "remotes"),
        FuncType::new(engine, [receiver.clone()], [array.clone()]),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "remotes", true, params).await?;
                Ok(())
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
    let repository = crate::mangle::package_symbol(MODULE_NAME, "Repository");
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "add"),
        FuncType::new(engine, [receiver.clone(), argument.clone()], []),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                invoke(caller, "add", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
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
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "commit", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "createBranch"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [],
        ),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                invoke(caller, "createBranch", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "switchBranch"),
        FuncType::new(engine, [receiver.clone(), argument.clone()], []),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                invoke(caller, "switchBranch", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "addRemote"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [],
        ),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                invoke(caller, "addRemote", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "setRemoteUrl"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [],
        ),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                invoke(caller, "setRemoteUrl", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "fetch"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [object.clone()],
        ),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "fetch", true, params).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&repository, "pull"),
        FuncType::new(
            engine,
            [receiver.clone(), argument.clone(), argument.clone()],
            [object.clone()],
        ),
        false,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = invoke(caller, "pull", true, params).await?;
                Ok(())
            })
        },
    )?;
    Ok(())
}

async fn invoke(
    caller: &mut Caller<'_, StoreData>,
    op: &str,
    method: bool,
    params: &[Val],
) -> Result<Val> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    let config = caller.data().git.clone().ok_or_else(|| {
        wasmtime::Error::msg("submilli:git is disabled; configure the blueprint git block")
    })?;
    let budget = WorkingBudget::reserve(&caller.data().tenant_limits)?;
    let max_bytes = (budget.bytes / 16).min(storage::MAX_BYTES);
    let (args, path) = before_deadline(
        deadline,
        decode_arguments(caller, op, method, params, max_bytes),
    )
    .await??;
    let principal = running_package(&*caller)
        .map_err(|_| wasmtime::Error::msg("git: cannot identify caller"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_guard = CancelOnDrop(cancelled.clone());
    let job = Job {
        op: op.to_owned(),
        path: path.clone(),
        caller: principal,
        config,
        security: caller.data().security_check.clone(),
        secrets: caller.data().secret_provider.clone(),
        http: caller.data().http_client.clone(),
        runtime: tokio::runtime::Handle::current(),
        cancelled,
        max_bytes,
        transferred: Arc::new(AtomicU64::new(0)),
        denial: Arc::new(Mutex::new(None)),
    };
    let workers = WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
        .clone();
    let permit = before_deadline(deadline, workers.acquire_owned()).await??;
    let vfs = caller.data().vfs.clone();
    let op = op.to_owned();
    let is_constructor = !method;
    let returns_string = op == "commit";
    let denial = job.denial.clone();
    let principal = job.caller.clone();
    let transferred = job.transferred.clone();
    let worker = caller.data_mut().blocking_work.spawn(move || {
        let _permit = permit;
        job.check_cancelled()?;
        worker::run(&vfs, &job, &op, &args).map(|result| (result, budget))
    });
    let outcome = finish_worker(worker, &cancel_guard.0, deadline).await;
    if let Some((capability, reason)) = denial
        .lock()
        .map_err(|_| wasmtime::Error::msg("git: denial lock poisoned"))?
        .take()
    {
        return Err(permission_denied(&principal, &capability, reason));
    }
    let (result, _budget) = outcome?;
    drop(cancel_guard);
    // Network bytes the worker moved; the operation is published, so this is
    // settled, not refused. The worker's own file and object work is not
    // counted yet (SUB-1129 reshapes it).
    fuel::settle(&mut *caller, fuel::IO, transferred.load(Ordering::Relaxed))?;
    encode_result(
        caller,
        result,
        &path,
        is_constructor,
        returns_string,
        max_bytes,
    )
}

async fn before_deadline<T>(
    deadline: tokio::time::Instant,
    future: impl std::future::Future<Output = T>,
) -> Result<T> {
    if tokio::time::Instant::now() >= deadline {
        bail!("git: operation timed out");
    }
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| wasmtime::Error::msg("git: operation timed out"))
}

async fn finish_worker<T>(
    worker: impl std::future::Future<Output = Result<T>>,
    cancelled: &AtomicBool,
    deadline: tokio::time::Instant,
) -> Result<T> {
    tokio::pin!(worker);
    match before_deadline(deadline, &mut worker).await {
        Ok(result) => result,
        Err(error) => {
            cancelled.store(true, Ordering::Relaxed);
            // A catchable timeout must not let guest code resume while the
            // worker is still publishing or rolling back repository changes.
            let _ = worker.await;
            Err(error)
        }
    }
}

async fn decode_arguments(
    caller: &mut Caller<'_, StoreData>,
    op: &str,
    method: bool,
    params: &[Val],
    max_bytes: u64,
) -> Result<(Vec<Value>, String)> {
    let mut args = Vec::new();
    let offset = usize::from(method);
    for param in &params[offset..] {
        let units = value::serialize(caller, param).await?;
        if units.len() as u64 * 2 > max_bytes {
            bail!("git: argument size limit exceeded");
        }
        fuel::charge(&mut *caller, fuel::PARSE, units.len() as u64)?;
        let text = String::from_utf16(&units)
            .map_err(|_| wasmtime::Error::msg("git: arguments must be valid Unicode"))?;
        args.push(serde_json::from_str::<Value>(&text)?);
    }
    let path = if method {
        let field = class::path(caller, &params[0])?;
        read_string_arg(&mut *caller, &field, "git repository")?
    } else {
        operations::text_arg(&args, usize::from(op == "clone"))?.to_owned()
    };
    let path = crate::runtime::fs::guest_normalize("/", &path)?;
    if path
        .split('/')
        .any(|part| part.eq_ignore_ascii_case(".git") || part.starts_with(".git-submilli-"))
    {
        bail!("git: repository path targets protected metadata");
    }
    Ok((args, path))
}

fn encode_result(
    caller: &mut Caller<'_, StoreData>,
    result: worker::Output,
    path: &str,
    is_constructor: bool,
    returns_string: bool,
    max_bytes: u64,
) -> Result<Val> {
    if is_constructor {
        let path = write_submilli_string_struct(caller, path)?;
        return Ok(Val::AnyRef(Some(path.to_anyref())));
    }
    let result = match result {
        worker::Output::Bytes(bytes) => {
            let value = write_submilli_uint8array_struct(caller, &bytes)?;
            return Ok(Val::AnyRef(Some(value.to_anyref())));
        }
        worker::Output::Json(value) => value,
    };
    if returns_string {
        let text = result
            .as_str()
            .ok_or_else(|| wasmtime::Error::msg("git: invalid string result"))?;
        let value = write_submilli_string_struct(caller, text)?;
        return Ok(Val::AnyRef(Some(value.to_anyref())));
    }
    let text = serde_json::to_string(&result)?;
    if text.len() > max_bytes as usize {
        bail!("git: result limit exceeded");
    }
    // Serialized and parsed back; the operation is already published.
    fuel::settle(&mut *caller, fuel::PARSE, 2 * text.len() as u64)?;
    value::deserialize(caller, &text.encode_utf16().collect::<Vec<_>>())
}

// Reserve working memory against the same tenant cap as Wasm. Snapshot and
// transport limits use a fraction of this reservation to cover simultaneous
// trees, index data, diff/JSON expansion, and gix decoding buffers.
struct WorkingBudget {
    bytes: u64,
    counter: Arc<std::sync::atomic::AtomicU64>,
}
impl WorkingBudget {
    fn reserve(limits: &crate::runtime::limits::TenantLimits) -> Result<Self> {
        let available = limits
            .max_total_bytes
            .saturating_sub(limits.observed_bytes())
            .saturating_sub(limits.host_attached_bytes());
        let bytes = available / 4 * 3;
        if bytes < 2 * 1024 * 1024 {
            bail!("git: insufficient tenant memory for repository work");
        }
        limits.charge_host_bytes(bytes)?;
        Ok(Self {
            bytes,
            counter: limits.host_attached_counter(),
        })
    }
}
impl Drop for WorkingBudget {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration_tests;
