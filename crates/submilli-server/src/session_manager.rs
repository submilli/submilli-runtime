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
//! Session metadata is authoritative in the store. Memory holds only live resource
//! handles; reads and writes consult committed records.
//!
//! Only `per_session` owns a directory the manager wipes. `ephemeral` dirs are
//! per-execute (owned by the runner); a named volume — a `named` root or a
//! `vfs.mounts` entry — is one the operator declared in the server config, and
//! is never wiped or reaped here: [`build_vfs`] only resolves the blueprint's
//! volume names through the [`VolumeRegistry`] and mounts what it finds.
//!
//! [`wipe_now`]: SessionManager::wipe_now
//! [`boot`]: SessionManager::boot

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use interpreter::runtime::{
    Access as RtAccess, CursorKey, CursorKeys, EmbeddingLimits, EmbeddingTokenBudget,
    ExecutionTokenBudget, HttpClient, LlmLimits, MountError, MountSpec, SessionKvLimits,
    SessionKvStore, SharedTokenBudget, Vfs, VfsInfo, VfsMode as RtVfsMode,
};
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings, VfsConfig};
use uuid::Uuid;

mod resources;

use crate::adapters::session::audit_log::SessionAuditLog;
use crate::adapters::session::cleanup_queue::StoredSessionCleanupQueue;
use crate::adapters::session::credentials::{CredentialCodec, CredentialError};
use crate::adapters::session::workspaces::LocalSessionWorkspaces;
use crate::application::sessions::ports::SessionWorkspaces;
use crate::domain::idempotent_request::ResponseRetention;

use crate::adapters::session::repository::{LoadedSession, SessionPersistence};
use crate::blueprint::StoreError;
use crate::config::{VolumeRoot, VolumeTable};
use crate::domain::session::{RootVfs, Session, SessionBinding, SessionId};
use crate::domain::tools::SessionTools;
use crate::runs::RunRegistry;
use crate::session_store::{ClosedReason, DurableSessionStore, SessionStatus};
use crate::volumes::VolumeRegistry;
use submilli_shared::secret_store::SecretCipher;

/// Builds a fresh per-session HTTP client (its own connection pool). Injected so
/// the manager owns each session's client lifecycle without depending on the
/// concrete reqwest type.
pub type HttpClientFactory = Arc<dyn Fn() -> Arc<dyn HttpClient> + Send + Sync>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReaperStartError {
    #[error("reaper interval must be nonzero")]
    ZeroInterval,
    #[error("reaper interval exceeds the representable timer deadline")]
    DeadlineOverflow,
}

/// Server-wide ceiling on `submilli:llm` tokens across every live execution.
/// Re-exported from the runtime rather than chosen again here, so the number an
/// operator raises is the number the refusal message names.
pub use interpreter::runtime::DEFAULT_MAX_ALL_EXECUTIONS_TOKENS;

/// Elements one `batch` dispatches at once. Re-exported from the provider that
/// enforces it, so the bound the operator configures and the bound the semaphore
/// takes cannot drift apart.
pub use submilli_shared::llm::provider::DEFAULT_MAX_CONCURRENCY;

/// Server-wide ceiling on `submilli:embedding` tokens across every live
/// execution. Separate from the LLM ceiling: an embedding index build must not
/// be able to starve `llm.call`, nor the reverse.
pub const DEFAULT_MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS: u64 = 50_000_000;

/// Sub-batches one embedding call sends at once.
pub use submilli_shared::embedding::DEFAULT_MAX_CONCURRENCY as DEFAULT_MAX_EMBEDDING_CONCURRENCY;

pub use crate::adapters::session::last_run::DEFAULT_MAX_LAST_RUN_BYTES;
use crate::adapters::session::last_run::LastRuns;
pub use crate::application::sessions::error::{BootError, SessionError};

impl From<CredentialError> for SessionError {
    fn from(error: CredentialError) -> Self {
        Self::Secrets(error.to_string())
    }
}

pub struct SessionManager {
    finalizers: crate::adapters::session::finalizers::StoredFinalizers,
    finalizer_executor:
        std::sync::OnceLock<Arc<dyn crate::application::sessions::finalization::FinalizerExecutor>>,
    finalization_lock: tokio::sync::Mutex<()>,
    /// Poisoning can leave execution scopes partly installed; never recover that map.
    resource_scopes:
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<ResourceScope>>>,
    audit: Option<crate::audit::AuditLog>,
    resources: resources::SessionResources,
    reaper_started: AtomicBool,
    credentials: CredentialCodec,
    unit_of_work: Arc<dyn crate::application::unit_of_work::UnitOfWorkFactory>,
    /// Durable root for `per_session` directories (keyed by session id): the
    /// volume root's `sessions/`.
    session_root: PathBuf,
    /// Where `ephemeral` scratch dirs go: the volume root's `ephemeral/`.
    ephemeral_root: EphemeralRoot,
    /// Operator-declared named volumes every blueprint resolves through.
    /// Passed in rather than looked up globally, so every mount path takes the
    /// same table and shares the same size limits.
    volumes: Arc<VolumeRegistry>,
    store: Arc<dyn DurableSessionStore>,
    database: Arc<crate::database::ServerDatabase>,
    repository: SessionPersistence,
    session_kv: SessionKvSettings,
    cursor_keys: Option<Arc<CursorKeys>>,
    llm: LlmSettings,
    embedding: EmbeddingSettings,
    /// Runs that may suspend at an application tool call. A session with one
    /// unfinished is not idle, and ending the session ends them.
    runs: Arc<RunRegistry>,
    /// How long keyed executes keep their responses for replay.
    response_retention: ResponseRetention,
    /// Each session's last run, which ends with it.
    last_runs: Arc<LastRuns>,
}

/// How long ending a session waits for its cancelled runs to stop before it
/// removes their files anyway.
const RUN_DRAIN: Duration = Duration::from_secs(10);

/// How a session's `submilli:session` store is bounded. The state itself is in
/// the database, so every limit is per session. Kept a struct beside
/// [`LlmSettings`] and [`EmbeddingSettings`], which [`CapabilitySettings`]
/// groups the same way.
#[derive(Clone, Default)]
pub struct SessionKvSettings {
    pub limits: SessionKvLimits,
}

impl SessionKvSettings {
    pub fn new(limits: SessionKvLimits) -> Self {
        Self { limits }
    }
}

/// Cursor keys derived from the server's key. Replacing that key changes their
/// id, so a cursor sealed before names a key the server no longer holds.
fn derive_cursor_keys(cipher: &SecretCipher) -> Arc<CursorKeys> {
    let (id, secret) = cipher.derive(b"submilli:session-list-cursor:v1");
    Arc::new(CursorKeys::new(CursorKey::new(id, secret), Vec::new()))
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

/// How an execution's `submilli:embedding` budget is built: the [`LlmSettings`]
/// shape with its own server-wide ledger, so exhausting one capability leaves
/// the other untouched.
#[derive(Clone)]
pub struct EmbeddingSettings {
    pub limits: EmbeddingLimits,
    /// Server-wide ceiling summed across every live execution.
    pub budget: SharedTokenBudget,
    /// Sub-batches one call sends at once.
    pub max_concurrency: usize,
}

impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self::new(
            EmbeddingLimits::default(),
            DEFAULT_MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS,
            DEFAULT_MAX_EMBEDDING_CONCURRENCY,
        )
    }
}

impl EmbeddingSettings {
    pub fn new(limits: EmbeddingLimits, total_tokens: u64, max_concurrency: usize) -> Self {
        Self {
            limits,
            budget: SharedTokenBudget::new(total_tokens),
            // Zero would deadlock the provider's semaphore.
            max_concurrency: max_concurrency.max(1),
        }
    }

    /// A fresh per-execution budget sharing this server's aggregate ceiling.
    fn build(&self) -> Arc<EmbeddingTokenBudget> {
        Arc::new(EmbeddingTokenBudget::new(self.limits, self.budget.clone()))
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
    pub embedding: EmbeddingSettings,
}

impl SessionManager {
    /// A manager whose `per_session` and `ephemeral` directories live under
    /// `volume_root`, which [`VolumeRoot::prepare`] has made ready.
    pub fn new(
        volume_root: &VolumeRoot,
        volumes: Arc<VolumeRegistry>,
        http_client_factory: HttpClientFactory,
        store: Arc<crate::session_store::SqliteSessionStore>,
        capabilities: CapabilitySettings,
    ) -> Self {
        let database = store.database_owner();
        let CapabilitySettings {
            session_kv,
            llm,
            embedding,
        } = capabilities;
        let session_root = volume_root.sessions_dir();
        let cursor_keys = database
            .ephemeral_cipher()
            .as_deref()
            .map(derive_cursor_keys);
        Self {
            finalizers: crate::adapters::session::finalizers::StoredFinalizers {
                database: database.clone(),
                sealer: crate::adapters::sealing::Sealer::new(database.ephemeral_cipher()),
            },
            finalizer_executor: std::sync::OnceLock::new(),
            finalization_lock: tokio::sync::Mutex::new(()),
            resource_scopes: std::sync::Mutex::new(std::collections::HashMap::new()),
            audit: None,
            credentials: CredentialCodec::new(database.ephemeral_cipher()),
            unit_of_work: crate::adapters::unit_of_work::for_sessions(
                database.clone(),
                session_root.clone(),
                None,
            ),
            resources: resources::SessionResources::new(http_client_factory),
            reaper_started: AtomicBool::new(false),
            repository: SessionPersistence::new(store.clone(), session_root.clone()),
            session_root,
            ephemeral_root: EphemeralRoot::in_root(volume_root),
            volumes,
            store,
            session_kv,
            cursor_keys,
            llm,
            embedding,
            runs: Arc::new(RunRegistry::default()),
            response_retention: ResponseRetention::default(),
            last_runs: Arc::new(LastRuns::new(
                database.clone(),
                database.ephemeral_cipher(),
                DEFAULT_MAX_LAST_RUN_BYTES,
            )),
            database,
        }
    }

    /// Keep a session's last run only up to `max_bytes` of JSON; a larger one
    /// keeps only its size.
    pub fn with_max_last_run_size(mut self, max_bytes: u64) -> Self {
        self.last_runs = Arc::new(self.last_runs.with_max_bytes(max_bytes));
        self
    }

    /// Each session's last run.
    pub(crate) fn last_runs(&self) -> &Arc<LastRuns> {
        &self.last_runs
    }

    /// Hold at most `max` runs waiting on the harness at once.
    pub fn with_max_suspended_runs(mut self, max: usize) -> Self {
        self.runs = Arc::new(RunRegistry::new(max));
        self
    }

    /// Keep keyed executes' responses for replay as `retention` says.
    pub fn with_response_retention(mut self, retention: ResponseRetention) -> Self {
        self.response_retention = retention;
        self
    }

    /// The runs that may suspend at an application tool call.
    pub fn runs(&self) -> &Arc<RunRegistry> {
        &self.runs
    }

    pub(crate) fn with_audit(mut self, audit: crate::audit::AuditLog) -> Self {
        self.audit = Some(audit);
        self
    }

    /// The HTTP client for `session_id`, built once and cached on the session so
    /// every execute in that session shares one connection pool — and no other
    /// session does. The cache entry is created on first access.
    pub fn http_client(&self, session_id: &str) -> Arc<dyn HttpClient> {
        self.resources.http_client(session_id)
    }

    /// The `submilli:session` store for `session_id`. The state is in the
    /// database, so every execute in the session — REST or MCP, before a
    /// restart or after — reads and writes one set of entries, and no other
    /// session does. Closing the session deletes them.
    pub fn session_kv_for_execute(&self, session_id: &str) -> Arc<dyn SessionKvStore> {
        Arc::new(crate::adapters::session_kv_store::DatabaseSessionKv::new(
            self.unit_of_work.clone(),
            session_id,
            self.session_kv.limits,
        ))
    }

    /// The keys `session.list` cursors are sealed with: derived from the
    /// server's secret-store key, so a cursor handed out before a restart still
    /// works after it. `None` without a key, which leaves cursors to the
    /// runtime's per-process key.
    pub fn cursor_keys(&self) -> Option<Arc<CursorKeys>> {
        self.cursor_keys.clone()
    }

    /// A fresh `submilli:llm` token budget for one execution, sharing this
    /// server's aggregate ceiling.
    ///
    /// This is deliberately *not* cached on the session: the reservation releases on `Drop`, so a budget held by a
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

    /// A fresh `submilli:embedding` budget for one execution, sharing the
    /// server-wide embedding ceiling. Per-execution for the same reason as
    /// [`Self::llm_budget_for_execute`]: its reservation releases on `Drop`.
    pub fn embedding_budget_for_execute(&self) -> Arc<EmbeddingTokenBudget> {
        self.embedding.build()
    }

    /// The fan-out bound one embedding call sends sub-batches at.
    pub fn embedding_max_concurrency(&self) -> usize {
        self.embedding.max_concurrency
    }

    /// The server-wide embedding token budget, for operator-facing reporting.
    pub fn embedding_budget(&self) -> &SharedTokenBudget {
        &self.embedding.budget
    }

    /// The server-wide LLM token budget, for operator-facing reporting.
    pub fn llm_budget(&self) -> &SharedTokenBudget {
        &self.llm.budget
    }

    pub fn with_cipher(mut self, cipher: Option<Arc<SecretCipher>>) -> Self {
        let cipher = cipher.or_else(|| self.database.ephemeral_cipher());
        self.unit_of_work = crate::adapters::unit_of_work::for_sessions(
            self.database.clone(),
            self.session_root.clone(),
            cipher.clone(),
        );
        self.cursor_keys = cipher.as_deref().map(derive_cursor_keys);
        self.last_runs = Arc::new(LastRuns::new(
            self.database.clone(),
            cipher.clone(),
            self.last_runs.max_bytes(),
        ));
        self.finalizers.sealer = crate::adapters::sealing::Sealer::new(cipher.clone());
        self.credentials = CredentialCodec::new(cipher);
        self
    }

    pub async fn get(&self, id: &str) -> Result<Session, SessionError> {
        crate::application::sessions::queries::GetSession::new(self.unit_of_work.as_ref())
            .execute(id)
            .await
    }

    pub async fn execution_bindings(
        &self,
        id: &str,
    ) -> Result<(SessionBinding, Option<Arc<HarnessSecretBindings>>), SessionError> {
        self.available(id)
            .await?
            .execution_bindings(&self.credentials)
            .map_err(Into::into)
    }

    async fn available(&self, id: &str) -> Result<LoadedSession, SessionError> {
        let loaded = self
            .repository
            .find(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        let now = self.store.now().await?;
        let mut unit = self.unit_of_work.begin().await?;
        let session = unit
            .get_session(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        crate::application::sessions::execution::require_available(unit.as_mut(), &session, now)
            .await?;
        Ok(loaded)
    }

    pub async fn bind(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: Arc<VarBindings>,
        secrets: Arc<HarnessSecretBindings>,
    ) -> Result<(), SessionError> {
        self.bind_with_tools(id, blueprint, variables, secrets, None)
            .await
    }

    /// [`Self::bind`], declaring `tools` if this creates the session. A rebind
    /// keeps the catalog the session was created with.
    pub async fn bind_with_tools(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: Arc<VarBindings>,
        secrets: Arc<HarnessSecretBindings>,
        tools: Option<SessionTools>,
    ) -> Result<(), SessionError> {
        crate::application::sessions::create::CreateSession::new(
            self.unit_of_work.as_ref(),
            &self.workspaces(),
            &StoredSessionCleanupQueue(self.store.as_ref()),
            &SessionAuditLog(self.audit.as_ref()),
        )
        .execute(id, blueprint, variables, secrets, tools)
        .await
    }

    /// The application tools `id` was created with.
    pub async fn tools(&self, id: &str) -> Result<Option<SessionTools>, SessionError> {
        Ok(self.available(id).await?.session().tools().cloned())
    }

    pub async fn variables(&self, id: &str) -> Result<Arc<VarBindings>, SessionError> {
        Ok(Arc::new(self.get(id).await?.binding().variables().clone()))
    }

    pub async fn harness_secrets(
        &self,
        id: &str,
    ) -> Result<Option<Arc<HarnessSecretBindings>>, SessionError> {
        Ok(self.execution_bindings(id).await?.1)
    }

    pub async fn rebind_harness_secrets(
        &self,
        id: &str,
        bindings: Arc<HarnessSecretBindings>,
    ) -> Result<(), SessionError> {
        crate::application::sessions::replace_credentials::ReplaceCredentials::new(
            self.unit_of_work.as_ref(),
            &SessionAuditLog(self.audit.as_ref()),
        )
        .execute(id, bindings)
        .await
    }

    pub async fn start_execution(&self, id: &str) -> Result<(), SessionError> {
        crate::application::sessions::execution::StartExecution::new(self.unit_of_work.as_ref())
            .execute(id)
            .await
    }

    /// Where `ephemeral` scratch directories go.
    pub(crate) fn ephemeral_root(&self) -> &EphemeralRoot {
        &self.ephemeral_root
    }

    /// The declared volume table, for registration checks and listings.
    pub(crate) fn volumes(&self) -> VolumeTable {
        self.volumes.table()
    }

    /// The declared volumes. Every caller of [`build_vfs`] reads them from here,
    /// so no mount route can skip volume resolution.
    pub(crate) fn volume_registry(&self) -> &VolumeRegistry {
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
        self.create_with_tools(blueprint, variables, harness_secrets, None)
            .await
    }

    /// [`Self::create`] with the application tools the harness declared.
    pub async fn create_with_tools(
        &self,
        blueprint: &Blueprint,
        variables: Arc<VarBindings>,
        harness_secrets: Arc<HarnessSecretBindings>,
        tools: Option<SessionTools>,
    ) -> Result<String, SessionError> {
        let session_id = Uuid::new_v4().to_string();
        self.bind_with_tools(&session_id, blueprint, variables, harness_secrets, tools)
            .await?;
        Ok(session_id)
    }

    pub async fn blueprint_name(&self, id: &str) -> Result<Option<String>, SessionError> {
        match self.get(id).await {
            Ok(session) => Ok(Some(session.binding().blueprint().to_owned())),
            Err(SessionError::UnknownSession) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn ensure(&self, id: &str, blueprint: &Blueprint) -> Result<(), SessionError> {
        crate::application::sessions::create::EnsureSession::new(
            self.unit_of_work.as_ref(),
            &self.workspaces(),
            &StoredSessionCleanupQueue(self.store.as_ref()),
            &SessionAuditLog(self.audit.as_ref()),
        )
        .execute(id, blueprint)
        .await
    }

    pub async fn contains(&self, id: &str) -> Result<bool, SessionError> {
        self.blueprint_name(id).await.map(|name| name.is_some())
    }

    pub async fn active_count(&self) -> Result<usize, SessionError> {
        self.store.active_count().await.map_err(Into::into)
    }

    /// The session's VFS + info, without its size limit: enough for reading its
    /// files. The blueprint name is validated against the session to stop a
    /// session being driven under a different sandbox than it was created with.
    pub async fn session_vfs(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let variables = self.variables(session_id).await?;
        self.session_vfs_with_variables(session_id, blueprint, &variables)
            .await
    }

    pub(crate) async fn session_vfs_with_variables(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let mut loaded = self.available(session_id).await?;
        let binding = SessionBinding::new(blueprint.name.clone(), variables.clone())?;
        loaded.session().verify_binding(&binding)?;
        let root = self
            .workspaces()
            .resolve_root(session_id, blueprint, variables)?;
        let resolving_legacy = matches!(loaded.session().root(), RootVfs::LegacyUnresolved);
        loaded.resolve_root(root)?;
        let vfs = build_vfs(
            blueprint,
            variables,
            loaded.session().root().owned_path(),
            &self.ephemeral_root,
            &self.volumes,
        )?;
        if resolving_legacy {
            self.repository.save(loaded).await?;
        }
        Ok((vfs, vfs_info(blueprint)))
    }

    fn workspaces(&self) -> LocalSessionWorkspaces<'_> {
        LocalSessionWorkspaces {
            session_root: &self.session_root,
            ephemeral_root: &self.ephemeral_root,
            volumes: &self.volumes,
        }
    }

    /// [`session_vfs`](Self::session_vfs) with the blueprint's size limit
    /// enforced, for running a program that may write.
    pub async fn vfs_for_execute(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let variables = self.variables(session_id).await?;
        self.vfs_for_execute_with_variables(session_id, blueprint, &variables)
            .await
    }

    pub(crate) async fn vfs_for_execute_with_variables(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let (vfs, info) = self
            .session_vfs_with_variables(session_id, blueprint, variables)
            .await?;
        Ok((attach_limits(vfs, blueprint, &self.volumes).await?, info))
    }

    /// Reconstruct accepted storage without provisioning directories. This is
    /// also valid for a closed session while its owner is draining cleanup.
    pub(crate) async fn reopen_vfs(
        &self,
        session_id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let session = {
            let mut unit = self.unit_of_work.begin().await?;
            unit.get_session(session_id)
                .await?
                .ok_or(SessionError::UnknownSession)?
        };
        let vfs = open_vfs(
            blueprint,
            variables,
            session.root().owned_path(),
            &self.ephemeral_root,
            &self.volumes,
            WorkspaceOpen::Existing,
        )?;
        Ok((
            attach_limits(vfs, blueprint, &self.volumes).await?,
            vfs_info(blueprint),
        ))
    }

    pub async fn touch(&self, id: &str) -> Result<bool, SessionError> {
        crate::application::sessions::execution::CompleteExecution::new(self.unit_of_work.as_ref())
            .execute(id)
            .await
    }

    pub async fn wipe_now(&self, id: &str) -> Result<bool, SessionError> {
        let closed = crate::application::sessions::close::CloseSession::new(
            self.unit_of_work.as_ref(),
            &SessionAuditLog(self.audit.as_ref()),
        )
        .execute(id, ClosedReason::Deleted)
        .await?;
        self.cleanup(id).await?;
        Ok(closed)
    }

    async fn cleanup(&self, id: &str) -> Result<(), SessionError> {
        let _finalization = self.finalization_lock.lock().await;
        if let Some(record) = self.store.load(id).await?
            && (record.status == SessionStatus::Active || !record.cleanup_pending)
        {
            return Ok(());
        }
        // Its waiting runs stop before their files go and before their
        // idempotency entries are purged. One still going after the drain
        // writes no outcome back when it ends (`RunEntry::cancel_unrecorded`).
        let scope = self.resource_scope(id);
        scope.cancel.cancel();
        self.runs.end_session(id, RUN_DRAIN).await;
        // The runner holds its read permit until blocking host work has drained.
        let _drained = scope.work.write().await;
        if let Some(executor) = self.finalizer_executor.get() {
            crate::application::sessions::finalization::finalize_session(
                &self.finalizers,
                executor.as_ref(),
                id,
            )
            .await?;
        } else {
            if self.finalizers.has_unfinished(id).await? {
                return Err(SessionError::Io("finalizer executor is unavailable".into()));
            }
        }
        if self.finalizers.has_unfinished(id).await? {
            return Err(SessionError::Io(
                "finalization completion could not be recorded; recovery is required".into(),
            ));
        }
        if !self.retire_session_runs(id).await? {
            // The session is closed and fenced, but its roots are still needed
            // to retry retirement. Leave cleanup_pending set for the reaper.
            return Ok(());
        }
        self.resources.evict(id);
        self.last_runs.remove_session_files(id).await;
        let Some(task) = self.store.cleanup_task(id).await? else {
            return Ok(());
        };
        if let Some(folder) = task.folder {
            validate_session_id(id)?;
            let root = self.session_root.join(id);
            if folder == root {
                match std::fs::remove_dir_all(root) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(SessionError::Io(error.to_string())),
                }
            } else {
                // A folder recorded anywhere else was made under an earlier
                // layout (`server/vfs/sessions/` before the volume root) or an
                // earlier `volume_dir`. It is not this server's to delete, and
                // refusing would fail every boot, so it stays for the operator.
                tracing::warn!(
                    session_id = id,
                    folder = %folder.display(),
                    "session workspace is not this session's directory under the volume root; left in place"
                );
            }
        }
        let owned_id = id.to_owned();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    sqlx::query("DELETE FROM finalizer_context WHERE session_id=?")
                        .bind(owned_id)
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await
            .map_err(|error| SessionError::Io(error.to_string()))?;
        self.store.complete_cleanup(id).await?;
        Ok(())
    }

    /// Called only after the session's execution and finalizers have drained,
    /// before deleting its owned root (which may also contain mount points).
    async fn retire_session_runs(&self, session: &str) -> Result<bool, SessionError> {
        use crate::application::sessions::recovery::RunQuery;
        let mut after = None;
        let mut finished = true;
        loop {
            let page = {
                let mut unit = self.unit_of_work.begin().await?;
                unit.list_runs(RunQuery {
                    session: Some(session.into()),
                    after,
                    limit: 100,
                    recovery: true,
                    ..Default::default()
                })
                .await?
            };
            let more = page.len() == 100;
            after = page.last().map(|run| run.reservation_id().clone());
            for run in page {
                if let Err(error) =
                    crate::recovery::retire(self, self.unit_of_work.as_ref(), run.reservation_id())
                        .await
                {
                    // Keep every root until both disk retirement and its
                    // transaction finish; a transient failure must be retryable.
                    finished = false;
                    tracing::warn!(run = %run.reservation_id(), %error, "run cleanup remains pending");
                }
            }
            if !more {
                return Ok(finished);
            }
        }
    }

    /// Expire idle sessions and clean up after them, and expire the keyed-execute
    /// responses past their retention. Returns how many sessions expired.
    pub async fn reap(&self, now: SystemTime) -> Result<usize, SessionError> {
        let now = if self.store.durable() {
            self.store.now().await?
        } else {
            now
        };
        // The ledger's bound must not depend on session cleanup succeeding, so
        // the sweep runs first and reports its own failure.
        if let Err(error) = self.expire_responses(now).await {
            tracing::warn!(%error, "expiring idempotent responses failed");
        }
        // A session whose run waits on the harness is not idle.
        let busy = self.runs.sessions_with_unfinished_runs();
        let count = crate::application::sessions::expire::ExpireSessions::new(
            self.unit_of_work.as_ref(),
            &SessionAuditLog(self.audit.as_ref()),
        )
        .execute(now, &busy)
        .await?;
        self.process_pending_cleanup().await?;
        Ok(count)
    }

    /// Drop the keyed-execute responses past their retention, leaving tombstones
    /// that still refuse to run the request again. Returns how many expired.
    async fn expire_responses(&self, now: SystemTime) -> Result<u64, SessionError> {
        Ok(crate::application::sessions::runs::ExpireResponses::new(
            self.unit_of_work.as_ref(),
            self.response_retention,
        )
        .execute(now)
        .await?)
    }

    pub(crate) async fn process_pending_cleanup(&self) -> Result<(), SessionError> {
        self.resources
            .retain_active(&self.store.active_ids().await?);
        for task in self.store.pending_cleanup().await? {
            self.cleanup(&task.session_id).await?;
        }
        Ok(())
    }

    pub async fn reap_now(&self) -> Result<usize, SessionError> {
        self.reap(SystemTime::now()).await
    }

    pub async fn boot(&self) -> Result<(), BootError> {
        self.store.initialize().await.map_err(BootError::Sessions)?;
        self.finalizers
            .recover()
            .await
            .map_err(BootError::Sessions)?;
        self.store.load_all().await.map_err(BootError::Sessions)?;
        self.ephemeral_root
            .sweep()
            .map_err(|source| BootError::Scratch {
                path: self.ephemeral_root.path.clone(),
                source,
            })?;
        crate::application::sessions::recover::RecoverSessions::new(
            self.unit_of_work.as_ref(),
            &SessionAuditLog(self.audit.as_ref()),
            &self.workspaces(),
            &StoredSessionCleanupQueue(self.store.as_ref()),
        )
        .execute()
        .await?;
        self.process_pending_cleanup()
            .await
            .map_err(|error| BootError::Sessions(StoreError::Io(error.to_string())))?;
        Ok(())
    }

    pub(crate) async fn validate_stores(&self) -> Result<(), BootError> {
        self.store.initialize().await.map_err(BootError::Sessions)?;
        self.store.load_all().await.map_err(BootError::Sessions)?;
        Ok(())
    }

    /// Spawn a background reaper if a tokio runtime is available. The sweep
    /// interval is coarse; expiry is wall-clock, so a session is wiped within
    /// one interval of its deadline. Without a runtime this is a no-op. With a
    /// runtime, invalid intervals are rejected even if the reaper already runs.
    ///
    /// # Panics
    /// An entered runtime must have its time driver enabled (via `enable_time`
    /// or `enable_all`). Timer construction checks that contract before startup
    /// is published; the timer remains attached to that same runtime afterward.
    pub fn spawn_reaper(self: &Arc<Self>, interval: Duration) -> Result<(), ReaperStartError> {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return Ok(());
        };
        if interval.is_zero() {
            return Err(ReaperStartError::ZeroInterval);
        }
        let deadline = tokio::time::Instant::now();
        Self::next_reaper_deadline(deadline, interval)?;
        if self.reaper_started.load(Ordering::Acquire) {
            return Ok(());
        }
        // The caller supplies a timer-enabled runtime under the documented API
        // contract. No callbacks or runtime changes intervene before spawning.
        let timer = tokio::time::sleep_until(deadline);
        if self
            .reaper_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }
        let manager = Arc::downgrade(self);
        handle.spawn(Self::run_reaper(manager, interval, deadline, timer));
        Ok(())
    }

    async fn run_reaper(
        manager: std::sync::Weak<Self>,
        interval: Duration,
        mut deadline: tokio::time::Instant,
        timer: tokio::time::Sleep,
    ) {
        tokio::pin!(timer);
        loop {
            timer.as_mut().await;
            let Some(manager) = manager.upgrade() else {
                return;
            };
            if let Err(error) = manager.reap_now().await {
                tracing::warn!(%error, "session reaping failed");
            }
            // Advance from the scheduled tick, preserving burst catch-up without
            // Tokio Interval's unchecked addition on its missed-tick path.
            match Self::next_reaper_deadline(deadline, interval) {
                Ok(next) => deadline = next,
                Err(error) => {
                    tracing::error!(%error, "session reaper stopped");
                    manager.reaper_started.store(false, Ordering::Release);
                    return;
                }
            }
            timer.as_mut().reset(deadline);
        }
    }

    fn next_reaper_deadline(
        deadline: tokio::time::Instant,
        interval: Duration,
    ) -> Result<tokio::time::Instant, ReaperStartError> {
        let next = deadline
            .checked_add(interval)
            .ok_or(ReaperStartError::DeadlineOverflow)?;
        // Tokio rounds registration deadlines up by 999,999 ns. Keep one
        // millisecond of headroom so Sleep::reset cannot overflow internally.
        next.checked_add(Duration::from_millis(1))
            .ok_or(ReaperStartError::DeadlineOverflow)?;
        Ok(next)
    }
}

fn validate_session_id(id: &str) -> Result<(), SessionError> {
    SessionId::parse(id.to_owned())
        .map(|_| ())
        .map_err(Into::into)
}

/// Where `ephemeral` scratch directories are allocated: the volume root's
/// `ephemeral/`, created at startup and never again.
#[derive(Debug, Clone)]
pub(crate) struct EphemeralRoot {
    path: PathBuf,
}

impl EphemeralRoot {
    pub(crate) fn in_root(root: &VolumeRoot) -> Self {
        Self {
            path: root.ephemeral_dir(),
        }
    }

    fn tempdir(&self) -> std::io::Result<Vfs> {
        Vfs::tempdir_in(&self.path)
    }

    /// Remove scratch directories an earlier process left behind: a scratch
    /// directory is removed when its execute ends, so at boot every one is a
    /// leftover from a crash. Entries without the scratch prefix are left alone,
    /// and a link with it is removed without following it. Only a root that
    /// cannot be opened fails the sweep; an entry that cannot be read or removed
    /// is logged and left for the next boot.
    ///
    /// This assumes one server per root. Workers sharing a root would remove
    /// each other's live scratch (SUB-1343).
    fn sweep(&self) -> std::io::Result<()> {
        let entries = std::fs::read_dir(&self.path)?;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                // The listing may not move past a failed entry; stop rather than
                // spin, and leave what is left for the next boot.
                Err(error) => {
                    tracing::warn!(
                        path = %self.path.display(),
                        %error,
                        "listing leftover ephemeral scratch failed; it is retried at the next boot"
                    );
                    break;
                }
            };
            let is_scratch = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(interpreter::runtime::vfs::TEMPDIR_PREFIX));
            if !is_scratch {
                continue;
            }
            let path = entry.path();
            let removed = entry.file_type().and_then(|file_type| {
                if file_type.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                }
            });
            if let Err(error) = removed {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "removing leftover ephemeral scratch failed; it is retried at the next boot"
                );
            }
        }
        Ok(())
    }
}

/// Build the `Vfs` for one execute. Every named volume — a `named` root or a
/// mount — resolves through `volumes` here, at the single point every VFS is
/// constructed, so no route into a session can mount a directory the operator
/// did not declare. Size limits are attached separately ([`attach_limits`]),
/// since only a program that may write needs them.
pub(crate) fn build_vfs(
    blueprint: &Blueprint,
    variables: &VarBindings,
    session_root: Option<&Path>,
    ephemeral_root: &EphemeralRoot,
    volumes: &VolumeRegistry,
) -> Result<Vfs, SessionError> {
    open_vfs(
        blueprint,
        variables,
        session_root,
        ephemeral_root,
        volumes,
        WorkspaceOpen::Prepare,
    )
}

#[derive(Clone, Copy)]
enum WorkspaceOpen {
    Prepare,
    Existing,
}

fn open_vfs(
    blueprint: &Blueprint,
    variables: &VarBindings,
    session_root: Option<&Path>,
    ephemeral_root: &EphemeralRoot,
    volumes: &VolumeRegistry,
    mode: WorkspaceOpen,
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
    let config = blueprint
        .vfs
        .resolve(variables)
        .map_err(|e| SessionError::InvalidVfs(e.to_string()))?;
    let root = match &config {
        VfsConfig::None => return Ok(Vfs::none()),
        VfsConfig::Ephemeral { .. } => ephemeral_root.tempdir().map_err(io("ephemeral"))?,
        VfsConfig::PerSession { .. } => {
            let root = session_root
                .ok_or_else(|| SessionError::Io("per_session vfs root missing".into()))?;
            Vfs::external_with_mode(root.to_path_buf(), RtVfsMode::PerSession)
                .map_err(io("per_session"))?
        }
        VfsConfig::Named {
            volume,
            access,
            sub_path,
            ..
        } => mount_root(volume, *access, sub_path.as_deref(), volumes, mode)?,
    };
    let vfs = config
        .mounts()
        .iter()
        .try_fold(root, |vfs, (path, mount)| {
            graft(
                vfs,
                path,
                &mount.volume,
                mount.access,
                mount.sub_path.as_deref(),
                volumes,
                mode,
            )
        })?;
    let vfs = match mode {
        WorkspaceOpen::Prepare => vfs.with_cwd(config.cwd()),
        WorkspaceOpen::Existing => vfs.with_cwd_path(config.cwd()),
    };
    vfs.map_err(|error| {
        tracing::error!(%error, "preparing working directory failed");
        SessionError::InvalidVfs(format!(
            "cwd {} must name an existing directory or a directory in writable storage",
            config.cwd()
        ))
    })
}

/// Enforce the size limits on `vfs`: the blueprint's `size_limit` on an
/// ephemeral or per-session root, and each named volume's own limit, shared with
/// every other VFS using it. Measuring walks a whole directory, so it runs on
/// the blocking pool rather than an async worker.
pub(crate) async fn attach_limits(
    vfs: Vfs,
    blueprint: &Blueprint,
    volumes: &VolumeRegistry,
) -> Result<Vfs, SessionError> {
    let mut vfs = attach_size_limit(vfs, blueprint).await?;
    if let Some(volume) = vfs.volume().map(str::to_string)
        && let Some(quota) = volumes.quota(&volume).await?
    {
        vfs = vfs.with_shared_quota(quota);
    }
    let mounts: Vec<(String, String)> = vfs
        .mounts()
        .iter()
        .map(|mount| (mount.guest_path().to_string(), mount.volume().to_string()))
        .collect();
    for (path, volume) in mounts {
        if let Some(quota) = volumes.quota(&volume).await? {
            vfs = vfs.with_mount_quota(&path, quota);
        }
    }
    Ok(vfs)
}

/// Enforce the blueprint's own `size_limit` on an ephemeral or per-session root.
async fn attach_size_limit(vfs: Vfs, blueprint: &Blueprint) -> Result<Vfs, SessionError> {
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

/// Open a named volume as the root. Failures are reported to the client by
/// volume name only: the host directory is the operator's business, and the
/// open error carries it verbatim, so it is logged rather than returned.
fn mount_root(
    volume: &str,
    access: Option<submilli_blueprint::Access>,
    sub_path: Option<&str>,
    volumes: &VolumeRegistry,
    mode: WorkspaceOpen,
) -> Result<Vfs, SessionError> {
    let resolved = match mode {
        WorkspaceOpen::Prepare => volumes.resolve(volume, access)?,
        WorkspaceOpen::Existing => volumes.resolve_existing(volume, access)?,
    };
    let vfs = Vfs::external_with_mode(resolved.host.clone(), RtVfsMode::Named).map_err(|err| {
        tracing::error!(
            volume,
            path = %resolved.host.display(),
            %err,
            "mounting declared volume failed"
        );
        SessionError::VolumeUnavailable(volume.to_string())
    })?;
    let vfs = vfs
        .with_access(runtime_access(resolved.access))
        .with_volume_name(volume);
    let vfs = match mode {
        WorkspaceOpen::Prepare => vfs.with_subpath(sub_path),
        WorkspaceOpen::Existing => vfs.reopen_subpath(sub_path),
    };
    vfs.map_err(|error| {
        tracing::error!(volume, %error, "opening volume subPath failed");
        SessionError::MountFailed {
            volume: volume.into(),
            reason: "subPath must name a directory; missing directories require read_write access"
                .into(),
        }
    })
}

/// Graft a named volume at `path` below the root.
fn graft(
    vfs: Vfs,
    path: &str,
    volume: &str,
    access: Option<submilli_blueprint::Access>,
    sub_path: Option<&str>,
    volumes: &VolumeRegistry,
    mode: WorkspaceOpen,
) -> Result<Vfs, SessionError> {
    let resolved = match mode {
        WorkspaceOpen::Prepare => volumes.resolve(volume, access)?,
        WorkspaceOpen::Existing => volumes.resolve_existing(volume, access)?,
    };
    let host = resolved.host.clone();
    let spec = MountSpec {
        guest_path: path.to_string(),
        host: resolved.host,
        volume: volume.to_string(),
        access: runtime_access(resolved.access),
        // Attached by `attach_limits`, and only for a run that may write.
        quota: None,
    };
    let vfs = match mode {
        WorkspaceOpen::Prepare => vfs.with_mount_subpath(spec, sub_path),
        WorkspaceOpen::Existing => vfs.reopen_mount_subpath(spec, sub_path),
    };
    vfs.map_err(|err| match err {
        MountError::Io(err) => {
            tracing::error!(
                volume,
                path = %host.display(),
                %err,
                "mounting declared volume failed"
            );
            SessionError::VolumeUnavailable(volume.to_string())
        }
        MountError::RootIo(err) => {
            // The root's own storage, not the volume's, failed.
            tracing::error!(volume, mount = path, %err, "preparing the mount point failed");
            SessionError::MountFailed {
                volume: volume.to_string(),
                reason: format!(
                    "the mount point {path} could not be prepared; the server log has the details"
                ),
            }
        }
        other => SessionError::MountFailed {
            volume: volume.to_string(),
            reason: other.to_string(),
        },
    })
}

fn runtime_access(access: submilli_blueprint::Access) -> RtAccess {
    match access {
        submilli_blueprint::Access::ReadOnly => RtAccess::ReadOnly,
        submilli_blueprint::Access::ReadWrite => RtAccess::ReadWrite,
    }
}

pub(crate) fn vfs_info(blueprint: &Blueprint) -> VfsInfo {
    VfsInfo {
        mode: match &blueprint.vfs {
            VfsConfig::None => RtVfsMode::None,
            VfsConfig::Ephemeral { .. } => RtVfsMode::Ephemeral,
            VfsConfig::PerSession { .. } => RtVfsMode::PerSession,
            VfsConfig::Named { .. } => RtVfsMode::Named,
        },
        size_limit: blueprint.vfs.size_limit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interpreter::runtime::SessionKvError;
    use std::time::UNIX_EPOCH;

    use crate::request_records::{Record as LedgerEntry, RequestRecords, code_fingerprint};
    use crate::session_store::{RootVfsType, SessionRecord};

    const TINY: Duration = Duration::from_millis(1);
    const HOUR: Duration = Duration::from_secs(3600);

    #[tokio::test]
    async fn reaper_rejects_invalid_intervals_then_starts_once() {
        let (manager, _root) = manager();
        let manager = Arc::new(manager);
        assert_eq!(
            manager.spawn_reaper(Duration::ZERO),
            Err(ReaperStartError::ZeroInterval)
        );
        assert_eq!(
            manager.spawn_reaper(Duration::MAX),
            Err(ReaperStartError::DeadlineOverflow)
        );
        assert!(!manager.reaper_started.load(Ordering::Acquire));
        assert_eq!(Arc::strong_count(&manager), 1);
        manager.spawn_reaper(HOUR).expect("healthy startup");
        assert!(manager.reaper_started.load(Ordering::Acquire));
        manager.spawn_reaper(HOUR).expect("repeat startup");
        assert_eq!(Arc::strong_count(&manager), 1);
        assert_eq!(
            manager.spawn_reaper(Duration::ZERO),
            Err(ReaperStartError::ZeroInterval)
        );
    }

    #[test]
    fn reaper_without_runtime_leaves_startup_available() {
        let (manager, _root) = manager();
        let manager = Arc::new(manager);
        manager.spawn_reaper(HOUR).expect("no runtime is a no-op");
        assert!(!manager.reaper_started.load(Ordering::Acquire));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("timer runtime");
        let _entered = runtime.enter();
        manager.spawn_reaper(HOUR).expect("healthy startup");
        assert!(manager.reaper_started.load(Ordering::Acquire));
    }

    #[test]
    fn reaper_timer_contract_is_checked_before_startup_state() {
        let (manager, _root) = manager();
        let manager = Arc::new(manager);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime without timers");
        {
            let _entered = runtime.enter();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                manager.spawn_reaper(HOUR)
            }));
            assert!(result.is_err(), "violating the timer contract may panic");
        }
        assert!(!manager.reaper_started.load(Ordering::Acquire));
        assert_eq!(Arc::strong_count(&manager), 1);
        let healthy_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("timer runtime");
        let _entered = healthy_runtime.enter();
        manager.spawn_reaper(HOUR).expect("healthy follow-up");
        assert!(manager.reaper_started.load(Ordering::Acquire));
    }

    #[test]
    fn reaper_deadlines_preserve_burst_catch_up_and_check_overflow() {
        let overdue = tokio::time::Instant::now()
            .checked_sub(HOUR)
            .expect("past deadline");
        let next = SessionManager::next_reaper_deadline(overdue, TINY).unwrap();
        assert_eq!(next.duration_since(overdue), TINY);
        assert!(next < tokio::time::Instant::now());
        assert_eq!(
            SessionManager::next_reaper_deadline(overdue, Duration::MAX),
            Err(ReaperStartError::DeadlineOverflow)
        );
    }

    #[test]
    fn reaper_deadlines_reserve_timer_rounding_headroom() {
        let start = tokio::time::Instant::now();
        let duration = |nanos: u128| {
            Duration::new(
                (nanos / 1_000_000_000).try_into().unwrap(),
                (nanos % 1_000_000_000).try_into().unwrap(),
            )
        };
        // Locate the platform boundary rather than assuming Instant's range is
        // identical on macOS, Linux and Windows.
        let mut valid = 0;
        let mut invalid = Duration::MAX.as_nanos();
        assert!(start.checked_add(duration(invalid)).is_none());
        while invalid - valid > 1 {
            let midpoint = valid + (invalid - valid) / 2;
            if start.checked_add(duration(midpoint)).is_some() {
                valid = midpoint;
            } else {
                invalid = midpoint;
            }
        }
        let last = start.checked_add(duration(valid)).unwrap();
        let previous = last.checked_sub(TINY).unwrap();
        assert_eq!(previous.checked_add(TINY), Some(last));
        assert_eq!(
            SessionManager::next_reaper_deadline(previous, TINY),
            Err(ReaperStartError::DeadlineOverflow)
        );
        let safe = previous.checked_sub(TINY).unwrap();
        assert_eq!(
            SessionManager::next_reaper_deadline(safe, TINY),
            Ok(previous)
        );
    }

    #[tokio::test]
    async fn reaper_deadline_overflow_clears_startup_state() {
        let (manager, _root) = manager();
        let manager = Arc::new(manager);
        manager.reaper_started.store(true, Ordering::Release);
        let deadline = tokio::time::Instant::now();
        // Inject a schedule rejected by the public API to exercise the task's
        // later-overflow branch without advancing the platform clock centuries.
        tokio::time::timeout(
            Duration::from_secs(2),
            SessionManager::run_reaper(
                Arc::downgrade(&manager),
                Duration::MAX,
                deadline,
                tokio::time::sleep_until(deadline),
            ),
        )
        .await
        .expect("overflow must stop the task");
        assert!(!manager.reaper_started.load(Ordering::Acquire));
        manager.spawn_reaper(HOUR).expect("healthy follow-up");
    }

    #[tokio::test]
    async fn reaper_sweeps_immediately_and_on_later_ticks() {
        let (mgr, _root) = manager();
        let mgr = Arc::new(mgr);
        let blueprint = per_session(HOUR);
        let first = mgr
            .create(&blueprint, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        set_last_activity(&mgr, &first, UNIX_EPOCH).await;
        // A long period demonstrates that the first sweep does not wait for it.
        mgr.spawn_reaper(HOUR).expect("start reaper");
        tokio::time::timeout(Duration::from_secs(2), async {
            while mgr.store.active_ids().await.unwrap().contains(&first) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("immediate sweep");

        let (later_manager, _later_root) = manager();
        let later_manager = Arc::new(later_manager);
        let initial = later_manager
            .create(&blueprint, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        set_last_activity(&later_manager, &initial, UNIX_EPOCH).await;
        later_manager
            .spawn_reaper(TINY)
            .expect("start periodic reaper");
        tokio::time::timeout(Duration::from_secs(2), async {
            while later_manager
                .store
                .active_ids()
                .await
                .unwrap()
                .contains(&initial)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("initial sweep before adding another session");
        let later = later_manager
            .create(&blueprint, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        set_last_activity(&later_manager, &later, UNIX_EPOCH).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while later_manager
                .store
                .active_ids()
                .await
                .unwrap()
                .contains(&later)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("subsequent sweep");
    }

    #[tokio::test]
    async fn an_idle_session_expires_with_its_last_run() {
        use crate::application::sessions::ports::LastRunStaging;
        let (manager, _root) = manager();
        let blueprint = per_session(HOUR);
        manager.ensure("idle", &blueprint).await.unwrap();
        let staged = manager
            .last_runs
            .stage("idle", b"{}".to_vec())
            .await
            .unwrap();
        let mut unit = manager.unit_of_work.begin().await.unwrap();
        unit.put_last_run("idle", "run", SystemTime::now(), &staged)
            .await
            .unwrap();
        unit.commit().await.unwrap();
        assert!(manager.last_runs.get("idle").await.unwrap().is_some());
        set_last_activity(&manager, "idle", UNIX_EPOCH).await;
        assert_eq!(manager.reap_now().await.unwrap(), 1);
        assert_eq!(manager.last_runs.get("idle").await.unwrap(), None);
    }

    #[tokio::test]
    async fn recovery_and_reaping_audit_the_session_lifecycle() {
        let (manager, root) = manager();
        let audit_path = root.path().join("audit.log");
        let manager = manager.with_audit(crate::audit::AuditLog::new(
            crate::audit::AuditConfig {
                file: Some(audit_path.clone()),
                ..Default::default()
            },
            None,
        ));
        let blueprint = per_session(HOUR);
        manager.ensure("expired-at-boot", &blueprint).await.unwrap();
        manager.ensure("active-at-boot", &blueprint).await.unwrap();
        set_last_activity(&manager, "expired-at-boot", UNIX_EPOCH).await;
        manager.boot().await.unwrap();
        set_last_activity(&manager, "active-at-boot", UNIX_EPOCH).await;
        assert_eq!(manager.reap_now().await.unwrap(), 1);
        assert_eq!(manager.reap_now().await.unwrap(), 0);
        let audit = std::fs::read_to_string(audit_path).unwrap();
        for id in ["expired-at-boot", "active-at-boot"] {
            let lines: Vec<_> = audit
                .lines()
                .filter(|line| line.contains(&format!("session_id={id}")))
                .collect();
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.contains("event=found"))
                    .count(),
                1
            );
            let expired: Vec<_> = lines
                .iter()
                .filter(|line| line.contains("event=expired"))
                .collect();
            assert_eq!(expired.len(), 1);
            assert!(expired[0].contains("reason=idle_timeout"));
            assert!(!lines.iter().any(|line| line.contains("event=deleted")));
        }
    }

    async fn set_last_activity(manager: &SessionManager, id: &str, time: SystemTime) {
        let mut record = manager.store.load(id).await.unwrap().unwrap();
        record.last_activity = time;
        manager.store.put(record).await.unwrap();
    }

    struct DelayedUnitOfWorkFactory(Arc<dyn crate::application::unit_of_work::UnitOfWorkFactory>);

    #[async_trait::async_trait]
    impl crate::application::unit_of_work::UnitOfWorkFactory for DelayedUnitOfWorkFactory {
        async fn begin(
            &self,
        ) -> Result<
            Box<dyn crate::application::unit_of_work::UnitOfWork>,
            crate::blueprint::StoreError,
        > {
            tokio::time::sleep(Duration::from_millis(250)).await;
            self.0.begin().await
        }
    }

    #[tokio::test]
    async fn start_execution_rechecks_expiry_after_waiting_for_the_unit_of_work() {
        let (mut manager, _dir) = manager();
        let blueprint = per_session(Duration::from_millis(100));
        manager.ensure("waiting", &blueprint).await.unwrap();
        let before = manager.store.load("waiting").await.unwrap().unwrap();
        manager.unit_of_work = Arc::new(DelayedUnitOfWorkFactory(manager.unit_of_work.clone()));

        assert!(matches!(
            manager.start_execution("waiting").await,
            Err(SessionError::UnknownSession)
        ));
        let after = manager.store.load("waiting").await.unwrap().unwrap();
        assert_eq!(before.last_activity, after.last_activity);
    }

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
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            ..Default::default()
        };
        let err = build_vfs(
            &blueprint,
            &VarBindings::new(),
            Some(&root),
            &EphemeralRoot::in_root(&VolumeRoot::default_root(outer.path())),
            &VolumeRegistry::default(),
        )
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

    fn seed_blueprints(database: &Arc<crate::database::ServerDatabase>) {
        use crate::blueprint::BlueprintStore;
        let blueprints = crate::blueprint::SqliteBlueprintStore::new(database.clone(), None);
        futures::executor::block_on(async {
            for name in ["p", "n", "e", "x", "bp"] {
                blueprints
                    .upsert(Blueprint {
                        name: name.into(),
                        ..Default::default()
                    })
                    .await
                    .unwrap();
            }
        });
    }

    /// A volume root at `dir`, prepared as at startup.
    fn test_root(dir: &Path) -> VolumeRoot {
        let root = VolumeRoot::configured(dir);
        root.prepare().expect("prepare the volume root");
        root
    }

    fn mem_store() -> Arc<crate::session_store::SqliteSessionStore> {
        let database = Arc::new(
            futures::executor::block_on(crate::database::ServerDatabase::open_ephemeral()).unwrap(),
        );
        seed_blueprints(&database);
        Arc::new(crate::session_store::SqliteSessionStore::new(
            database,
            None,
            std::path::PathBuf::new(),
        ))
    }

    fn file_store(dir: &std::path::Path) -> Arc<crate::session_store::SqliteSessionStore> {
        let database = Arc::new(
            futures::executor::block_on(crate::database::ServerDatabase::open(
                &dir.join("server.db"),
            ))
            .unwrap(),
        );
        seed_blueprints(&database);
        Arc::new(crate::session_store::SqliteSessionStore::new(
            database,
            None,
            dir.to_path_buf(),
        ))
    }

    fn indeterminate_run(session_id: &str, key: &str) -> LedgerEntry {
        LedgerEntry::indeterminate(session_id, key, code_fingerprint("main"))
    }

    fn manager() -> (SessionManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        (
            SessionManager::new(
                &test_root(dir.path()),
                Arc::default(),
                no_http(),
                mem_store(),
                CapabilitySettings::default(),
            ),
            dir,
        )
    }

    /// A manager plus its retained run records, for the cleanup tests.
    fn manager_with_ledger() -> (SessionManager, tempfile::TempDir, Arc<RequestRecords>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = mem_store();
        let ledger = Arc::new(RequestRecords::with_database(store.database_owner()));
        (
            SessionManager::new(
                &test_root(dir.path()),
                Arc::default(),
                no_http(),
                store,
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
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
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
    async fn failed_binding_cleans_new_workspace_and_preserves_existing_session() {
        let (mgr, root) = manager();
        let bad = submilli_blueprint::parse("name: x\nvfs:\n  mode: per_session\n  mounts:\n    /missing: {mode: named, volume: absent}\n").unwrap();
        assert!(
            mgr.bind("new", &bad, Arc::new(VarBindings::new()), no_secrets())
                .await
                .is_err()
        );
        assert!(!root.path().join("sessions/new").exists());
        assert!(!mgr.contains("new").await.unwrap());
        let good = submilli_blueprint::parse("name: x\nvfs: per_session\nvariables:\n  user: {}\n")
            .unwrap();
        let original = Arc::new(VarBindings::from([("user".into(), "ada".into())]));
        mgr.bind("existing", &good, original.clone(), no_secrets())
            .await
            .unwrap();
        std::fs::write(root.path().join("sessions/existing/kept"), "keep").unwrap();
        assert!(
            mgr.bind("existing", &bad, Arc::new(VarBindings::new()), no_secrets())
                .await
                .is_err()
        );
        assert_eq!(mgr.variables("existing").await.unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(root.path().join("sessions/existing/kept")).unwrap(),
            "keep"
        );
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
        assert!(root.path().join("sessions").join(&id).is_dir());
    }

    #[tokio::test]
    async fn none_mode_allocates_no_dir() {
        let (mgr, root) = manager();
        let id = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        assert!(!root.path().join("sessions").join(&id).exists());
    }

    #[tokio::test]
    async fn ensure_is_idempotent() {
        let (mgr, _root) = manager();
        let bp = per_session(HOUR);
        mgr.ensure("sid", &bp).await.unwrap();
        mgr.ensure("sid", &bp).await.unwrap();
        assert!(mgr.contains("sid").await.unwrap());
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
        assert_eq!(vfs.root(), root.path().join("sessions").join(&id));
        assert_eq!(info.mode, RtVfsMode::PerSession);
    }

    #[tokio::test]
    async fn ephemeral_uses_configured_root() {
        let root = tempfile::tempdir().expect("volume root");
        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            mem_store(),
            CapabilitySettings::default(),
        );
        let bp = Blueprint {
            name: "e".into(),
            vfs: VfsConfig::Ephemeral {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            ..Default::default()
        };
        mgr.ensure("sid", &bp).await.unwrap();
        let (vfs, _) = mgr.vfs_for_execute("sid", &bp).await.unwrap();
        assert!(
            vfs.root().starts_with(root.path().join("ephemeral")),
            "ephemeral scratch should live under the volume root"
        );
    }

    #[tokio::test]
    async fn per_session_dir_survives_a_restart() {
        let dir = tempfile::tempdir().expect("durable session root");
        let bp = per_session(HOUR);

        // First boot: write a file into the session's VFS.
        let mgr = SessionManager::new(
            &test_root(dir.path()),
            Arc::default(),
            no_http(),
            mem_store(),
            CapabilitySettings::default(),
        );
        mgr.ensure("sid", &bp).await.unwrap();
        let (v1, _) = mgr.vfs_for_execute("sid", &bp).await.unwrap();
        std::fs::write(v1.root().join("a.txt"), b"hi").unwrap();

        // Restart: a fresh manager over the same durable root, in-memory state
        // gone. Reconnecting with the same session id finds the files intact.
        let restarted = SessionManager::new(
            &test_root(dir.path()),
            Arc::default(),
            no_http(),
            mem_store(),
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
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store.clone(),
            CapabilitySettings::default(),
        );
        mgr.ensure("sid", &bp).await.unwrap();

        // Restart: fresh manager + empty cache, same store + root.
        let restarted = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store,
            CapabilitySettings::default(),
        );
        assert!(
            restarted.contains("sid").await.unwrap(),
            "authoritative reads do not need cache hydration"
        );
        restarted.boot().await.expect("boot");
        assert!(
            restarted.contains("sid").await.unwrap(),
            "boot must rehydrate the session"
        );
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
                root_vfs_type: crate::session_store::RootVfsType::PerSession,
                mcp_state: None,
                variables: Default::default(),
                ..SessionRecord::default()
            })
            .await
            .unwrap();
        let dir = root.path().join("sessions/old");
        std::fs::create_dir_all(&dir).unwrap();

        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store,
            CapabilitySettings::default(),
        );
        mgr.boot().await.expect("boot");
        assert!(!mgr.contains("old").await.unwrap());
        assert_eq!(mgr.reap_now().await.unwrap(), 0);
        assert!(!dir.exists(), "an idle rehydrated session is reaped");
    }

    #[tokio::test]
    async fn reaping_expires_responses_past_their_retention() {
        use crate::domain::idempotent_request::{IdempotentRequest, RecordedOutcome, RunStatus};
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            file_store(store_dir.path()),
            CapabilitySettings::default(),
        )
        .with_response_retention(ResponseRetention {
            ttl: Duration::ZERO,
            keep_last: 0,
        });
        mgr.bind("s1", &per_session(HOUR), Arc::default(), no_secrets())
            .await
            .unwrap();
        let completed_at = SystemTime::now() - Duration::from_secs(1);
        let request = IdempotentRequest::reserve(
            "s1".into(),
            "k".into(),
            "f".into(),
            "r".into(),
            "g".into(),
            completed_at,
        )
        .unwrap();
        let mut unit = mgr.unit_of_work.begin().await.unwrap();
        unit.save_run(request).await.unwrap();
        unit.commit().await.unwrap();
        crate::application::sessions::runs::claim_run(mgr.unit_of_work.as_ref(), "r", "g")
            .await
            .unwrap();
        let mut unit = mgr.unit_of_work.begin().await.unwrap();
        let mut request = unit.get_run("r").await.unwrap().unwrap();
        request.complete("r", completed_at).unwrap();
        unit.save_run_result(
            "r",
            RecordedOutcome {
                status: 200,
                body: "bytes".into(),
            },
        )
        .await
        .unwrap();
        unit.save_run(request).await.unwrap();
        unit.commit().await.unwrap();

        assert_eq!(mgr.reap_now().await.unwrap(), 0, "the session stays");
        let mut unit = mgr.unit_of_work.begin().await.unwrap();
        assert_eq!(
            unit.run_for_key("s1", "k").await.unwrap().unwrap().status(),
            RunStatus::Expired
        );
    }

    #[tokio::test]
    async fn restart_persists_variables_and_encrypted_harness_secrets() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let bp = per_session(HOUR);
        let key = store_dir.path().join("key");
        std::fs::write(&key, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
        let cipher = Arc::new(
            SecretCipher::new(&submilli_shared::secret_store::KeySource::File(key)).unwrap(),
        );
        let vars = Arc::new(VarBindings::from([(
            "tenant".to_string(),
            "u_42".to_string(),
        )]));

        {
            let mgr = SessionManager::new(
                &test_root(root.path()),
                Arc::default(),
                no_http(),
                file_store(store_dir.path()),
                CapabilitySettings::default(),
            )
            .with_cipher(Some(cipher.clone()));
            let bound = Arc::new(HarnessSecretBindings::from([(
                "TOKEN".to_string(),
                "session-only".to_string(),
            )]));
            mgr.bind("s1", &bp, Arc::clone(&vars), bound).await.unwrap();
            assert_eq!(
                mgr.variables("s1")
                    .await
                    .unwrap()
                    .get("tenant")
                    .map(String::as_str),
                Some("u_42")
            );
            assert_eq!(
                mgr.harness_secrets("s1")
                    .await
                    .unwrap()
                    .and_then(|values| values.get("TOKEN").cloned())
                    .as_deref(),
                Some("session-only")
            );
            mgr.database.close().await.unwrap();
        }

        // A fresh manager over the same store + root: boot rehydrates the entry,
        // and its variable bindings come back with it.
        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            file_store(store_dir.path()),
            CapabilitySettings::default(),
        )
        .with_cipher(Some(cipher));
        mgr.boot().await.expect("boot");
        assert_eq!(
            mgr.variables("s1")
                .await
                .unwrap()
                .get("tenant")
                .map(String::as_str),
            Some("u_42")
        );
        assert_eq!(
            mgr.harness_secrets("s1")
                .await
                .unwrap()
                .unwrap()
                .get("TOKEN")
                .map(String::as_str),
            Some("session-only")
        );
    }

    #[tokio::test]
    async fn boot_sweeps_orphan_dir_with_no_record() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        // A leftover per_session dir whose session was never persisted.
        let orphan = root.path().join("sessions/ghost");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("a.txt"), b"x").unwrap();

        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            file_store(store_dir.path()),
            CapabilitySettings::default(),
        );
        mgr.boot().await.expect("boot");
        assert!(
            !orphan.exists(),
            "boot must sweep a dir with no live session"
        );
    }

    #[tokio::test]
    async fn boot_preserves_record_whose_dir_is_gone() {
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
                root_vfs_type: crate::session_store::RootVfsType::PerSession,
                mcp_state: None,
                variables: Default::default(),
                ..SessionRecord::default()
            })
            .await
            .unwrap();

        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store.clone(),
            CapabilitySettings::default(),
        );
        mgr.boot().await.expect("boot");
        assert!(
            mgr.contains("vanished").await.unwrap(),
            "local workspace absence cannot retire an authoritative record"
        );
        assert!(
            store.load_all().await.expect("list sessions").len() == 1,
            "the authoritative record is preserved"
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
        let dir = root.path().join("sessions").join(&id);
        std::fs::write(dir.join("a.txt"), b"hi").unwrap();
        assert!(mgr.wipe_now(&id).await.unwrap());
        assert!(
            !dir.exists(),
            "explicit terminate must wipe the session dir"
        );
        assert!(!mgr.contains(&id).await.unwrap());
    }

    #[tokio::test]
    async fn wipe_now_preserves_indeterminate_run_evidence() {
        let (mgr, _root, ledger) = manager_with_ledger();
        let id = mgr
            .create(
                &per_session(HOUR),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        ledger.put(indeterminate_run(&id, "k")).await.unwrap();

        assert!(mgr.wipe_now(&id).await.unwrap());
        assert!(!mgr.contains(&id).await.unwrap());
        assert_eq!(
            ledger.load(&id, "k").await.unwrap(),
            Some(indeterminate_run(&id, "k"))
        );
    }

    #[tokio::test]
    async fn reap_preserves_indeterminate_run_evidence() {
        let (mgr, _root, ledger) = manager_with_ledger();
        let id = mgr
            .create(
                &per_session(TINY),
                Arc::new(VarBindings::new()),
                no_secrets(),
            )
            .await
            .unwrap();
        ledger.put(indeterminate_run(&id, "k")).await.unwrap();

        assert_eq!(mgr.reap(way_later()).await.unwrap(), 1);
        assert!(!mgr.contains(&id).await.unwrap());
        assert_eq!(
            ledger.load(&id, "k").await.unwrap(),
            Some(indeterminate_run(&id, "k"))
        );
    }

    #[tokio::test]
    async fn boot_preserves_ledger_when_only_the_workspace_is_missing() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        let ledger = Arc::new(RequestRecords::with_database(store.database_owner()));
        store
            .put(SessionRecord {
                session_id: "vanished".into(),
                blueprint_name: "p".into(),
                idle_timeout: HOUR,
                last_activity: SystemTime::now(),
                root_vfs_type: crate::session_store::RootVfsType::PerSession,
                mcp_state: None,
                variables: Default::default(),
                ..SessionRecord::default()
            })
            .await
            .unwrap();
        ledger
            .put(indeterminate_run("vanished", "k"))
            .await
            .unwrap();

        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store,
            CapabilitySettings::default(),
        );
        mgr.boot().await.expect("boot");

        assert!(ledger.load("vanished", "k").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn boot_keeps_the_ledger_of_a_session_that_resumed() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("session root");
        let store = file_store(store_dir.path());
        let ledger = Arc::new(RequestRecords::with_database(store.database_owner()));
        let bp = per_session(HOUR);

        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            Arc::clone(&store),
            CapabilitySettings::default(),
        );
        mgr.ensure("sid", &bp).await.unwrap();
        ledger.put(indeterminate_run("sid", "k")).await.unwrap();

        let restarted = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store,
            CapabilitySettings::default(),
        );
        restarted.boot().await.expect("boot");

        assert!(
            ledger.load("sid", "k").await.unwrap().is_some(),
            "a resumed session keeps its ledger"
        );
    }

    #[tokio::test]
    async fn wipe_now_unknown_is_false() {
        let (mgr, _root) = manager();
        assert!(!mgr.wipe_now("nope").await.unwrap());
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
        assert_eq!(mgr.reap(way_later()).await.unwrap(), 1);
        assert!(!root.path().join("sessions").join(&id).exists());
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
        assert_eq!(mgr.reap(SystemTime::now()).await.unwrap(), 0);
        assert!(root.path().join("sessions").join(&id).is_dir());
    }

    #[tokio::test]
    async fn named_volumes_survive_reap_and_orphan_reconciliation() {
        let session_root = tempfile::tempdir().expect("tempdir");
        let data = tempfile::tempdir().expect("data");
        let local = data.path().join("local");
        std::fs::create_dir(&local).unwrap();
        std::fs::write(local.join("kept.txt"), "kept").unwrap();
        let registry = VolumeRegistry::new(
            VolumeTable::from([
                (
                    "managed".to_string(),
                    crate::config::VolumeSpec::managed(crate::config::SizeLimit::Bytes(1 << 20)),
                ),
                (
                    "local".to_string(),
                    crate::config::VolumeSpec::local_path(&local),
                ),
            ]),
            data.path().join("volumes"),
        );
        std::fs::create_dir(data.path().join("volumes")).unwrap();
        let mgr = SessionManager::new(
            &test_root(session_root.path()),
            Arc::new(registry),
            no_http(),
            mem_store(),
            CapabilitySettings::default(),
        );
        let bp = submilli_blueprint::parse(
            "name: x\nidle_timeout: 1s\nvfs:\n  mode: per_session\n  mounts:\n    /managed: {mode: named, volume: managed}\n    /local: {mode: named, volume: local, access: read_only}\n",
        )
        .unwrap();
        let id = mgr
            .create(&bp, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        let (vfs, _) = mgr.vfs_for_execute(&id, &bp).await.unwrap();
        let managed = data.path().join("volumes/managed");
        assert!(
            managed.is_dir(),
            "the managed volume is created on first use"
        );
        std::fs::write(managed.join("note.txt"), "note").unwrap();
        assert_eq!(vfs.mounts().len(), 2);
        assert!(
            vfs.mounts()
                .iter()
                .any(|mount| mount.volume() == "managed" && mount.quota().is_some()),
            "the managed volume carries its shared limit"
        );
        drop(vfs);
        mgr.reap(way_later()).await.unwrap();
        assert!(
            !session_root.path().join("sessions").join(&id).exists(),
            "the session dir is wiped"
        );
        mgr.boot().await.expect("boot");
        assert_eq!(
            std::fs::read_to_string(managed.join("note.txt")).unwrap(),
            "note"
        );
        assert_eq!(
            std::fs::read_to_string(local.join("kept.txt")).unwrap(),
            "kept"
        );
    }

    #[tokio::test]
    async fn rebind_preserves_owned_root_until_session_cleanup() {
        let (mgr, root) = manager();
        let bp = Blueprint {
            name: "x".into(),
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            ..Default::default()
        };
        let id = mgr
            .create(&bp, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        for mode in [
            VfsConfig::None,
            VfsConfig::Ephemeral {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            VfsConfig::Named {
                volume: "shared".into(),
                access: None,
                mounts: Default::default(),
                cwd: None,
                sub_path: None,
            },
        ] {
            let changed = Blueprint {
                vfs: mode,
                ..bp.clone()
            };
            assert!(
                mgr.bind(&id, &changed, Arc::new(VarBindings::new()), no_secrets())
                    .await
                    .is_err()
            );
            assert_eq!(
                mgr.store.load(&id).await.unwrap().unwrap().root_vfs_type,
                RootVfsType::PerSession
            );
        }
        mgr.wipe_now(&id).await.unwrap();
        assert!(!root.path().join("sessions").join(id).exists());
    }

    #[tokio::test]
    async fn legacy_named_root_resolution_survives_restart_and_refuses_relocation() {
        let root = tempfile::tempdir().unwrap();
        let records = tempfile::tempdir().unwrap();
        let volume = tempfile::tempdir().unwrap();
        let other_volume = tempfile::tempdir().unwrap();
        let blueprint = Blueprint {
            name: "x".into(),
            vfs: VfsConfig::Named {
                volume: "shared".into(),
                access: None,
                mounts: Default::default(),
                cwd: None,
                sub_path: Some("users/ada".into()),
            },
            ..Default::default()
        };
        let make_manager = |volume: &Path| {
            SessionManager::new(
                &test_root(root.path()),
                Arc::new(VolumeRegistry::new(
                    VolumeTable::from([(
                        "shared".into(),
                        crate::config::VolumeSpec::local_path(volume),
                    )]),
                    root.path().join("managed"),
                )),
                no_http(),
                file_store(records.path()),
                CapabilitySettings::default(),
            )
        };
        let manager = make_manager(volume.path());
        manager
            .store
            .put(SessionRecord {
                session_id: "legacy".into(),
                blueprint_name: "x".into(),
                last_activity: SystemTime::now(),
                idle_timeout: HOUR,
                legacy_root_unresolved: true,
                ..Default::default()
            })
            .await
            .unwrap();
        manager.session_vfs("legacy", &blueprint).await.unwrap();
        let resolved = manager.store.load("legacy").await.unwrap().unwrap();
        assert!(!resolved.legacy_root_unresolved);
        assert_eq!(resolved.root_vfs_type, RootVfsType::Named);
        assert_eq!(
            resolved.root_vfs_path,
            Some(volume.path().join("users/ada"))
        );
        manager.database.close().await.unwrap();
        drop(manager);
        let restarted = make_manager(other_volume.path());
        assert!(restarted.session_vfs("legacy", &blueprint).await.is_err());
        assert!(!other_volume.path().join("users/ada").exists());
    }

    #[tokio::test]
    async fn a_named_root_creates_no_session_dir() {
        let (mut mgr, root) = manager();
        let volume = tempfile::tempdir().unwrap();
        mgr.volumes = Arc::new(VolumeRegistry::new(
            VolumeTable::from([(
                "workspace".into(),
                crate::config::VolumeSpec::local_path(volume.path()),
            )]),
            volume.path().join("managed"),
        ));
        let bp = Blueprint {
            name: "x".into(),
            vfs: VfsConfig::Named {
                volume: "workspace".into(),
                access: None,
                mounts: Default::default(),
                cwd: None,
                sub_path: Some("users/ada".into()),
            },
            ..Default::default()
        };
        let id = mgr
            .create(&bp, Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        let record = mgr.store.load(&id).await.unwrap().unwrap();
        assert_eq!(record.root_vfs_type, RootVfsType::Named);
        assert_eq!(record.root_vfs_path, Some(volume.path().join("users/ada")));
        assert!(!root.path().join("sessions").join(&id).exists());
        let changed = Blueprint {
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            ..bp.clone()
        };
        assert!(mgr.session_vfs(&id, &changed).await.is_err());
        assert!(
            mgr.bind(&id, &changed, Arc::new(VarBindings::new()), no_secrets())
                .await
                .is_err()
        );
        let unchanged = mgr.store.load(&id).await.unwrap().unwrap();
        assert_eq!(unchanged.root_vfs_type, RootVfsType::Named);
        assert_eq!(unchanged.root_vfs_path, record.root_vfs_path);
        mgr.reap(way_later()).await.unwrap();
        assert!(root.path().is_dir());
        assert!(volume.path().join("users/ada").is_dir());
    }

    fn key(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Every execute in a session, by either route, reads and writes the same
    /// entries, and no other session sees them.
    #[tokio::test]
    async fn one_state_per_session_shared_across_calls() {
        let (mgr, _root) = manager();
        let id = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();

        mgr.session_kv_for_execute(&id)
            .set(&key("k"), &key("\"v\""))
            .await
            .expect("set");
        assert!(
            mgr.session_kv_for_execute(&id)
                .has(&key("k"))
                .await
                .expect("has"),
            "a later execute in the same session must see the write"
        );

        let other = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        assert!(
            !mgr.session_kv_for_execute(&other)
                .has(&key("k"))
                .await
                .expect("has"),
            "a different session must not see it"
        );
    }

    #[tokio::test]
    async fn a_session_that_was_never_created_has_no_state() {
        let (mgr, _root) = manager();
        let error = mgr
            .session_kv_for_execute("unknown")
            .set(&key("k"), &key("\"v\""))
            .await
            .expect_err("no session to hold it");
        assert!(
            matches!(error, SessionKvError::SessionClosed { .. }),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn idle_expiry_ends_the_session_state() {
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
            .await
            .expect("set");

        assert_eq!(mgr.reap(way_later()).await.unwrap(), 1);
        assert_eq!(kv_rows(&mgr, &id).await, 0, "the expiry deleted them");
        assert!(matches!(
            mgr.session_kv_for_execute(&id).has(&key("k")).await,
            Err(SessionKvError::SessionClosed { .. })
        ));
    }

    /// A run still holding the store when its session closes cannot read the
    /// state, and cannot write it back.
    #[tokio::test]
    async fn a_handle_outliving_its_session_cannot_restore_it() {
        let (mgr, _root) = manager();
        let id = mgr
            .create(&none_bp(), Arc::new(VarBindings::new()), no_secrets())
            .await
            .unwrap();
        let held = mgr.session_kv_for_execute(&id);
        held.set(&key("k"), &key("\"v\"")).await.expect("set");

        assert!(mgr.wipe_now(&id).await.unwrap());

        assert!(matches!(
            held.has(&key("k")).await,
            Err(SessionKvError::SessionClosed { .. })
        ));
        assert!(matches!(
            held.set(&key("k2"), &key("\"v\"")).await,
            Err(SessionKvError::SessionClosed { .. })
        ));
        assert_eq!(kv_rows(&mgr, &id).await, 0);
    }

    async fn kv_rows(mgr: &SessionManager, id: &str) -> i64 {
        let id = id.to_owned();
        mgr.database
            .read(move |connection| {
                Box::pin(async move {
                    sqlx::query_scalar("SELECT COUNT(*) FROM session_kv WHERE session_id=?")
                        .bind(id)
                        .fetch_one(connection)
                        .await
                        .map_err(Into::into)
                })
            })
            .await
            .unwrap()
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

    /// Two managers over one database are two views of the same state: a write
    /// through one is read through the other, and a close through one ends the
    /// state the other's store reaches.
    #[tokio::test]
    async fn managers_over_one_database_share_session_state() {
        let root = tempfile::tempdir().unwrap();
        let store = mem_store();
        let make_manager = || {
            SessionManager::new(
                &test_root(root.path()),
                Arc::default(),
                no_http(),
                store.clone(),
                CapabilitySettings::default(),
            )
        };
        let first = make_manager();
        let second = make_manager();
        let id = first
            .create(&none_bp(), Arc::default(), no_secrets())
            .await
            .unwrap();
        second
            .session_kv_for_execute(&id)
            .set(&key("k"), &key("\"v\""))
            .await
            .unwrap();
        assert_eq!(
            first
                .session_kv_for_execute(&id)
                .get(&key("k"))
                .await
                .unwrap(),
            Some(key("\"v\""))
        );
        first.wipe_now(&id).await.unwrap();
        assert!(matches!(
            second.session_kv_for_execute(&id).get(&key("k")).await,
            Err(SessionKvError::SessionClosed { .. })
        ));
    }

    /// Writers through independent stores cannot both spend the same headroom:
    /// the limit holds for the session's rows, not for what one store has seen.
    #[tokio::test]
    async fn concurrent_writers_cannot_overspend_the_session_limit() {
        let root = tempfile::tempdir().unwrap();
        let store = mem_store();
        let one_entry = interpreter::runtime::entry_bytes(&key("k0"), &key("\"v\""));
        let settings = SessionKvSettings::new(SessionKvLimits {
            max_session_bytes: one_entry * 3,
            ..SessionKvLimits::default()
        });
        let managers: Vec<_> = (0..4)
            .map(|_| {
                SessionManager::new(
                    &test_root(root.path()),
                    Arc::default(),
                    no_http(),
                    store.clone(),
                    CapabilitySettings {
                        session_kv: settings.clone(),
                        ..CapabilitySettings::default()
                    },
                )
            })
            .collect();
        let id = managers[0]
            .create(&none_bp(), Arc::default(), no_secrets())
            .await
            .unwrap();
        let writes = managers.iter().enumerate().flat_map(|(m, mgr)| {
            let id = id.clone();
            (0..2).map(move |n| {
                let kv = mgr.session_kv_for_execute(&id);
                async move { kv.set(&key(&format!("{m}{n}")), &key("\"v\"")).await }
            })
        });
        let outcomes = futures::future::join_all(writes).await;
        let written = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
        assert_eq!(written, 3, "{outcomes:?}");
        assert!(
            outcomes.iter().all(|outcome| matches!(
                outcome,
                Ok(()) | Err(SessionKvError::LimitExceeded { .. })
            ))
        );
        assert_eq!(kv_rows(&managers[0], &id).await, 3);
    }

    #[tokio::test]
    async fn database_failure_prevents_boot_folder_cleanup() {
        let (manager, root) = manager();
        let orphan = root.path().join("sessions/orphan");
        std::fs::create_dir_all(&orphan).unwrap();
        manager.database.close().await.unwrap();
        assert!(manager.boot().await.is_err());
        assert!(orphan.exists());
    }

    /// A workspace recorded under the layout before the volume root, or under
    /// an earlier `volume_dir`, neither blocks boot nor is deleted.
    #[tokio::test]
    async fn boot_completes_cleanup_of_a_folder_outside_the_volume_root() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let root = tempfile::tempdir().expect("volume root");
        let elsewhere = tempfile::tempdir().expect("old layout");
        let folder = elsewhere.path().join("vfs/sessions/old");
        std::fs::create_dir_all(&folder).unwrap();
        let store = file_store(store_dir.path());
        store
            .put(SessionRecord {
                session_id: "old".into(),
                blueprint_name: "p".into(),
                status: SessionStatus::Closed,
                closed_reason: Some(ClosedReason::Deleted),
                root_vfs_type: crate::session_store::RootVfsType::PerSession,
                ..SessionRecord::default()
            })
            .await
            .unwrap();
        store
            .schedule_cleanup(crate::session_store::SessionCleanup {
                session_id: "old".into(),
                folder: Some(folder.clone()),
            })
            .await
            .unwrap();

        let mgr = SessionManager::new(
            &test_root(root.path()),
            Arc::default(),
            no_http(),
            store.clone(),
            CapabilitySettings::default(),
        );
        mgr.boot().await.expect("boot");
        assert!(store.pending_cleanup().await.unwrap().is_empty());
        assert!(folder.exists(), "a folder outside the root is not ours");
    }

    fn ephemeral_bp() -> Blueprint {
        Blueprint {
            name: "e".into(),
            vfs: VfsConfig::Ephemeral {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            ..Default::default()
        }
    }

    /// A manager over a volume root at `root` that was never prepared, so its
    /// directories exist only where a test creates them: as after a lost mount.
    fn lost_mount_manager(root: &Path) -> SessionManager {
        SessionManager::new(
            &VolumeRoot::configured(root),
            Arc::default(),
            no_http(),
            mem_store(),
            CapabilitySettings::default(),
        )
    }

    #[tokio::test]
    async fn a_lost_session_root_fails_the_session_and_is_not_recreated() {
        let root = tempfile::tempdir().unwrap();
        let mgr = lost_mount_manager(root.path());
        mgr.ensure("sid", &per_session(HOUR))
            .await
            .expect_err("a session needs its root");
        assert!(!root.path().join("sessions").exists());
        // With scratch present, it is the session sweep that refuses.
        std::fs::create_dir(root.path().join("ephemeral")).unwrap();
        let err = mgr.boot().await.expect_err("the sweep needs its root");
        assert!(matches!(err, BootError::Sessions(_)), "{err}");
        assert!(!root.path().join("sessions").exists());
    }

    #[tokio::test]
    async fn boot_refuses_a_lost_ephemeral_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sessions")).unwrap();
        let err = lost_mount_manager(root.path())
            .boot()
            .await
            .expect_err("the sweep needs its root");
        assert!(matches!(err, BootError::Scratch { .. }), "{err}");
        assert!(!root.path().join("ephemeral").exists());
    }

    #[tokio::test]
    async fn a_lost_ephemeral_root_fails_the_run_and_is_not_recreated() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sessions")).unwrap();
        let mgr = lost_mount_manager(root.path());
        let bp = ephemeral_bp();
        mgr.ensure("sid", &bp)
            .await
            .expect_err("an ephemeral run needs its root");
        assert!(!root.path().join("ephemeral").exists());

        std::fs::create_dir(root.path().join("ephemeral")).unwrap();
        mgr.ensure("sid", &bp).await.unwrap();
        let (vfs, _) = mgr.vfs_for_execute("sid", &bp).await.unwrap();
        assert!(vfs.root().starts_with(root.path().join("ephemeral")));
    }

    #[tokio::test]
    async fn boot_removes_leftover_scratch_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let ephemeral = root.path().join("ephemeral");
        let leftover = ephemeral.join(format!(
            "{}crashed",
            interpreter::runtime::vfs::TEMPDIR_PREFIX
        ));
        std::fs::create_dir_all(leftover.join("deep")).unwrap();
        std::fs::write(leftover.join("deep/a.txt"), b"x").unwrap();
        let foreign = ephemeral.join("not-scratch");
        std::fs::create_dir(&foreign).unwrap();
        let stray_file =
            ephemeral.join(format!("{}file", interpreter::runtime::vfs::TEMPDIR_PREFIX));
        std::fs::write(&stray_file, b"x").unwrap();
        std::fs::create_dir(root.path().join("sessions")).unwrap();
        // A link with the prefix goes, but what it points at stays.
        #[cfg(unix)]
        let (link, target) = {
            let target = tempfile::tempdir().unwrap();
            std::fs::write(target.path().join("kept.txt"), b"x").unwrap();
            let link = ephemeral.join(format!("{}link", interpreter::runtime::vfs::TEMPDIR_PREFIX));
            std::os::unix::fs::symlink(target.path(), &link).unwrap();
            (link, target)
        };

        lost_mount_manager(root.path()).boot().await.expect("boot");
        assert!(!leftover.exists());
        assert!(!stray_file.exists());
        assert!(foreign.exists());
        #[cfg(unix)]
        {
            assert!(std::fs::symlink_metadata(&link).is_err());
            assert!(target.path().join("kept.txt").exists());
        }
    }
}

#[derive(Default)]
struct ResourceScope {
    work: Arc<tokio::sync::RwLock<()>>,
    cancel: tokio_util::sync::CancellationToken,
}
pub(crate) struct ResourceUse {
    _scope: Arc<ResourceScope>,
    _permit: tokio::sync::OwnedRwLockReadGuard<()>,
    pub cancel: tokio_util::sync::CancellationToken,
}
impl SessionManager {
    fn resource_scope(&self, id: &str) -> Arc<ResourceScope> {
        let mut scopes = self
            .resource_scopes
            .lock()
            .expect("session resource scopes poisoned");
        scopes.retain(|_, scope| scope.strong_count() > 0);
        if let Some(scope) = scopes.get(id).and_then(std::sync::Weak::upgrade) {
            return scope;
        }
        let scope = Arc::new(ResourceScope::default());
        scopes.insert(id.to_owned(), Arc::downgrade(&scope));
        scope
    }
    pub(crate) async fn admit_resource_work(&self, id: &str) -> Result<ResourceUse, SessionError> {
        let scope = self.resource_scope(id);
        let permit = scope.work.clone().read_owned().await;
        self.available(id).await?;
        if scope.cancel.is_cancelled() {
            return Err(SessionError::UnknownSession);
        }
        Ok(ResourceUse {
            _permit: permit,
            cancel: scope.cancel.clone(),
            _scope: scope,
        })
    }
    pub(crate) fn set_finalizer_executor(
        &self,
        executor: Arc<dyn crate::application::sessions::finalization::FinalizerExecutor>,
    ) {
        // Boot installs this once under its initialization lock, before recovery.
        let _ = self.finalizer_executor.set(executor);
    }
    pub(crate) fn finalizer_store(&self) -> crate::adapters::session::finalizers::StoredFinalizers {
        self.finalizers.clone()
    }
    pub(crate) fn finalizer_provider(
        &self,
        id: &str,
        packages: &crate::app::PreparedBlueprintPackages,
    ) -> Result<Arc<dyn interpreter::runtime::session_finalizers::SessionFinalizers>, String> {
        let mut package_versions = std::collections::BTreeMap::new();
        for module in &packages.modules {
            if !module.declaration.values.contains_key("finalizer") {
                continue;
            }
            let required =
                crate::finalizers::dependency_names(packages, &module.metadata.package_name)?;
            let versions: std::collections::BTreeMap<_, _> = packages
                .modules
                .iter()
                .filter(|module| required.contains(&module.metadata.package_name))
                .map(|module| {
                    (
                        module.metadata.package_name.clone(),
                        module.metadata.package_version.clone(),
                    )
                })
                .collect();
            let versions = serde_json::to_string(&versions).map_err(|error| error.to_string())?;
            package_versions.insert(module.metadata.package_name.clone(), versions);
        }
        Ok(Arc::new(
            crate::adapters::session::finalizers::RegistrationProvider {
                store: self.finalizers.clone(),
                session: id.to_owned(),
                package_versions,
                limits: self.session_kv.limits,
            },
        ))
    }
    pub(crate) async fn finalizer_bindings(
        &self,
        id: &str,
    ) -> Result<
        (
            crate::domain::session::SessionBinding,
            Arc<HarnessSecretBindings>,
        ),
        SessionError,
    > {
        let loaded = self
            .repository
            .find(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        let binding = loaded.session().binding().clone();
        let session_id = id.to_owned();
        let id = id.to_owned();
        let sealed: Option<Vec<u8>> = self.database.read(move |connection| Box::pin(async move {
            sqlx::query_scalar("SELECT encrypted_harness_bindings FROM finalizer_context WHERE session_id=?").bind(id).fetch_one(connection).await.map_err(Into::into)
        })).await.map_err(|error| SessionError::Io(error.to_string()))?;
        let credentials = sealed.map_or(
            crate::adapters::session::credentials::SessionCredentials::Absent,
            crate::adapters::session::credentials::SessionCredentials::Encrypted,
        );
        let secrets = self
            .credentials
            .open(&session_id, &credentials)?
            .unwrap_or_default();
        Ok((binding, secrets))
    }
}

impl SessionManager {
    pub async fn close_owned(self: &Arc<Self>, id: &str) -> Result<bool, SessionError> {
        let manager = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move { manager.wipe_now(&id).await })
            .await
            .map_err(|_| SessionError::Io("session cleanup task failed".into()))?
    }
    pub(crate) async fn finalizer_vfs(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        let loaded = self
            .repository
            .find(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        let root = match loaded.session().root() {
            RootVfs::PerSession { path } => path.clone(),
            _ => self.session_root.join(id),
        };
        let vfs = build_vfs(
            blueprint,
            variables,
            Some(&root),
            &self.ephemeral_root,
            &self.volumes,
        )?;
        Ok((
            attach_limits(vfs, blueprint, &self.volumes).await?,
            vfs_info(blueprint),
        ))
    }
}
