//! Per-session lifecycle: VFS directories, idle reaping, and durable bookkeeping.
//!
//! A session exists for every VFS mode (it always holds the `lastRun` result and
//! an idle timer); `per_session` additionally owns a disk-backed directory that
//! survives across executes within one session and is wiped when the session
//! ends. Sessions end two ways:
//!
//! * **idle** — no execute for `idle_timeout`; the background reaper wipes it.
//! * **explicit terminate** — the MCP transport's HTTP `DELETE` (or the REST
//!   `DELETE /v1/sessions/{id}`) wipes the session immediately via [`wipe_now`].
//!
//! Streamable HTTP has no connection to drop, so there is no grace window or
//! resume token: the `MCP-Session-Id` is the durable handle a client reuses.
//!
//! The in-memory `HashMap` is the authoritative live cache; a
//! [`DurableSessionStore`] mirrors it write-through so resume and reaping survive
//! a restart. [`boot`] rehydrates the cache and sweeps orphan directories. Idle
//! timers are wall-clock (`SystemTime`) so they are measured against persisted
//! timestamps, not process uptime.
//!
//! Only `per_session` owns a directory the manager wipes. `ephemeral` dirs are
//! per-execute (owned by the runner); a `persistent` directory is one the
//! operator declared as a volume in the server config, and is never created,
//! wiped, or reaped here — [`build_vfs`] only resolves the blueprint's volume
//! name through the declared table and mounts what it finds.
//!
//! [`wipe_now`]: SessionManager::wipe_now
//! [`boot`]: SessionManager::boot

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use interpreter::runtime::{
    ExecutionTokenBudget, HttpClient, InMemorySessionKv, LlmLimits, SessionKvLimits,
    SessionKvStore, SharedKvBudget, SharedTokenBudget, Vfs, VfsInfo, VfsMode as RtVfsMode,
};
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings, VfsConfig};
use uuid::Uuid;

use crate::config::VolumeTable;
use crate::idempotency_store::IdempotencyStore;
use crate::session_store::{DurableSessionStore, SessionRecord};

/// Builds a fresh per-session HTTP client (its own connection pool). Injected so
/// the manager owns each session's client lifecycle without depending on the
/// concrete reqwest type.
pub type HttpClientFactory = Arc<dyn Fn() -> Arc<dyn HttpClient> + Send + Sync>;

/// Minimum gap between persisting a session's `last_activity`. An exact
/// timestamp is not worth an fsync per execute against a multi-hour idle window;
/// a crash can lose at most this much idle-timer freshness.
const PERSIST_INTERVAL: Duration = Duration::from_secs(30);

/// Server-wide ceiling on retained `submilli:session` bytes across every live
/// session. Deliberately generous — 64 sessions at the 16 MiB per-session cap,
/// or thousands of realistically-sized ones — because the per-session limit is
/// the bound an operator reasons about, and a tight aggregate would refuse
/// writes for a reason no single program could see coming. It exists so a
/// process cannot be pushed out of memory by session count alone; an operator
/// sizing for many concurrent sessions raises it via `max_session_state_memory`.
pub const DEFAULT_TOTAL_SESSION_KV_BYTES: u64 = 1024 * 1024 * 1024;

/// Server-wide ceiling on `submilli:llm` tokens across every live execution.
/// Re-exported from the runtime rather than chosen again here, so the number an
/// operator raises is the number the refusal message names.
pub use interpreter::runtime::DEFAULT_MAX_ALL_EXECUTIONS_TOKENS;

/// Elements one `batch` dispatches at once. Re-exported from the provider that
/// enforces it, so the bound the operator configures and the bound the semaphore
/// takes cannot drift apart.
pub use submilli_shared::llm::provider::DEFAULT_MAX_CONCURRENCY;

#[derive(Debug)]
pub enum SessionError {
    UnknownSession,
    Io(String),
    /// The blueprint names a volume this server does not declare — the operator
    /// removed or renamed it since the blueprint was registered.
    UnknownVolume(String),
    /// A declared volume could not be mounted: its directory is gone, is not a
    /// directory, or is unreadable. The host path is deliberately absent — it is
    /// logged server-side instead, so a client learns only the volume name.
    VolumeUnavailable(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::UnknownSession => f.write_str("unknown or expired session"),
            SessionError::Io(msg) => write!(f, "session vfs io: {msg}"),
            SessionError::UnknownVolume(name) => write!(
                f,
                "volume '{name}' is not declared on this server; ask the operator to declare it \
                 under `volumes:` in the server config"
            ),
            SessionError::VolumeUnavailable(name) => write!(
                f,
                "volume '{name}' is declared but unavailable; the server log has the details"
            ),
        }
    }
}

impl std::error::Error for SessionError {}

struct SessionEntry {
    blueprint_name: String,
    /// Directory the manager owns and wipes (`per_session` only).
    vfs_root: Option<PathBuf>,
    idle_timeout: Duration,
    last_activity: SystemTime,
    /// `last_activity` value last written to the durable store; drives the
    /// debounce in [`SessionManager::touch`].
    persisted_activity: SystemTime,
    /// This session's HTTP client (own connection pool), built on first use.
    /// Dropped with the entry on reap/terminate, so no pool outlives its session
    /// and no connection is shared across sessions.
    http_client: Option<Arc<dyn HttpClient>>,
    /// Resolved `${vars.NAME}` bindings, bound once at session init and persisted
    /// in the session record. Empty for sessions that bind none.
    variables: Arc<VarBindings>,
    /// Trusted harness credentials are memory-only. `None` after restart means
    /// the session must be rebound before a required harness secret can run.
    harness_secrets: Option<Arc<HarnessSecretBindings>>,
    /// `submilli:session` storage for this session, built on first use. Memory
    /// only, like [`Self::http_client`]: dropped with the entry, so every way a
    /// session ends takes the state with it, and a session restored from a
    /// record starts empty.
    session_kv: Option<Arc<dyn SessionKvStore>>,
}

struct State {
    sessions: HashMap<String, SessionEntry>,
}

pub struct SessionManager {
    inner: Mutex<State>,
    /// Durable root for `per_session` directories (keyed by session id).
    session_root: PathBuf,
    /// Root for `ephemeral` scratch dirs; `None` uses the OS temp dir.
    ephemeral_root: Option<PathBuf>,
    /// Operator-declared volumes a `persistent` blueprint resolves through.
    /// Passed in rather than looked up globally, so every mount path takes the
    /// same table.
    volumes: Arc<VolumeTable>,
    http_client_factory: HttpClientFactory,
    store: Arc<dyn DurableSessionStore>,
    /// Idempotency entries are session-scoped, so they end when the session
    /// does. Held here because every record-driven removal path funnels through
    /// [`SessionManager::forget`].
    idempotency: Arc<dyn IdempotencyStore>,
    session_kv: SessionKvSettings,
    llm: LlmSettings,
}

/// How a session's `submilli:session` store is built. Cloned into every session
/// that allocates one, so the aggregate budget is shared across them all.
#[derive(Clone)]
pub struct SessionKvSettings {
    pub limits: SessionKvLimits,
    /// Server-wide ceiling on retained session-KV bytes across live sessions.
    /// Reserved compare-and-swap, so two sessions cannot both claim the same
    /// headroom.
    pub budget: SharedKvBudget,
}

impl Default for SessionKvSettings {
    fn default() -> Self {
        Self::new(SessionKvLimits::default(), DEFAULT_TOTAL_SESSION_KV_BYTES)
    }
}

impl SessionKvSettings {
    pub fn new(limits: SessionKvLimits, total_bytes: u64) -> Self {
        Self {
            limits,
            budget: SharedKvBudget::new(total_bytes),
        }
    }

    fn build(&self) -> Arc<dyn SessionKvStore> {
        Arc::new(InMemorySessionKv::with_shared_budget(
            self.limits,
            self.budget.clone(),
        ))
    }
}

/// How an execution's `submilli:llm` token budget is built. Cloned into every
/// execution that allocates one, so the server-wide ceiling is shared across
/// them all — the [`SessionKvSettings`] shape, for the same reason.
///
/// The unit of allocation differs, and deliberately. A session-KV store is
/// *per-session*, cached on the entry so two executes in one session see one set
/// of entries. A token budget is *per-execution*: [`ExecutionTokenBudget`]
/// releases its reservation on `Drop`, so caching one on the session would hold
/// every execute's spend until the session ended, and the per-execution ceiling
/// would bound a session's whole lifetime instead of one run.
#[derive(Clone)]
pub struct LlmSettings {
    pub limits: LlmLimits,
    /// Server-wide ceiling summed across every live execution. Reserved
    /// compare-and-swap, so two executions cannot both claim the same headroom.
    pub budget: SharedTokenBudget,
    /// Elements dispatched at once within one `batch` (KTD4).
    pub max_concurrency: usize,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self::new(
            LlmLimits::default(),
            DEFAULT_MAX_ALL_EXECUTIONS_TOKENS,
            DEFAULT_MAX_CONCURRENCY,
        )
    }
}

impl LlmSettings {
    pub fn new(limits: LlmLimits, total_tokens: u64, max_concurrency: usize) -> Self {
        Self {
            limits,
            budget: SharedTokenBudget::new(total_tokens),
            // Zero would deadlock the provider's semaphore, so it clamps to one
            // here as well as at the provider — an operator who writes 0 gets
            // serial dispatch, not a hang.
            max_concurrency: max_concurrency.max(1),
        }
    }

    /// A fresh per-execution budget sharing this server's aggregate ceiling.
    fn build(&self) -> Arc<ExecutionTokenBudget> {
        Arc::new(ExecutionTokenBudget::new(self.limits, self.budget.clone()))
    }
}

/// The stateful host capabilities a session's executions draw on, each carrying
/// the aggregate it reserves against. Grouped so the manager's constructor stays
/// readable as the set grows — the reason [`crate::runner::HostServices`] is a
/// struct rather than a parameter list.
#[derive(Clone, Default)]
pub struct CapabilitySettings {
    pub session_kv: SessionKvSettings,
    pub llm: LlmSettings,
}

impl SessionManager {
    pub fn new(
        session_root: PathBuf,
        ephemeral_root: Option<PathBuf>,
        volumes: Arc<VolumeTable>,
        http_client_factory: HttpClientFactory,
        store: Arc<dyn DurableSessionStore>,
        idempotency: Arc<dyn IdempotencyStore>,
        capabilities: CapabilitySettings,
    ) -> Self {
        let CapabilitySettings { session_kv, llm } = capabilities;
        Self {
            inner: Mutex::new(State {
                sessions: HashMap::new(),
            }),
            session_root,
            ephemeral_root,
            volumes,
            http_client_factory,
            store,
            idempotency,
            session_kv,
            llm,
        }
    }

    /// The HTTP client for `session_id`, built once and cached on the session so
    /// every execute in that session shares one connection pool — and no other
    /// session does. An unregistered session id gets a throwaway client.
    pub fn http_client(&self, session_id: &str) -> Arc<dyn HttpClient> {
        let mut state = self.lock();
        match state.sessions.get_mut(session_id) {
            Some(entry) => entry
                .http_client
                .get_or_insert_with(|| (self.http_client_factory)())
                .clone(),
            None => (self.http_client_factory)(),
        }
    }

    /// The `submilli:session` store for `session_id`, built once and cached on
    /// the session so every execute in it — REST or MCP — reads and writes one
    /// set of entries, and no other session does.
    ///
    /// An unregistered session id gets a throwaway store: the one-shot
    /// `POST /v1/execute` route mints a transient session per call, so its state
    /// has nothing to outlive the call and is discarded at completion. The
    /// aggregate budget still covers it, and releases when it drops.
    pub fn session_kv_for_execute(&self, session_id: &str) -> Arc<dyn SessionKvStore> {
        let mut state = self.lock();
        match state.sessions.get_mut(session_id) {
            Some(entry) => entry
                .session_kv
                .get_or_insert_with(|| self.session_kv.build())
                .clone(),
            None => self.session_kv.build(),
        }
    }

    /// A fresh `submilli:llm` token budget for one execution, sharing this
    /// server's aggregate ceiling.
    ///
    /// Unlike [`Self::session_kv_for_execute`] this is deliberately *not* cached
    /// on the session: the reservation releases on `Drop`, so a budget held by a
    /// session entry would keep every past execute's spend charged until the
    /// session ended — and the per-execution ceiling would silently become a
    /// per-session one. Every execute gets its own, and the aggregate is what
    /// ties them together.
    ///
    /// The one-shot `POST /v1/execute` route therefore needs no special case:
    /// its transient session counts against the same aggregate and releases when
    /// the run returns.
    pub fn llm_budget_for_execute(&self) -> Arc<ExecutionTokenBudget> {
        self.llm.build()
    }

    /// The fan-out bound one `batch` dispatches at (KTD4).
    pub fn llm_max_concurrency(&self) -> usize {
        self.llm.max_concurrency
    }

    /// The server-wide LLM token budget, for operator-facing reporting.
    pub fn llm_budget(&self) -> &SharedTokenBudget {
        &self.llm.budget
    }

    /// The server-wide session-KV budget, for operator-facing reporting.
    pub fn session_kv_budget(&self) -> &SharedKvBudget {
        &self.session_kv.budget
    }

    /// Register a session and bind its resolved `${vars.NAME}` values in one step
    /// — the MCP `initialize` entry point, which runs before the first execute.
    /// Idempotent like [`Self::ensure`]; a re-`initialize` replaces the bindings.
    pub async fn bind(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
        variables: Arc<VarBindings>,
        harness_secrets: Arc<HarnessSecretBindings>,
    ) -> Result<(), SessionError> {
        self.ensure(session_id, blueprint).await?;
        let record = {
            let mut state = self.lock();
            let Some(entry) = state.sessions.get_mut(session_id) else {
                return Ok(());
            };
            entry.variables = variables;
            entry.harness_secrets = Some(harness_secrets);
            entry.to_record(session_id)
        };
        self.persist(record).await;
        Ok(())
    }

    /// The session's bound variables, or an empty set if none were bound (an
    /// absent `${vars.NAME}` is then a non-match at filter eval).
    pub fn variables(&self, session_id: &str) -> Arc<VarBindings> {
        self.lock()
            .sessions
            .get(session_id)
            .map(|entry| Arc::clone(&entry.variables))
            .unwrap_or_default()
    }

    pub fn harness_secrets(&self, session_id: &str) -> Option<Arc<HarnessSecretBindings>> {
        self.lock()
            .sessions
            .get(session_id)
            .and_then(|entry| entry.harness_secrets.as_ref().map(Arc::clone))
    }

    pub fn rebind_harness_secrets(
        &self,
        session_id: &str,
        harness_secrets: Arc<HarnessSecretBindings>,
    ) -> Result<(), SessionError> {
        let mut state = self.lock();
        let entry = state
            .sessions
            .get_mut(session_id)
            .ok_or(SessionError::UnknownSession)?;
        entry.harness_secrets = Some(harness_secrets);
        Ok(())
    }

    /// Root for `ephemeral` scratch directories, if one was configured.
    pub(crate) fn ephemeral_root(&self) -> Option<&Path> {
        self.ephemeral_root.as_deref()
    }

    /// The declared volume table. Every caller of [`build_vfs`] reads it from
    /// here, so no mount route can skip volume resolution.
    pub(crate) fn volumes(&self) -> &VolumeTable {
        &self.volumes
    }

    /// Allocate a fresh session (server-chosen id) bound to a blueprint and its
    /// resolved `${vars.NAME}` values, and, for `per_session`, its scratch
    /// directory. Used by the explicit `POST /v1/sessions` flow.
    pub async fn create(
        &self,
        blueprint: &Blueprint,
        variables: Arc<VarBindings>,
        harness_secrets: Arc<HarnessSecretBindings>,
    ) -> Result<String, SessionError> {
        let session_id = Uuid::new_v4().to_string();
        self.bind(&session_id, blueprint, variables, harness_secrets)
            .await?;
        Ok(session_id)
    }

    /// The blueprint a session is bound to, or `None` for an unknown session.
    /// The session-scoped execute path reads it to load the bound sandbox.
    pub fn blueprint_name(&self, session_id: &str) -> Option<String> {
        self.lock()
            .sessions
            .get(session_id)
            .map(|entry| entry.blueprint_name.clone())
    }

    /// Idempotently register a session under `session_id`. Callers invoke this
    /// so any client-chosen id works and a `per_session` VFS persists across
    /// executes that reuse the same id.
    pub async fn ensure(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
    ) -> Result<(), SessionError> {
        if self.contains(session_id) {
            return Ok(());
        }

        let owns_vfs_dir = matches!(blueprint.vfs, VfsConfig::PerSession { .. });
        let vfs_root = if owns_vfs_dir {
            let dir = self.session_root.join(session_id);
            std::fs::create_dir_all(&dir).map_err(|e| SessionError::Io(e.to_string()))?;
            Some(dir)
        } else {
            None
        };

        let now = SystemTime::now();
        let entry = SessionEntry {
            blueprint_name: blueprint.name.clone(),
            vfs_root,
            idle_timeout: blueprint.idle_timeout,
            last_activity: now,
            persisted_activity: now,
            http_client: None,
            // Bindings are attached separately via `bind` (MCP init); a plain
            // `ensure` (REST, or MCP execute after init) leaves them empty.
            variables: Arc::new(VarBindings::new()),
            harness_secrets: None,
            session_kv: None,
        };

        let inserted = {
            let mut state = self.lock();
            // Lost a creation race — keep the winner; our dir path is identical.
            match state.sessions.entry(session_id.to_string()) {
                Entry::Occupied(_) => false,
                Entry::Vacant(slot) => {
                    slot.insert(entry);
                    true
                }
            }
        };

        if inserted {
            crate::metrics::session_init(vfs_kind(&blueprint.vfs));
            self.persist(SessionRecord {
                session_id: session_id.to_string(),
                blueprint_name: blueprint.name.clone(),
                idle_timeout: blueprint.idle_timeout,
                last_activity: now,
                owns_vfs_dir,
                mcp_state: None,
                variables: VarBindings::new(),
            })
            .await;
        }
        Ok(())
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.lock().sessions.contains_key(session_id)
    }

    /// Number of live sessions — what `submilli server status` reports as
    /// active connections.
    pub fn active_count(&self) -> usize {
        self.lock().sessions.len()
    }

    /// The session's VFS + info, without its size limit: enough for reading its
    /// files. The blueprint name is validated against the session to stop a
    /// session being driven under a different sandbox than it was created with.
    pub fn session_vfs(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let session_root = {
            let state = self.lock();
            let entry = state
                .sessions
                .get(session_id)
                .ok_or(SessionError::UnknownSession)?;
            if entry.blueprint_name != blueprint.name {
                return Err(SessionError::UnknownSession);
            }
            entry.vfs_root.clone()
        };

        let vfs = build_vfs(
            blueprint,
            session_root.as_deref(),
            self.ephemeral_root.as_deref(),
            &self.volumes,
        )?;
        Ok((vfs, vfs_info(blueprint)))
    }

    /// [`session_vfs`](Self::session_vfs) with the blueprint's size limit
    /// enforced, for running a program that may write.
    pub async fn vfs_for_execute(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let (vfs, info) = self.session_vfs(session_id, blueprint)?;
        Ok((attach_size_limit(vfs, blueprint).await?, info))
    }

    /// Mark execute activity, keeping the session alive and resetting idle.
    /// Persistence of the new `last_activity` is debounced (see
    /// [`PERSIST_INTERVAL`]).
    pub async fn touch(&self, session_id: &str) -> bool {
        let record = {
            let mut state = self.lock();
            let Some(entry) = state.sessions.get_mut(session_id) else {
                return false;
            };
            let now = SystemTime::now();
            entry.last_activity = now;
            if now
                .duration_since(entry.persisted_activity)
                .unwrap_or_default()
                < PERSIST_INTERVAL
            {
                return true;
            }
            entry.persisted_activity = now;
            entry.to_record(session_id)
        };
        self.persist(record).await;
        true
    }

    /// Terminate a session now: wipe its owned directory and drop the entry.
    /// The MCP transport's HTTP `DELETE` maps here. Returns whether a session
    /// existed.
    pub async fn wipe_now(&self, session_id: &str) -> bool {
        let existed = {
            let mut state = self.lock();
            remove_entry(&mut state, session_id)
        };
        if existed {
            self.forget(session_id).await;
        }
        existed
    }

    /// Terminate all live sessions bound to a removed blueprint. This drops both
    /// the in-memory lifecycle entry and the durable record, including rmcp's
    /// restore payload, so re-creating the blueprint by the same name cannot
    /// resurrect old MCP sessions.
    pub async fn wipe_blueprint(&self, blueprint_name: &str) {
        let ids: Vec<String> = {
            let mut state = self.lock();
            let ids: Vec<String> = state
                .sessions
                .iter()
                .filter(|(_, entry)| entry.blueprint_name == blueprint_name)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                remove_entry(&mut state, id);
            }
            ids
        };
        for id in &ids {
            self.forget(id).await;
        }
    }

    /// Wipe sessions whose idle window has elapsed. Returns the count wiped
    /// (useful for tests).
    pub async fn reap(&self, now: SystemTime) -> usize {
        let expired: Vec<String> = {
            let mut state = self.lock();
            let ids: Vec<String> = state
                .sessions
                .iter()
                .filter(|(_, e)| e.is_expired(now))
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                remove_entry(&mut state, id);
            }
            ids
        };
        for id in &expired {
            self.forget(id).await;
        }
        expired.len()
    }

    pub async fn reap_now(&self) -> usize {
        self.reap(SystemTime::now()).await
    }

    /// Rehydrate the in-memory cache from the durable store and reconcile the
    /// `per_session` directory tree against it. Call once, before serving:
    /// pre-restart sessions become resumable and directories no live session
    /// owns are swept.
    pub async fn boot(&self) {
        for record in self.store.load_all().await {
            let vfs_root = record
                .owns_vfs_dir
                .then(|| self.session_root.join(&record.session_id));
            let last_activity = record.last_activity;
            let entry = SessionEntry {
                blueprint_name: record.blueprint_name,
                vfs_root,
                idle_timeout: record.idle_timeout,
                last_activity,
                persisted_activity: last_activity,
                http_client: None,
                variables: Arc::new(record.variables),
                harness_secrets: None,
                // Nothing in `SessionRecord` carries KV state, so a restored
                // session starts empty — the same restart semantics the
                // harness secrets and the HTTP client already have.
                session_kv: None,
            };
            self.lock().sessions.insert(record.session_id, entry);
        }
        self.reconcile_orphans().await;
        self.reconcile_ledger().await;
    }

    /// Drop idempotency entries for sessions that did not come back. `forget`
    /// covers every path where a *record* exists to drive removal; two escape
    /// it, and this one sweep covers both. `reconcile_orphans` deletes orphan
    /// VFS directories without calling `forget`, and an operator can pair a
    /// file-backed ledger with the default in-memory session store, in which
    /// case no record survives a restart to reach `forget` at all.
    ///
    /// Runs after `reconcile_orphans`, so records it dropped are already gone
    /// from the live set and their ledgers are swept in the same pass.
    async fn reconcile_ledger(&self) {
        for session_id in self.idempotency.session_ids().await {
            if self.contains(&session_id) {
                continue;
            }
            if let Err(err) = self.idempotency.purge_session(&session_id).await {
                tracing::warn!(?err, "failed to purge orphan idempotency entries");
            }
        }
    }

    /// Sweep `per_session` directories no live session owns, and drop records
    /// whose directory has vanished. Only `session_root` holds these dirs, so a
    /// session in any other mode is untouched.
    async fn reconcile_orphans(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.session_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if path.is_dir() && !self.contains(name) {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }

        let stale: Vec<String> = {
            let state = self.lock();
            state
                .sessions
                .iter()
                .filter(|(_, e)| e.vfs_root.as_deref().is_some_and(|p| !p.exists()))
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &stale {
            self.lock().sessions.remove(id);
            self.forget(id).await;
        }
    }

    /// Spawn a background reaper if a tokio runtime is available. The sweep
    /// interval is coarse; expiry is wall-clock, so a session is wiped within
    /// one interval of its deadline.
    pub fn spawn_reaper(self: &Arc<Self>, interval: Duration) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let manager = Arc::clone(self);
        handle.spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                manager.reap_now().await;
            }
        });
    }

    /// Write a record through to the durable store. A failure is logged, not
    /// propagated: the in-memory cache is authoritative for a live process;
    /// persistence is a best-effort mirror for surviving a restart.
    async fn persist(&self, mut record: SessionRecord) {
        if record.mcp_state.is_none() {
            match self.store.load(&record.session_id).await {
                Ok(Some(existing)) => record.mcp_state = existing.mcp_state,
                Ok(None) => {}
                Err(err) => tracing::warn!(?err, "failed to load previous session record"),
            }
        }
        if let Err(err) = self.store.put(record).await {
            tracing::warn!(?err, "failed to persist session record");
        }
    }

    /// Drop everything keyed by a session that is ending. A failed purge is
    /// logged, not propagated — unlike a *reservation* write, whose failure
    /// refuses the request: the session is already gone by the time we get
    /// here, so the worst case is leaked entries no live session can reach.
    async fn forget(&self, session_id: &str) {
        if let Err(err) = self.store.remove(session_id).await {
            tracing::warn!(?err, "failed to drop session record");
        }
        if let Err(err) = self.idempotency.purge_session(session_id).await {
            tracing::warn!(?err, "failed to purge session idempotency entries");
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.lock().expect("session manager mutex poisoned")
    }
}

impl SessionEntry {
    fn is_expired(&self, now: SystemTime) -> bool {
        now.duration_since(self.last_activity).unwrap_or_default() > self.idle_timeout
    }

    fn to_record(&self, session_id: &str) -> SessionRecord {
        SessionRecord {
            session_id: session_id.to_string(),
            blueprint_name: self.blueprint_name.clone(),
            idle_timeout: self.idle_timeout,
            last_activity: self.last_activity,
            owns_vfs_dir: self.vfs_root.is_some(),
            mcp_state: None,
            variables: (*self.variables).clone(),
        }
    }
}

/// Wipe a session's owned directory and drop its in-memory bookkeeping. Returns
/// whether an entry existed. The durable record is dropped separately by the
/// caller (outside the lock).
/// Dropping the entry drops its `session_kv` handle. A run still holding one
/// keeps reading its own entries until it returns — no execution sees the store
/// vanish mid-call — but nothing can reach it again, so the state is gone and
/// its aggregate reservation is released with the last handle.
fn remove_entry(state: &mut State, session_id: &str) -> bool {
    match state.sessions.remove(session_id) {
        Some(entry) => {
            if let Some(root) = entry.vfs_root {
                // Best-effort: a failed wipe leaves a dir under the storage
                // root, but the session is gone either way.
                let _ = std::fs::remove_dir_all(&root);
            }
            true
        }
        None => false,
    }
}

fn vfs_kind(vfs: &VfsConfig) -> &'static str {
    match vfs {
        VfsConfig::None => "none",
        VfsConfig::Ephemeral { .. } => "ephemeral",
        VfsConfig::PerSession { .. } => "per_session",
        VfsConfig::Persistent { .. } => "persistent",
    }
}

/// Build the `Vfs` for one execute. `persistent` mode resolves its blueprint's
/// volume name through `volumes` here — at the single point every mount is
/// constructed — so no route into a session can mount a directory the operator
/// did not declare.
pub(crate) fn build_vfs(
    blueprint: &Blueprint,
    session_root: Option<&Path>,
    ephemeral_root: Option<&Path>,
    volumes: &VolumeTable,
) -> Result<Vfs, SessionError> {
    // `Vfs` formats the host root into its error, and these roots are the server's own
    // storage layout. Redact for the same reason a volume's target is redacted: the
    // client chose the mode, not the directory, so the directory is not theirs to learn.
    let io = |mode: &'static str| {
        move |err: std::io::Error| {
            tracing::error!(mode, %err, "building the session vfs failed");
            SessionError::Io(format!(
                "the {mode} workspace could not be opened; the server log has the details"
            ))
        }
    };
    match &blueprint.vfs {
        VfsConfig::None => Ok(Vfs::none()),
        VfsConfig::Ephemeral { .. } => match ephemeral_root {
            Some(root) => Vfs::tempdir_in(root).map_err(io("ephemeral")),
            None => Vfs::tempdir().map_err(io("ephemeral")),
        },
        VfsConfig::PerSession { .. } => {
            let root = session_root
                .ok_or_else(|| SessionError::Io("per_session vfs root missing".into()))?;
            Vfs::external_with_mode(root.to_path_buf(), RtVfsMode::PerSession)
                .map_err(io("per_session"))
        }
        VfsConfig::Persistent { volume } => mount_volume(volume, volumes),
    }
}

/// Enforce the blueprint's `size_limit` on `vfs`. Attaching it walks the whole
/// directory, so the walk runs on the blocking pool rather than an async worker.
pub(crate) async fn attach_size_limit(
    vfs: Vfs,
    blueprint: &Blueprint,
) -> Result<Vfs, SessionError> {
    let Some(limit) = blueprint.vfs.size_limit() else {
        return Ok(vfs);
    };
    let mode = blueprint.vfs.mode_str();
    let (vfs, unmeasurable) = tokio::task::spawn_blocking(move || {
        let measured = vfs.measure_usage();
        let used = measured.as_ref().ok().copied();
        (vfs.with_measured_limit(limit, used), measured.err())
    })
    .await
    .map_err(|err| SessionError::Io(format!("measuring the {mode} workspace failed: {err}")))?;
    if let Some(err) = unmeasurable {
        // The session still opens, treated as full; the program is told so when
        // it tries to write.
        tracing::warn!(mode, %err, "the workspace could not be measured against its size limit");
    }
    Ok(vfs)
}

/// Mount a declared volume. Both failures are reported to the client by volume
/// name only: the host directory is the operator's business, and the mount
/// error carries it verbatim, so it is logged rather than returned.
fn mount_volume(volume: &str, volumes: &VolumeTable) -> Result<Vfs, SessionError> {
    let Some(target) = volumes.get(volume) else {
        tracing::warn!(
            volume,
            "blueprint names a volume this server does not declare"
        );
        return Err(SessionError::UnknownVolume(volume.to_string()));
    };
    Vfs::external_with_mode(target.clone(), RtVfsMode::Persistent).map_err(|err| {
        tracing::error!(
            volume,
            path = %target.display(),
            %err,
            "mounting declared volume failed"
        );
        SessionError::VolumeUnavailable(volume.to_string())
    })
}

pub(crate) fn vfs_info(blueprint: &Blueprint) -> VfsInfo {
    VfsInfo {
        mode: match &blueprint.vfs {
            VfsConfig::None => RtVfsMode::None,
            VfsConfig::Ephemeral { .. } => RtVfsMode::Ephemeral,
            VfsConfig::PerSession { .. } => RtVfsMode::PerSession,
            VfsConfig::Persistent { .. } => RtVfsMode::Persistent,
        },
        size_limit: blueprint.vfs.size_limit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    use crate::idempotency_store::{
        FileIdempotencyStore, InMemoryIdempotencyStore, LedgerEntry, code_fingerprint,
    };
    use crate::session_store::{FileDurableSessionStore, InMemoryDurableSessionStore};

    const TINY: Duration = Duration::from_millis(1);
    const HOUR: Duration = Duration::from_secs(3600);

    /// A `per_session` mount failure is the one path that still reached the client with
    /// the server's own storage layout in it — the volume paths were redacted, this one
    /// was not, and the plan's bar is that no response body carries a host directory.
    #[test]
    fn a_failed_session_mount_withholds_the_host_directory() {
        let outer = tempfile::tempdir().expect("tempdir");
        let root = outer.path().join("session-root");
        std::fs::write(&root, b"not a directory").expect("plant a file where a dir belongs");

        let blueprint = Blueprint {
            name: "bp".into(),
            vfs: VfsConfig::PerSession { size_limit: None },
            ..Default::default()
        };
        let err = build_vfs(&blueprint, Some(&root), None, &VolumeTable::new())
            .expect_err("mounting a file as a session root must fail");

        let msg = err.to_string();
        assert!(
            !msg.contains(root.to_str().expect("utf-8 path")),
            "the host directory must not reach the client: {msg}",
        );
        assert!(
            msg.contains("per_session"),
            "the client still learns which workspace failed: {msg}",
        );
    }

    /// A client factory these tests never invoke (none request an http client).
    fn no_http() -> HttpClientFactory {
        Arc::new(|| unreachable!("session_manager tests never request an http client"))
    }

    fn mem_store() -> Arc<dyn DurableSessionStore> {
        Arc::new(InMemoryDurableSessionStore::default())
    }

    fn file_store(dir: &std::path::Path) -> Arc<dyn DurableSessionStore> {
        Arc::new(FileDurableSessionStore::new(dir.to_path_buf()).expect("file store"))
    }

    fn mem_ledger() -> Arc<dyn IdempotencyStore> {
        Arc::new(InMemoryIdempotencyStore::default())
    }

    fn file_ledger(dir: &std::path::Path) -> Arc<dyn IdempotencyStore> {
        Arc::new(FileIdempotencyStore::new(dir.to_path_buf()).expect("file ledger"))
    }

    fn reserved(session_id: &str, key: &str) -> LedgerEntry {
        LedgerEntry::reserved(session_id, key, code_fingerprint("main"))
    }

    fn manager() -> (SessionManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        (
            SessionManager::new(
                dir.path().to_path_buf(),
                None,
                Arc::default(),
                no_http(),
                mem_store(),
                mem_ledger(),
                CapabilitySettings::default(),
            ),
            dir,
        )
    }

    /// A manager plus a handle on the ledger it purges, for the cleanup tests.
    fn manager_with_ledger() -> (SessionManager, tempfile::TempDir, Arc<dyn IdempotencyStore>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let ledger = mem_ledger();
        (
            SessionManager::new(
                dir.path().to_path_buf(),
                None,
                Arc::default(),
                no_http(),
                mem_store(),
                Arc::clone(&ledger),
                CapabilitySettings::default(),
            ),
            dir,
            ledger,
        )
    }

    fn per_session(idle: Duration) -> Blueprint {
        Blueprint {
            name: "p".into(),
            idle_timeout: idle,
            vfs: VfsConfig::PerSession { size_limit: None },
            ..Default::default()
        }
    }

    fn none_bp() -> Blueprint {
        Blueprint {
            name: "n".into(),
            vfs: VfsConfig::None,
            ..Default::default()
        }
    }

    /// A `SystemTime` far enough past now that any tiny window has elapsed.
    fn way_later() -> SystemTime {
        SystemTime::now() + HOUR
    }

    fn no_secrets() -> Arc<HarnessSecretBindings> {
        Arc::new(HarnessSecretBindings::new())
    }

    #[tokio::test]
    async fn create_allocates_per_session_dir() {
        let (mgr, root) = manager();
        let id = mgr
            .create(
                &per_session(HOUR),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        assert!(root.path().join(&id).is_dir());
    }

    #[tokio::test]
    async fn none_mode_allocates_no_dir() {
        let (mgr, root) = manager();
        let id = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        assert!(!root.path().join(&id).exists());
    }

    #[tokio::test]
    async fn ensure_is_idempotent() {
        let (mgr, _root) = manager();
        let bp = per_session(HOUR);
        mgr.ensure("sid", &bp).await.unwrap();
        mgr.ensure("sid", &bp).await.unwrap();
        assert!(mgr.contains("sid"));
    }

    #[tokio::test]
    async fn vfs_for_execute_uses_session_dir() {
        let (mgr, root) = manager();
        let bp = per_session(HOUR);
        let id = mgr
            .create(&bp, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        let (vfs, info) = mgr.vfs_for_execute(&id, &bp).await.unwrap();
        assert_eq!(vfs.mode(), RtVfsMode::PerSession);
        assert_eq!(vfs.root(), root.path().join(&id));
        assert_eq!(info.mode, RtVfsMode::PerSession);
    }

    #[tokio::test]
    async fn ephemeral_uses_configured_root() {
        let session_dir = tempfile::tempdir().expect("session dir");
        let eph_dir = tempfile::tempdir().expect("ephemeral dir");
        let mgr = SessionManager::new(
            session_dir.path().to_path_buf(),
            Some(eph_dir.path().to_path_buf()),
            Arc::default(),
            no_http(),
            mem_store(),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        let bp = Blueprint {
            name: "e".into(),
            vfs: VfsConfig::Ephemeral { size_limit: None },
            ..Default::default()
        };
        mgr.ensure("sid", &bp).await.unwrap();
        let (vfs, _) = mgr.vfs_for_execute("sid", &bp).await.unwrap();
        assert!(
            vfs.root().starts_with(eph_dir.path()),
            "ephemeral scratch should live under the configured root"
        );
    }

    #[tokio::test]
    async fn per_session_dir_survives_a_restart() {
        let dir = tempfile::tempdir().expect("durable session root");
        let bp = per_session(HOUR);

        // First boot: write a file into the session's VFS.
        let mgr = SessionManager::new(
            dir.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            mem_store(),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        mgr.ensure("sid", &bp).await.unwrap();
        let (v1, _) = mgr.vfs_for_execute("sid", &bp).await.unwrap();
        std::fs::write(v1.root().join("a.txt"), b"hi").unwrap();

        // Restart: a fresh manager over the same durable root, in-memory state
        // gone. Reconnecting with the same session id finds the files intact.
        let restarted = SessionManager::new(
            dir.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            mem_store(),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        restarted.ensure("sid", &bp).await.unwrap();
        let (v2, _) = restarted.vfs_for_execute("sid", &bp).await.unwrap();
        assert_eq!(std::fs::read(v2.root().join("a.txt")).unwrap(), b"hi");
    }

    #[tokio::test]
    async fn boot_rehydrates_session_for_resume() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        let bp = per_session(HOUR);

        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            store.clone(),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        mgr.ensure("sid", &bp).await.unwrap();

        // Restart: fresh manager + empty cache, same store + root.
        let restarted = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            store,
            mem_ledger(),
            CapabilitySettings::default(),
        );
        assert!(!restarted.contains("sid"));
        restarted.boot().await;
        assert!(restarted.contains("sid"), "boot must rehydrate the session");
        // Resume works without a fresh `ensure`, and the binding is intact.
        restarted.vfs_for_execute("sid", &bp).await.unwrap();
    }

    #[tokio::test]
    async fn boot_reaps_session_idle_past_persisted_timeout() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        // A record whose last activity is ancient — already past its idle window.
        store
            .put(SessionRecord {
                session_id: "old".into(),
                blueprint_name: "p".into(),
                idle_timeout: TINY,
                last_activity: UNIX_EPOCH,
                owns_vfs_dir: true,
                mcp_state: None,
                variables: Default::default(),
            })
            .await
            .unwrap();
        let dir = root.path().join("old");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            store,
            mem_ledger(),
            CapabilitySettings::default(),
        );
        mgr.boot().await;
        assert!(mgr.contains("old"));
        assert_eq!(mgr.reap_now().await, 1);
        assert!(!dir.exists(), "an idle rehydrated session is reaped");
    }

    #[tokio::test]
    async fn restart_persists_variables_but_drops_harness_secrets() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let bp = per_session(HOUR);
        let vars = Arc::new(VarBindings::from([(
            "tenant".to_string(),
            "u_42".to_string(),
        )]));

        {
            let mgr = SessionManager::new(
                root.path().to_path_buf(),
                None,
                Arc::default(),
                no_http(),
                file_store(store_dir.path()),
                mem_ledger(),
                CapabilitySettings::default(),
            );
            let bound = Arc::new(HarnessSecretBindings::from([(
                "TOKEN".to_string(),
                "session-only".to_string(),
            )]));
            mgr.bind("s1", &bp, Arc::clone(&vars), bound).await.unwrap();
            assert_eq!(
                mgr.variables("s1").get("tenant").map(String::as_str),
                Some("u_42")
            );
            assert_eq!(
                mgr.harness_secrets("s1")
                    .and_then(|values| values.get("TOKEN").cloned())
                    .as_deref(),
                Some("session-only")
            );
        }

        // A fresh manager over the same store + root: boot rehydrates the entry,
        // and its variable bindings come back with it.
        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            file_store(store_dir.path()),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        mgr.boot().await;
        assert_eq!(
            mgr.variables("s1").get("tenant").map(String::as_str),
            Some("u_42")
        );
        assert!(mgr.harness_secrets("s1").is_none());
    }

    #[tokio::test]
    async fn boot_sweeps_orphan_dir_with_no_record() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        // A leftover per_session dir whose session was never persisted.
        let orphan = root.path().join("ghost");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("a.txt"), b"x").unwrap();

        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            file_store(store_dir.path()),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        mgr.boot().await;
        assert!(
            !orphan.exists(),
            "boot must sweep a dir with no live session"
        );
    }

    #[tokio::test]
    async fn boot_drops_record_whose_dir_is_gone() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        // A persisted per_session record, but its directory never existed.
        store
            .put(SessionRecord {
                session_id: "vanished".into(),
                blueprint_name: "p".into(),
                idle_timeout: HOUR,
                last_activity: SystemTime::now(),
                owns_vfs_dir: true,
                mcp_state: None,
                variables: Default::default(),
            })
            .await
            .unwrap();

        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            store.clone(),
            mem_ledger(),
            CapabilitySettings::default(),
        );
        mgr.boot().await;
        assert!(!mgr.contains("vanished"), "a record with no dir is dropped");
        assert!(
            store.load_all().await.is_empty(),
            "the stale record is purged from the store too"
        );
    }

    #[tokio::test]
    async fn vfs_for_execute_unknown_session_errs() {
        let (mgr, _root) = manager();
        let err = mgr.vfs_for_execute("missing", &per_session(HOUR)).await;
        assert!(matches!(err, Err(SessionError::UnknownSession)));
    }

    #[tokio::test]
    async fn wipe_now_removes_dir() {
        let (mgr, root) = manager();
        let id = mgr
            .create(
                &per_session(HOUR),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        let dir = root.path().join(&id);
        std::fs::write(dir.join("a.txt"), b"hi").unwrap();
        assert!(mgr.wipe_now(&id).await);
        assert!(
            !dir.exists(),
            "explicit terminate must wipe the session dir"
        );
        assert!(!mgr.contains(&id));
    }

    #[tokio::test]
    async fn wipe_now_purges_the_session_ledger() {
        let (mgr, _root, ledger) = manager_with_ledger();
        let id = mgr
            .create(
                &per_session(HOUR),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        ledger.put(reserved(&id, "k")).await.unwrap();

        assert!(mgr.wipe_now(&id).await);
        assert_eq!(ledger.load(&id, "k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn reap_purges_the_session_ledger() {
        let (mgr, _root, ledger) = manager_with_ledger();
        let id = mgr
            .create(
                &per_session(TINY),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        ledger.put(reserved(&id, "k")).await.unwrap();

        assert_eq!(mgr.reap(way_later()).await, 1);
        assert_eq!(ledger.load(&id, "k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn wipe_blueprint_purges_every_bound_session_ledger() {
        let (mgr, _root, ledger) = manager_with_ledger();
        let bp = per_session(HOUR);
        let mut ids = Vec::new();
        for _ in 0..2 {
            let id = mgr
                .create(&bp, Arc::new(VarBindings::new()), no_secrets())
                .await
                .unwrap();
            ledger.put(reserved(&id, "k")).await.unwrap();
            ids.push(id);
        }
        // A session on a different blueprint must keep its entries.
        mgr.ensure("other", &none_bp()).await.unwrap();
        ledger.put(reserved("other", "k")).await.unwrap();

        mgr.wipe_blueprint(&bp.name).await;

        for id in &ids {
            assert_eq!(ledger.load(id, "k").await.unwrap(), None);
        }
        assert!(ledger.load("other", "k").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn boot_purges_the_ledger_of_a_record_whose_dir_is_gone() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        let ledger = mem_ledger();
        store
            .put(SessionRecord {
                session_id: "vanished".into(),
                blueprint_name: "p".into(),
                idle_timeout: HOUR,
                last_activity: SystemTime::now(),
                owns_vfs_dir: true,
                mcp_state: None,
                variables: Default::default(),
            })
            .await
            .unwrap();
        ledger.put(reserved("vanished", "k")).await.unwrap();

        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            store,
            Arc::clone(&ledger),
            CapabilitySettings::default(),
        );
        mgr.boot().await;

        assert_eq!(ledger.load("vanished", "k").await.unwrap(), None);
    }

    /// The escape path `forget` cannot cover: a file-backed ledger paired with
    /// the default in-memory session store. Nothing survives the restart to
    /// drive removal, so boot's own sweep is the only thing that reclaims it.
    #[tokio::test]
    async fn boot_purges_a_ledger_whose_session_never_came_back() {
        let ledger_dir = tempfile::tempdir().expect("ledger dir");
        let root = tempfile::tempdir().expect("session root");
        let ledger = file_ledger(ledger_dir.path());
        ledger.put(reserved("gone", "k")).await.unwrap();

        // Fresh manager, empty session store — "gone" has no record to rehydrate.
        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            mem_store(),
            Arc::clone(&ledger),
            CapabilitySettings::default(),
        );
        mgr.boot().await;

        assert_eq!(ledger.load("gone", "k").await.unwrap(), None);
        assert!(ledger.session_ids().await.is_empty());
    }

    #[tokio::test]
    async fn boot_keeps_the_ledger_of_a_session_that_resumed() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        let ledger = mem_ledger();
        let bp = per_session(HOUR);

        let mgr = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            Arc::clone(&store),
            Arc::clone(&ledger),
            CapabilitySettings::default(),
        );
        mgr.ensure("sid", &bp).await.unwrap();
        ledger.put(reserved("sid", "k")).await.unwrap();

        let restarted = SessionManager::new(
            root.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            store,
            Arc::clone(&ledger),
            CapabilitySettings::default(),
        );
        restarted.boot().await;

        assert!(
            ledger.load("sid", "k").await.unwrap().is_some(),
            "a resumed session keeps its ledger"
        );
    }

    #[tokio::test]
    async fn wipe_now_unknown_is_false() {
        let (mgr, _root) = manager();
        assert!(!mgr.wipe_now("nope").await);
    }

    #[tokio::test]
    async fn reap_wipes_idle_session() {
        let (mgr, root) = manager();
        let id = mgr
            .create(
                &per_session(TINY),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        assert_eq!(mgr.reap(way_later()).await, 1);
        assert!(!root.path().join(&id).exists());
    }

    #[tokio::test]
    async fn reap_keeps_active_session() {
        let (mgr, root) = manager();
        let id = mgr
            .create(
                &per_session(HOUR),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        assert_eq!(mgr.reap(SystemTime::now()).await, 0);
        assert!(root.path().join(&id).is_dir());
    }

    #[tokio::test]
    async fn persistent_creates_no_dir_and_survives_reap() {
        let (mgr, root) = manager();
        let bp = Blueprint {
            name: "x".into(),
            vfs: VfsConfig::Persistent {
                volume: "workspace".into(),
            },
            ..Default::default()
        };
        let id = mgr
            .create(&bp, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        // No scratch dir is allocated for persistent mode.
        assert!(!root.path().join(&id).exists());
        mgr.reap(way_later()).await;
        // The operator's directory is untouched.
        assert!(root.path().is_dir());
    }

    fn key(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// One store per session, cached — so every execute in a session, by either
    /// route, reads and writes the same entries.
    #[tokio::test]
    async fn one_store_per_session_shared_across_calls() {
        let (mgr, _root) = manager();
        let id = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();

        mgr.session_kv_for_execute(&id)
            .set(&key("k"), &key("\"v\""))
            .expect("set");
        assert!(
            mgr.session_kv_for_execute(&id).has(&key("k")).expect("has"),
            "a later execute in the same session must see the write"
        );

        let other = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        assert!(
            !mgr.session_kv_for_execute(&other)
                .has(&key("k"))
                .expect("has"),
            "a different session must not see it"
        );
    }

    /// The one-shot execute route mints a transient session id that was never
    /// registered; each such call gets its own store, discarded at return.
    #[tokio::test]
    async fn an_unregistered_session_gets_a_throwaway_store() {
        let (mgr, _root) = manager();
        mgr.session_kv_for_execute("one-shot")
            .set(&key("k"), &key("\"v\""))
            .expect("set");
        assert!(
            !mgr.session_kv_for_execute("one-shot")
                .has(&key("k"))
                .expect("has"),
            "an unregistered id must not accumulate state across calls"
        );
    }

    #[tokio::test]
    async fn idle_expiry_drops_the_session_store() {
        let (mgr, _root) = manager();
        let bp = Blueprint {
            name: "n".into(),
            idle_timeout: TINY,
            vfs: VfsConfig::None,
            ..Default::default()
        };
        let id = mgr
            .create(&bp, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        mgr.session_kv_for_execute(&id)
            .set(&key("k"), &key("\"v\""))
            .expect("set");

        assert_eq!(mgr.reap(way_later()).await, 1);
        assert!(
            !mgr.session_kv_for_execute(&id).has(&key("k")).expect("has"),
            "a reaped session's state must not survive its entry"
        );
    }

    /// A handle a run is still holding keeps answering — an execution never sees
    /// its store vanish mid-call — but the session id can no longer reach it, so
    /// the state is unreachable rather than resurrectable.
    #[tokio::test]
    async fn a_handle_outliving_its_session_cannot_be_reached_again() {
        let (mgr, _root) = manager();
        let id = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        let held = mgr.session_kv_for_execute(&id);
        held.set(&key("k"), &key("\"v\"")).expect("set");

        assert!(mgr.wipe_now(&id).await);

        assert!(held.has(&key("k")).expect("has"), "the in-flight handle");
        assert!(
            !mgr.session_kv_for_execute(&id).has(&key("k")).expect("has"),
            "the session id resolves to a fresh, empty store"
        );
    }

    /// Every execution's budget reserves against *one* aggregate, so two live
    /// executions cannot both claim the same headroom.
    ///
    /// Asserted on the manager rather than through a server, because this is the
    /// property the `Clone` on [`LlmSettings`] exists for: a `build()` that
    /// minted a fresh `SharedTokenBudget` would pass every single-execution test
    /// and fail only here.
    #[test]
    fn every_execution_budget_shares_one_aggregate() {
        let (mgr, _dir) = manager();
        let first = mgr.llm_budget_for_execute();
        let second = mgr.llm_budget_for_execute();

        first.reserve("m", 1_000).expect("the first fits");
        assert_eq!(
            mgr.llm_budget().used(),
            1_000,
            "the first reservation must land on the server-wide budget"
        );

        second.reserve("m", 1_000).expect("the second fits");
        assert_eq!(
            mgr.llm_budget().used(),
            2_000,
            "a second execution must reserve against the same aggregate, not its own copy"
        );

        // Dropping one returns only its own share.
        drop(first);
        assert_eq!(
            mgr.llm_budget().used(),
            1_000,
            "dropping one execution must release its reservation and no one else's"
        );
    }

    /// Zero would deadlock the provider's fan-out semaphore, so it clamps to one
    /// here as well as at the provider — an operator who writes 0 gets serial
    /// dispatch, not a hang.
    #[test]
    fn a_zero_concurrency_bound_clamps_to_one() {
        let settings = LlmSettings::new(LlmLimits::default(), 1_000, 0);
        assert_eq!(settings.max_concurrency, 1);
    }

    /// The aggregate budget is reserved atomically across sessions and released
    /// when a session ends — not by evicting another session's entries.
    #[tokio::test]
    async fn the_aggregate_budget_is_shared_and_released_on_teardown() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Room for exactly one entry of the size written below.
        let settings = SessionKvSettings::new(SessionKvLimits::default(), 128);
        let budget = settings.budget.clone();
        let mgr = SessionManager::new(
            dir.path().to_path_buf(),
            None,
            Arc::default(),
            no_http(),
            mem_store(),
            mem_ledger(),
            CapabilitySettings {
                session_kv: settings,
                ..CapabilitySettings::default()
            },
        );

        let first = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        let second = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();

        mgr.session_kv_for_execute(&first)
            .set(&key("k"), &key("\"v\""))
            .expect("the first session fits");
        assert!(budget.used() > 0);
        mgr.session_kv_for_execute(&second)
            .set(&key("k"), &key("\"v\""))
            .expect_err("the second exceeds the shared budget");
        assert!(
            mgr.session_kv_for_execute(&first)
                .has(&key("k"))
                .expect("has"),
            "an exhausted budget must never evict another session's entries"
        );

        assert!(mgr.wipe_now(&first).await);
        assert_eq!(
            budget.used(),
            0,
            "ending a session must release its reservation"
        );
        mgr.session_kv_for_execute(&second)
            .set(&key("k"), &key("\"v\""))
            .expect("capacity released");
    }
}
