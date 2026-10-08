//! The running playground: the embedded server and the control listener, both on
//! loopback, in this process.
//!
//! The server listener is `submilli-server` itself under the playground's tokens.
//! The control listener serves the page and the control API; the routes later
//! steps add go into [`ControlRoutes`].

use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::header::{CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use submilli_server::audit::AuditConfig;
use submilli_server::{AppState, AuthConfig, RunTelemetry, ServerConfig};
use tokio::sync::Notify;

use super::control_auth::{self, Caller, ControlAuth, LoginRefusal, Now};
use super::log::warn;
use super::packages::{self, ClosureEntry, Freshness, ProjectPackages};
use super::project::Project;
use super::state::{APP_TOKEN, InstanceLock, InstanceRecord, StateDir};
use super::store::redact::WatchedSecretStore;
use super::store::{KnownSecrets, Recorder, Store};
use super::watch::{self, Applier, BlueprintStatus};
use super::{Egress, ReadyRecord};

/// How long in-flight requests may finish after a stop before they are dropped.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long the control listener may take to close once the server has drained.
const CONTROL_CLOSE_BUDGET: Duration = Duration::from_secs(2);

pub(crate) struct HostOptions {
    pub(crate) project: Project,
    pub(crate) egress: Egress,
    pub(crate) nonce: String,
    /// Held until the instance is ready, so a concurrent start waits and attaches.
    pub(crate) start_lock: Option<File>,
    /// Print the ready record once serving, as `--foreground` does; a detached
    /// child's output goes to its log, and the parent prints the record instead.
    pub(crate) print: Option<super::Output>,
}

/// What the control listener reports about this instance. Built once at start.
/// Every field defaults when absent, so a CLI reads an instance of another
/// version rather than failing on a field one of them lacks.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Description {
    pub(crate) project: PathBuf,
    pub(crate) blueprint: BlueprintInfo,
    pub(crate) url: String,
    pub(crate) server_url: String,
    pub(crate) pid: u32,
    pub(crate) app_token_file: PathBuf,
    pub(crate) state_dir: PathBuf,
    pub(crate) log_file: PathBuf,
    pub(crate) egress_grants: Vec<String>,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    /// The blueprint's package closure at start: each package's origin and whether a
    /// program may import it. `status` reads it again for the blueprint in force.
    pub(crate) packages: Vec<ClosureEntry>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BlueprintInfo {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

/// What `GET /api/status` answers: the description, with the closure of the
/// blueprint in force now, and what changes while the instance runs. Defaults
/// like [`Description`].
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Status {
    #[serde(flatten)]
    pub(crate) description: Description,
    /// Serving, and not draining.
    pub(crate) running: bool,
    /// Draining after a stop or a signal; it exits once its runs end.
    pub(crate) stopping: bool,
    pub(crate) browser_sessions: usize,
    pub(crate) blueprint_status: BlueprintStatus,
    /// Why the closure of the blueprint in force could not be read; `packages` is
    /// then the closure at start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) packages_error: Option<String>,
    /// Whether the server listener answers, which `status` adds after asking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) health: Option<String>,
}

/// Test-only, hidden: how long a serving process waits after taking the instance
/// lock, in milliseconds, so a test can act on an instance that is starting. Unset
/// outside tests; capped at [`START_DELAY_CAP`] so a stray value cannot wedge a
/// start.
const START_DELAY_ENV: &str = "SUBMILLI_PLAYGROUND_TEST_START_DELAY_MS";
const START_DELAY_CAP: Duration = Duration::from_secs(60);

/// How serving ended, when it ended without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Served {
    /// It announced itself, served, and drained.
    Drained,
    /// It was stopped before it announced itself, by `signal` when one did it.
    StoppedWhileStarting { signal: Option<libc::c_int> },
}

/// Serve until a stop, a signal, or `POST /v1/shutdown`, then drain and remove
/// the instance record and the ready file. Refused while another process serves the project.
pub(crate) fn serve(options: HostOptions) -> Result<Served> {
    restrict_new_files();
    let state_dir = StateDir::for_project(&options.project.root);
    state_dir.create()?;
    // Held until this process exits, however it exits: no second instance can serve
    // this state directory, even one whose start never saw this one's record.
    let Some(instance) = state_dir.instance_lock()? else {
        anyhow::bail!(
            "another Submilli playground is already serving {}; stop it with `submilli \
             playground stop`",
            options.project.root.display()
        );
    };
    if let Some(delay) = std::env::var(START_DELAY_ENV)
        .ok()
        .and_then(|ms| ms.parse().ok())
    {
        std::thread::sleep(Duration::from_millis(delay).min(START_DELAY_CAP));
    }
    let tokens = state_dir.tokens()?;
    let blueprint_yaml = std::fs::read_to_string(&options.project.blueprint)
        .with_context(|| format!("reading {}", options.project.blueprint.display()))?;
    let blueprint = submilli_blueprint::parse(&blueprint_yaml)
        .with_context(|| format!("parsing {}", options.project.blueprint.display()))?;
    // Project packages are built and installed before the blueprint is registered,
    // which requires them in the store.
    let packages = Arc::new(ProjectPackages::new(
        &options.project.package_dir,
        submilli_build::default_package_store_dir(),
    ));
    let (_, closure) = packages
        .prepare(&blueprint)
        .map_err(|failure| anyhow::anyhow!("{failure}"))?;
    let secrets = KnownSecrets::default();
    let store = Arc::new(Store::open(&state_dir.store_dir()).context("opening the run store")?);
    let secret_store = Arc::new(WatchedSecretStore::new(
        crate::commands::local::open_secret_store()?,
        secrets.clone(),
    ));
    let mut config = server_config(
        &state_dir,
        &options.egress,
        tokens.clone(),
        Some(secret_store),
        Some(Arc::new(Recorder::new(Arc::clone(&store), secrets))),
    );
    config.volumes = super::project::volumes(&options.project);
    config.session_cipher = Some(Arc::new(state_dir.session_cipher()?));
    let freshness = Arc::new(Freshness::new(Arc::clone(&packages)));
    config.pre_execute = Some(Arc::clone(&freshness) as _);
    let runtime = submilli_server::runtime(&config).context("starting the async runtime")?;
    let result = runtime.block_on(run(
        options,
        state_dir,
        Arc::new(HeldInstance::new(instance)),
        config,
        tokens,
        Serving {
            name: blueprint.name.clone(),
            store,
            packages,
            freshness,
            closure,
        },
    ));
    // Teardown is bounded: a stray connection task must not keep the process up.
    runtime.shutdown_timeout(Duration::from_millis(250));
    result
}

/// The blueprint the playground serves, by the name it had at start, and the store
/// its versions are logged in.
struct Serving {
    name: String,
    store: Arc<Store>,
    packages: Arc<ProjectPackages>,
    /// The package check runs get, which the blueprint watcher shares.
    freshness: Arc<Freshness>,
    /// The closure as it was at start, for the ready record.
    closure: Vec<ClosureEntry>,
}

/// The instance lock this process holds for its whole life, and whether the
/// instance has begun to drain: answered by `status`, and written into
/// `instance.lock` so a command that finds no instance record still sees it. It is
/// kept until the process exits, so the lock goes only when the process does.
struct HeldInstance {
    /// Dropped only when the process exits.
    instance: InstanceLock,
    stopping: AtomicBool,
    /// The signal that began the stop, or 0 when none did.
    stopped_by: AtomicI32,
}

impl HeldInstance {
    fn new(instance: InstanceLock) -> Self {
        Self {
            instance,
            stopping: AtomicBool::new(false),
            stopped_by: AtomicI32::new(0),
        }
    }

    /// Note that `signal` arrived, then begin stopping.
    fn signaled(&self, signal: libc::c_int) {
        let _ = self
            .stopped_by
            .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
        self.begin_stopping();
    }

    fn stopped_by(&self) -> Option<libc::c_int> {
        Some(self.stopped_by.load(Ordering::SeqCst)).filter(|signal| *signal != 0)
    }

    fn begin_stopping(&self) {
        if !self.stopping.swap(true, Ordering::SeqCst)
            && let Err(error) = self.instance.write_holder(true)
        {
            warn(&format!("{error:#}"));
        }
    }

    fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }
}

async fn run(
    options: HostOptions,
    state_dir: StateDir,
    held: Arc<HeldInstance>,
    mut config: ServerConfig,
    tokens: Vec<submilli_server::ApiToken>,
    serving: Serving,
) -> Result<Served> {
    // Before anything announces this instance: a signal from then on drains it
    // rather than killing it with its record on disk.
    let signals = submilli_server::EmbeddedSignals::install()?;
    // The server drains on the same signals; this notes that it has begun.
    let _signal_watch = tokio::spawn(note_signals(Arc::clone(&held))?);
    config.database = Some(Arc::new(
        submilli_server::database::ServerDatabase::open(&state_dir.database_path()).await?,
    ));
    let blueprints = submilli_server::prepare_blueprint_store(&config).await?;
    config.blueprints = Some(Arc::clone(&blueprints));
    let state = AppState::new(config).await?;
    state.boot().await?;
    let _store_watch = packages::watch_store(state.clone(), serving.packages.store_root())?;
    // Every save, then the file as it is now, through the trusted local path. The
    // watch comes first so a save during the first apply is seen, not missed.
    let applier = Arc::new(
        Applier::new(
            state.clone(),
            serving.store,
            options.project.blueprint.clone(),
            serving.name.clone(),
        )
        .with_packages(serving.freshness),
    );
    let _watch = watch::watch(Arc::clone(&applier), watch::DEBOUNCE)?;
    applier.start().await?;
    let blueprint_status = applier.status();

    let server = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("binding the server listener")?;
    let control = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("binding the control listener")?;
    let server_port = server.local_addr()?.port();
    let control_port = control.local_addr()?.port();

    let description = describe(
        &options,
        &state_dir,
        &serving.name,
        control_port,
        server_port,
        serving.closure,
    );
    let auth = Arc::new(ControlAuth::new(tokens));
    let shared = ControlState(Arc::new(ControlInner {
        auth: Arc::clone(&auth),
        port: control_port,
        nonce: options.nonce.clone(),
        server_shutdown: state.shutdown_signal(),
        description: description.clone(),
        blueprint_status,
        blueprints,
        blueprint_name: serving.name.clone(),
        packages: Arc::clone(&serving.packages),
        held: Arc::clone(&held),
    }));
    let control_stop = Arc::new(Notify::new());
    let control_task = tokio::spawn({
        let control_stop = Arc::clone(&control_stop);
        let router = ControlRoutes::new().router(shared);
        async move {
            axum::serve(control, router)
                .with_graceful_shutdown(async move { control_stop.notified().await })
                .await
        }
    });

    let record = InstanceRecord {
        pid: std::process::id(),
        control_port,
        server_port,
        nonce: options.nonce.clone(),
    };
    let served = async {
        test_announce_delay(&held).await;
        // Told to stop while it started: it never announces itself, so no command
        // finds a record of, or a link to, an instance that is already going.
        if held.is_stopping() {
            super::log::note("the playground was stopped while it was starting");
            return Ok(Served::StoppedWhileStarting {
                signal: held.stopped_by(),
            });
        }
        // The record first, so the ready file never names an instance without one.
        state_dir.write_record(&record)?;
        state_dir.write_ready(&record)?;
        drop(options.start_lock);
        if let Some(output) = options.print {
            let code = auth.mint_login_code(Now::current())?;
            ReadyRecord::new(&description, None, &code, false).print(output);
        }
        submilli_server::serve_embedded(server, state, SHUTDOWN_GRACE, signals)
            .await
            .map(|()| Served::Drained)
    }
    .await;

    // Also after a drain no signal or stop route began (`POST /v1/shutdown`).
    held.begin_stopping();
    auth.drop_sessions();
    control_stop.notify_one();
    if tokio::time::timeout(CONTROL_CLOSE_BUDGET, control_task)
        .await
        .is_err()
    {
        warn("the control listener did not close in time");
    }
    state_dir.remove_if_ours(&options.nonce);
    served
}

/// A future that marks `held` stopping on SIGTERM or SIGINT, registered now. A
/// signal this process ignores is left ignored, as [`submilli_server::EmbeddedSignals`]
/// leaves it.
fn note_signals(held: Arc<HeldInstance>) -> Result<impl Future<Output = ()>> {
    use submilli_server::WatchedSignal;
    use tokio::signal::unix::SignalKind;
    let mut terminate =
        WatchedSignal::unless_ignored(SignalKind::terminate()).context("watching for SIGTERM")?;
    let mut interrupt =
        WatchedSignal::unless_ignored(SignalKind::interrupt()).context("watching for SIGINT")?;
    Ok(async move {
        let signal = tokio::select! {
            () = terminate.next() => libc::SIGTERM,
            () = interrupt.next() => libc::SIGINT,
        };
        held.signaled(signal);
    })
}

/// Test-only, hidden: how long a serving process waits once its signal handlers
/// are in, just before it would announce itself, in milliseconds, so a test can
/// signal an instance that is starting and handles signals. Unset outside tests;
/// capped like [`START_DELAY_ENV`]. A stop ends the wait early.
const ANNOUNCE_DELAY_ENV: &str = "SUBMILLI_PLAYGROUND_TEST_ANNOUNCE_DELAY_MS";

async fn test_announce_delay(held: &HeldInstance) {
    let Some(delay) = std::env::var(ANNOUNCE_DELAY_ENV)
        .ok()
        .and_then(|ms| ms.parse().ok())
    else {
        return;
    };
    super::log::note("waiting before announcing (test)");
    let delay = Duration::from_millis(delay).min(START_DELAY_CAP);
    let started = tokio::time::Instant::now();
    while started.elapsed() < delay && !held.is_stopping() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn server_config(
    state_dir: &StateDir,
    egress: &Egress,
    tokens: Vec<submilli_server::ApiToken>,
    secret_store: Option<Arc<dyn submilli_server::SecretStore>>,
    run_recorder: Option<Arc<dyn submilli_server::record::RunRecorderFactory>>,
) -> ServerConfig {
    ServerConfig {
        // Main's audit writes whole permission-check contexts, unredacted, to the
        // log: a second copy of run data outside the playground's own store.
        audit: AuditConfig {
            enabled: false,
            ..AuditConfig::default()
        },
        auth: AuthConfig::Tokens(tokens),
        network_policy: egress.policy(),
        blueprint_dir: Some(state_dir.blueprints_dir()),
        session_store_dir: Some(state_dir.sessions_dir()),
        session_storage_root: Some(state_dir.session_vfs_dir()),
        ephemeral_storage_root: Some(state_dir.ephemeral_vfs_dir()),
        managed_volume_root: Some(state_dir.volumes_dir()),
        // The developer's own stores, so `build publish-local` and `secret put`
        // keep working.
        package_store_root: Some(submilli_build::default_package_store_dir()),
        secret_store,
        // Every run the server executes, whoever sent it, goes to the playground's store.
        run_recorder,
        // No run content leaves the machine, whatever the CLI's telemetry setting.
        run_telemetry: RunTelemetry::Off,
        ..ServerConfig::default()
    }
}

fn describe(
    options: &HostOptions,
    state_dir: &StateDir,
    blueprint: &str,
    control_port: u16,
    server_port: u16,
    packages: Vec<ClosureEntry>,
) -> Description {
    let (provider, model) = detect_provider();
    let provider = provider.map(str::to_owned);
    Description {
        project: options.project.root.clone(),
        blueprint: BlueprintInfo {
            name: blueprint.to_owned(),
            path: options.project.blueprint.clone(),
        },
        url: format!("http://127.0.0.1:{control_port}/"),
        server_url: format!("http://127.0.0.1:{server_port}"),
        pid: std::process::id(),
        app_token_file: state_dir.token_path(APP_TOKEN),
        state_dir: state_dir.root().to_path_buf(),
        log_file: state_dir.log_path(),
        egress_grants: options.egress.grants(),
        provider,
        model,
        packages,
    }
}

/// The bring-your-own-key provider the environment names when the playground
/// starts. Only which variable is set is read here, never its value.
fn detect_provider() -> (Option<&'static str>, Option<String>) {
    let set = |key: &str| std::env::var_os(key).is_some_and(|value| !value.is_empty());
    let provider = [
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
        ("GEMINI_API_KEY", "google"),
        ("GOOGLE_API_KEY", "google"),
    ]
    .into_iter()
    .find_map(|(key, provider)| set(key).then_some(provider));
    let model = std::env::var("SUBMILLI_PLAYGROUND_MODEL")
        .ok()
        .filter(|model| !model.trim().is_empty());
    (provider, model)
}

/// Files the server creates under the state directory must be owner-only too.
fn restrict_new_files() {
    // SAFETY: umask takes and returns a plain mode and touches no memory.
    unsafe {
        libc::umask(0o077);
    }
}

#[derive(Clone)]
struct ControlState(Arc<ControlInner>);

struct ControlInner {
    auth: Arc<ControlAuth>,
    port: u16,
    nonce: String,
    server_shutdown: Arc<Notify>,
    description: Description,
    /// The version in force and the last refused save.
    blueprint_status: Arc<std::sync::Mutex<BlueprintStatus>>,
    /// The server's blueprint store, for the blueprint in force.
    blueprints: Arc<dyn submilli_server::blueprint::BlueprintStore>,
    blueprint_name: String,
    packages: Arc<ProjectPackages>,
    held: Arc<HeldInstance>,
}

/// The control listener's routes, by who may call them. Later steps add their
/// routes to the group whose callers they accept.
pub(crate) struct ControlRoutes {
    /// No credential: the page, the login-code exchange, and the nonce challenge.
    public: Router<ControlState>,
    /// The admin token only: the CLI.
    admin: Router<ControlState>,
    /// The admin token or a browser session: the CLI and the page.
    viewer: Router<ControlState>,
}

impl ControlRoutes {
    fn new() -> Self {
        Self {
            public: Router::new()
                .route("/", get(page))
                .route("/api/challenge", post(challenge))
                .route("/api/login", post(login)),
            admin: Router::new()
                .route("/api/login-codes", post(mint_login_code))
                .route("/api/stop", post(stop)),
            viewer: Router::new().route("/api/status", get(status)),
        }
    }

    fn router(self, state: ControlState) -> Router {
        let admin = self
            .admin
            .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));
        let viewer = self.viewer.route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_viewer,
        ));
        Router::new()
            .merge(self.public)
            .merge(admin)
            .merge(viewer)
            .fallback(not_found)
            .layer(middleware::from_fn_with_state(state.clone(), same_origin))
            .layer(middleware::from_fn(no_sniff))
            .with_state(state)
    }
}

fn error(status: StatusCode, code: &'static str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

fn unauthorized() -> Response {
    error(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "send `Authorization: Bearer <token>` with the playground's admin token or a browser \
         session token",
    )
}

fn forbidden() -> Response {
    error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "this token may not use this control route",
    )
}

async fn no_sniff(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

async fn same_origin(State(state): State<ControlState>, request: Request, next: Next) -> Response {
    if !control_auth::same_origin(request.headers(), state.0.port) {
        return error(
            StatusCode::FORBIDDEN,
            "forbidden_origin",
            "the control listener answers only requests addressed to it from its own origin",
        );
    }
    next.run(request).await
}

async fn require_admin(
    State(state): State<ControlState>,
    request: Request,
    next: Next,
) -> Response {
    match state.0.auth.caller(request.headers(), Now::current()) {
        Some(Caller::Admin) => next.run(request).await,
        Some(_) => forbidden(),
        None => unauthorized(),
    }
}

async fn require_viewer(
    State(state): State<ControlState>,
    request: Request,
    next: Next,
) -> Response {
    match state.0.auth.caller(request.headers(), Now::current()) {
        Some(Caller::Admin | Caller::Browser) => next.run(request).await,
        Some(Caller::Token(_)) => forbidden(),
        None => unauthorized(),
    }
}

/// An unknown control route answers 401 to a caller with no credential, so the
/// route table is not readable without one.
async fn not_found(State(state): State<ControlState>, request: Request) -> Response {
    if request.uri().path().starts_with("/api/")
        && state
            .0
            .auth
            .caller(request.headers(), Now::current())
            .is_none()
    {
        return unauthorized();
    }
    error(StatusCode::NOT_FOUND, "not_found", "no such control route")
}

#[derive(Deserialize)]
struct ChallengeRequest {
    challenge: String,
}

async fn challenge(
    State(state): State<ControlState>,
    Json(request): Json<ChallengeRequest>,
) -> Response {
    match control_auth::challenge_response(&state.0.nonce, &request.challenge) {
        Some(response) => Json(json!({ "response": response })).into_response(),
        None => error(
            StatusCode::BAD_REQUEST,
            "invalid_challenge",
            "send 32 random bytes as 64 hex characters",
        ),
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    code: String,
}

async fn login(State(state): State<ControlState>, Json(request): Json<LoginRequest>) -> Response {
    match state.0.auth.exchange(&request.code, Now::current()) {
        Ok(token) => Json(json!({
            "session_token": token,
            "expires_in_secs": control_auth::SESSION_TTL.as_secs(),
        }))
        .into_response(),
        Err(LoginRefusal::Invalid) => error(
            StatusCode::BAD_REQUEST,
            "invalid_login_code",
            "the login code is unknown, used, or expired; run `submilli playground open` for a \
             new link",
        ),
        Err(LoginRefusal::RateLimited) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many failed logins; wait a minute",
        ),
    }
}

async fn mint_login_code(State(state): State<ControlState>) -> Response {
    match state.0.auth.mint_login_code(Now::current()) {
        Ok(code) => Json(json!({
            "code": code,
            "expires_in_secs": control_auth::LOGIN_CODE_TTL.as_secs(),
        }))
        .into_response(),
        Err(failure) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &failure.to_string(),
        ),
    }
}

async fn status(State(state): State<ControlState>) -> Response {
    let mut description = state.0.description.clone();
    // The closure of the blueprint in force now, which a save may have changed.
    let packages_error = match current_closure(&state).await {
        Ok(Some(closure)) => {
            description.packages = closure;
            None
        }
        Ok(None) => None,
        Err(message) => Some(message),
    };
    let blueprint_status = state
        .0
        .blueprint_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let stopping = state.0.held.is_stopping();
    Json(Status {
        description,
        running: !stopping,
        stopping,
        browser_sessions: state.0.auth.browser_sessions(Now::current()),
        blueprint_status,
        packages_error,
        health: None,
    })
    .into_response()
}

async fn current_closure(state: &ControlState) -> Result<Option<Vec<ClosureEntry>>, String> {
    let blueprint = match state.0.blueprints.get(&state.0.blueprint_name).await {
        Ok(Some(blueprint)) => blueprint,
        Ok(None) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let packages = Arc::clone(&state.0.packages);
    tokio::task::spawn_blocking(move || packages.closure(&blueprint))
        .await
        .map_err(|error| error.to_string())?
        .map(Some)
        .map_err(|failure| failure.to_string())
}

async fn stop(State(state): State<ControlState>) -> Response {
    state.0.held.begin_stopping();
    state.0.server_shutdown.notify_one();
    (StatusCode::ACCEPTED, Json(json!({ "stopping": true }))).into_response()
}

/// A stand-in until the page arrives: it trades the link's login code for a
/// browser session, keeps the token out of the address bar, and shows the status.
async fn page() -> Response {
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        PAGE,
    )
        .into_response()
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Submilli playground</title></head>
<body><h1>Submilli playground</h1><pre id="status">Signing in...</pre>
<script>
(async () => {
  const out = document.getElementById("status");
  const code = new URLSearchParams(location.hash.slice(1)).get("login");
  history.replaceState(null, "", location.pathname + location.search);
  try {
    if (code) {
      const r = await fetch("/api/login", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code }) });
      if (r.ok) localStorage.setItem("submilli-playground-session", (await r.json()).session_token);
    }
    const token = localStorage.getItem("submilli-playground-session");
    const r = await fetch("/api/status", { headers: token ? { authorization: "Bearer " + token } : {} });
    if (r.status === 401) { localStorage.removeItem("submilli-playground-session"); out.textContent = "Run `submilli playground open` for a new link."; return; }
    out.textContent = JSON.stringify(await r.json(), null, 2);
  } catch (e) { out.textContent = String(e); }
})();
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_playground_turns_off_run_telemetry_and_the_audit_trail() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(dir.path());
        state.create().unwrap();
        let tokens = state.tokens().unwrap();
        let config = server_config(&state, &Egress::default(), tokens, None, None);
        assert_eq!(config.run_telemetry, RunTelemetry::Off);
        assert!(!config.audit.enabled);
        assert!(matches!(config.auth, AuthConfig::Tokens(ref tokens) if tokens.len() == 5));
    }

    #[test]
    fn egress_denies_loopback_unless_granted() {
        let denied = Egress::default();
        assert!(denied.grants().is_empty());
        let granted = Egress {
            allow_localhost: true,
            allow_private: false,
            allow_ip: vec!["10.1.0.0/16".parse().unwrap()],
        };
        assert_eq!(
            granted.grants(),
            ["allow-localhost", "allow-ip 10.1.0.0/16"]
        );
        let _: submilli_server::NetworkPolicy = granted.policy();
    }
}
