//! Capability-controlled Git operations. No subprocesses or guest-visible credentials.
use crate::runtime::effects;
use crate::runtime::host::permission_denied;
pub(crate) mod class;
pub mod declaration;
mod diff;
mod history;
mod index_limits;
mod install;
mod location;
mod lock;
pub(crate) mod log_cache;
mod metadata_scan;
mod meter;
mod object;
mod operations;
mod pack_index_check;
mod pack_limits;
mod stage;
mod storage;
mod transport;
mod work;
mod worker;

use crate::runtime::Vfs;
use crate::runtime::decision::{CallOrigin, CallSite, EntryPath};
use crate::runtime::fuel;
use crate::runtime::{HttpClient, SecretProvider, SecurityCheck, StoreData};
use crate::stdlib::shared::running_package;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use wasmtime::{Caller, Result, bail};

static WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

pub const MODULE_NAME: &str = "submilli:git";
pub use declaration::package_declaration;
pub use install::install;
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
    /// When the operation times out, waits included.
    deadline: tokio::time::Instant,
    cancelled: Arc<AtomicBool>,
    max_bytes: u64,
    transferred: Arc<AtomicU64>,
    /// The worker's own file and object work, charged when it returns.
    meter: Arc<meter::Meter>,
    algorithm_fuel: Arc<work::AlgorithmWork>,
    denial: Arc<Mutex<Option<(String, String)>>>,
    history_cache: Option<Arc<log_cache::Cache>>,
    /// Where in the program, and in a package, the operation started; captured only
    /// while recording.
    origin: CallOrigin,
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
        // Git's decisions are made on a worker thread, after the program's stack is gone;
        // they reuse the origin the host call captured on entry.
        let site = CallSite::new(
            self.security
                .recorder()
                .map(|recorder| recorder.begin_call(&self.caller, capability, self.origin.clone())),
            EntryPath::Git,
        );
        crate::stdlib::shared::check_and_audit(
            self.security.as_ref(),
            &self.caller,
            capability,
            &context,
            "/",
            site,
        )
        .map_err(|reason| {
            // Poison may leave the denied capability and reason partly updated.
            // AGENTS.md permits poisoned-lock panics rather than recovering this
            // attribution; it does not permit the panic that caused poisoning.
            let mut denial = self.denial.lock().expect("git worker denial lock poisoned");
            *denial = Some((capability.to_owned(), reason.clone()));
            permission_denied(&self.caller, capability, reason)
        })
    }

    /// One of the workers Git shares across the process, waited for until
    /// the operation's deadline or its cancellation.
    fn worker_permit(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        let workers = WORKERS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
            .clone();
        // Polled from the worker's own thread, which is not the runtime's.
        loop {
            match workers.clone().try_acquire_owned() {
                Ok(permit) => return Ok(permit),
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err(crate::runtime::host::fatal_host_error(
                        "git: workers closed",
                    ));
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {}
            }
            self.check_cancelled()?;
            if tokio::time::Instant::now() >= self.deadline {
                bail!("git: operation timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
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

fn open(vfs: &Vfs, job: &Job, (): ()) -> Result<()> {
    let (_permit, _snapshot) = worker::open_repository(vfs, job)?;
    Ok(())
}

fn status(vfs: &Vfs, job: &Job, (): ()) -> Result<Value> {
    let (_permit, snapshot) = worker::open_repository(vfs, job)?;
    operations::status(&snapshot)
}

fn branches(vfs: &Vfs, job: &Job, (): ()) -> Result<Value> {
    let (_permit, snapshot) = worker::open_repository(vfs, job)?;
    operations::branches(&snapshot)
}

fn remotes(vfs: &Vfs, job: &Job, (): ()) -> Result<Value> {
    let (_permit, snapshot) = worker::open_repository(vfs, job)?;
    operations::remotes(&snapshot)
}

fn log(vfs: &Vfs, job: &Job, arguments: Value) -> Result<Value> {
    worker::validate_branch_options(&arguments)?;
    let (_permit, snapshot) = worker::open_repository(vfs, job)?;
    operations::log(&snapshot, &arguments)
}

fn diff(vfs: &Vfs, job: &Job, arguments: Value) -> Result<Value> {
    worker::validate_branch_options(&arguments)?;
    let (_permit, snapshot) = worker::open_repository(vfs, job)?;
    diff::read(&snapshot, &arguments)
}

fn show(vfs: &Vfs, job: &Job, arguments: ShowArguments) -> Result<effects::OutcomeBytes> {
    let (_permit, snapshot) = worker::open_repository(vfs, job)?;
    operations::show(&snapshot, &arguments.revision, &arguments.path).map(effects::OutcomeBytes)
}

fn init(vfs: &Vfs, job: &Job, arguments: Value) -> Result<()> {
    let branch = worker::requested_branch(&arguments, "main")?;
    job.check("git.init", serde_json::json!({"path":job.path}))?;
    let (_permit, snapshot) = worker::create_repository(vfs, job, branch)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn clone_repository(vfs: &Vfs, job: &Job, arguments: CloneArguments) -> Result<()> {
    let branch = worker::requested_branch(&arguments.options, "")?;
    let url = transport::canonical_url(&arguments.url)?;
    job.check(
        "git.clone",
        serde_json::json!({"path":job.path,"remote":url,"remoteName":"origin","branch":branch}),
    )?;
    let (_permit, mut snapshot) = worker::create_clone_repository(vfs, job, branch)?;
    worker::clone_repository(&mut snapshot, job, &arguments.url, branch)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn add(vfs: &Vfs, job: &Job, arguments: Vec<String>) -> Result<()> {
    let (_permit, snapshot) = worker::open_repository_for_write(vfs, job)?;
    operations::add(&snapshot, &arguments)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn commit(vfs: &Vfs, job: &Job, arguments: String) -> Result<String> {
    let (_permit, snapshot) = worker::open_repository_for_write(vfs, job)?;
    job.check(
        "git.commit",
        serde_json::json!({"path":job.path,"branch":operations::current_branch(&snapshot)?}),
    )?;
    let id = operations::commit(&snapshot, &arguments, &job.config)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(id)
}

fn create_branch(vfs: &Vfs, job: &Job, arguments: BranchArguments) -> Result<()> {
    let (_permit, snapshot) = worker::open_repository_for_write(vfs, job)?;
    operations::create_branch(&snapshot, &arguments.name, &arguments.start)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn switch_branch(vfs: &Vfs, job: &Job, arguments: String) -> Result<()> {
    let (_permit, snapshot) = worker::open_repository_for_write(vfs, job)?;
    operations::checkout(&snapshot, &arguments)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn add_remote(vfs: &Vfs, job: &Job, arguments: RemoteArguments) -> Result<()> {
    let (_permit, mut snapshot) = worker::open_repository_for_write(vfs, job)?;
    operations::add_remote(&mut snapshot, &arguments.name, &arguments.url)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn set_remote_url(vfs: &Vfs, job: &Job, arguments: RemoteArguments) -> Result<()> {
    let (_permit, mut snapshot) = worker::open_repository_for_write(vfs, job)?;
    operations::set_remote_url(&mut snapshot, &arguments.name, &arguments.url)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(())
}

fn fetch(vfs: &Vfs, job: &Job, arguments: FetchArguments) -> Result<Value> {
    let (_permit, snapshot) = worker::open_repository_for_write(vfs, job)?;
    let branches = transport::fetch(&snapshot, job, &arguments.remote, &arguments.branch)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(serde_json::json!({"branches":branches.branches}))
}

fn pull(vfs: &Vfs, job: &Job, arguments: FetchArguments) -> Result<Value> {
    let (_permit, snapshot) = worker::open_repository_for_write(vfs, job)?;
    let result = transport::pull(&snapshot, job, &arguments.remote, &arguments.branch)?;
    job.check_cancelled()?;
    job.algorithm_fuel.before_effect();
    snapshot.publish()?;
    Ok(result)
}

#[derive(Serialize)]
struct ShowArguments {
    revision: String,
    path: String,
}
#[derive(Serialize)]
struct CloneArguments {
    url: String,
    options: Value,
}
#[derive(Serialize)]
struct BranchArguments {
    name: String,
    start: String,
}
#[derive(Serialize)]
struct RemoteArguments {
    name: String,
    url: String,
}
#[derive(Serialize)]
struct FetchArguments {
    remote: String,
    branch: String,
}

#[derive(Serialize)]
struct GitRequest<A> {
    operation: &'static str,
    path: String,
    arguments: A,
}

/// A single call's deadline and native-memory reservation. The blocking worker
/// retains the reservation if the awaiting guest is cancelled.
struct GitCall {
    operation: &'static str,
    config: GitConfig,
    deadline: tokio::time::Instant,
    budget: Arc<WorkingBudget>,
    max_bytes: u64,
}
impl GitCall {
    fn new(caller: &Caller<'_, StoreData>, operation: &'static str) -> Result<Self> {
        let deadline = tokio::time::Instant::now()
            .checked_add(std::time::Duration::from_secs(60))
            .ok_or_else(|| {
                crate::runtime::host::fatal_host_error("git deadline is out of range")
            })?;
        let config = caller.data().git.clone().ok_or_else(|| {
            wasmtime::Error::msg("submilli:git is disabled; configure the blueprint git block")
        })?;
        let budget = Arc::new(WorkingBudget::reserve(
            &caller.data().tenant_limits,
            operation,
        )?);
        let max_bytes = budget.max_bytes(operation);
        Ok(Self {
            operation,
            config,
            deadline,
            budget,
            max_bytes,
        })
    }

    async fn execute<A, T, F>(
        &self,
        caller: &mut Caller<'_, StoreData>,
        path: String,
        arguments: A,
        operation: F,
    ) -> Result<T>
    where
        A: Serialize + Send + 'static,
        T: Serialize + DeserializeOwned + Send + 'static,
        F: FnOnce(&Vfs, &Job, A) -> Result<T> + Send + 'static,
    {
        let op = self.operation;
        let config = self.config.clone();
        let deadline = self.deadline;
        let max_bytes = self.max_bytes;
        let worker_budget = Arc::clone(&self.budget);
        let request = GitRequest {
            operation: op,
            path,
            arguments,
        };
        effects::execute(caller, "git", request, move |caller, _, request| {
            Box::pin(async move {
                let path = request.path;
                let args = request.arguments;
                let budget = worker_budget;
                let principal = running_package(&*caller).map_err(|_| {
                    crate::runtime::host::fatal_host_error("git: cannot identify caller")
                })?;
                let cancelled = Arc::new(AtomicBool::new(false));
                let cancel_guard = CancelOnDrop(cancelled.clone());
                // The worker stops itself once its work passes what the run has left.
                let meter = Arc::new(meter::Meter::new(
                    fuel::remaining_fuel(&mut *caller)?,
                    Some(cancelled.clone()),
                ));
                let job = Job {
                    op: op.to_owned(),
                    path: path.clone(),
                    caller: principal,
                    config,
                    security: caller.data().security_check.clone(),
                    secrets: caller.data().secret_provider.clone(),
                    http: caller.data().http_client.clone(),
                    runtime: tokio::runtime::Handle::try_current().map_err(|_| {
                        crate::runtime::host::fatal_host_error(
                            "git worker requires a Tokio runtime",
                        )
                    })?,
                    deadline,
                    cancelled,
                    max_bytes,
                    transferred: Arc::new(AtomicU64::new(0)),
                    meter: Arc::clone(&meter),
                    algorithm_fuel: Arc::new(work::AlgorithmWork::with_meter(Arc::clone(&meter))),
                    denial: Arc::new(Mutex::new(None)),
                    history_cache: caller.data().git_history.clone(),
                    origin: crate::stdlib::shared::call_origin(&*caller),
                };
                let vfs = caller.data().vfs.clone();
                let denial = job.denial.clone();
                let principal = job.caller.clone();
                let worker = async {
                    caller
                        .data_mut()
                        .blocking_work
                        .spawn(move || {
                            job.check_cancelled()?;
                            let _budget = budget;
                            operation(&vfs, &job, args)
                        })
                        .await
                        .map_err(blocking_worker_failure)?
                };
                let outcome = finish_worker(worker, &cancel_guard.0, deadline).await;
                // The work is done, and may be published, whether or not it succeeded:
                // it is settled, not refused.
                meter.settle(&mut *caller)?;
                if let Err(error) = &outcome
                    && meter.is_exhausted()
                    && !crate::runtime::host::ends_the_run(error)
                    && !error.is::<stage::NeedsHostRecovery>()
                {
                    // Stopped for fuel before publishing: the run ends as a Wasm loop
                    // out of fuel would, and the program cannot catch it. A host failure,
                    // or a publication that needs recovery, says more and goes first.
                    return Err(wasmtime::Trap::OutOfFuel.into());
                }

                let outcome = match outcome {
                    Err(error)
                        if crate::runtime::host::ends_the_run(&error)
                            || error.is::<stage::NeedsHostRecovery>() =>
                    {
                        if error.is::<stage::NeedsHostRecovery>() {
                            return Err(crate::runtime::host::fatal_host_error(error));
                        }
                        return Err(error);
                    }
                    other => other,
                };
                // Poison may leave denial attribution partly updated by the worker. AGENTS.md
                // permits this poisoned-lock panic instead of recovery, including after worker
                // cleanup; it does not permit the panic that caused poisoning.
                let denial = denial.lock().expect("git: denial lock poisoned").take();
                drop(cancel_guard);
                if let Some((capability, reason)) = denial {
                    return Err(permission_denied(&principal, &capability, reason));
                }
                outcome
            })
        })
        .await
    }
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
            match worker.await {
                Err(worker_error)
                    if crate::runtime::host::ends_the_run(&worker_error)
                        || worker_error.is::<stage::NeedsHostRecovery>() =>
                {
                    Err(worker_error)
                }
                _ => Err(error),
            }
        }
    }
}

fn blocking_worker_failure(error: crate::runtime::blocking::BlockingWorkError) -> wasmtime::Error {
    crate::runtime::host::fatal_host_error("git blocking worker failed").context(error)
}

/// The least Git works with: what opening a repository and a fetch's index
/// buffers take, measured at under 1 MB, with room to spare.
const MIN_WORKING_BYTES: u64 = 4 * 1024 * 1024;

/// Working memory reserved for one Git operation against the same cap as
/// Wasm, while the program waits for it.
///
/// gix runs natively, outside the metered heap, so the operation reserves
/// three quarters of what the run has free, and bounds what it holds by
/// counting: trees, the index, worktree paths, a blob, a diff, a fetched
/// pack's delta chains. Each count is held to the reservation divided by
/// what the operation was measured to need per byte counted, beyond its
/// fixed working set (`tests/git_memory.rs`).
struct WorkingBudget {
    bytes: u64,
    counter: Arc<std::sync::atomic::AtomicU64>,
}
impl WorkingBudget {
    fn reserve(limits: &crate::runtime::limits::TenantLimits, op: &str) -> Result<Self> {
        let available = limits
            .max_total_bytes
            .saturating_sub(limits.observed_bytes())
            .saturating_sub(limits.host_attached_bytes());
        let bytes = available / 4 * 3;
        if bytes < MIN_WORKING_BYTES {
            const MB: u64 = 1024 * 1024;
            bail!(
                "git.{op} needs at least {} MB of working memory; {} MB is free under \
                 max_execution_memory ({} MB). Raise max_execution_memory",
                MIN_WORKING_BYTES * 4 / 3 / MB,
                available / MB,
                limits.max_total_bytes / MB
            );
        }
        limits.charge_host_bytes(bytes)?;
        Ok(Self {
            bytes,
            counter: limits.host_attached_counter(),
        })
    }

    /// What `op` may count while it works: the reservation divided by what
    /// it was measured to hold per byte counted. A diff holds its patch two
    /// or three times over as it is built and encoded; everything else
    /// holds at most about three times what it counts.
    fn max_bytes(&self, op: &str) -> u64 {
        let ratio = if op == "diff" { 8 } else { 4 };
        (self.bytes.saturating_sub(MIN_WORKING_BYTES / 4) / ratio).min(storage::MAX_WORKING_BYTES)
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
