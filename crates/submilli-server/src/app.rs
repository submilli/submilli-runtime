use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    middleware,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{MethodRouter, delete, get, post},
};
use interpreter::runtime::{
    EmbeddingProvider, HttpClient, LlmProvider, ReqwestHttpClient, RuntimeConfig, StoreData,
    install_runtime_host_functions,
};
use interpreter::{PackageDeclaration, ScriptImports};
use submilli_blueprint::Blueprint;
use submilli_build::{ArtifactMetadata, PackageStore, PackageStoreError};
use submilli_shared::embedding::{
    BlueprintEmbeddingProvider, EmbeddingDispatch, HttpEmbeddingDispatch,
    HttpEmbeddingDispatchError,
};
use submilli_shared::llm::{
    BlueprintLlmProvider, HttpModelDispatch, HttpModelDispatchError, ModelDispatch,
};
use submilli_shared::secret_store::SecretStore;
use tokio::sync::{Mutex as AsyncMutex, Notify, RwLock as AsyncRwLock};
use wasmtime::{Engine, Linker, Module};

use crate::ServerConfig;
use crate::auth::{Access, AuthConfig, Guard};
use crate::blueprint::BlueprintStore;
#[cfg(test)]
use crate::blueprint::InMemoryBlueprintStore;
use crate::config::{OAuthProvider, VolumeTable};
use crate::graceful_shutdown::GracefulShutdownTracker;
use crate::idempotency::Coordinator;
use crate::idempotency_store::{FileIdempotencyStore, IdempotencyStore, InMemoryIdempotencyStore};
use crate::mcp::{
    BlueprintServiceCache, McpCatalog, discover_all, discover_selected, new_service_cache,
};
use crate::session::{InMemorySessionStore, SessionStore};
use crate::session_manager::{
    CapabilitySettings, DEFAULT_MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS,
    DEFAULT_MAX_ALL_EXECUTIONS_TOKENS, DEFAULT_MAX_CONCURRENCY, DEFAULT_MAX_EMBEDDING_CONCURRENCY,
    DEFAULT_TOTAL_SESSION_KV_BYTES, EmbeddingSettings, HttpClientFactory, LlmSettings,
    SessionKvSettings, SessionManager,
};
use crate::session_store::{
    DurableSessionStore, FileDurableSessionStore, InMemoryDurableSessionStore,
};
use submilli_shared::mcp::discovery::{DiscoveryAuth, DiscoveryError};

#[cfg(test)]
type McpSetup = Arc<dyn Fn() -> std::result::Result<(), DiscoveryError> + Send + Sync>;

use submilli_shared::mcp_token::OAuthTokenManager;

/// How often the background reaper sweeps for expired sessions.
const REAP_INTERVAL: Duration = Duration::from_secs(30);

/// Subdirectory of the session-store directory holding the idempotency ledger.
const IDEMPOTENCY_SUBDIR: &str = "idempotency";

/// Long-lived engine amortises Cranelift init cost across requests.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

type LlmDispatchFactory = Arc<
    dyn Fn(
            Arc<Blueprint>,
            Option<Arc<dyn SecretStore>>,
            Arc<interpreter::runtime::NetworkPolicy>,
        ) -> std::result::Result<HttpModelDispatch, HttpModelDispatchError>
        + Send
        + Sync,
>;

type EmbeddingDispatchFactory = Arc<
    dyn Fn(
            Arc<Blueprint>,
            Option<Arc<dyn SecretStore>>,
            Arc<interpreter::runtime::NetworkPolicy>,
        ) -> std::result::Result<HttpEmbeddingDispatch, HttpEmbeddingDispatchError>
        + Send
        + Sync,
>;

struct AppStateInner {
    boot_lock: AsyncMutex<()>,
    booted: AtomicBool,
    router_ready: AtomicBool,
    graceful_shutdown: Arc<GracefulShutdownTracker>,
    database: Option<Arc<crate::database::ServerDatabase>>,
    audit: crate::audit::AuditLog,
    auth: Arc<AuthConfig>,
    engine: Engine,
    base_linker: Linker<StoreData>,
    runtime: RuntimeConfig,
    sessions: Arc<dyn SessionStore>,
    blueprints: Arc<dyn BlueprintStore>,
    secret_store: Option<Arc<dyn SecretStore>>,
    /// Mints/rotates `@mcp/<server>` OAuth access tokens. `Some` only when a
    /// secret store is configured (OAuth refresh tokens have nowhere to live
    /// otherwise).
    oauth_tokens: Option<Arc<OAuthTokenManager>>,
    session_manager: Arc<SessionManager>,
    session_store: Arc<dyn DurableSessionStore>,
    /// Mediates `Idempotency-Key` reservations against the ledger it owns.
    idempotency: Arc<Coordinator>,
    /// Per-blueprint MCP services, built lazily and evicted on blueprint change.
    /// Poison means a panic may have interrupted service registration or eviction.
    /// AGENTS.md permits poisoned-lock panics rather than reusing partial state;
    /// the panic that caused poisoning is still subject to the no-panic policy.
    mcp_services: BlueprintServiceCache,
    /// `Host` headers the MCP endpoint accepts (DNS-rebinding guard). `None`
    /// leaves rmcp's loopback-only default in place.
    mcp_allowed_hosts: Option<Vec<String>>,
    /// Configured OAuth client apps, matched by authorization-server host.
    mcp_oauth_providers: Arc<Vec<OAuthProvider>>,
    /// Outbound HTTP for the OAuth handlers (discovery + code exchange).
    oauth_http: Arc<dyn HttpClient>,
    /// The outbound-address policy every client the server builds starts from:
    /// script HTTP, model providers, MCP servers, OAuth exchanges.
    network_policy: Arc<interpreter::stdlib::http::NetworkPolicy>,
    /// Per-blueprint discovered `@mcp/<server>` catalogs (the typed import
    /// surface), built lazily on first execute and evicted on blueprint change.
    /// Poison may leave the catalog map and its generation partly updated.
    /// AGENTS.md permits panicking on poisoned access instead of recovering that
    /// state; it does not permit the panic that caused poisoning.
    mcp_catalogs: Mutex<HashMap<String, Arc<McpCatalog>>>,
    mcp_catalog_generation: AtomicU64,
    package_store: PackageStore,
    /// Where package installs read the GitHub token from; read on each
    /// install so a replaced file takes effect without a restart.
    github_token_file: Option<PathBuf>,
    /// Poison may leave the package map and eviction generation inconsistent.
    /// AGENTS.md permits poisoned-lock panics rather than reusing partial state;
    /// the panic that caused poisoning is still subject to the no-panic policy.
    prepared_packages: Mutex<HashMap<String, Arc<PreparedBlueprintPackages>>>,
    /// Bumped by every eviction. A prepare snapshots it before reading the
    /// store and only caches its result if no eviction happened in between:
    /// the artifacts it read may otherwise be the ones the eviction retired.
    prepared_generation: AtomicU64,
    /// Signalled by `POST /v1/shutdown`; awaited by `serve` to drain gracefully.
    shutdown: Arc<Notify>,
    /// Bound address, set by `serve` once the listener is up. Reported by status.
    bind_addr: OnceLock<SocketAddr>,
    /// An embedder-supplied override for the outbound model dispatch, shared by
    /// every blueprint's provider. `None` — the default — means
    /// [`AppState::llm_provider_for`] builds the real per-blueprint HTTP one.
    llm_dispatch: Option<Arc<dyn ModelDispatch>>,
    llm_dispatch_factory: LlmDispatchFactory,
    /// The embedding counterpart of `llm_dispatch`.
    embedding_dispatch: Option<Arc<dyn EmbeddingDispatch>>,
    embedding_dispatch_factory: EmbeddingDispatchFactory,
    #[cfg(test)]
    mcp_setup: McpSetup,
    run_recorder: Option<Arc<dyn crate::record::RunRecorderFactory>>,
    /// Session events for a recorder that wants them.
    event_hub: Option<Arc<crate::record::events::EventHub>>,
    /// Cancellers of the recorded runs in flight, by execution id. Poison means a panic
    /// interrupted a registration; AGENTS.md permits the poisoned-lock panic.
    running: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
    run_telemetry: crate::config::RunTelemetry,
    /// The opaque version tag each registered blueprint carries, by name: set by
    /// [`AppState::apply_local_blueprint`], cleared by every other registration.
    /// Every write to the blueprint store holds this lock for writing across the
    /// write, and a run's lookup holds it for reading across its read, so a run
    /// sees a blueprint and its tag from one registration.
    blueprint_tags: AsyncRwLock<HashMap<String, String>>,
}

/// Removes a run's canceller once the run is over.
pub(crate) struct RunRegistration {
    state: AppState,
    execution_id: String,
}

impl Drop for RunRegistration {
    fn drop(&mut self) {
        if let Ok(mut running) = self.state.inner.running.lock() {
            running.remove(&self.execution_id);
        }
    }
}

impl AppState {
    /// Construct application state with the blueprint store supplied in config.
    /// Store selection and migration belong to server startup.
    pub fn new(config: ServerConfig) -> Result<Self> {
        Self::with_dispatch_factories(
            config,
            Arc::new(HttpModelDispatch::new),
            Arc::new(HttpEmbeddingDispatch::new),
        )
    }

    #[cfg(test)]
    fn with_llm_dispatch_factory(
        config: ServerConfig,
        llm_dispatch_factory: LlmDispatchFactory,
    ) -> Result<Self> {
        Self::with_dispatch_factories(
            config,
            llm_dispatch_factory,
            Arc::new(HttpEmbeddingDispatch::new),
        )
    }

    #[cfg(test)]
    fn with_embedding_dispatch_factory(
        config: ServerConfig,
        embedding_dispatch_factory: EmbeddingDispatchFactory,
    ) -> Result<Self> {
        Self::with_dispatch_factories(
            config,
            Arc::new(HttpModelDispatch::new),
            embedding_dispatch_factory,
        )
    }

    fn with_dispatch_factories(
        config: ServerConfig,
        llm_dispatch_factory: LlmDispatchFactory,
        embedding_dispatch_factory: EmbeddingDispatchFactory,
    ) -> Result<Self> {
        submilli_shared::mcp::schema_registry::initialize_builtin_packs();
        let blueprints = config.blueprints.context(
            "AppState requires a prepared blueprint store; supply ServerConfig.blueprints",
        )?;
        let audit = config
            .audit_log
            .unwrap_or_else(|| crate::audit::AuditLog::new(config.audit, None));
        let runtime = config.runtime;
        let engine = server_engine(&runtime)?;
        if runtime.timeout.is_some_and(|timeout| !timeout.is_zero()) {
            crate::execution_timeout::start_ticker(&engine)?;
        }
        let mut base_linker = Linker::<StoreData>::new(&engine);
        install_runtime_host_functions(&mut base_linker)?;
        // One HTTP client (connection pool) is built per session for isolation —
        // see `SessionManager`. The factory captures the server-wide SSRF policy.
        let policy = Arc::new(config.network_policy);
        let http_client_factory: HttpClientFactory = {
            let policy = Arc::clone(&policy);
            Arc::new(move || {
                Arc::new(ReqwestHttpClient::new(Arc::clone(&policy))) as Arc<dyn HttpClient>
            })
        };
        let sessions = config
            .sessions
            .unwrap_or_else(|| Arc::new(InMemorySessionStore::default()));
        let secret_store = config.secret_store;
        let mcp_oauth_providers = Arc::new(config.mcp_oauth_providers);
        // Dedicated HTTP client (its own pool) for token-endpoint exchanges,
        // governed by the same SSRF policy as script-issued requests.
        let oauth_tokens = secret_store.clone().map(|store| {
            let http = Arc::new(ReqwestHttpClient::new(Arc::clone(&policy))) as Arc<dyn HttpClient>;
            Arc::new(OAuthTokenManager::new(
                store,
                http,
                Arc::clone(&mcp_oauth_providers),
            ))
        });
        // Outbound HTTP for the OAuth handlers (discovery + code exchange), same
        // SSRF policy.
        let oauth_http =
            Arc::new(ReqwestHttpClient::new(Arc::clone(&policy))) as Arc<dyn HttpClient>;

        // Both roots are created lazily on first use (`create_dir_all` makes
        // parents), so construction never touches the filesystem — tests and
        // read-only deploys stay happy.
        let session_root = config
            .session_storage_root
            .unwrap_or_else(crate::config::default_session_storage_root);
        let session_store_dir = config.session_store_dir;
        let session_store: Arc<dyn DurableSessionStore> =
            match (config.session_store, &session_store_dir) {
                (Some(store), _) => store,
                (None, Some(dir)) => Arc::new(FileDurableSessionStore::new(dir.clone())?),
                (None, None) => Arc::new(InMemoryDurableSessionStore::default()),
            };
        // The ledger rides the session store's directory rather than its own
        // knob. `is_record_file` skips subdirectories, so the two never
        // see each other's entries.
        let idempotency_store: Arc<dyn IdempotencyStore> =
            match (config.idempotency_store, &session_store_dir) {
                (Some(store), _) => store,
                (None, Some(dir)) => {
                    Arc::new(FileIdempotencyStore::new(dir.join(IDEMPOTENCY_SUBDIR))?)
                }
                (None, None) => Arc::new(InMemoryIdempotencyStore::default()),
            };
        let session_manager = Arc::new(
            SessionManager::new(
                session_root,
                config.ephemeral_storage_root,
                Arc::new(crate::volumes::VolumeRegistry::new(
                    config.volumes,
                    config
                        .managed_volume_root
                        .unwrap_or_else(crate::config::default_managed_volume_root),
                )),
                http_client_factory,
                Arc::clone(&session_store),
                Arc::clone(&idempotency_store),
                CapabilitySettings {
                    session_kv: SessionKvSettings::new(
                        config.session_kv_limits,
                        config
                            .max_session_state_memory
                            .unwrap_or(DEFAULT_TOTAL_SESSION_KV_BYTES),
                    ),
                    llm: LlmSettings::new(
                        config.llm_limits,
                        config
                            .max_llm_tokens
                            .unwrap_or(DEFAULT_MAX_ALL_EXECUTIONS_TOKENS),
                        config
                            .max_llm_concurrency
                            .unwrap_or(DEFAULT_MAX_CONCURRENCY),
                    ),
                    embedding: EmbeddingSettings::new(
                        config.embedding_limits,
                        config
                            .max_embedding_tokens
                            .unwrap_or(DEFAULT_MAX_ALL_EXECUTIONS_EMBEDDING_TOKENS),
                        config
                            .max_embedding_concurrency
                            .unwrap_or(DEFAULT_MAX_EMBEDDING_CONCURRENCY),
                    ),
                },
            )
            .with_audit(audit.clone()),
        );

        Ok(Self {
            inner: Arc::new(AppStateInner {
                boot_lock: AsyncMutex::new(()),
                booted: AtomicBool::new(false),
                router_ready: AtomicBool::new(false),
                graceful_shutdown: Arc::new(GracefulShutdownTracker::default()),
                database: config.database,
                audit,
                auth: Arc::new(config.auth),
                network_policy: Arc::clone(&policy),
                engine,
                base_linker,
                runtime,
                sessions,
                blueprints,
                secret_store,
                oauth_tokens,
                session_manager,
                session_store,
                idempotency: Arc::new(Coordinator::new(idempotency_store)),
                mcp_services: new_service_cache(),
                mcp_allowed_hosts: config.mcp_allowed_hosts,
                mcp_oauth_providers,
                oauth_http,
                mcp_catalogs: Mutex::new(HashMap::new()),
                mcp_catalog_generation: AtomicU64::new(0),
                package_store: layered_package_store(
                    config.package_store_root,
                    config.package_fallback_root,
                ),
                github_token_file: config.github_token_file,
                prepared_packages: Mutex::new(HashMap::new()),
                prepared_generation: AtomicU64::new(0),
                shutdown: Arc::new(Notify::new()),
                bind_addr: OnceLock::new(),
                llm_dispatch: config.llm_dispatch,
                llm_dispatch_factory,
                embedding_dispatch: config.embedding_dispatch,
                embedding_dispatch_factory,
                #[cfg(test)]
                mcp_setup: Arc::new(|| Ok(())),
                event_hub: config
                    .run_recorder
                    .as_ref()
                    .filter(|factory| factory.wants_events())
                    .map(|factory| crate::record::events::EventHub::new(factory.clone())),
                run_recorder: config.run_recorder,
                running: Mutex::new(HashMap::new()),
                run_telemetry: config.run_telemetry,
                blueprint_tags: AsyncRwLock::new(HashMap::new()),
            }),
        })
    }

    /// Rehydrate persisted sessions and sweep orphan directories. Await once
    /// before serving so reconnects resolve and stale directories are reclaimed.
    pub async fn boot(&self) -> Result<(), crate::session_manager::BootError> {
        if self.inner.booted.load(Ordering::Acquire) {
            return Ok(());
        }
        let _boot_guard = self.inner.boot_lock.lock().await;
        if self.inner.booted.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.session_manager.boot().await?;
        self.inner.session_manager.volume_registry().prepare();
        self.inner.session_manager.spawn_reaper(REAP_INTERVAL);
        self.inner.booted.store(true, Ordering::Release);
        Ok(())
    }

    async fn ready_for_router(&self) -> Result<(), crate::session_manager::BootError> {
        if self.inner.router_ready.load(Ordering::Acquire) {
            return Ok(());
        }
        let _boot_guard = self.inner.boot_lock.lock().await;
        if self.inner.router_ready.load(Ordering::Acquire) {
            return Ok(());
        }
        if !self.inner.booted.load(Ordering::Acquire) {
            self.inner.session_manager.validate_stores().await?;
            self.inner.session_manager.spawn_reaper(REAP_INTERVAL);
        }
        self.inner.router_ready.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn graceful_shutdown(&self) -> Arc<GracefulShutdownTracker> {
        Arc::clone(&self.inner.graceful_shutdown)
    }

    /// Handle `serve` awaits for graceful shutdown; `POST /v1/shutdown` signals it.
    pub fn shutdown_signal(&self) -> Arc<Notify> {
        Arc::clone(&self.inner.shutdown)
    }

    /// Record the bound address once the listener is up (idempotent).
    pub fn set_bind_addr(&self, addr: SocketAddr) {
        let _ = self.inner.bind_addr.set(addr);
    }

    pub(crate) fn bind_addr(&self) -> Option<SocketAddr> {
        self.inner.bind_addr.get().copied()
    }

    pub fn audit(&self) -> &crate::audit::AuditLog {
        &self.inner.audit
    }

    pub(crate) fn run_recorder(&self) -> Option<&Arc<dyn crate::record::RunRecorderFactory>> {
        self.inner.run_recorder.as_ref()
    }

    pub(crate) fn event_hub(&self) -> Option<&Arc<crate::record::events::EventHub>> {
        self.inner.event_hub.as_ref()
    }

    /// Cancels a recorded run in flight, whoever sent it. The run ends with a
    /// `cancelled` outcome once its workers have drained. `false` when no recorded run
    /// with that id is running.
    pub fn cancel_run(&self, execution_id: &str) -> bool {
        let canceller = self
            .inner
            .running
            .lock()
            .expect("running-run registry lock poisoned")
            .remove(execution_id);
        canceller.is_some_and(|canceller| canceller.send(()).is_ok())
    }

    /// Registers a recorded run's canceller until the returned guard drops.
    pub(crate) fn register_run(
        &self,
        execution_id: &str,
    ) -> (RunRegistration, tokio::sync::oneshot::Receiver<()>) {
        let (canceller, cancelled) = tokio::sync::oneshot::channel();
        self.inner
            .running
            .lock()
            .expect("running-run registry lock poisoned")
            .insert(execution_id.to_owned(), canceller);
        (
            RunRegistration {
                state: self.clone(),
                execution_id: execution_id.to_owned(),
            },
            cancelled,
        )
    }

    pub fn database(&self) -> Option<Arc<crate::database::ServerDatabase>> {
        self.inner.database.clone()
    }

    pub(crate) fn auth(&self) -> Arc<AuthConfig> {
        Arc::clone(&self.inner.auth)
    }

    pub(crate) fn run_telemetry(&self) -> crate::config::RunTelemetry {
        self.inner.run_telemetry
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    pub(crate) fn base_linker(&self) -> &Linker<StoreData> {
        &self.inner.base_linker
    }

    pub(crate) fn runtime(&self) -> &RuntimeConfig {
        &self.inner.runtime
    }

    pub(crate) fn sessions(&self) -> &Arc<dyn SessionStore> {
        &self.inner.sessions
    }

    /// The blueprint a run is decided under and its version tag, read together:
    /// a concurrent registration lands wholly before or wholly after this lookup.
    /// The tag is `None` for a blueprint registered without one.
    pub(crate) async fn blueprint_for_run(
        &self,
        name: &str,
    ) -> std::result::Result<Option<(Blueprint, Option<String>)>, crate::blueprint::StoreError>
    {
        let tags = self.inner.blueprint_tags.read().await;
        let found = self.blueprints().get(name).await?;
        Ok(found.map(|blueprint| (blueprint, tags.get(name).cloned())))
    }

    /// Held across a write to the blueprint store, so no run reads the blueprint
    /// and its tag from different registrations. A writer that registers without
    /// a tag removes the name's entry.
    pub(crate) async fn blueprint_tags_for_write(
        &self,
    ) -> tokio::sync::RwLockWriteGuard<'_, HashMap<String, String>> {
        self.inner.blueprint_tags.write().await
    }

    pub(crate) fn blueprints(&self) -> &Arc<dyn BlueprintStore> {
        &self.inner.blueprints
    }

    pub(crate) fn secret_store(&self) -> Option<&Arc<dyn SecretStore>> {
        self.inner.secret_store.as_ref()
    }

    pub(crate) fn network_policy(&self) -> &Arc<interpreter::stdlib::http::NetworkPolicy> {
        &self.inner.network_policy
    }

    /// The OAuth access-token manager for `@mcp/<server>` calls, present when a
    /// secret store is configured. The MCP transport calls into it to attach a
    /// bearer token and to refresh on a mid-call `401`.
    pub fn oauth_token_manager(&self) -> Option<&Arc<OAuthTokenManager>> {
        self.inner.oauth_tokens.as_ref()
    }

    pub(crate) fn session_manager(&self) -> &Arc<SessionManager> {
        &self.inner.session_manager
    }

    /// The outbound `submilli:llm` provider for one blueprint.
    ///
    /// Built per execute rather than cached: it borrows the blueprint, which is
    /// re-read whenever the blueprint changes, and it resolves credentials from
    /// *that* blueprint's `llm:` rows. Both execution routes funnel through here
    /// so a provider cannot reach one and miss the other.
    ///
    /// An embedder-supplied [`ModelDispatch`] on [`ServerConfig`] overrides the
    /// default; absent one, the real HTTP dispatch is built here against this
    /// blueprint and the configured secret store — the same pair
    /// `StreamableHttpTransport` is built from on both routes. That override is
    /// the seam the tests drive, so it stays; what changed is that its absence
    /// now means "use the real one" rather than "there is no provider".
    ///
    /// Always `Some`, therefore. A deployment with no model configured still
    /// gets a catchable error (R12) — it arrives from the blueprint instead, as
    /// the undeclared-model refusal naming the `llm.models:` block to add, which
    /// is the more actionable of the two.
    ///
    /// `harness_secrets` is this session's trusted bindings, and it is a
    /// parameter rather than state for the same reason it is one on the MCP
    /// transport: `harness:` is a valid source for any `${secrets.X}`, including
    /// an `llm.providers.*.api_key`, and the bindings arrive per session. Omit
    /// them and such a key resolves to nothing — which would surface as a
    /// credential failure on a blueprint that is in fact correct.
    pub(crate) fn llm_provider_for(
        &self,
        blueprint: &Arc<Blueprint>,
        harness_secrets: &Arc<submilli_blueprint::HarnessSecretBindings>,
        network_policy: &Arc<interpreter::runtime::NetworkPolicy>,
    ) -> std::result::Result<Option<Arc<dyn LlmProvider>>, HttpModelDispatchError> {
        let dispatch = match self.inner.llm_dispatch.as_ref() {
            Some(installed) => Arc::clone(installed),
            None => Arc::new(
                (self.inner.llm_dispatch_factory)(
                    Arc::clone(blueprint),
                    self.secret_store().cloned(),
                    Arc::clone(network_policy),
                )?
                .with_harness_secrets(Arc::clone(harness_secrets)),
            ) as Arc<dyn ModelDispatch>,
        };
        Ok(Some(Arc::new(
            BlueprintLlmProvider::new(Arc::clone(blueprint), dispatch)
                .with_max_concurrency(self.inner.session_manager.llm_max_concurrency()),
        )))
    }

    /// The outbound `submilli:embedding` provider for one blueprint, or `None`
    /// when the blueprint declares no embedding aliases (the module is then not
    /// offered, so nothing would reach it). Built per execute for the same
    /// reasons as [`Self::llm_provider_for`], and overridable the same way.
    pub(crate) fn embedding_provider_for(
        &self,
        blueprint: &Arc<Blueprint>,
        harness_secrets: &Arc<submilli_blueprint::HarnessSecretBindings>,
        network_policy: &Arc<interpreter::runtime::NetworkPolicy>,
    ) -> std::result::Result<Option<Arc<dyn EmbeddingProvider>>, HttpEmbeddingDispatchError> {
        if blueprint.embedding.models.is_empty() {
            return Ok(None);
        }
        let dispatch = match self.inner.embedding_dispatch.as_ref() {
            Some(installed) => Arc::clone(installed),
            None => Arc::new(
                (self.inner.embedding_dispatch_factory)(
                    Arc::clone(blueprint),
                    self.secret_store().cloned(),
                    Arc::clone(network_policy),
                )?
                .with_harness_secrets(Arc::clone(harness_secrets)),
            ) as Arc<dyn EmbeddingDispatch>,
        };
        Ok(Some(Arc::new(BlueprintEmbeddingProvider::new(
            blueprint,
            dispatch,
            self.inner.session_manager.embedding_max_concurrency(),
        ))))
    }

    /// The operator-declared volume table, read from the session manager so
    /// the listing endpoint and mount-time resolution share one source.
    pub(crate) fn volumes(&self) -> VolumeTable {
        self.inner.session_manager.volumes()
    }

    pub(crate) fn session_store(&self) -> &Arc<dyn DurableSessionStore> {
        &self.inner.session_store
    }

    pub(crate) fn idempotency(&self) -> &Arc<Coordinator> {
        &self.inner.idempotency
    }

    pub(crate) fn mcp_services(&self) -> &BlueprintServiceCache {
        &self.inner.mcp_services
    }

    pub(crate) fn mcp_allowed_hosts(&self) -> Option<&[String]> {
        self.inner.mcp_allowed_hosts.as_deref()
    }

    pub(crate) fn mcp_oauth_providers(&self) -> &[OAuthProvider] {
        &self.inner.mcp_oauth_providers
    }

    pub(crate) fn oauth_http(&self) -> &Arc<dyn HttpClient> {
        &self.inner.oauth_http
    }

    /// The auth inputs MCP discovery needs, drawn from this server's state.
    fn discovery_auth<'a>(
        &'a self,
        harness_secrets: Option<&'a Arc<submilli_blueprint::HarnessSecretBindings>>,
    ) -> DiscoveryAuth<'a> {
        DiscoveryAuth {
            secret_store: self.secret_store(),
            oauth: self.oauth_token_manager(),
            harness_secrets,
            network_policy: self.network_policy(),
        }
    }

    /// The `@mcp/<server>` catalog for a blueprint, discovered (and cached) on
    /// first use. A blueprint with no `mcp:` block skips discovery entirely.
    pub(crate) async fn mcp_catalog(
        &self,
        blueprint_name: &str,
        blueprint: &Blueprint,
    ) -> std::result::Result<Arc<McpCatalog>, DiscoveryError> {
        // Exercise discovery-error handling without a network dependency in tests.
        #[cfg(test)]
        (self.inner.mcp_setup)()?;
        if blueprint.mcp.is_empty() {
            return Ok(Arc::new(McpCatalog::empty()));
        }
        let generation = self.inner.mcp_catalog_generation.load(Ordering::Acquire);
        let key = mcp_catalog_cache_key(blueprint_name, None);
        if let Some(cached) = self.cached_mcp_catalog(&key) {
            return Ok(cached);
        }
        // Discovery does network I/O, so it runs without the cache lock held; a
        // concurrent first-caller may also discover — the first to insert wins.
        let discovered =
            Arc::new(discover_all(self.discovery_auth(None), blueprint_name, blueprint).await?);
        let mut cache = self
            .inner
            .mcp_catalogs
            .lock()
            .expect("mcp catalog poisoned");
        if self.inner.mcp_catalog_generation.load(Ordering::Acquire) != generation {
            return Ok(discovered);
        }
        Ok(Arc::clone(cache.entry(key).or_insert(discovered)))
    }

    fn cached_mcp_catalog(&self, key: &str) -> Option<Arc<McpCatalog>> {
        self.inner
            .mcp_catalogs
            .lock()
            .expect("mcp catalog poisoned")
            .get(key)
            .map(Arc::clone)
    }

    pub(crate) async fn mcp_catalog_for_imports(
        &self,
        blueprint_name: &str,
        blueprint: &Blueprint,
        servers: &BTreeSet<String>,
        harness_secrets: &Arc<submilli_blueprint::HarnessSecretBindings>,
        network_policy: &Arc<interpreter::runtime::NetworkPolicy>,
    ) -> std::result::Result<Arc<McpCatalog>, DiscoveryError> {
        // Exercise discovery-error handling without a network dependency in tests.
        #[cfg(test)]
        (self.inner.mcp_setup)()?;
        if servers.is_empty() || blueprint.mcp.is_empty() {
            return Ok(Arc::new(McpCatalog::empty()));
        }
        let declared: BTreeSet<String> = servers
            .iter()
            .filter(|server| blueprint.mcp.contains_key(*server))
            .cloned()
            .collect();
        if declared.is_empty() {
            return Ok(Arc::new(McpCatalog::empty()));
        }
        let generation = self.inner.mcp_catalog_generation.load(Ordering::Acquire);
        let key = mcp_catalog_cache_key(blueprint_name, Some(&declared));
        let session_scoped = !harness_secrets.is_empty();
        if !session_scoped && let Some(cached) = self.cached_mcp_catalog(&key) {
            return Ok(cached);
        }
        let discovered = Arc::new(
            discover_selected(
                DiscoveryAuth {
                    network_policy,
                    ..self.discovery_auth(Some(harness_secrets))
                },
                blueprint_name,
                blueprint,
                &declared,
            )
            .await?,
        );
        if session_scoped {
            return Ok(discovered);
        }
        let mut cache = self
            .inner
            .mcp_catalogs
            .lock()
            .expect("mcp catalog poisoned");
        if self.inner.mcp_catalog_generation.load(Ordering::Acquire) != generation {
            return Ok(discovered);
        }
        Ok(Arc::clone(cache.entry(key).or_insert(discovered)))
    }

    pub(crate) fn package_store(&self) -> &PackageStore {
        &self.inner.package_store
    }

    pub(crate) fn github_token_file(&self) -> Option<&Path> {
        self.inner.github_token_file.as_deref()
    }

    fn cached_prepared_packages(&self, key: &str) -> Option<Arc<PreparedBlueprintPackages>> {
        self.inner
            .prepared_packages
            .lock()
            .expect("prepared package cache poisoned")
            .get(key)
            .map(Arc::clone)
    }

    pub(crate) fn prepared_packages_for_imports(
        &self,
        blueprint_name: &str,
        blueprint: &Blueprint,
        imports: &ScriptImports,
    ) -> std::result::Result<Arc<PreparedBlueprintPackages>, PreparePackagesError> {
        let registry_roots: BTreeSet<String> = imports
            .registry_packages
            .iter()
            .filter(|package| blueprint.packages.contains(*package))
            .cloned()
            .collect();
        if registry_roots.is_empty() && imports.stdlib.is_empty() {
            return Ok(Arc::new(PreparedBlueprintPackages::default()));
        }
        let key = format!(
            "{}:git={}",
            prepared_packages_cache_key(blueprint_name, &registry_roots, &imports.stdlib),
            blueprint.git.is_some()
        );
        if let Some(cached) = self.cached_prepared_packages(&key) {
            return Ok(cached);
        }
        let generation = self.inner.prepared_generation.load(Ordering::Acquire);
        let mut selected = self.prepare_selected_packages(&registry_roots, &imports.stdlib)?;
        if blueprint.git.is_none() {
            selected
                .stdlib_declarations
                .retain(|decl| decl.package_name != "submilli:git");
        }
        let prepared = Arc::new(selected);
        let mut cache = self
            .inner
            .prepared_packages
            .lock()
            .expect("prepared package cache poisoned");
        if self.inner.prepared_generation.load(Ordering::Acquire) != generation {
            // An eviction (install, uninstall, or a blueprint change) landed
            // while the store was being read; serve this set once and let the
            // next call rebuild from disk.
            return Ok(prepared);
        }
        Ok(Arc::clone(cache.entry(key).or_insert(prepared)))
    }

    fn prepare_selected_packages(
        &self,
        registry_roots: &BTreeSet<String>,
        stdlib_names: &BTreeSet<String>,
    ) -> std::result::Result<PreparedBlueprintPackages, PreparePackagesError> {
        let stdlib_declarations = selected_stdlib_declarations(stdlib_names);
        let mut script_declarations = Vec::with_capacity(registry_roots.len());
        let artifacts = self
            .package_store()
            .load_closure(registry_roots.iter().map(String::as_str))?;
        let mut modules = Vec::with_capacity(artifacts.len());
        for artifact in artifacts {
            let name = artifact.metadata.package_name.clone();
            let module = package_module(&self.inner.engine, &artifact.wasm).map_err(|source| {
                // The artifact came from whichever root holds it, so name that
                // directory rather than the owned root's would-be path.
                let package_dir = self
                    .package_store()
                    .locate(&name)
                    .ok()
                    .flatten()
                    .map(|located| located.dir);
                PreparePackagesError::Module {
                    name: name.clone(),
                    package_dir,
                    source,
                }
            })?;
            if registry_roots.contains(&name) {
                script_declarations.push(artifact.package_declaration.clone());
            }
            modules.push(PreparedPackageModule {
                module,
                declaration: artifact.package_declaration,
                type_info: artifact.type_info,
                metadata: artifact.metadata,
                sources: artifact.sources,
            });
        }
        Ok(PreparedBlueprintPackages {
            stdlib_declarations,
            script_declarations,
            modules,
        })
    }

    /// Drop the cached MCP service for a blueprint, taking its live `MCP-Session-Id`
    /// map down with it. Only blueprint *removal* does this — a blueprint update
    /// keeps the service so existing sessions stay connected (the execute path
    /// re-fetches the blueprint per call, so they pick up the new config).
    pub(crate) fn evict_mcp_service(&self, name: &str) {
        self.inner
            .mcp_services
            .lock()
            .expect("mcp service cache poisoned")
            .remove(name);
    }

    /// Drop the discovered `@mcp/<server>` catalog so the next execute rediscovers
    /// it against the current `mcp:` block. Called on blueprint update and removal,
    /// including [`Self::apply_local_blueprint`].
    pub fn evict_mcp_catalog(&self, name: &str) {
        let mut catalogs = self
            .inner
            .mcp_catalogs
            .lock()
            .expect("mcp catalog poisoned");
        self.inner
            .mcp_catalog_generation
            .fetch_add(1, Ordering::AcqRel);
        catalogs.retain(|key, _| !cache_key_belongs_to_blueprint(key, name));
    }

    /// Drop every cached module set. An install can change what any blueprint
    /// resolves — a new transitive dependency, or an owned copy now shadowing a
    /// fallback one — and no cache key records which files a set came from, so
    /// the only sound eviction after an install is a full one.
    pub fn evict_all_prepared_packages(&self) {
        let mut cache = self
            .inner
            .prepared_packages
            .lock()
            .expect("prepared package cache poisoned");
        // Under the lock, so a prepare that checks the generation while
        // holding it sees the bump and the cleared map together.
        self.inner
            .prepared_generation
            .fetch_add(1, Ordering::AcqRel);
        cache.clear();
    }

    /// Drop the prepared package modules cached for blueprint `name`, so the next
    /// run loads them from the package store again: after a blueprint change, or
    /// once an embedder has rebuilt a package that blueprint uses.
    pub fn evict_prepared_packages(&self, name: &str) {
        let mut cache = self
            .inner
            .prepared_packages
            .lock()
            .expect("prepared package cache poisoned");
        self.inner
            .prepared_generation
            .fetch_add(1, Ordering::AcqRel);
        cache.retain(|key, _| !cache_key_belongs_to_blueprint(key, name));
    }

    pub(crate) async fn wipe_blueprint_sessions(&self, name: &str) {
        self.inner.session_manager.wipe_blueprint(name).await;
    }
}

fn package_module(engine: &Engine, wasm: &[u8]) -> wasmtime::Result<Module> {
    Module::new(engine, wasm)
}

/// The server runs every guest on the default on-demand allocator, not the
/// pooling allocator. Pooling backs each linear memory and GC heap with a
/// fixed-size slot it can neither grow nor move beyond, so its slot size must be
/// baked into the engine — forcing one global memory ceiling across all tenants
/// and pre-reserving it per slot regardless of load. It also can't honour the
/// growable, movable GC heap our `RuntimeConfig` asks for: a guest heap told it
/// may grow/move walks past its fixed slot during collection and corrupts the
/// host (SUB-555). On-demand allocation is per-store: each run reserves only what
/// it uses and is bounded by its own `TenantLimits`, which lets callers set a
/// different cap per tenant. The instantiation-setup cost pooling would amortise
/// is negligible next to an agent script's multi-second runtime.
fn server_engine(runtime: &RuntimeConfig) -> wasmtime::Result<Engine> {
    Engine::new(&runtime.wasmtime_config())
}

#[derive(Clone, Debug, Default)]
pub struct PreparedBlueprintPackages {
    pub stdlib_declarations: Vec<PackageDeclaration>,
    /// Declarations of the blueprint-listed packages — the script's importable
    /// surface. Closure-only dependencies are linked but not importable.
    pub script_declarations: Vec<PackageDeclaration>,
    /// The full dependency closure in instantiation (topological) order.
    pub modules: Vec<PreparedPackageModule>,
}

#[derive(Clone, Debug)]
pub struct PreparedPackageModule {
    pub module: Module,
    pub declaration: PackageDeclaration,
    pub type_info: interpreter::TypeInfoTable,
    pub metadata: ArtifactMetadata,
    pub sources: Vec<submilli_build::ArtifactSource>,
}

#[derive(Debug)]
pub enum PreparePackagesError {
    Store(PackageStoreError),
    Module {
        name: String,
        /// Where the artifact was read from, when the store could still say.
        package_dir: Option<PathBuf>,
        source: wasmtime::Error,
    },
}

impl fmt::Display for PreparePackagesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreparePackagesError::Store(err) => err.fmt(f),
            PreparePackagesError::Module {
                name,
                package_dir: Some(package_dir),
                source,
            } => write!(
                f,
                "failed to compile package `{name}` wasm from {}: {source}",
                package_dir.display()
            ),
            PreparePackagesError::Module {
                name,
                package_dir: None,
                source,
            } => write!(f, "failed to compile package `{name}` wasm: {source}"),
        }
    }
}

impl std::error::Error for PreparePackagesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PreparePackagesError::Store(err) => Some(err),
            PreparePackagesError::Module { .. } => None,
        }
    }
}

impl From<PackageStoreError> for PreparePackagesError {
    fn from(value: PackageStoreError) -> Self {
        Self::Store(value)
    }
}

/// The server's package store: the owned root (defaulted when unset) layered
/// over the optional read-only fallback.
fn layered_package_store(root: Option<PathBuf>, fallback: Option<PathBuf>) -> PackageStore {
    let store = PackageStore::new(root.unwrap_or_else(crate::config::default_package_store_dir));
    match fallback {
        Some(fallback) => store.with_fallback(fallback),
        None => store,
    }
}

pub fn app(state: AppState) -> Router {
    let boot_state = state.clone();
    let graceful_shutdown = state.graceful_shutdown();
    routes(state.auth(), state.audit().clone())
        .router
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            boot_state,
            ensure_router_ready,
        ))
        .layer(middleware::from_fn_with_state(
            graceful_shutdown,
            crate::graceful_shutdown::run,
        ))
}

async fn ensure_router_ready(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(error) = state.ready_for_router().await {
        tracing::error!(%error, source = ?std::error::Error::source(&error), "server boot failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "server startup failed").into_response();
    }
    next.run(request).await
}

/// Every route and the access it requires, in registration order.
pub fn route_table() -> Vec<(&'static str, Access)> {
    routes(
        Arc::new(AuthConfig::Disabled),
        crate::audit::AuditLog::new(
            crate::audit::AuditConfig {
                enabled: false,
                ..Default::default()
            },
            None,
        ),
    )
    .table
}

fn routes(auth: Arc<AuthConfig>, audit: crate::audit::AuditLog) -> Routes {
    use crate::handlers::{
        admin, blueprint, capabilities, execute, last_run, mcp_auth, packages, secret, sessions,
        volumes,
    };

    Routes::new(auth, audit)
        .route("/healthz", Access::Public, get(admin::healthz))
        .route("/v1/status", Access::Admin, get(admin::status))
        .route("/v1/shutdown", Access::Admin, post(admin::shutdown))
        .route("/v1/execute", Access::User, post(execute::handle))
        .route("/v1/sessions", Access::User, post(sessions::create))
        .route(
            "/v1/sessions/{session_id}/execute",
            Access::User,
            post(sessions::execute),
        )
        .route(
            "/v1/sessions/{session_id}/rebind",
            Access::User,
            post(sessions::rebind),
        )
        .route(
            "/v1/sessions/{session_id}/last-run",
            Access::User,
            get(last_run::handle),
        )
        .route(
            "/v1/sessions/{session_id}",
            Access::User,
            delete(sessions::disconnect),
        )
        .route(
            "/v1/blueprints",
            Access::Admin,
            post(blueprint::add).get(blueprint::list),
        )
        .route(
            "/v1/blueprints/{name}",
            Access::Admin,
            get(blueprint::show)
                .put(blueprint::apply)
                .delete(blueprint::remove),
        )
        // This and the per-blueprint `packages/*` and `builtins*` reads further
        // down are what an agent calls to learn what its blueprint offers, so
        // they need no more than the token that runs code against it.
        .route(
            "/v1/blueprints/{name}/prompt",
            Access::User,
            get(blueprint::prompt),
        )
        .route(
            "/v1/secrets",
            Access::Admin,
            post(secret::put).get(secret::list),
        )
        // Write-only externally: secrets can be stored, listed, and deleted, but
        // never read back over the API. The runtime reads values in-process.
        .route("/v1/secrets/{*key}", Access::Admin, delete(secret::remove))
        .route("/v1/packages", Access::Admin, get(packages::installed))
        .route(
            "/v1/packages/{*name}",
            Access::Admin,
            delete(packages::uninstall),
        )
        .route(
            "/v1/packages/install",
            Access::Admin,
            post(packages::install),
        )
        .route(
            "/v1/blueprints/{name}/packages/search",
            Access::User,
            get(packages::blueprint_search),
        )
        .route(
            "/v1/blueprints/{name}/packages/docs",
            Access::User,
            get(packages::blueprint_docs),
        )
        .route(
            "/v1/blueprints/{name}/builtins",
            Access::User,
            get(packages::blueprint_builtins),
        )
        .route(
            "/v1/blueprints/{name}/builtins/docs",
            Access::User,
            get(packages::blueprint_builtin_docs),
        )
        .route("/v1/capabilities", Access::Admin, get(capabilities::list))
        // Read-only, and names only: the host directory behind a volume name
        // never crosses this boundary.
        .route("/v1/volumes", Access::Admin, get(volumes::list))
        .route(
            "/v1/mcp/{blueprint}/auth-status",
            Access::Admin,
            get(mcp_auth::auth_status),
        )
        .route(
            "/v1/mcp/{blueprint}/{server}/auth-config",
            Access::Admin,
            get(mcp_auth::auth_config),
        )
        .route(
            "/v1/mcp/{blueprint}/{server}/refresh-token",
            Access::Admin,
            post(mcp_auth::put_refresh_token).delete(mcp_auth::delete_refresh_token),
        )
        .route(
            "/v1/mcp/{blueprint}/{server}/oauth/exchange",
            Access::Admin,
            post(mcp_auth::oauth_exchange),
        )
        .route(
            "/mcp/{blueprint}",
            Access::User,
            post(crate::mcp::mcp_handler)
                .get(crate::mcp::mcp_handler)
                .delete(crate::mcp::mcp_handler),
        )
}

/// The router under construction. Its only way to add a route takes the
/// [`Access`] that route requires, so an endpoint cannot be registered without
/// deciding who may call it.
struct Routes {
    router: Router<AppState>,
    auth: Arc<AuthConfig>,
    audit: crate::audit::AuditLog,
    table: Vec<(&'static str, Access)>,
}

impl Routes {
    fn new(auth: Arc<AuthConfig>, audit: crate::audit::AuditLog) -> Self {
        Self {
            router: Router::new(),
            auth,
            audit,
            table: Vec::new(),
        }
    }

    fn route(
        mut self,
        path: &'static str,
        access: Access,
        handlers: MethodRouter<AppState>,
    ) -> Self {
        let handlers = match access {
            Access::Public => handlers,
            // Layered on the method router rather than each handler, so a
            // request with a method the path does not serve is refused for its
            // missing token before it learns which methods exist.
            Access::User | Access::Admin => handlers.layer(middleware::from_fn_with_state(
                Guard::new(self.auth.clone(), access, self.audit.clone()),
                crate::auth::require,
            )),
        };
        self.router = self.router.route(path, handlers);
        self.table.push((path, access));
        self
    }
}

fn selected_stdlib_declarations(names: &BTreeSet<String>) -> Vec<PackageDeclaration> {
    interpreter::runtime::stdlib_package_declarations()
        .into_iter()
        .filter(|defs| names.contains(&defs.package_name))
        .collect()
}

fn mcp_catalog_cache_key(blueprint_name: &str, servers: Option<&BTreeSet<String>>) -> String {
    match servers {
        Some(servers) => format!("mcp:{blueprint_name}:{}", join_key_parts(servers)),
        None => format!("mcp:{blueprint_name}:*"),
    }
}

fn prepared_packages_cache_key(
    blueprint_name: &str,
    registry_roots: &BTreeSet<String>,
    stdlib_names: &BTreeSet<String>,
) -> String {
    format!(
        "pkg:{blueprint_name}:{}:{}",
        join_key_parts(registry_roots),
        join_key_parts(stdlib_names)
    )
}

fn join_key_parts(parts: &BTreeSet<String>) -> String {
    parts.iter().cloned().collect::<Vec<_>>().join("\u{1f}")
}

fn cache_key_belongs_to_blueprint(key: &str, blueprint_name: &str) -> bool {
    key.starts_with(&format!("mcp:{blueprint_name}:"))
        || key.starts_with(&format!("pkg:{blueprint_name}:"))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn oauth_deposit_invalidates_catalog_discovered_before_login() {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            submilli_shared::secret_store::PlaintextFileSecretStore::open(directory.path().into())
                .unwrap(),
        );
        let blueprint = submilli_blueprint::parse("name: test\nmcp:\n  local:\n    url: http://127.0.0.1:1/mcp\n    auth:\n      type: oauth2\n").unwrap();
        let state = AppState::new(ServerConfig {
            secret_store: Some(store),
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed([blueprint.clone()]).expect("seed blueprints"),
            )),
            ..crate::config::test_config()
        })
        .unwrap();
        let before = state.mcp_catalog("test", &blueprint).await.unwrap();
        assert!(
            before
                .warnings()
                .any(|warning| warning.message.contains("not authenticated"))
        );
        let credential =
            serde_json::from_value(serde_json::json!({"access_token": "test-token"})).unwrap();
        let _ = crate::handlers::mcp_auth::put_refresh_token(
            axum::extract::State(state.clone()),
            axum::extract::Path(("test".into(), "local".into())),
            axum::Json(credential),
        )
        .await
        .unwrap();
        let after = state.mcp_catalog("test", &blueprint).await.unwrap();
        assert!(
            !Arc::ptr_eq(&before, &after),
            "catalog must be rediscovered after authentication"
        );
        assert!(
            !after
                .warnings()
                .any(|warning| warning.message.contains("not authenticated"))
        );
        let manager = state.oauth_token_manager().unwrap();
        assert_eq!(
            manager.access_token("test", "local").await.unwrap(),
            "test-token"
        );
        let credential =
            serde_json::from_value(serde_json::json!({"access_token": "replacement-token"}))
                .unwrap();
        let _ = crate::handlers::mcp_auth::put_refresh_token(
            axum::extract::State(state.clone()),
            axum::extract::Path(("test".into(), "local".into())),
            axum::Json(credential),
        )
        .await
        .unwrap();
        assert_eq!(
            manager.access_token("test", "local").await.unwrap(),
            "replacement-token"
        );
    }

    use super::*;
    use interpreter::FileId;
    use interpreter::runtime::{
        Vfs, dispatch_main_async, install_runtime_async, install_tenant_limits,
    };

    #[test]
    fn app_state_requires_a_prepared_blueprint_store() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("blueprints");
        let result = AppState::new(ServerConfig {
            blueprint_dir: Some(source.clone()),
            ..Default::default()
        });
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("prepared blueprint store")
        );
        assert!(!source.exists());
    }

    #[tokio::test]
    async fn failed_boot_does_not_start_the_reaper() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("sessions");
        let store = Arc::new(FileDurableSessionStore::new(root.clone()).expect("session store"));
        let state = AppState::new(ServerConfig {
            session_store: Some(store),
            ..crate::config::test_config()
        })
        .expect("app state");
        std::fs::remove_dir(&root).expect("make store unavailable");

        assert!(matches!(
            state.boot().await,
            Err(crate::session_manager::BootError::Sessions(_))
        ));
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 1);

        std::fs::create_dir(&root).expect("restore store");
        state.boot().await.expect("healthy boot");
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 2);
        state.boot().await.expect("repeat boot");
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 2);
    }

    #[tokio::test]
    async fn direct_router_still_starts_the_reaper() {
        use tower::ServiceExt;

        let state = AppState::new(crate::config::test_config()).expect("app state");
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 1);

        let router = app(state.clone());
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 1);
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/healthz")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 2);
    }

    #[tokio::test]
    async fn direct_router_refuses_requests_until_boot_succeeds() {
        use tower::ServiceExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("sessions");
        let store = Arc::new(FileDurableSessionStore::new(root.clone()).expect("session store"));
        let state = AppState::new(ServerConfig {
            session_store: Some(store),
            ..crate::config::test_config()
        })
        .expect("app state");
        let router = app(state.clone());
        std::fs::remove_dir(&root).expect("make store unavailable");

        let request = || {
            axum::http::Request::builder()
                .uri("/healthz")
                .body(axum::body::Body::empty())
                .expect("request")
        };
        let response = router.clone().oneshot(request()).await.expect("response");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 1);

        std::fs::create_dir(&root).expect("restore store");
        let response = router.oneshot(request()).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(Arc::strong_count(&state.inner.session_manager), 2);
    }

    /// Compile `src`, run it through the server engine, and return the dispatch
    /// result. A high fuel budget keeps the allocation-heavy guests off the fuel
    /// meter so the GC heap / tenant cap is what bounds them.
    async fn run_on_server_engine(src: &str) -> wasmtime::Result<Option<String>> {
        let compiled =
            interpreter::compile_script(src, "<test>", FileId(0), &[], &[]).expect("compiles");
        let runtime = RuntimeConfig {
            fuel: 50_000_000_000,
            ..RuntimeConfig::default()
        };
        let engine = server_engine(&runtime).expect("server engine");
        let module = Module::new(&engine, &compiled.wasm).expect("module");
        let data = StoreData::with_vfs(Vfs::tempdir().unwrap());
        let mut store = runtime.store_async(&engine, data).expect("store");
        install_tenant_limits(&mut store);
        let mut linker = Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let inst = linker.instantiate_async(&mut store, &module).await.unwrap();
        dispatch_main_async(&mut store, &inst).await
    }

    /// The ledger has no operator knob of its own: it is derived from the
    /// session-store directory, and an explicitly injected store wins over it.
    #[tokio::test]
    async fn session_store_dir_derives_a_file_backed_ledger() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _state = AppState::new(ServerConfig {
            session_store_dir: Some(dir.path().to_path_buf()),
            ..crate::config::test_config()
        })
        .expect("app state");

        assert!(
            dir.path().join(IDEMPOTENCY_SUBDIR).is_dir(),
            "the ledger lands in a subdirectory of the session store dir"
        );
    }

    #[tokio::test]
    async fn an_explicit_ledger_wins_over_the_derived_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _state = AppState::new(ServerConfig {
            session_store_dir: Some(dir.path().to_path_buf()),
            idempotency_store: Some(Arc::new(InMemoryIdempotencyStore::default())),
            ..crate::config::test_config()
        })
        .expect("app state");

        assert!(
            !dir.path().join(IDEMPOTENCY_SUBDIR).exists(),
            "an explicit store must suppress the derived directory entirely"
        );
    }

    // A guest that asks for more memory than its `TenantLimits` cap must trap
    // *catchably* — the test reaching this assertion at all proves the host did
    // not panic or abort the way the corrupting pooled heap did.
    #[tokio::test]
    async fn server_guest_over_tenant_cap_traps_cleanly() {
        // ~120 MB single GC array, far past the 50 MB tenant cap.
        let out = run_on_server_engine(
            r#"function main(): number {
                 const s: string = "x".repeat(60000000);
                 return s.length;
               }"#,
        )
        .await;
        assert!(
            out.is_err(),
            "over-cap allocation must trap, not succeed: {out:?}",
        );
    }
}

#[cfg(test)]
mod embedding_setup_tests;

#[cfg(test)]
mod llm_setup_tests;

#[cfg(test)]
mod mcp_setup_tests;

#[cfg(test)]
mod blueprint_mutation_tests;

#[cfg(test)]
mod local_apply_tests;
