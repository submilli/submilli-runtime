//! Wasmtime engine configuration for Submilli.

pub(crate) mod array_storage;
pub mod blocking;
pub mod disk_quota;
pub mod exec;
pub mod fs;
pub mod fuel;
pub mod gc_singleton;
pub mod host;
pub mod intrinsic_types;
pub mod json;
pub mod limits;
pub mod llm;
pub mod mcp;
pub mod metrics;
pub mod number;
pub mod prelude;
pub mod secrets;
pub mod security;
pub mod session_kv;
pub mod vfs;
pub mod watchdog;

pub use disk_quota::{DiskQuota, Holder, OpenFileGuard, QuotaCharge, QuotaExceeded};
pub use exec::{RunResult, dispatch_main_async, instantiate_program_async};
pub use host::{
    INTERNAL_MODULE_NAME, NUMBER_MODULE_NAME, host_package_declarations,
    install_async as install_runtime_async,
    install_host_functions as install_runtime_host_functions,
    install_store_bound as install_runtime_store_bound, internal_host_package_declarations,
    stdlib_package_declarations,
};
pub use json::JSON_MODULE_NAME;
pub use limits::{
    DEFAULT_MAX_STORE_BYTES, MemoryCapExceeded, MemoryExhausted, TenantLimits,
    install_tenant_limits, is_memory_exhausted,
};
pub use llm::{
    DEFAULT_MAX_ALL_EXECUTIONS_TOKENS, DEFAULT_MAX_EXECUTION_TOKENS, ExecutionTokenBudget,
    FailureReason, LLM_MODULE_NAME, LlmCallError, LlmFailure, LlmLimitKind, LlmLimits, LlmModel,
    LlmOutcome, LlmProvider, PromptBoundKind, SharedTokenBudget,
};
pub use mcp::{
    MCP_MODULE_NAME, McpCallError, McpTransport, install_mcp_async, mcp_call_package_declaration,
};
pub use metrics::{HttpMetric, MetricsSink, NoopMetricsSink};
pub use prelude::bigint::ops::BIGINT_MODULE_NAME;
pub use prelude::temporal::shared::TEMPORAL_MODULE_NAME;
pub use secrets::{NoopSecretProvider, SecretProvider};
pub use security::{AllowAllCheck, CheckOutcome, SecurityCheck};
pub use session_kv::{
    InMemorySessionKv, SessionKvEntry, SessionKvError, SessionKvLimitKind, SessionKvLimits,
    SessionKvPage, SessionKvStore, SharedKvBudget,
};
pub use vfs::{
    Access, MountError, MountSpec, Vfs, VfsMode, measure_dir, measure_host_dir, measure_with_held,
    regular_files,
};
pub use watchdog::Watchdog;

pub use crate::stdlib::http::{
    AuthProxy, AuthProxyError, HttpClient, HttpError, HttpRequest, HttpResponse, NetworkPolicy,
    NoopAuthProxy, ReqwestHttpClient,
};

use std::cell::RefCell;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wasmtime::{
    ArrayRef, AsContextMut, Config, Engine, Instance, Linker, Module, OptLevel, Rooted, Store,
    WasmBacktraceDetails,
};

use crate::{PackageDeclaration, TypeInfoTable};

/// Filesystem metadata surfaced to scripts via `submilli:fs.info()`: the
/// active mode and the byte cap the VFS enforces, so the script — and the LLM —
/// can branch on the sandbox shape. Never exposes the host path.
#[derive(Debug, Clone)]
pub struct VfsInfo {
    pub mode: VfsMode,
    pub size_limit: Option<u64>,
}

pub struct StoreData {
    #[cfg(test)]
    pub(crate) array_growth: array_storage::GrowthStats,
    /// After cancelling a host call, the execution owner drains this before
    /// reusing or releasing the store. Blocking work may still be cleaning up.
    pub blocking_work: blocking::BlockingWork,
    pub git: Option<crate::stdlib::git::GitConfig>,
    pub console: Box<dyn Write + Send>,
    pub vfs: Vfs,
    pub vfs_info: VfsInfo,
    pub security_check: Arc<dyn SecurityCheck>,
    pub fs_max_read_size: u64,
    pub http_client: Arc<dyn HttpClient>,
    pub http_max_response_size: u64,
    pub auth_proxy: Arc<dyn AuthProxy>,
    pub secret_provider: Arc<dyn SecretProvider>,
    /// The outbound `@mcp/<server>` transport the `submilli:mcp.call` host fn
    /// dispatches through, present when the embedder wires one. `None` in the
    /// pure-interpreter path, where MCP calls throw "transport not configured".
    pub mcp_transport: Option<Arc<dyn McpTransport>>,
    /// The model provider `submilli:llm` dispatches through, present only when
    /// the embedder wires one. Left `None` the runtime has no model access at
    /// all rather than silently completing against some default — the guest
    /// surface reports that as a catchable configuration error
    /// ([`LlmCallError::NotConfigured`]), the same rule `session_kv` follows.
    pub llm_provider: Option<Arc<dyn LlmProvider>>,
    /// This execution's token budget, reserving against its own ceiling and the
    /// server-wide one together. `None` in the pure-interpreter path, where
    /// there is no aggregate to protect and nothing to charge against; the
    /// prompt-count and prompt-size bounds still apply there, because they
    /// bound pathological shapes rather than spend. The embedder installs one
    /// per execution, and dropping it is what returns the reservation.
    pub llm_budget: Option<Arc<ExecutionTokenBudget>>,
    /// Session-scoped key-value storage, present only when the embedder wires a
    /// provider. Left `None` the store stays absent rather than silently
    /// becoming per-execution scratch state that no later `execute` can read —
    /// the guest surface reports that as a configuration error.
    pub session_kv: Option<Arc<dyn SessionKvStore>>,
    /// Embedder sink for host-operation metrics (HTTP transport latencies).
    /// Defaults to [`NoopMetricsSink`]; the server installs a Sentry-backed one.
    pub metrics: Arc<dyn metrics::MetricsSink>,
    pub tenant_limits: TenantLimits,
    /// Fuel charged by host functions for their own work; the rest of the fuel
    /// spent went to Wasm instructions. See [`fuel::charge_host_fuel`].
    pub host_fuel: u64,
    /// Host charges not yet applied to the engine's fuel (see
    /// [`fuel::HOST_FUEL_BATCH`]).
    pub host_fuel_pending: u64,
    /// The engine's fuel right after the last application of pending host
    /// charges; `None` before the first.
    pub host_fuel_applied_at: Option<u64>,
    /// Test-segment labels recorded by `submilli:test.label`, in call order.
    /// Only the test runner installs that host fn; an ordinary run leaves this
    /// empty. The runner reads it after `main()` returns to attribute the
    /// pass/fail outcome to the segment that was open at the time.
    pub test_labels: RefCell<Vec<String>>,
    /// Recovered runtime types + the prelude instance handle host functions use
    /// to build *real* `$string`/`$Array` structs (vtable + payload) instead of
    /// raw arrays. `None` until the prelude instantiates; set by
    /// `install_prelude_async`. See [`crate::runtime::host::HostAbi`].
    pub host_abi: Option<crate::runtime::host::HostAbi>,
    /// This store's intrinsic types, built on first use; see
    /// [`crate::runtime::intrinsic_types::intrinsic_types`].
    pub(crate) intrinsic_types: Option<Arc<crate::runtime::intrinsic_types::IntrinsicTypes>>,
    /// The bound-receiver closure environment type, built on first use for the
    /// same reason as [`Self::intrinsic_types`].
    pub(crate) closure_receiver_type: Option<wasmtime::StructType>,
    /// The call-metadata closure environment type, built on first use for the
    /// same reason as [`Self::intrinsic_types`].
    pub(crate) call_metadata_type: Option<wasmtime::StructType>,
    /// Runtime type metadata keyed by package name.
    pub type_info: std::collections::BTreeMap<String, TypeInfoTable>,
    /// Depth of the in-flight universal-vtable walk; see
    /// [`MAX_VTABLE_WALK_DEPTH`].
    pub vtable_walk_depth: u32,
}

/// The nesting the universal-vtable walk allows before it reports a runaway.
///
/// Pinned to `serde_json`'s own recursion limit, which is what bounds
/// `JSON.parse`: a document the runtime is willing to parse must still be
/// comparable and re-serializable, or `JSON.parse` would accept graphs that
/// `JSON.stringify` then refuses. Measured headroom: a debug build on a 2 MB
/// test-harness thread aborts between 160 and 200 levels, so the bound sits
/// below the point where the native stack runs out.
pub(crate) const MAX_VTABLE_WALK_DEPTH: u32 = 128;

pub const DEFAULT_FS_MAX_READ_SIZE: u64 = 50 * 1024 * 1024;

pub const DEFAULT_HTTP_MAX_RESPONSE_SIZE: u64 = 50 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct LinkedPackageModule<'a> {
    pub module: &'a Module,
    pub declaration: &'a PackageDeclaration,
    pub type_info: &'a TypeInfoTable,
}

impl StoreData {
    pub fn with_vfs(vfs: Vfs) -> Self {
        Self::with_vfs_and_cap(vfs, DEFAULT_MAX_STORE_BYTES)
    }

    pub fn with_vfs_and_cap(vfs: Vfs, max_store_bytes: u64) -> Self {
        let vfs_info = VfsInfo {
            mode: vfs.mode(),
            size_limit: None,
        };
        Self {
            #[cfg(test)]
            array_growth: array_storage::GrowthStats::default(),
            console: Box::new(std::io::stderr()),
            vfs,
            vfs_info,
            security_check: security::default_check(),
            git: None,
            blocking_work: blocking::BlockingWork::default(),
            fs_max_read_size: DEFAULT_FS_MAX_READ_SIZE,
            http_client: crate::stdlib::http::default_http_client(),
            http_max_response_size: DEFAULT_HTTP_MAX_RESPONSE_SIZE,
            auth_proxy: crate::stdlib::http::default_auth_proxy(),
            secret_provider: Arc::new(secrets::NoopSecretProvider),
            mcp_transport: None,
            llm_provider: None,
            llm_budget: None,
            session_kv: None,
            metrics: Arc::new(metrics::NoopMetricsSink),
            tenant_limits: TenantLimits::new(max_store_bytes),
            host_fuel: 0,
            host_fuel_pending: 0,
            host_fuel_applied_at: None,
            test_labels: RefCell::new(Vec::new()),
            host_abi: None,
            intrinsic_types: None,
            closure_receiver_type: None,
            call_metadata_type: None,
            type_info: std::collections::BTreeMap::new(),
            vtable_walk_depth: 0,
        }
    }

    pub fn install_type_info(&mut self, table: TypeInfoTable) {
        self.type_info.insert(table.package_name.clone(), table);
    }

    pub fn with_tempdir() -> std::io::Result<Self> {
        Ok(Self::with_vfs(Vfs::tempdir()?))
    }
}

pub async fn install_package_modules_async(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    packages: &[LinkedPackageModule<'_>],
) -> wasmtime::Result<()> {
    for package in packages {
        let name = package.declaration.package_name.as_str();
        if store.data().git.is_none()
            && package
                .module
                .imports()
                .any(|import| import.module() == "submilli:git")
        {
            wasmtime::bail!(
                "package `{name}` imports submilli:git; configure the blueprint git block"
            );
        }
        // The module's declared name *is* its principal at every gated call, so bind it to the
        // name the package is being linked under. Without this, a prebuilt `pkg.wasm` naming
        // itself `main` — or naming another package — would be granted that principal's
        // permissions and, for `main`, the operator's injected credentials. The bytes are not
        // necessarily ones this compiler produced: `submilli run` and the server both
        // instantiate artifacts straight from the package store.
        match package.module.name() {
            Some(declared) if declared == name => {}
            Some(declared) => wasmtime::bail!(
                "package `{name}`: its module declares the name `{declared}`, which would give \
                 it that principal's permissions. Rebuild the package."
            ),
            None => wasmtime::bail!(
                "package `{name}`: its module declares no name, so its gated calls cannot be \
                 attributed. Rebuild the package."
            ),
        }
        store
            .data_mut()
            .install_type_info(package.type_info.clone());
        // Instantiation runs the package's module-level initializers. They are the
        // package's own code, executing in the package's own module, so identity read
        // off the running frame names them without any bracketing here.
        let instance = {
            let outcome = linker.instantiate_async(&mut *store, package.module).await;
            name_the_failing_initializer(&mut *store, name, outcome)?
        };
        linker.instance(&mut *store, name, instance)?;
    }
    Ok(())
}

/// Initializers run inside the Wasm start function, so anything they throw —
/// a denial most of all — escapes instantiation as the engine's opaque
/// `ThrownException`, whose Display is "wasm exception thrown". The thrown
/// value is still on the store, so recover its text the way an uncaught throw
/// from `main` is recovered and say which package it came from. Without this an
/// operator who granted a capability under `main:` rather than the package's own
/// block — the mistake this attribution makes easy — gets no package, no
/// capability, and no reason.
fn name_the_failing_initializer(
    store: &mut Store<StoreData>,
    package: &str,
    outcome: wasmtime::Result<Instance>,
) -> wasmtime::Result<Instance> {
    let err = match outcome {
        Ok(instance) => return Ok(instance),
        Err(err) => err,
    };
    let recovered = exec::uncaught_error(store, err);
    let Some(thrown) = recovered.downcast_ref::<crate::backtrace::ThrownError>() else {
        return Err(recovered);
    };
    Err(wasmtime::Error::new(crate::backtrace::ThrownError {
        message: format!(
            "package `{package}` failed to initialize: {}",
            thrown.message
        ),
        backtrace: thrown.backtrace.clone(),
    }))
}

/// Native stack bytes per byte of `max_wasm_stack`; see
/// [`RuntimeConfig::native_stack_size`].
const NATIVE_STACK_PER_WASM_BYTE: usize = 32;
const MIN_NATIVE_STACK: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub fuel: u64,
    pub max_wasm_stack: usize,
    pub memory_reservation: u64,
    pub memory_guard_size: u64,
    pub memory_reservation_for_growth: u64,
    pub max_store_bytes: u64,
    pub timeout: Option<Duration>,
    pub async_yield_fuel: Option<u64>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            // Temporarily very high: large structural JSON.stringify and other
            // per-code-unit Wasm work still burns fuel, and exhausting it mid-run
            // is a worse failure than the loose runaway-loop bound this gives up.
            // The real CPU cap belongs in a wall-clock timeout; revisit once the
            // hot encoding paths are off the meter.
            fuel: 1_000_000_000_000,
            max_wasm_stack: 512 * 1024,
            memory_reservation: 3 * 1024 * 1024,
            memory_guard_size: 64 * 1024,
            memory_reservation_for_growth: 16 * 1024 * 1024,
            max_store_bytes: DEFAULT_MAX_STORE_BYTES,
            timeout: None,
            // 10K fuel ≈ ~10K wasm instructions. At the default fuel budget a
            // CPU-bound full-budget call yields thousands of times before
            // exhaustion — fine-grained enough for fair scheduling, coarse enough
            // that yield overhead stays negligible.
            async_yield_fuel: Some(10_000),
        }
    }
}

impl RuntimeConfig {
    /// The native stack a thread running programs under this config needs.
    ///
    /// `max_wasm_stack` is an interpreter budget, but a host call that re-enters
    /// Wasm (a callback passed to `map`, say) nests interpreter frames on the
    /// thread's own stack. The engine charges each crossing 4 KiB of the budget
    /// so re-entry traps before the thread overflows; a debug build spends up to
    /// ~64 KiB of native stack per crossing, so the thread is sized well past the
    /// budget. An overflow here would abort every session in the process.
    pub fn native_stack_size(&self) -> usize {
        self.max_wasm_stack
            .saturating_mul(NATIVE_STACK_PER_WASM_BYTE)
            .max(MIN_NATIVE_STACK)
    }

    pub fn engine(&self) -> wasmtime::Result<Engine> {
        Engine::new(&self.wasmtime_config())
    }

    /// Same as [`engine`](Self::engine). `Config::async_support` is a no-op in
    /// wasmtime 44+; this entry point exists so async embedders have a distinct
    /// call site to evolve independently.
    pub fn engine_async(&self) -> wasmtime::Result<Engine> {
        Engine::new(&self.wasmtime_config())
    }

    pub fn wasmtime_config(&self) -> Config {
        let mut config = Config::new();
        // Winch lacks `wasm_gc` + `wasm_function_references`; Pulley is ~10× slower.
        // Strategy::Auto → Cranelift JIT.
        //
        // No optimization passes: the per-request user script is compiled fresh by
        // `Module::new` (it can't be AOT-cached — it's LLM-generated), and for
        // short-lived scripts that compile cost dominates the run. The trade is
        // global, though: precompiled prelude/stdlib share this engine, so they run
        // unoptimized too. TODO: precompile stdlib + curated packages on a separate
        // high-opt engine and `deserialize` the optimized artifacts here, so only the
        // user script pays the unoptimized-codegen tax.
        config.cranelift_opt_level(OptLevel::None);
        config.consume_fuel(true);
        config.max_wasm_stack(self.max_wasm_stack);

        config.epoch_interruption(true);

        config.memory_reservation(self.memory_reservation);
        config.memory_guard_size(self.memory_guard_size);
        config.memory_reservation_for_growth(self.memory_reservation_for_growth);
        config.memory_may_move(true);
        config.memory_init_cow(true);

        // The engine grants this reservation without consulting the store limiter.
        // Start at zero so every GC byte is charged to the tenant's aggregate cap,
        // including stores whose cap differs from this engine's RuntimeConfig.
        config.gc_heap_reservation(0);
        config.gc_heap_guard_size(self.memory_guard_size);
        config.gc_heap_reservation_for_growth(self.memory_reservation_for_growth);
        config.gc_heap_may_move(true);

        config.wasm_gc(true);
        config.wasm_function_references(true);
        config.wasm_exceptions(true);

        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);

        config
    }

    pub fn store<T>(&self, engine: &Engine, data: T) -> wasmtime::Result<Store<T>> {
        let mut store = Store::new(engine, data);
        store.set_fuel(self.fuel)?;
        // With epoch_interruption(true), an unset deadline traps immediately.
        // Use 1 for watchdog-tripped timeouts, MAX to effectively disable.
        let delta = if self.timeout.is_some() { 1 } else { u64::MAX };
        store.set_epoch_deadline(delta);
        store.epoch_deadline_trap();
        Ok(store)
    }

    pub fn store_async<T>(&self, engine: &Engine, data: T) -> wasmtime::Result<Store<T>> {
        let mut store = self.store(engine, data)?;
        store.fuel_async_yield_interval(self.async_yield_fuel)?;
        Ok(store)
    }

    pub fn arm_timeout(&self, engine: &Engine) -> Option<Watchdog> {
        self.timeout.map(|d| watchdog::arm(engine, d))
    }

    /// One-shot runner: compile, instantiate, call `main`, return typed result
    /// and captured console. Async like the rest of the runtime — a caller that
    /// isn't on a runtime bridges it itself (`pollster::block_on` for
    /// compute/`fs`-only programs, a tokio runtime when `http` is involved). For
    /// live console streaming, build a custom `Store<StoreData>` and call
    /// [`dispatch_main_async`] directly.
    pub async fn run(&self, wasm_bytes: &[u8]) -> wasmtime::Result<RunResult> {
        self.run_with_type_info(wasm_bytes, None).await
    }

    pub async fn run_compiled(
        &self,
        compiled: &crate::compile::CompiledScript,
    ) -> wasmtime::Result<RunResult> {
        self.run_with_type_info(&compiled.wasm, Some(compiled.type_info.clone()))
            .await
    }

    async fn run_with_type_info(
        &self,
        wasm_bytes: &[u8],
        type_info: Option<TypeInfoTable>,
    ) -> wasmtime::Result<RunResult> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, self.max_store_bytes);
        data.console = Box::new(ConsoleSink(Arc::clone(&buf)));
        if let Some(type_info) = type_info {
            data.install_type_info(type_info);
        }
        let engine = self.engine()?;
        let mut store = self.store_async(&engine, data)?;
        install_tenant_limits(&mut store);
        let module = Module::new(&engine, wasm_bytes)?;
        let mut linker = Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store).await?;
        let _watchdog = self.arm_timeout(&engine);
        let inst = instantiate_program_async(&linker, &mut store, &module).await?;
        let value = dispatch_main_async(&mut store, &inst).await?;
        let captured = buf
            .lock()
            .map_err(|_| host::fatal_host_error("console buffer lock poisoned"))?
            .clone();
        let console = String::from_utf8(captured)
            .map_err(|e| wasmtime::Error::msg(format!("console output not utf-8: {e}")))?;
        Ok(RunResult { value, console })
    }
}

struct ConsoleSink(Arc<Mutex<Vec<u8>>>);

impl Write for ConsoleSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("console buffer lock poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Decode a Submilli `(ref $string)` (packed UTF-16) into a Rust `String`.
pub(crate) fn read_submilli_string(
    mut ctx: impl AsContextMut<Data = StoreData>,
    msg: Rooted<ArrayRef>,
) -> wasmtime::Result<String> {
    let units = host::read_code_units(&mut ctx, msg, "string")?;
    fuel::charge(ctx, fuel::SCAN, units.len() as u64)?;
    Ok(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod console_capture_tests {
    use super::*;

    #[test]
    fn poisoned_console_writer_returns_error_without_discarding_bytes() {
        let buffer = Arc::new(Mutex::new(b"before\n".to_vec()));
        let poisoned = Arc::clone(&buffer);
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("injected console poison");
        })
        .join();
        let mut sink = ConsoleSink(Arc::clone(&buffer));
        assert!(sink.write_all(b"after\n").is_err());
        assert_eq!(*buffer.lock().unwrap_err().into_inner(), b"before\n");
        let healthy = Arc::new(Mutex::new(Vec::new()));
        ConsoleSink(Arc::clone(&healthy))
            .write_all(b"healthy\n")
            .unwrap();
        assert_eq!(*healthy.lock().unwrap(), b"healthy\n");
    }
}
