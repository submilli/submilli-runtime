//! The host's run path: one program in one session, from compile to result. Every
//! entry point runs through [`RunHost::run`]: MCP execute, the MCP file tools,
//! exec, rerun and Test.
//!
//! Copied from `submilli-server`: `handlers/execute.rs` (`execute_core_with`,
//! `prepare_and_run`, `live_world`, `outside_clients`, the response shape),
//! `runner.rs` (`run`, `run_inner`: the store, the deadline, cancellation),
//! `error.rs` (the error JSON), `record/mod.rs` (a recording finished once on
//! every exit path), `app.rs` (`cancel_run`, `register_run`, `llm_provider_for`,
//! `embedding_provider_for`, `mcp_catalog_for_imports`) and
//! `session_manager/resources.rs` (an HTTP client per session). The pieces in
//! `run/` name their own sources.
//!
//! The host runs one project in one process, so it keeps no idempotency ledger,
//! no audit and no telemetry, and the playground's package check is called
//! directly before each compile rather than through a hook.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::session_finalizers::SessionFinalizers;
use interpreter::runtime::{
    CallRecord, ConsoleLine, CursorKey, CursorKeys, DecisionLog, DecisionLogConfig,
    DecisionLogOutput, DecisionRecord, EmbeddingLimits, EmbeddingProvider, EmbeddingTokenBudget,
    ExecutionTokenBudget, HttpClient, LinkedPackageModule, LlmLimits, LlmProvider, McpTransport,
    NetworkPolicy, RecordObserver, ReqwestHttpClient, RuntimeConfig, SessionKvStore,
    SharedTokenBudget, StoreData, Vfs, VfsInfo, VfsMode, install_package_modules_async,
    install_runtime_host_functions, install_runtime_store_bound, install_tenant_limits,
};
use interpreter::{compile_parsed_script_timed, dispatch_main_async, instantiate_program_async};
use serde::Serialize;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings, VfsConfig};
use submilli_build::PackageStore;
use submilli_shared::embedding::{
    BlueprintEmbeddingProvider, EmbeddingDispatch, HttpEmbeddingDispatch,
};
use submilli_shared::host::SecretStore;
use submilli_shared::llm::{BlueprintLlmProvider, HttpModelDispatch, ModelDispatch};
use submilli_shared::mcp::McpCatalog;
use submilli_shared::mcp::discovery::{DiscoveryAuth, discover_selected};
use submilli_shared::mcp_auth::SecretStoreGrants;
use submilli_shared::mcp_token::OAuthTokenManager;
use submilli_shared::secret_store::SecretCipher;
use submilli_shared::{BlueprintAuthProxy, BlueprintSecretProvider, PolicyCheck};
use wasmtime::{Engine, Linker, Module};

use super::super::packages::Freshness;
use super::blueprints::AppliedBlueprint;
use super::sessions::{ResourceUse, RunEnd, Sessions};
use super::volumes::Volumes;

mod compile;
mod console;
mod packages;
mod timeout;

pub(crate) use compile::{ParsedExecute, on_compiler_thread, parse};
pub(crate) use packages::{PackageCache, PreparedPackages, finalizer_package_versions};

use compile::{
    FILENAME, classify_runtime_error, compile_failure, raised_by_top_level_statements,
    register_package_sources,
};
use console::{ConsoleLines, Sink, captured_console, split_console};

// ---- The error a caller sees ---------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKind {
    CompileError,
    Timeout,
    FuelExhausted,
    MemoryExhausted,
    StackExhausted,
    Cancelled,
    /// A capability denial the runtime threw escaped the program. A denial the
    /// program constructed itself is a `RuntimeError`.
    PermissionDenied,
    RuntimeError,
    BlueprintNotFound,
    PackageResolution,
    /// The request itself is malformed for this blueprint.
    InvalidRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DiagnosticNote {
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DiagnosticPayload {
    pub(crate) severity: &'static str,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) notes: Vec<DiagnosticNote>,
}

/// Who was refused what, and by which layer: `policy` (the blueprint),
/// `invariant` (refused by the runtime ahead of any policy), or `read_only` (a
/// write into a read-only volume).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DenialDetails {
    pub(crate) caller: String,
    pub(crate) capability: String,
    pub(crate) source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExecuteError {
    pub(crate) kind: ErrorKind,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<DiagnosticPayload>,
    /// Present exactly when `kind` is `permission_denied`; its fields sit beside
    /// `kind` in the JSON.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub(crate) denial: Option<DenialDetails>,
}

impl ExecuteError {
    pub(crate) fn new(kind: ErrorKind, message: String) -> Self {
        Self {
            kind,
            message,
            diagnostics: Vec::new(),
            denial: None,
        }
    }
}

/// What a run returns to its caller.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ExecuteResponse {
    pub(crate) execution_id: String,
    pub(crate) session_id: String,
    /// `main()`'s output, verbatim: a `string` return as-is, other returns as
    /// their JSON text.
    pub(crate) result: Option<String>,
    /// Empty on success: the session's last run keeps the console.
    pub(crate) console: Vec<String>,
    pub(crate) error: Option<ExecuteError>,
    /// `@mcp/<server>` discovery notices (dropped or degraded tools).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) discovery_warnings: Vec<String>,
}

/// A run's `{result, console, error}` with the full console: what its session
/// keeps as its last run.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LastRun {
    pub(crate) result: Option<String>,
    pub(crate) console: Vec<String>,
    pub(crate) error: Option<ExecuteError>,
}

/// What one run produced, and whether the program reached the runner. A failure
/// before dispatch means nothing ran and no effects landed.
pub(crate) struct RunOutput {
    pub(crate) response: ExecuteResponse,
    pub(crate) dispatched: bool,
    /// `None` when nothing reached the runner.
    pub(crate) last_run: Option<LastRun>,
}

/// What the runner itself produced.
#[derive(Default)]
pub(crate) struct RunOutcome {
    pub(crate) usage: ExecutionUsage,
    /// Already-JSON-encoded `main` return.
    pub(crate) value: Option<String>,
    pub(crate) console_raw: String,
    pub(crate) error: Option<ExecuteError>,
    pub(crate) discovery_warnings: Vec<String>,
}

impl RunOutcome {
    /// A failure of the host's own, marked `internal:`.
    fn internal_failure(message: &str) -> Self {
        Self {
            error: Some(ExecuteError::new(
                ErrorKind::RuntimeError,
                format!("internal: {message}"),
            )),
            ..Self::default()
        }
    }

    fn cancelled() -> Self {
        let mut outcome = Self::internal_failure("execution cancelled");
        if let Some(error) = &mut outcome.error {
            error.kind = ErrorKind::Cancelled;
        }
        outcome
    }

    /// The run's `{result, console, error}` with the full console.
    fn last_run(&self) -> LastRun {
        let console = split_console(&self.console_raw);
        match &self.error {
            Some(error) => LastRun {
                result: None,
                console,
                error: Some(error.clone()),
            },
            None => LastRun {
                result: self.value.clone(),
                console,
                error: None,
            },
        }
    }
}

// ---- Recording -----------------------------------------------------------------

/// How a run reached the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunEntry {
    /// A session run: rerun, or a program run in a session.
    Session,
    /// The MCP execute tool.
    Mcp,
    /// An MCP file tool; its run is the one policy decision.
    McpFileTool { tool: String },
    /// A one-off program, such as `exec`.
    Program,
    /// A test of a recorded run under a newer blueprint.
    Test,
}

/// What a run is, captured as it starts.
#[derive(Clone)]
pub(crate) struct RunStart {
    /// The run's id; its response carries it too, and [`RunHost::cancel`] stops it
    /// by it while it is in flight.
    pub(crate) execution_id: String,
    /// Who started it, as its caller named them.
    pub(crate) label: String,
    pub(crate) entry: RunEntry,
    /// The `execution_id` of the recorded run this run tests.
    pub(crate) test_of: Option<String>,
    /// The MCP client's name from its `initialize`.
    pub(crate) client: Option<String>,
    /// The MCP client's id for the tool call that started the run.
    pub(crate) tool_call_id: Option<String>,
    pub(crate) session_id: String,
    pub(crate) blueprint_name: String,
    /// The version tag the blueprint was applied as.
    pub(crate) blueprint_version: String,
    pub(crate) variables: Arc<VarBindings>,
    /// The session's harness secrets, so a recorder can keep their values out of
    /// what it stores. A recorder must never store or log them.
    pub(crate) harness_secrets: Arc<HarnessSecretBindings>,
    /// The program's source; `None` for a file tool.
    pub(crate) code: Option<Arc<str>>,
}

/// How a run ended.
pub(crate) struct FinishedRun {
    /// Whether the program reached the runner.
    pub(crate) dispatched: bool,
    pub(crate) error: Option<ExecuteError>,
    pub(crate) result: Option<String>,
    /// Console output, in full.
    pub(crate) console: String,
    pub(crate) usage: ExecutionUsage,
    /// Decisions and calls. Empty for a run that never dispatched.
    pub(crate) log: DecisionLogOutput,
    /// The `@mcp/<server>` packages the program was compiled against; `None` when
    /// it never reached the runner.
    pub(crate) mcp_catalog: Option<Arc<McpCatalog>>,
    pub(crate) wall: Duration,
}

/// One thing a recorded run did, as it happened.
pub(crate) enum RunEvent<'a> {
    CallStarted(&'a CallRecord),
    Decision(&'a DecisionRecord),
    /// The call as it ended, without its payload contents.
    CallFinished(&'a CallRecord),
    ConsoleLine(&'a ConsoleLine<'a>),
}

/// Decides, for each run, whether it is recorded.
pub(crate) trait RunRecorder: Send + Sync {
    /// A run is starting. `None` leaves it unrecorded.
    fn start(&self, run: RunStart) -> Option<Arc<dyn RecordedRun>>;
}

/// One recorded run.
pub(crate) trait RecordedRun: Send + Sync {
    /// Caps for this run's decision and call records.
    fn log_config(&self) -> DecisionLogConfig {
        DecisionLogConfig::default()
    }

    /// One event, in the order the run made them.
    fn event(&self, _event: RunEvent<'_>) {}

    /// The run ended. Called exactly once, on every exit path: a run refused
    /// before dispatch, one whose caller went away, and one whose task was lost.
    fn finish(&self, run: FinishedRun);

    /// The size, as JSON, of what the caller received. Called once, after
    /// [`Self::finish`], for a run that dispatched.
    fn returned(&self, _bytes: u64) {}
}

/// A run being recorded. Clones share one run; dropping the last clone of a run
/// nobody finished finishes it.
#[derive(Clone)]
pub(crate) struct Recording {
    shared: Arc<RecordingShared>,
}

struct RecordingShared {
    run: Arc<dyn RecordedRun>,
    started: Instant,
    dispatched: AtomicBool,
    finished: AtomicBool,
    returned: AtomicBool,
}

impl Drop for RecordingShared {
    fn drop(&mut self) {
        // This may run while the thread is unwinding: a panic of the recorder's
        // must not escape and abort the process.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let dispatched = self.dispatched.load(Ordering::Acquire);
            let error = if dispatched {
                ExecuteError::new(
                    ErrorKind::RuntimeError,
                    "internal: the run ended without reporting how".into(),
                )
            } else {
                ExecuteError::new(
                    ErrorKind::Cancelled,
                    "the caller abandoned the run before it ran".into(),
                )
            };
            self.finish(without_log(dispatched, error, self.started));
        }));
    }
}

impl RecordingShared {
    fn finish(&self, run: FinishedRun) {
        if self.finished.swap(true, Ordering::AcqRel) {
            return;
        }
        self.run.finish(run);
    }
}

fn without_log(dispatched: bool, error: ExecuteError, started: Instant) -> FinishedRun {
    FinishedRun {
        dispatched,
        error: Some(error),
        result: None,
        console: String::new(),
        usage: ExecutionUsage::default(),
        log: DecisionLogOutput::default(),
        mcp_catalog: None,
        wall: started.elapsed(),
    }
}

impl Recording {
    fn new(run: Arc<dyn RecordedRun>) -> Self {
        Self {
            shared: Arc::new(RecordingShared {
                run,
                started: Instant::now(),
                dispatched: AtomicBool::new(false),
                finished: AtomicBool::new(false),
                returned: AtomicBool::new(false),
            }),
        }
    }

    fn mark_dispatched(&self) {
        self.shared.dispatched.store(true, Ordering::Release);
    }

    /// The decision log for the run, reporting each record as an event.
    pub(crate) fn log(&self) -> Arc<DecisionLog> {
        DecisionLog::new(
            self.shared.run.log_config(),
            Some(Arc::new(Events(Arc::clone(&self.shared.run))) as Arc<dyn RecordObserver>),
        )
    }

    pub(crate) fn finish(&self, run: FinishedRun) {
        self.shared.finish(run);
    }

    fn undispatched(&self, error: &ExecuteError) {
        self.finish(without_log(false, error.clone(), self.shared.started));
    }

    fn lost(&self, error: &ExecuteError, mcp_catalog: Arc<McpCatalog>) {
        self.finish(FinishedRun {
            mcp_catalog: Some(mcp_catalog),
            ..without_log(true, error.clone(), self.shared.started)
        });
    }

    fn returned(&self, bytes: u64) {
        if !self.shared.finished.load(Ordering::Acquire)
            || self.shared.returned.swap(true, Ordering::AcqRel)
        {
            return;
        }
        self.shared.run.returned(bytes);
    }

    pub(crate) fn started(&self) -> Instant {
        self.shared.started
    }
}

/// Reports a run's records to its recorder as they are made.
struct Events(Arc<dyn RecordedRun>);

impl RecordObserver for Events {
    fn call_started(&self, call: &CallRecord) {
        self.0.event(RunEvent::CallStarted(call));
    }
    fn decision(&self, record: &DecisionRecord) {
        self.0.event(RunEvent::Decision(record));
    }
    fn call_finished(&self, call: &CallRecord) {
        self.0.event(RunEvent::CallFinished(call));
    }
    fn console_line(&self, line: &ConsoleLine<'_>) {
        self.0.event(RunEvent::ConsoleLine(line));
    }
}

// ---- Cancelling ----------------------------------------------------------------

/// The runs in flight, by execution id, each with what cancels it.
#[derive(Default)]
struct Cancels {
    running: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
}

impl Cancels {
    fn register(self: &Arc<Self>, id: &str) -> (Registration, tokio::sync::oneshot::Receiver<()>) {
        let (canceller, cancelled) = tokio::sync::oneshot::channel();
        self.lock().insert(id.to_owned(), canceller);
        (
            Registration {
                cancels: Arc::clone(self),
                id: id.to_owned(),
            },
            cancelled,
        )
    }

    fn cancel(&self, id: &str) -> bool {
        self.lock()
            .remove(id)
            .is_some_and(|canceller| canceller.send(()).is_ok())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, tokio::sync::oneshot::Sender<()>>> {
        // Poison means a panic interrupted an insert or a removal. AGENTS.md
        // permits the poisoned-lock panic rather than reading the registry.
        self.running
            .lock()
            .expect("running-run registry lock poisoned")
    }
}

/// Keeps a run cancellable until it drops.
struct Registration {
    cancels: Arc<Cancels>,
    id: String,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.cancels.lock().remove(&self.id);
    }
}

// ---- Finalizers ----------------------------------------------------------------

/// Where a session run's finalizer registrations go. `package_versions` is
/// [`finalizer_package_versions`] of the packages the run links, computed before
/// the run starts.
pub(crate) trait FinalizerRegistry: Send + Sync {
    fn provider(
        &self,
        session_id: &str,
        package_versions: BTreeMap<String, String>,
    ) -> Result<Arc<dyn SessionFinalizers>, String>;
}

// ---- The host ------------------------------------------------------------------

/// How the host runs programs, fixed when it starts.
pub(crate) struct RunHostConfig {
    pub(crate) runtime: RuntimeConfig,
    pub(crate) network_policy: NetworkPolicy,
    pub(crate) package_store: PackageStore,
    pub(crate) secret_store: Option<Arc<dyn SecretStore>>,
    /// Seals `session.list` cursors, so one handed out before a restart still
    /// works after it.
    pub(crate) session_cipher: Option<Arc<SecretCipher>>,
    pub(crate) recorder: Option<Arc<dyn RunRecorder>>,
    /// The playground's package check, called before each compile.
    pub(crate) freshness: Option<Arc<Freshness>>,
    pub(crate) finalizers: Option<Arc<dyn FinalizerRegistry>>,
    pub(crate) llm_limits: LlmLimits,
    pub(crate) embedding_limits: EmbeddingLimits,
    /// Replaces the HTTP model dispatch built per run from the blueprint.
    pub(crate) llm_dispatch: Option<Arc<dyn ModelDispatch>>,
    /// Replaces the HTTP embedding dispatch built per run from the blueprint.
    pub(crate) embedding_dispatch: Option<Arc<dyn EmbeddingDispatch>>,
}

impl RunHostConfig {
    pub(crate) fn new(package_store: PackageStore) -> Self {
        Self {
            runtime: RuntimeConfig::default(),
            network_policy: NetworkPolicy::default(),
            package_store,
            secret_store: None,
            session_cipher: None,
            recorder: None,
            freshness: None,
            finalizers: None,
            llm_limits: LlmLimits::default(),
            embedding_limits: EmbeddingLimits::default(),
            llm_dispatch: None,
            embedding_dispatch: None,
        }
    }
}

/// At most this many sessions keep an HTTP client cached.
const MAX_CACHED_HTTP_CLIENTS: usize = 256;

/// Runs programs in the host's sessions.
pub(crate) struct RunHost {
    engine: Engine,
    base_linker: Linker<StoreData>,
    runtime: RuntimeConfig,
    sessions: Arc<Sessions>,
    volumes: Arc<Volumes>,
    network_policy: Arc<NetworkPolicy>,
    secret_store: Option<Arc<dyn SecretStore>>,
    oauth: Option<Arc<OAuthTokenManager>>,
    cursor_keys: Option<Arc<CursorKeys>>,
    packages: Arc<PackageCache>,
    mcp_catalogs: Mutex<McpCatalogs>,
    http_clients: Mutex<HashMap<String, Arc<dyn HttpClient>>>,
    llm_limits: LlmLimits,
    llm_budget: SharedTokenBudget,
    embedding_limits: EmbeddingLimits,
    embedding_budget: SharedTokenBudget,
    llm_dispatch: Option<Arc<dyn ModelDispatch>>,
    embedding_dispatch: Option<Arc<dyn EmbeddingDispatch>>,
    cancels: Arc<Cancels>,
    recorder: Option<Arc<dyn RunRecorder>>,
    freshness: Option<Arc<Freshness>>,
    finalizers: Option<Arc<dyn FinalizerRegistry>>,
}

/// The discovered `@mcp/<server>` catalogs, keyed by the servers imported, each
/// beside the blueprint it was discovered from.
#[derive(Default)]
struct McpCatalogs {
    /// The blueprint tag stops a run from being served another blueprint's catalog.
    entries: HashMap<String, (Arc<Blueprint>, Arc<McpCatalog>)>,
    /// Bumped by every clear; it stops a run on a blueprint older than the latest
    /// save from caching its catalog again, and holding that blueprint, after the clear.
    generation: u64,
}

impl McpCatalogs {
    /// The catalog kept under `key`, when it was discovered from `blueprint`.
    fn cached(&self, key: &str, blueprint: &Arc<Blueprint>) -> Option<Arc<McpCatalog>> {
        self.entries
            .get(key)
            .filter(|(from, _)| Arc::ptr_eq(from, blueprint))
            .map(|(_, catalog)| Arc::clone(catalog))
    }

    /// Keep a catalog discovered from `blueprint` since `generation`, returning
    /// the one now cached under `key`. A clear since `generation` means a save
    /// replaced `blueprint`: the caller still gets what it discovered, but it is
    /// not kept, so the old blueprint is not held.
    fn keep(
        &mut self,
        key: String,
        blueprint: &Arc<Blueprint>,
        generation: u64,
        discovered: Arc<McpCatalog>,
    ) -> Arc<McpCatalog> {
        if self.generation != generation {
            return discovered;
        }
        if let Some(cached) = self.cached(&key, blueprint) {
            return cached;
        }
        self.entries
            .insert(key, (Arc::clone(blueprint), Arc::clone(&discovered)));
        discovered
    }
}

/// Who opens a run's file system.
pub(crate) enum VfsSource {
    /// The run path: the session's directory, scratch or volumes, size limits
    /// attached.
    Session,
    /// The caller opened it, completely, under its own rules.
    Provided { vfs: Vfs, vfs_info: VfsInfo },
}

/// One run to make.
pub(crate) struct RunRequest {
    /// The open session the run executes against.
    pub(crate) session_id: String,
    pub(crate) code: String,
    /// Who started it, for its record.
    pub(crate) label: String,
    pub(crate) entry: RunEntry,
    /// The blueprint the run is decided under, read once for this run.
    pub(crate) blueprint: AppliedBlueprint,
    /// The session's bound variables.
    pub(crate) variables: Arc<VarBindings>,
    /// The session's harness secrets.
    pub(crate) harness_secrets: Arc<HarnessSecretBindings>,
    pub(crate) client: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) test_of: Option<String>,
    pub(crate) vfs: VfsSource,
    /// Takes the place of the session's own finalizer registrations, for a run
    /// that must not register any, such as a test replay.
    pub(crate) finalizers: Option<Arc<dyn SessionFinalizers>>,
    /// Takes the place of the run's outside world and the session's values, for a
    /// run that must not touch either, such as a test replay.
    pub(crate) world: Option<Box<dyn RunWorld>>,
}

/// The connectors a run reaches outside through.
pub(crate) struct Connectors {
    pub(crate) http: Arc<dyn HttpClient>,
    pub(crate) mcp: Arc<dyn McpTransport>,
    pub(crate) llm: Option<Arc<dyn LlmProvider>>,
    pub(crate) embedding: Option<Arc<dyn EmbeddingProvider>>,
}

/// Stand-ins for a run's outside world. The run's file system comes from
/// [`VfsSource::Provided`] and its finalizers from [`RunRequest::finalizers`];
/// its token budgets are its own, so what it spends is not counted against live
/// runs.
pub(crate) trait RunWorld: Send {
    /// The `@mcp/<server>` packages the program compiles against; no server is
    /// contacted to discover them.
    fn mcp_catalog(&self) -> Arc<McpCatalog>;

    /// The run's `submilli:session` values.
    fn session_kv(&self) -> Arc<dyn SessionKvStore>;

    /// The connectors the run uses, given the live ones it would have used.
    fn connectors(&self, live: Connectors) -> Connectors;

    /// The run's cancel signal, given the host's, which [`RunHost::cancel`] fires.
    fn cancel(
        &mut self,
        host: tokio::sync::oneshot::Receiver<()>,
    ) -> tokio::sync::oneshot::Receiver<()>;

    /// What the run reports to, given what the recorder returned for it.
    fn recording(&self, run: Arc<dyn RecordedRun>) -> Arc<dyn RecordedRun> {
        run
    }
}

impl RunHost {
    /// Starts the engine and its deadline clock.
    pub(crate) fn new(
        config: RunHostConfig,
        sessions: Arc<Sessions>,
        volumes: Arc<Volumes>,
    ) -> anyhow::Result<Self> {
        let runtime = config.runtime;
        let engine = Engine::new(&runtime.wasmtime_config())?;
        if runtime.timeout.is_some_and(|timeout| !timeout.is_zero()) {
            timeout::start_ticker(&engine)?;
        }
        let mut base_linker = Linker::<StoreData>::new(&engine);
        install_runtime_host_functions(&mut base_linker)?;
        let network_policy = Arc::new(config.network_policy);
        let oauth = config.secret_store.as_ref().map(|store| {
            Arc::new(OAuthTokenManager::new(
                Arc::new(SecretStoreGrants::new(Arc::clone(store))),
                Some(Arc::clone(store)),
                Arc::new(ReqwestHttpClient::new(Arc::clone(&network_policy)))
                    as Arc<dyn HttpClient>,
                Arc::new(Vec::new()),
            ))
        });
        let cursor_keys = config.session_cipher.as_deref().map(|cipher| {
            let (id, secret) = cipher.derive(b"submilli:session-list-cursor:v1");
            Arc::new(CursorKeys::new(CursorKey::new(id, secret), Vec::new()))
        });
        Ok(Self {
            engine,
            base_linker,
            runtime,
            sessions,
            volumes,
            network_policy,
            secret_store: config.secret_store,
            oauth,
            cursor_keys,
            packages: Arc::new(PackageCache::new(config.package_store)),
            mcp_catalogs: Mutex::new(McpCatalogs::default()),
            http_clients: Mutex::new(HashMap::new()),
            llm_limits: config.llm_limits,
            llm_budget: SharedTokenBudget::new(
                interpreter::runtime::DEFAULT_MAX_ALL_EXECUTIONS_TOKENS,
            ),
            embedding_limits: config.embedding_limits,
            embedding_budget: SharedTokenBudget::new(MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS),
            llm_dispatch: config.llm_dispatch,
            embedding_dispatch: config.embedding_dispatch,
            cancels: Arc::default(),
            recorder: config.recorder,
            freshness: config.freshness,
            finalizers: config.finalizers,
        })
    }

    /// Cancel the run `execution_id`. Returns whether it was in flight and took
    /// the cancel.
    pub(crate) fn cancel(&self, execution_id: &str) -> bool {
        self.cancels.cancel(execution_id)
    }

    /// Whether runs are recorded: a test run is reported from its own record.
    pub(crate) fn records(&self) -> bool {
        self.recorder.is_some()
    }

    /// Drop every prepared package set, so the next run links what the store
    /// holds now.
    pub(crate) fn clear_prepared_packages(&self) {
        self.packages.clear();
    }

    /// Drop every discovered `@mcp/<server>` catalog, so the next run discovers
    /// against the blueprint in force.
    pub(crate) fn clear_mcp_catalogs(&self) {
        let mut catalogs = self.lock_catalogs();
        catalogs.entries.clear();
        catalogs.generation += 1;
    }

    /// Start recording a run that is not a program, such as an MCP file tool's one
    /// decision; `None` when the recorder leaves it unrecorded. The caller
    /// finishes it.
    pub(crate) fn record(&self, run: RunStart) -> Option<Recording> {
        self.recorder
            .as_ref()
            .and_then(|recorder| recorder.start(run).map(Recording::new))
    }

    /// Every `@mcp/<server>` package `blueprint` declares, for the package tools
    /// to describe.
    pub(crate) async fn blueprint_mcp_catalog(
        &self,
        blueprint: &Arc<Blueprint>,
    ) -> Result<Arc<McpCatalog>, String> {
        let servers = blueprint.mcp.keys().cloned().collect();
        self.mcp_catalog_for_imports(blueprint, &servers, &Arc::default())
            .await
    }

    /// Run one program. The recorder is asked about it first, and sees its end on
    /// every path; the run can be cancelled by its execution id while it is in
    /// flight.
    pub(crate) async fn run(&self, request: RunRequest) -> RunOutput {
        let execution_id = uuid::Uuid::new_v4().to_string();
        let recording = self.recorder.as_ref().and_then(|recorder| {
            let run = recorder.start(run_start(&execution_id, &request))?;
            Some(Recording::new(match &request.world {
                Some(world) => world.recording(run),
                None => run,
            }))
        });
        let (_registered, cancel_requested) = self.cancels.register(&execution_id);
        let output = self
            .prepare_and_run(&execution_id, request, recording.clone(), cancel_requested)
            .await;
        if let Some(recording) = &recording {
            match (&output.response.error, output.dispatched) {
                // The runner finishes a dispatched run itself; one that never got
                // there is finished here.
                (Some(error), false) => recording.undispatched(error),
                _ => recording.returned(returned_bytes(&output.response)),
            }
        }
        output
    }

    async fn prepare_and_run(
        &self,
        execution_id: &str,
        request: RunRequest,
        recording: Option<Recording>,
        cancel_requested: tokio::sync::oneshot::Receiver<()>,
    ) -> RunOutput {
        let RunRequest {
            session_id,
            code,
            blueprint: applied,
            variables,
            harness_secrets,
            vfs: vfs_source,
            finalizers: finalizers_override,
            world,
            ..
        } = request;
        let mut world = world;
        let cancel_requested = match &mut world {
            Some(world) => world.cancel(cancel_requested),
            None => cancel_requested,
        };
        let blueprint = applied.blueprint;
        let fail = |kind: ErrorKind, message: String| RunOutput {
            response: failure_response(execution_id, &session_id, kind, message),
            dispatched: false,
            last_run: None,
        };

        let parsed = match parse(&code) {
            Ok(parsed) => parsed,
            Err(error) => return fail(ErrorKind::RuntimeError, error.to_string()),
        };
        let script_imports = match parsed.imports() {
            Ok(imports) => imports,
            Err(message) => return fail(ErrorKind::RuntimeError, message),
        };
        if let Some(freshness) = &self.freshness {
            let wanted: BTreeSet<String> = script_imports
                .registry_packages
                .iter()
                .filter(|package| blueprint.packages.contains(*package))
                .cloned()
                .collect();
            let packages = Arc::clone(&self.packages);
            if let Err(message) = freshness.before_run(wanted, move || packages.clear()).await {
                return fail(ErrorKind::PackageResolution, message);
            }
        }

        let llm_provider = match self.llm_provider_for(&blueprint, &harness_secrets) {
            Ok(provider) => provider,
            Err(message) => return fail(ErrorKind::RuntimeError, message),
        };
        let embedding_provider = match self.embedding_provider_for(&blueprint, &harness_secrets) {
            Ok(provider) => provider,
            Err(message) => return fail(ErrorKind::RuntimeError, message),
        };

        let session_run = match SessionRun::begin(&self.sessions, &session_id, execution_id).await {
            Ok(run) => run,
            Err(message) => return fail(ErrorKind::RuntimeError, message),
        };
        let resource_use = match self.sessions.admit_resource_work(&session_id).await {
            Ok(scope) => scope,
            Err(error) => return fail(ErrorKind::RuntimeError, error.to_string()),
        };

        let mcp_catalog = match &world {
            Some(world) => world.mcp_catalog(),
            None => match self
                .mcp_catalog_for_imports(&blueprint, &script_imports.mcp_servers, &harness_secrets)
                .await
            {
                Ok(catalog) => catalog,
                Err(message) => return fail(ErrorKind::RuntimeError, message),
            },
        };
        let (vfs, vfs_info) = match vfs_source {
            VfsSource::Provided { vfs, vfs_info } => (vfs, vfs_info),
            VfsSource::Session => match self.session_vfs(&session_id, &blueprint, &variables).await
            {
                Ok(vfs) => vfs,
                Err(message) => return fail(ErrorKind::RuntimeError, message),
            },
        };
        let packages =
            match self
                .packages
                .prepare_for_imports(&self.engine, &blueprint, &script_imports)
            {
                Ok(packages) => packages,
                Err(error) => return fail(ErrorKind::PackageResolution, error.to_string()),
            };
        // Which versions a finalizer is registered under is settled before the
        // program runs, from the packages it links.
        let session_finalizers = match finalizers_override {
            Some(replay) => Some(replay),
            None => match finalizer_package_versions(&packages) {
                Ok(versions) => match &self.finalizers {
                    Some(registry) => match registry.provider(&session_id, versions) {
                        Ok(provider) => Some(provider),
                        Err(message) => return fail(ErrorKind::RuntimeError, message),
                    },
                    None => None,
                },
                Err(message) => return fail(ErrorKind::RuntimeError, message),
            },
        };

        let live = Connectors {
            http: self.http_client(&session_id),
            mcp: self.mcp_transport(&blueprint, &harness_secrets),
            llm: llm_provider,
            embedding: embedding_provider,
        };
        let (connectors, session_kv, llm_budget, embedding_budget) = match &world {
            Some(world) => (
                world.connectors(live),
                world.session_kv(),
                SharedTokenBudget::new(interpreter::runtime::DEFAULT_MAX_ALL_EXECUTIONS_TOKENS),
                SharedTokenBudget::new(MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS),
            ),
            None => (
                live,
                self.sessions.values(&session_id),
                self.llm_budget.clone(),
                self.embedding_budget.clone(),
            ),
        };
        let policy: Arc<dyn interpreter::runtime::SecurityCheck> = Arc::new(
            PolicyCheck::with_variables(Arc::clone(&blueprint), Arc::clone(&variables)),
        );
        let services = Services {
            inputs: Inputs {
                git: submilli_shared::resolve_git(&blueprint, &variables)
                    .map_err(|error| error.to_string()),
                auth_proxy: Arc::new(BlueprintAuthProxy::with_harness(
                    Arc::clone(&blueprint),
                    self.secret_store.clone(),
                    Arc::clone(&harness_secrets),
                )),
                secret_provider: Arc::new(BlueprintSecretProvider::with_harness(
                    Arc::clone(&blueprint),
                    self.secret_store.clone(),
                    Arc::clone(&harness_secrets),
                )),
                variables: Arc::clone(&variables),
                security_check: policy,
                http_client: connectors.http,
                mcp_transport: connectors.mcp,
                session_kv,
                session_finalizers,
                cursor_keys: self.cursor_keys.clone(),
                llm_provider: connectors.llm,
                llm_budget: Arc::new(ExecutionTokenBudget::new(self.llm_limits, llm_budget)),
                embedding_provider: connectors.embedding,
                embedding_budget: Arc::new(EmbeddingTokenBudget::new(
                    self.embedding_limits,
                    embedding_budget,
                )),
                cancel_requested,
            },
            recording,
            resource_use,
            session_run,
        };
        let outcome = dispatch(
            &code,
            parsed,
            Runtime {
                engine: &self.engine,
                base_linker: &self.base_linker,
                config: &self.runtime,
            },
            (vfs, vfs_info),
            services,
            &packages,
            &mcp_catalog,
        )
        .await;
        let response = into_response(execution_id, &session_id, &outcome);
        RunOutput {
            response,
            dispatched: true,
            last_run: Some(outcome.last_run()),
        }
    }

    /// The session's file system, built completely: its root, mounts and working
    /// directory, with every size limit attached.
    async fn session_vfs(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<(Vfs, VfsInfo), String> {
        let failed =
            |error: super::volumes::VolumeError| format!("internal: vfs init failed: {error}");
        let session_root = self.sessions.session_dir(session_id);
        let vfs = self
            .volumes
            .build_vfs(blueprint, variables, Some(&session_root))
            .map_err(failed)?;
        let vfs = self
            .volumes
            .attach_limits(vfs, blueprint)
            .await
            .map_err(failed)?;
        Ok((vfs, vfs_info(blueprint)))
    }

    /// The outbound `submilli:llm` provider for one run, resolving credentials
    /// from that run's blueprint and harness secrets.
    fn llm_provider_for(
        &self,
        blueprint: &Arc<Blueprint>,
        harness_secrets: &Arc<HarnessSecretBindings>,
    ) -> Result<Option<Arc<dyn LlmProvider>>, String> {
        let dispatch = match &self.llm_dispatch {
            Some(installed) => Arc::clone(installed),
            None => Arc::new(
                HttpModelDispatch::new(
                    Arc::clone(blueprint),
                    self.secret_store.clone(),
                    Arc::clone(&self.network_policy),
                )
                .map_err(|error| error.to_string())?
                .with_harness_secrets(Arc::clone(harness_secrets)),
            ) as Arc<dyn ModelDispatch>,
        };
        Ok(Some(Arc::new(
            BlueprintLlmProvider::new(Arc::clone(blueprint), dispatch)
                .with_max_concurrency(submilli_shared::llm::provider::DEFAULT_MAX_CONCURRENCY),
        )))
    }

    /// The outbound `submilli:embedding` provider for one run, or `None` when the
    /// blueprint declares no embedding aliases.
    fn embedding_provider_for(
        &self,
        blueprint: &Arc<Blueprint>,
        harness_secrets: &Arc<HarnessSecretBindings>,
    ) -> Result<Option<Arc<dyn EmbeddingProvider>>, String> {
        if blueprint.embedding.models.is_empty() {
            return Ok(None);
        }
        let dispatch = match &self.embedding_dispatch {
            Some(installed) => Arc::clone(installed),
            None => Arc::new(
                HttpEmbeddingDispatch::new(
                    Arc::clone(blueprint),
                    self.secret_store.clone(),
                    Arc::clone(&self.network_policy),
                )
                .map_err(|error| error.to_string())?
                .with_harness_secrets(Arc::clone(harness_secrets)),
            ) as Arc<dyn EmbeddingDispatch>,
        };
        Ok(Some(Arc::new(BlueprintEmbeddingProvider::new(
            blueprint,
            dispatch,
            submilli_shared::embedding::DEFAULT_MAX_CONCURRENCY,
        ))))
    }

    /// The `@mcp/<server>` catalog for the declared servers a program imports,
    /// discovered on first use and kept, except for a session with harness
    /// secrets, whose discovery may see what other sessions do not.
    async fn mcp_catalog_for_imports(
        &self,
        blueprint: &Arc<Blueprint>,
        servers: &BTreeSet<String>,
        harness_secrets: &Arc<HarnessSecretBindings>,
    ) -> Result<Arc<McpCatalog>, String> {
        let declared: BTreeSet<String> = servers
            .iter()
            .filter(|server| blueprint.mcp.contains_key(*server))
            .cloned()
            .collect();
        if declared.is_empty() {
            return Ok(Arc::new(McpCatalog::empty()));
        }
        let key = declared.iter().cloned().collect::<Vec<_>>().join("\u{1f}");
        let session_scoped = !harness_secrets.is_empty();
        // A save's clear can land between taking the blueprint and here;
        // McpCatalogs handles it.
        let generation = {
            let catalogs = self.lock_catalogs();
            if !session_scoped && let Some(cached) = catalogs.cached(&key, blueprint) {
                return Ok(cached);
            }
            catalogs.generation
        };
        let discovered = Arc::new(
            discover_selected(
                DiscoveryAuth {
                    secret_store: self.secret_store.as_ref(),
                    oauth: self.oauth.as_ref(),
                    harness_secrets: Some(harness_secrets),
                    network_policy: &self.network_policy,
                },
                &blueprint.name,
                blueprint,
                &declared,
            )
            .await
            .map_err(|error| error.to_string())?,
        );
        if session_scoped {
            return Ok(discovered);
        }
        Ok(self
            .lock_catalogs()
            .keep(key, blueprint, generation, discovered))
    }

    fn mcp_transport(
        &self,
        blueprint: &Arc<Blueprint>,
        harness_secrets: &Arc<HarnessSecretBindings>,
    ) -> Arc<dyn McpTransport> {
        Arc::new(
            submilli_shared::mcp::transport::StreamableHttpTransport::new(
                blueprint.name.clone(),
                Arc::clone(blueprint),
                self.oauth.clone(),
                self.secret_store.clone(),
                Arc::clone(&self.network_policy),
            )
            .with_harness_secrets(Arc::clone(harness_secrets)),
        )
    }

    /// The session's HTTP client, built once so its runs share one connection
    /// pool and no other session does.
    fn http_client(&self, session_id: &str) -> Arc<dyn HttpClient> {
        // Poison means a panic interrupted an insert or an eviction. AGENTS.md
        // permits the poisoned-lock panic rather than reading the cache.
        let mut clients = self
            .http_clients
            .lock()
            .expect("http client cache poisoned");
        // Running requests retain their Arc even if its cache slot is reused.
        if clients.len() >= MAX_CACHED_HTTP_CLIENTS
            && !clients.contains_key(session_id)
            && let Some(other) = clients.keys().next().cloned()
        {
            clients.remove(&other);
        }
        Arc::clone(clients.entry(session_id.to_owned()).or_insert_with(|| {
            Arc::new(ReqwestHttpClient::new(Arc::clone(&self.network_policy)))
                as Arc<dyn HttpClient>
        }))
    }

    pub(crate) fn sessions(&self) -> &Arc<Sessions> {
        &self.sessions
    }

    pub(crate) fn package_store(&self) -> &PackageStore {
        self.packages.store()
    }

    pub(crate) fn runtime_config(&self) -> &RuntimeConfig {
        &self.runtime
    }

    /// What a closing session's finalizer runs against: the session's files and
    /// connectors under `blueprint`, with its console discarded, no session
    /// values, and no further finalizer registrations. Mirrors
    /// `submilli-server`'s `finalizers.rs` (`host_services`).
    pub(crate) async fn finalizer_store_data(
        &self,
        session_id: &str,
        blueprint: &Arc<Blueprint>,
        variables: &Arc<VarBindings>,
        secrets: &Arc<HarnessSecretBindings>,
    ) -> Result<StoreData, String> {
        let (vfs, vfs_info) = self.session_vfs(session_id, blueprint, variables).await?;
        let mut data = StoreData::with_vfs_and_cap(vfs, self.runtime.max_store_bytes);
        data.vfs_info = vfs_info;
        data.finalizing = true;
        data.git = submilli_shared::resolve_git(blueprint, variables)
            .map_err(|error| error.to_string())?;
        data.llm_provider = self.llm_provider_for(blueprint, secrets)?;
        data.llm_budget = Some(Arc::new(ExecutionTokenBudget::new(
            self.llm_limits,
            self.llm_budget.clone(),
        )));
        data.embedding_provider = self.embedding_provider_for(blueprint, secrets)?;
        data.embedding_budget = Some(Arc::new(EmbeddingTokenBudget::new(
            self.embedding_limits,
            self.embedding_budget.clone(),
        )));
        data.console = Box::new(std::io::sink());
        data.variables = Arc::clone(variables);
        data.security_check = Arc::new(PolicyCheck::with_variables(
            Arc::clone(blueprint),
            Arc::clone(variables),
        ));
        data.http_client = self.http_client(session_id);
        data.mcp_transport = Some(self.mcp_transport(blueprint, secrets));
        data.secret_provider = Arc::new(BlueprintSecretProvider::with_harness(
            Arc::clone(blueprint),
            self.secret_store.clone(),
            Arc::clone(secrets),
        ));
        data.auth_proxy = Arc::new(BlueprintAuthProxy::with_harness(
            Arc::clone(blueprint),
            self.secret_store.clone(),
            Arc::clone(secrets),
        ));
        Ok(data)
    }

    fn lock_catalogs(&self) -> std::sync::MutexGuard<'_, McpCatalogs> {
        // Poison means a panic interrupted an insert or a clear. AGENTS.md permits
        // the poisoned-lock panic rather than serving a catalog that may be stale.
        self.mcp_catalogs.lock().expect("mcp catalog poisoned")
    }
}

/// The server's default ceiling on embedding tokens across every live run.
const MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS: u64 = 50_000_000;

pub(crate) fn vfs_info(blueprint: &Blueprint) -> VfsInfo {
    VfsInfo {
        mode: match &blueprint.vfs {
            VfsConfig::None => VfsMode::None,
            VfsConfig::Ephemeral { .. } => VfsMode::Ephemeral,
            VfsConfig::PerSession { .. } => VfsMode::PerSession,
            VfsConfig::Named { .. } => VfsMode::Named,
        },
        size_limit: blueprint.vfs.size_limit(),
    }
}

fn run_start(execution_id: &str, request: &RunRequest) -> RunStart {
    RunStart {
        execution_id: execution_id.to_owned(),
        label: request.label.clone(),
        entry: request.entry.clone(),
        test_of: request.test_of.clone(),
        client: request.client.clone(),
        tool_call_id: request.tool_call_id.clone(),
        session_id: request.session_id.clone(),
        blueprint_name: request.blueprint.blueprint.name.clone(),
        blueprint_version: request.blueprint.version_tag.clone(),
        variables: Arc::clone(&request.variables),
        harness_secrets: Arc::clone(&request.harness_secrets),
        code: match request.entry {
            RunEntry::McpFileTool { .. } => None,
            _ => Some(Arc::from(request.code.as_str())),
        },
    }
}

/// The size, as JSON, of what the caller received.
fn returned_bytes(response: &ExecuteResponse) -> u64 {
    #[derive(Serialize)]
    struct Returned<'a> {
        result: &'a Option<String>,
        console: &'a [String],
        error: &'a Option<ExecuteError>,
    }
    serde_json::to_vec(&Returned {
        result: &response.result,
        console: &response.console,
        error: &response.error,
    })
    .map_or(0, |bytes| bytes.len() as u64)
}

fn failure_response(
    execution_id: &str,
    session_id: &str,
    kind: ErrorKind,
    message: String,
) -> ExecuteResponse {
    ExecuteResponse {
        execution_id: execution_id.to_owned(),
        session_id: session_id.to_owned(),
        result: None,
        console: Vec::new(),
        error: Some(ExecuteError::new(kind, message)),
        discovery_warnings: Vec::new(),
    }
}

/// The response for what the runner produced. Console output is returned only
/// with an error; on success the session's last run keeps it.
fn into_response(execution_id: &str, session_id: &str, outcome: &RunOutcome) -> ExecuteResponse {
    let (result, console, error) = match &outcome.error {
        Some(error) => (
            None,
            split_console(&outcome.console_raw),
            Some(error.clone()),
        ),
        None => (outcome.value.clone(), Vec::new(), None),
    };
    ExecuteResponse {
        execution_id: execution_id.to_owned(),
        session_id: session_id.to_owned(),
        result,
        console,
        error,
        discovery_warnings: outcome.discovery_warnings.clone(),
    }
}

// ---- The session's side of a run ----------------------------------------------

/// A run counted in flight on its session from [`SessionRun::begin`]: ended by
/// [`SessionRun::end`] with its last run, or, dropped first, as one that never
/// started or whose end is unknown, so its session can still idle out.
struct SessionRun {
    sessions: Arc<Sessions>,
    session_id: String,
    run_id: String,
    dispatched: bool,
    ended: bool,
}

impl SessionRun {
    async fn begin(
        sessions: &Arc<Sessions>,
        session_id: &str,
        run_id: &str,
    ) -> Result<Self, String> {
        sessions
            .begin_run(session_id)
            .await
            .map_err(|error| format!("internal: session init failed: {error}"))?;
        Ok(Self {
            sessions: Arc::clone(sessions),
            session_id: session_id.to_owned(),
            run_id: run_id.to_owned(),
            dispatched: false,
            ended: false,
        })
    }

    async fn end(mut self, last_run: &LastRun) -> anyhow::Result<()> {
        self.ended = true;
        let json = serde_json::to_vec(last_run)?;
        self.sessions
            .end_run(&self.session_id, &self.run_id, RunEnd::Finished(&json))
            .await
    }
}

impl Drop for SessionRun {
    fn drop(&mut self) {
        if self.ended {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let sessions = Arc::clone(&self.sessions);
        let session_id = std::mem::take(&mut self.session_id);
        let run_id = std::mem::take(&mut self.run_id);
        let dispatched = self.dispatched;
        handle.spawn(async move {
            let end = if dispatched {
                RunEnd::Unknown
            } else {
                RunEnd::NeverStarted
            };
            if let Err(error) = sessions.end_run(&session_id, &run_id, end).await {
                super::super::log::warn(&format!("ending the session's run failed: {error:#}"));
            }
        });
    }
}

// ---- The runner ----------------------------------------------------------------

/// What a program runs against.
struct Services {
    inputs: Inputs,
    recording: Option<Recording>,
    resource_use: ResourceUse,
    session_run: SessionRun,
}

struct Runtime<'a> {
    engine: &'a Engine,
    base_linker: &'a Linker<StoreData>,
    config: &'a RuntimeConfig,
}

/// Run `code` on a task of its own, which owns the store: a caller that stops
/// waiting cancels the run, and the task drains its workers, finishes its
/// recording and ends its session run before it exits.
async fn dispatch(
    code: &str,
    parsed: ParsedExecute,
    runtime: Runtime<'_>,
    (vfs, vfs_info): (Vfs, VfsInfo),
    mut services: Services,
    packages: &Arc<PreparedPackages>,
    mcps: &Arc<McpCatalog>,
) -> RunOutcome {
    let owned_code = code.to_owned();
    let engine = runtime.engine.clone();
    let linker = runtime.base_linker.clone();
    let config = runtime.config.clone();
    let packages = Arc::clone(packages);
    let owner_mcps = Arc::clone(mcps);
    let (caller_alive, caller_gone) = tokio::sync::oneshot::channel::<()>();
    // This clone stays outside the owner task, so a run whose task panics is
    // still finished.
    let recording = services.recording.clone();
    if let Some(recording) = &recording {
        recording.mark_dispatched();
    }
    services.session_run.dispatched = true;
    let owner = tokio::spawn(async move {
        let mut services = services;
        // Closing the session cancels its work, as an explicit cancel does.
        let requested = std::mem::replace(
            &mut services.inputs.cancel_requested,
            tokio::sync::oneshot::channel().1,
        );
        services.inputs.cancel_requested =
            cancel_on_close_or_request(services.resource_use.cancel.clone(), requested);
        let owner_recording = services.recording.take();
        let log = owner_recording.as_ref().map(Recording::log);
        let resource_use = services.resource_use;
        let session_run = services.session_run;
        let outcome = run_inner(
            &owned_code,
            parsed,
            Runtime {
                engine: &engine,
                base_linker: &linker,
                config: &config,
            },
            vfs,
            vfs_info,
            services.inputs,
            &packages,
            &owner_mcps,
            caller_gone,
            log.clone(),
        )
        .await;
        // The session's files and values may go once its work has drained.
        drop(resource_use);
        let mut outcome = outcome;
        if session_run.end(&outcome.last_run()).await.is_err() {
            // Effects may already have occurred: keep the value and usage.
            outcome.error = RunOutcome::internal_failure(
                "session completion activity could not commit; execution may have performed effects",
            )
            .error;
        }
        if let (Some(owner_recording), Some(log)) = (owner_recording, log) {
            owner_recording.finish(FinishedRun {
                dispatched: true,
                error: outcome.error.clone(),
                result: outcome.value.clone(),
                console: outcome.console_raw.clone(),
                usage: outcome.usage,
                log: log.finish(),
                mcp_catalog: Some(Arc::clone(&owner_mcps)),
                wall: owner_recording.started().elapsed(),
            });
        }
        outcome
    });
    let outcome = match owner.await {
        Ok(outcome) => outcome,
        Err(error) => {
            let outcome = RunOutcome::internal_failure(&format!("execution task failed: {error}"));
            if let (Some(recording), Some(error)) = (&recording, &outcome.error) {
                // A no-op when the owner had already finished the run.
                recording.lost(error, Arc::clone(mcps));
            }
            outcome
        }
    };
    drop(caller_alive);
    outcome
}

/// A cancel signal that fires when `resource_cancel` does, as closing the session
/// does, or when `requested` is sent. A forwarding task watches both until the
/// returned receiver is dropped.
fn cancel_on_close_or_request(
    resource_cancel: tokio_util::sync::CancellationToken,
    requested: tokio::sync::oneshot::Receiver<()>,
) -> tokio::sync::oneshot::Receiver<()> {
    let (mut sender, receiver) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        tokio::select! {
            () = resource_cancel.cancelled() => { let _ = sender.send(()); }
            Ok(()) = requested => { let _ = sender.send(()); }
            () = sender.closed() => {}
        }
    });
    receiver
}

/// The per-run host capabilities [`run_inner`] installs in the store.
struct Inputs {
    git: Result<Option<interpreter::stdlib::git::GitConfig>, String>,
    auth_proxy: Arc<dyn interpreter::runtime::AuthProxy>,
    variables: Arc<VarBindings>,
    secret_provider: Arc<dyn interpreter::runtime::SecretProvider>,
    security_check: Arc<dyn interpreter::runtime::SecurityCheck>,
    http_client: Arc<dyn HttpClient>,
    mcp_transport: Arc<dyn McpTransport>,
    session_kv: Arc<dyn SessionKvStore>,
    session_finalizers: Option<Arc<dyn SessionFinalizers>>,
    cursor_keys: Option<Arc<CursorKeys>>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    llm_budget: Arc<ExecutionTokenBudget>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    embedding_budget: Arc<EmbeddingTokenBudget>,
    cancel_requested: tokio::sync::oneshot::Receiver<()>,
}

// The per-run signals and the decision log are separate arguments on purpose: they
// are not host services, and a grouping struct would only be built to be taken
// apart here.
#[allow(clippy::too_many_arguments)]
async fn run_inner(
    code: &str,
    mut parsed: ParsedExecute,
    runtime: Runtime<'_>,
    vfs: Vfs,
    vfs_info: VfsInfo,
    inputs: Inputs,
    packages: &PreparedPackages,
    mcps: &McpCatalog,
    mut caller_gone: tokio::sync::oneshot::Receiver<()>,
    log: Option<Arc<DecisionLog>>,
) -> RunOutcome {
    let git = match inputs.git {
        Ok(git) => git,
        Err(error) => return RunOutcome::internal_failure(&error),
    };
    let discovery_warnings: Vec<String> = mcps
        .warnings()
        .map(|w| format!("@mcp/{}: {}", w.server, w.message))
        .collect();
    if parsed.parsed.has_errors() {
        return compile_failure(
            &parsed.sources,
            parsed.parsed.diagnostics(),
            discovery_warnings,
        );
    }

    let package_refs: Vec<_> = packages.script_declarations.iter().collect();
    let virtual_refs = mcps.defs_refs();
    // Package sources are registered so package-originated traps render source
    // context.
    if let Err(error) = register_package_sources(&mut parsed.sources, packages) {
        return RunOutcome::internal_failure(&error.to_string());
    }
    let compiled = on_compiler_thread(|| {
        compile_parsed_script_timed(
            code,
            FILENAME,
            &parsed.parsed,
            &packages.stdlib_declarations,
            &package_refs,
            &virtual_refs,
        )
    });
    let compiled = match compiled {
        Ok(Ok(out)) => out,
        Ok(Err(diags)) => return compile_failure(&parsed.sources, &diags, discovery_warnings),
        Err(error) => return RunOutcome::internal_failure(&error.to_string()),
    };

    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    // The file system is complete here: its filesystem is bound at construction.
    let mut data = StoreData::with_vfs_and_cap(vfs, runtime.config.max_store_bytes);
    data.git = git;
    data.vfs_info = vfs_info;
    data.auth_proxy = inputs.auth_proxy;
    data.variables = inputs.variables;
    data.secret_provider = inputs.secret_provider;
    data.security_check = inputs.security_check;
    // Outermost, so the host functions see the recorder; installed before packages
    // run their top-level statements, so those decisions are recorded too.
    if let Some(log) = &log {
        data.security_check = log.wrap(data.security_check.clone());
    }
    data.http_client = inputs.http_client;
    data.mcp_transport = Some(inputs.mcp_transport);
    data.session_kv = Some(inputs.session_kv);
    data.session_finalizers = inputs.session_finalizers;
    data.cursor_keys = inputs.cursor_keys;
    data.llm_provider = inputs.llm_provider;
    data.llm_budget = Some(inputs.llm_budget);
    data.embedding_provider = inputs.embedding_provider;
    data.embedding_budget = Some(inputs.embedding_budget);
    data.console = Box::new(Sink {
        output: buf.clone(),
        lines: log.clone().map(ConsoleLines::new),
    });
    data.install_type_info(compiled.type_info.clone());

    let mut store = match runtime.config.store_async(runtime.engine, data) {
        Ok(s) => s,
        Err(err) => return RunOutcome::internal_failure(&format!("store init failed: {err}")),
    };
    // The shared ticker must not interrupt setup. Arm this store only when guest
    // code begins (the packages' top-level statements, then the program's).
    store.set_epoch_deadline(u64::MAX);
    let execution = async {
        install_tenant_limits(&mut store);
        let module = match Module::new(runtime.engine, &compiled.wasm) {
            Ok(m) => m,
            Err(err) => return RunOutcome::internal_failure(&format!("module load failed: {err}")),
        };
        let mut linker = runtime.base_linker.clone();
        let package_modules: Vec<_> = packages
            .modules
            .iter()
            .map(|package| LinkedPackageModule {
                module: &package.module,
                declaration: &package.declaration,
                type_info: &package.type_info,
            })
            .collect();
        if let Err(err) = install_runtime_store_bound(&mut linker, &mut store) {
            return RunOutcome::internal_failure(&format!("install runtime failed: {err}"));
        }
        // Installing a package runs its top-level statements, and instantiation
        // runs the program's, so the deadline covers both.
        timeout::arm(&mut store, runtime.config.timeout, |data| data.clock_paused);
        let installed =
            install_package_modules_async(&mut linker, &mut store, &package_modules).await;
        let dispatch = 'program: {
            match installed {
                Ok(()) => {}
                Err(err) if raised_by_top_level_statements(&err) => break 'program Err(err),
                Err(err) => {
                    return RunOutcome::internal_failure(&format!(
                        "install packages failed: {err}"
                    ));
                }
            }
            let instance = match instantiate_program_async(&linker, &mut store, &module).await {
                Ok(instance) => instance,
                Err(err) if raised_by_top_level_statements(&err) => break 'program Err(err),
                Err(err) => {
                    return RunOutcome::internal_failure(&format!(
                        "instantiate_async failed: {err}"
                    ));
                }
            };
            dispatch_main_async(&mut store, &instance).await
        };
        let console_raw = captured_console(&buf);
        match dispatch {
            Ok(value) => RunOutcome {
                value,
                console_raw,
                discovery_warnings,
                ..RunOutcome::default()
            },
            Err(err) => RunOutcome {
                error: Some(classify_runtime_error(&err, &parsed.sources, parsed.file)),
                console_raw,
                discovery_warnings,
                ..RunOutcome::default()
            },
        }
    };
    // Only a sent cancel counts: the canceller is dropped, unsent, when the run ends.
    let cancel_requested = inputs.cancel_requested;
    let cancel_sent = async {
        if cancel_requested.await.is_ok() {
            return;
        }
        std::future::pending::<()>().await;
    };
    let mut outcome = tokio::select! {
        biased;
        _ = &mut caller_gone => RunOutcome::cancelled(),
        () = cancel_sent => RunOutcome::cancelled(),
        outcome = execution => outcome,
    };
    if let Err(error) = store.data_mut().blocking_work.finish().await {
        record_worker_cleanup_failure(&mut outcome, &error);
    }
    match ExecutionUsage::capture(&store, runtime.config.fuel) {
        Ok(usage) => outcome.usage = usage,
        Err(error) => {
            return RunOutcome::internal_failure(&format!("usage capture failed: {error}"));
        }
    }
    outcome
}

fn record_worker_cleanup_failure(
    outcome: &mut RunOutcome,
    error: &interpreter::runtime::blocking::BlockingWorkDrainError,
) {
    let secondary = format!("worker cleanup failure: {error}");
    if let Some(primary) = outcome.error.as_mut() {
        primary.message.push('\n');
        primary.message.push_str(&secondary);
        return;
    }
    outcome.value = None;
    outcome.error = RunOutcome::internal_failure(&secondary).error;
}

#[cfg(test)]
mod tests;
