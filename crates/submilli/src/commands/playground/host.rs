//! The running playground: the host's server listener and the control listener,
//! both on loopback, in this process.
//!
//! The server listener serves the host's MCP endpoint under the playground's
//! tokens, and a health check. The control listener serves the page and the
//! control API, whose routes are [`ControlRoutes`].

use std::fs::File;
use std::mem::ManuallyDrop;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Json;
use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use submilli_build::PackageStore;
use submilli_shared::secret_store::SecretStore;
use tokio::sync::Notify;

use super::api::{self, Group};
use super::control_auth::{self, Caller, ControlAuth, LoginRefusal, Now};
use super::controls::act::{Actions, Binding};
use super::controls::render::Page;
use super::log::warn;
use super::packages::{self, ClosureEntry, Freshness, ProjectPackages};
use super::project::Project;
use super::state::{APP_TOKEN, InstanceLock, InstanceRecord, STAND_IN_TOKEN, StateDir};

use super::store::redact::WatchedSecretStore;
use super::store::{KnownSecrets, Recorder, Store};
use super::watch::{self, Applier, BlueprintStatus};
use super::{Egress, ReadyRecord};
use app::App;
use auth::ApiToken;
use run::{RunHost, RunHostConfig};

pub(super) mod app;
pub(super) mod auth;
pub(super) mod blueprints;
pub(super) mod finalizers;
pub(super) mod mcp;
pub(super) mod record;
pub(super) mod run;
pub(super) mod serve;
pub(super) mod sessions;
pub(super) mod volumes;

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
    /// Open the printed record's login link in the browser.
    pub(crate) open: bool,
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
    /// Where the provider's key was found, in words: "the project's .env".
    pub(crate) key_from: Option<String>,
    /// The project's `.env` is in a git repository that doesn't ignore it.
    pub(crate) env_file_tracked: bool,
    /// The models a chat can use, as `<provider>:<model>`, the default first: one for
    /// each provider the playground has a key for. `status` reads them again, so a key
    /// added to the `.env` or the secret store shows without a restart.
    pub(crate) chat_models: Vec<String>,
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
    /// The variables the blueprint in force declares, by name.
    pub(crate) declared_variables: Vec<DeclaredVariable>,
    /// Why the closure of the blueprint in force could not be read; `packages` is
    /// then the closure at start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) packages_error: Option<String>,
    /// Whether the server listener answers, which `status` adds after asking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) health: Option<String>,
    /// The binding new runs get: variable values, and secret names only.
    pub(crate) binding: BindingView,
    /// Each provider chat works with, the variable its key goes under, and where its key
    /// is found now: the page offers to add or replace one.
    pub(crate) model_keys: Vec<super::agent::ProviderKey>,
}

/// A variable the blueprint declares, and whether a run must have a value for it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DeclaredVariable {
    pub(crate) name: String,
    pub(crate) required: bool,
}

/// The binding as `status` shows it: the variables, and only the names of the secrets.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BindingView {
    pub(crate) variables: std::collections::BTreeMap<String, String>,
    pub(crate) secrets: Vec<String>,
    /// Secrets bound before the playground started, whose values were not kept.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) to_bind_again: Vec<String>,
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

/// Serve until a stop or a signal, then drain and remove the instance record and the
/// ready file. Refused while another process serves the project.
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
    let serving = Serving::prepare(&options, &state_dir)?;
    let runtime = serve::runtime(serving.config.runtime.native_stack_size())
        .context("starting the async runtime")?;
    let result = runtime.block_on(run(
        options,
        state_dir,
        Arc::new(HeldInstance::new(instance)),
        serving,
        tokens,
    ));
    // Teardown is bounded: a stray connection task must not keep the process up.
    runtime.shutdown_timeout(Duration::from_millis(250));
    result
}

/// What the playground serves, made before the async runtime starts: the blueprint
/// by the name it had at start, and the store its versions are logged in.
struct Serving {
    name: String,
    store: Arc<Store>,
    /// The host's run recorder, which knows the runs in flight.
    recorder: Recorder,
    /// Secret values the recorder cuts out; `bind` adds the development values it holds.
    secrets: KnownSecrets,
    secret_store: Arc<dyn SecretStore>,
    package_store: PackageStore,
    /// How the host runs programs; its finalizers are added once the sessions boot.
    config: RunHostConfig,
    packages: Arc<ProjectPackages>,
    /// The package check runs get, which the blueprint watcher shares.
    freshness: Arc<Freshness>,
    /// The closure as it was at start, for the ready record.
    closure: Vec<ClosureEntry>,
}

impl Serving {
    /// Read the blueprint, build and install its project packages, and open the
    /// stores the host records into.
    fn prepare(options: &HostOptions, state_dir: &StateDir) -> Result<Self> {
        let blueprint_yaml = std::fs::read_to_string(&options.project.blueprint)
            .with_context(|| format!("reading {}", options.project.blueprint.display()))?;
        let blueprint = submilli_blueprint::parse(&blueprint_yaml)
            .with_context(|| format!("parsing {}", options.project.blueprint.display()))?;
        // Project packages are built and installed before the blueprint is applied,
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
        let secret_store: Arc<dyn SecretStore> = Arc::new(WatchedSecretStore::new(
            crate::commands::local::open_secret_store()?,
            secrets.clone(),
        ));
        let recorder = Recorder::new(Arc::clone(&store), secrets.clone());
        let freshness = Arc::new(Freshness::new(Arc::clone(&packages)));
        // The developer's own package store, so `build publish-local` keeps working.
        let package_store = PackageStore::new(submilli_build::default_package_store_dir());
        let mut config = RunHostConfig::new(package_store.clone());
        config.network_policy = options.egress.policy();
        config.secret_store = Some(Arc::clone(&secret_store));
        config.session_cipher = Some(Arc::new(state_dir.session_cipher()?));
        // Every run the host executes, whoever sent it, goes to the playground's store.
        config.recorder = Some(Arc::new(recorder.clone()));
        config.freshness = Some(Arc::clone(&freshness));
        Ok(Self {
            name: blueprint.name.clone(),
            store,
            recorder,
            secrets,
            secret_store,
            package_store,
            config,
            packages,
            freshness,
            closure,
        })
    }
}

/// The instance lock this process holds for its whole life, and whether the
/// instance has begun to drain: answered by `status`, and written into
/// `instance.lock` so a command that finds no instance record still sees it. It is
/// kept until the process exits, so the lock goes only when the process does.
struct HeldInstance {
    /// Native database cleanup can outlive the async runtime. Leave this single
    /// descriptor for the kernel to close at process exit, so `stop` cannot report
    /// success while the old process still owns the database. This CLI serves
    /// only one playground per process, including on startup failure.
    instance: ManuallyDrop<InstanceLock>,
    stopping: AtomicBool,
    /// The signal that began the stop, or 0 when none did.
    stopped_by: AtomicI32,
}

impl HeldInstance {
    fn new(instance: InstanceLock) -> Self {
        Self {
            instance: ManuallyDrop::new(instance),
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
    serving: Serving,
    tokens: Vec<ApiToken>,
) -> Result<Served> {
    let server_shutdown = Arc::new(Notify::new());
    // Before anything announces this instance: a signal from then on drains it
    // rather than killing it with its record on disk.
    let stops = serve::Stops::install(Arc::clone(&server_shutdown))?;
    // The listener drains on the same signals; this notes that it has begun.
    let _signal_watch = tokio::spawn(note_signals(Arc::clone(&held))?);
    let sessions = Arc::new(sessions::Sessions::boot(&state_dir).await?);
    let served = serve_sessions(
        options,
        &state_dir,
        &held,
        serving,
        tokens,
        Arc::clone(&sessions),
        server_shutdown,
        stops,
    )
    .await;
    // Its native cleanup can outlive the runtime; the instance lock outlives both.
    if let Err(error) = sessions.close_database().await {
        warn(&format!("closing the session database failed: {error:#}"));
    }
    served
}

/// Start the host on the booted sessions, bind its listeners, announce it, and
/// serve until it drains; then close the control listener and remove the records.
#[allow(clippy::too_many_arguments)]
async fn serve_sessions(
    mut options: HostOptions,
    state_dir: &StateDir,
    held: &Arc<HeldInstance>,
    serving: Serving,
    tokens: Vec<ApiToken>,
    sessions: Arc<sessions::Sessions>,
    server_shutdown: Arc<Notify>,
    stops: serve::Stops,
) -> Result<Served> {
    let (host, closure) = start_host(&options, state_dir, serving, &tokens, sessions).await?;
    let server = serve::bind().await?;
    let control = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("binding the control listener")?;
    let server_port = server.local_addr()?.port();
    let control_port = control.local_addr()?.port();

    let description = describe(
        &options,
        state_dir,
        &host.name,
        control_port,
        server_port,
        closure,
    );
    let auth = Arc::new(ControlAuth::new(tokens));
    let actions = Arc::new(actions(
        &options,
        state_dir,
        &host,
        description.url.clone(),
        server_port,
    ));
    // Flipped once the server has drained, so the event feeds end and the control
    // listener can close.
    let (stopped, stopping) = tokio::sync::watch::channel(false);
    let shared = ControlState(Arc::new(ControlInner {
        auth: Arc::clone(&auth),
        port: control_port,
        nonce: options.nonce.clone(),
        server_shutdown,
        description: description.clone(),
        blueprint_status: host.applier.status(),
        app: Arc::clone(&host.app),
        blueprint_name: host.name.clone(),
        packages: Arc::clone(&host.packages),
        held: Arc::clone(held),
        actions,
        stopping,
    }));
    let control = ControlListener::spawn(control, ControlRoutes::new().router(shared));

    let record = InstanceRecord {
        pid: std::process::id(),
        control_port,
        server_port,
        nonce: options.nonce.clone(),
    };
    let start_lock = options.start_lock.take();
    let print = options.print.take();
    let served = async {
        if let Some(stopped) = announce(
            start_lock,
            print,
            options.open,
            state_dir,
            held,
            &record,
            &auth,
            &description,
        )
        .await?
        {
            return Ok(stopped);
        }
        serve::serve(
            server,
            serve::router_with_mcp(host.mcp),
            stops,
            SHUTDOWN_GRACE,
        )
        .await
        .map(|_| Served::Drained)
    }
    .await;

    // Also after a drain no signal or stop route began.
    held.begin_stopping();
    auth.drop_sessions();
    stopped.send_replace(true);
    control.close().await;
    state_dir.remove_if_ours(&options.nonce);
    served
}

/// The host once its blueprint is applied: what the listeners serve, and the
/// watches and reaper that run until serving ends.
struct Started {
    name: String,
    store: Arc<Store>,
    recorder: Recorder,
    secrets: KnownSecrets,
    packages: Arc<ProjectPackages>,
    app: Arc<App>,
    applier: Arc<Applier>,
    mcp: Arc<mcp::McpHost>,
    _reaper: ReaperTask,
    _watch: watch::Watch,
    _store_watch: packages::StoreWatch,
}

/// Build the run host on `sessions`, apply the blueprint as it is now while
/// watching it for saves, and finish the closes a stop cut short. The closure
/// at start comes back beside it, for the ready record.
async fn start_host(
    options: &HostOptions,
    state_dir: &StateDir,
    serving: Serving,
    tokens: &[ApiToken],
    sessions: Arc<sessions::Sessions>,
) -> Result<(Started, Vec<ClosureEntry>)> {
    let mut config = serving.config;
    let volumes = Arc::new(volumes::Volumes::boot(
        state_dir,
        super::project::volumes(&options.project),
    ));
    let blueprints = Arc::new(blueprints::Blueprints::new(
        serving.name.clone(),
        Arc::clone(&volumes) as Arc<dyn blueprints::LocalVolumes>,
        Some(serving.secret_store),
        serving.package_store.clone(),
    ));
    config.finalizers = Some(Arc::new(sessions.finalizers().clone()));
    let run_host = Arc::new(RunHost::new(
        config,
        Arc::clone(&sessions),
        Arc::clone(&volumes),
    )?);
    finalizers::HostFinalizers::install(&run_host, Arc::clone(&blueprints) as _);
    let app = Arc::new(App {
        run: Arc::clone(&run_host),
        sessions: Arc::clone(&sessions),
        blueprints: Arc::clone(&blueprints),
        volumes: Arc::clone(&volumes),
    });
    let store_watch = packages::watch_store(
        {
            let run = Arc::clone(&run_host);
            move || run.clear_prepared_packages()
        },
        serving.packages.store_root(),
    )?;
    // Every save, then the file as it is now, through the trusted local path. The
    // watch comes first so a save during the first apply is seen, not missed.
    let applier = Arc::new(
        Applier::new(
            Arc::clone(&app) as Arc<dyn watch::LocalBlueprints>,
            Arc::clone(&serving.store),
            options.project.blueprint.clone(),
            serving.name.clone(),
        )
        .with_packages(serving.freshness),
    );
    let watch = watch::watch(Arc::clone(&applier), watch::DEBOUNCE)?;
    applier.start().await?;
    // A close a stop cut short finishes now, under the blueprint just applied.
    sessions.finish_closing().await?;
    let reaper = ReaperTask(sessions.spawn_reaper(REAP_INTERVAL));

    let mcp = Arc::new(mcp::McpHost::new(
        mcp::Shared {
            run: Arc::clone(&run_host),
            sessions: Arc::clone(&sessions),
            blueprints: Arc::clone(&blueprints),
            volumes,
            package_store: serving.package_store,
            events: Some(Arc::clone(serving.recorder.events())),
        },
        Arc::from(tokens),
        &serving.name,
    ));
    let started = Started {
        name: serving.name,
        store: serving.store,
        recorder: serving.recorder,
        secrets: serving.secrets,
        packages: serving.packages,
        app,
        applier,
        mcp,
        _reaper: reaper,
        _watch: watch,
        _store_watch: store_watch,
    };
    Ok((started, serving.closure))
}

/// What the control routes act through, starting from the variables the last
/// `bind` saved; secret values were never saved.
fn actions(
    options: &HostOptions,
    state_dir: &StateDir,
    host: &Started,
    url: String,
    server_port: u16,
) -> Actions {
    let binding = match state_dir.read_binding() {
        Ok(saved) => Binding::remembered(saved.unwrap_or_default()),
        Err(error) => {
            warn(&format!(
                "the saved binding was not read, so the playground starts with none: {error:#}"
            ));
            Binding::default()
        }
    };
    Actions {
        app: Arc::clone(&host.app),
        store: Arc::clone(&host.store),
        recorder: host.recorder.clone(),
        secrets: host.secrets.clone(),
        binding: std::sync::Mutex::new(binding),
        binding_file: state_dir.binding_path(),
        applier: Arc::clone(&host.applier),
        blueprint_name: host.name.clone(),
        blueprint_path: options.project.blueprint.clone(),
        project_root: options.project.root.clone(),
        page: Page { base: Some(url) },
        cancelling: std::sync::Mutex::default(),
        chatting: Arc::default(),
        chat: super::chat::ChatIntake::default(),
        server_port,
    }
}

/// Write the instance record and the ready file, release the start lock, and print
/// the ready record when asked. Returns how serving ended instead when the
/// instance was told to stop while it started.
#[allow(clippy::too_many_arguments)]
async fn announce(
    start_lock: Option<File>,
    print: Option<super::Output>,
    open: bool,
    state_dir: &StateDir,
    held: &HeldInstance,
    record: &InstanceRecord,
    auth: &ControlAuth,
    description: &Description,
) -> Result<Option<Served>> {
    test_announce_delay(held).await;
    // Told to stop while it started: it never announces itself, so no command
    // finds a record of, or a link to, an instance that is already going.
    if held.is_stopping() {
        super::log::note("the playground was stopped while it was starting");
        return Ok(Some(Served::StoppedWhileStarting {
            signal: held.stopped_by(),
        }));
    }
    // The record first, so the ready file never names an instance without one.
    state_dir.write_record(record)?;
    state_dir.write_ready(record)?;
    drop(start_lock);
    if let Some(output) = print {
        let code = auth.mint_login_code(Now::current())?;
        ReadyRecord::new(description, None, &code, false).announce(output, open, state_dir);
    }
    Ok(None)
}

/// The control listener, served on a task of its own until closed.
struct ControlListener {
    stop: Arc<Notify>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl ControlListener {
    fn spawn(listener: tokio::net::TcpListener, router: Router) -> Self {
        let stop = Arc::new(Notify::new());
        let task = tokio::spawn({
            let stop = Arc::clone(&stop);
            async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move { stop.notified().await })
                    .await
            }
        });
        Self { stop, task }
    }

    /// Stop accepting and wait, within a budget, for the open requests to end.
    async fn close(self) {
        self.stop.notify_one();
        if tokio::time::timeout(CONTROL_CLOSE_BUDGET, self.task)
            .await
            .is_err()
        {
            warn("the control listener did not close in time");
        }
    }
}

/// How often idle sessions are closed, as the server reaps them.
const REAP_INTERVAL: Duration = Duration::from_secs(30);

/// The idle-session reaper, stopped when serving ends.
struct ReaperTask(tokio::task::JoinHandle<()>);

impl Drop for ReaperTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A future that marks `held` stopping on SIGTERM or SIGINT, registered now. A
/// signal this process ignores is left ignored, as [`serve::Stops`] leaves it.
fn note_signals(held: Arc<HeldInstance>) -> Result<impl Future<Output = ()>> {
    use serve::WatchedSignal;
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

fn describe(
    options: &HostOptions,
    state_dir: &StateDir,
    blueprint: &str,
    control_port: u16,
    server_port: u16,
    packages: Vec<ClosureEntry>,
) -> Description {
    super::keys::use_project(&options.project.root);
    let (provider, model, key_from) = detect_provider();
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
        key_from,
        env_file_tracked: super::keys::env_file_tracked(&options.project.root),
        chat_models: super::agent::offered_here(),
        packages,
    }
}

/// The bring-your-own-key provider chat uses now, its model, and where its key was
/// found. Only where a key is is reported, never its value.
fn detect_provider() -> (Option<&'static str>, Option<String>, Option<String>) {
    match super::agent::configured() {
        Ok(choice) => {
            let from = super::agent::key_source(&choice).map(|source| source.words().to_owned());
            (Some(choice.provider), Some(choice.model), from)
        }
        // Not configured, or configured wrong: `chat` says how; the model variable is
        // shown as it was set.
        Err(_) => (None, super::keys::value(super::agent::MODEL_VARIABLE), None),
    }
}

/// Files the server creates under the state directory must be owner-only too.
fn restrict_new_files() {
    // SAFETY: umask takes and returns a plain mode and touches no memory.
    unsafe {
        libc::umask(0o077);
    }
}

#[derive(Clone)]
pub(crate) struct ControlState(Arc<ControlInner>);

impl ControlState {
    /// Who sent a request with `headers`.
    pub(crate) fn caller(&self, headers: &axum::http::HeaderMap) -> Option<Caller> {
        self.0.auth.caller(headers, Now::current())
    }

    pub(crate) fn actions(&self) -> &Actions {
        &self.0.actions
    }

    pub(crate) fn actions_arc(&self) -> &Arc<Actions> {
        &self.0.actions
    }

    /// The project's packages and the store they install into.
    pub(crate) fn packages(&self) -> &Arc<ProjectPackages> {
        &self.0.packages
    }

    /// Becomes `true` once the playground has drained and is closing its listener.
    pub(crate) fn stopping(&self) -> tokio::sync::watch::Receiver<bool> {
        self.0.stopping.clone()
    }
}

pub(crate) struct ControlInner {
    auth: Arc<ControlAuth>,
    port: u16,
    nonce: String,
    server_shutdown: Arc<Notify>,
    description: Description,
    /// The version in force and the last refused save.
    blueprint_status: Arc<std::sync::Mutex<BlueprintStatus>>,
    /// The host, for the blueprint in force.
    app: Arc<App>,
    blueprint_name: String,
    packages: Arc<ProjectPackages>,
    held: Arc<HeldInstance>,
    actions: Arc<Actions>,
    stopping: tokio::sync::watch::Receiver<bool>,
}

/// The control listener's routes, by who may call them, built from [`api::ROUTES`].
pub(crate) struct ControlRoutes {
    /// No credential: the page, the login-code exchange, and the nonce challenge.
    public: Router<ControlState>,
    /// The admin token only: the CLI.
    admin: Router<ControlState>,
    /// The admin token or a browser session: the CLI and the page.
    viewer: Router<ControlState>,
    /// The `stand-in` token only: the bridge.
    stand_in: Router<ControlState>,
}

impl ControlRoutes {
    fn new() -> Self {
        let group = |group: Group| {
            api::by_group(group).fold(Router::new(), |router, route| {
                router.route(route.path, (route.handler)())
            })
        };
        Self {
            public: group(Group::Public),
            admin: group(Group::Admin),
            viewer: group(Group::Viewer),
            stand_in: group(Group::StandIn),
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
        let stand_in = self.stand_in.route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_stand_in,
        ));
        Router::new()
            .merge(self.public)
            .merge(admin)
            .merge(viewer)
            .merge(stand_in)
            .fallback(not_found)
            .layer(axum::extract::DefaultBodyLimit::max(api::MAX_BODY_BYTES))
            .layer(middleware::from_fn_with_state(state.clone(), same_origin))
            .layer(middleware::from_fn(no_sniff))
            .with_state(state)
    }
}

pub(crate) fn error(status: StatusCode, code: &'static str, message: &str) -> Response {
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

/// The bridge's routes: only the `stand-in` token, never the admin token, another
/// token, or a browser session.
async fn require_stand_in(
    State(state): State<ControlState>,
    request: Request,
    next: Next,
) -> Response {
    match state.0.auth.caller(request.headers(), Now::current()) {
        Some(Caller::Token(name)) if name == STAND_IN_TOKEN => next.run(request).await,
        Some(_) => forbidden(),
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
pub(crate) struct ChallengeRequest {
    challenge: String,
}

pub(crate) async fn challenge(
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
pub(crate) struct LoginRequest {
    code: String,
}

pub(crate) async fn login(
    State(state): State<ControlState>,
    Json(request): Json<LoginRequest>,
) -> Response {
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

pub(crate) async fn mint_login_code(State(state): State<ControlState>) -> Response {
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

pub(crate) async fn status(State(state): State<ControlState>) -> Response {
    let mut description = state.0.description.clone();
    // A key added to the project's `.env` or the secret store since start counts now.
    let mut model_keys = Vec::new();
    if let Ok(((provider, model, key_from), offered, keys)) = tokio::task::spawn_blocking(|| {
        (
            detect_provider(),
            super::agent::offered_here(),
            super::agent::keys_now(),
        )
    })
    .await
    {
        description.provider = provider.map(str::to_owned);
        description.model = model;
        description.key_from = key_from;
        description.chat_models = offered;
        model_keys = keys;
    }
    let mut declared_variables = Vec::new();
    // The variables and the closure of the blueprint in force now, which a save may have
    // changed.
    let packages_error = match blueprint_in_force(&state).await {
        Ok(Some(blueprint)) => {
            declared_variables = blueprint
                .variables
                .iter()
                .map(|(name, declared)| DeclaredVariable {
                    name: name.clone(),
                    required: declared.required,
                })
                .collect();
            match closure_of(&state, blueprint).await {
                Ok(closure) => {
                    description.packages = closure;
                    None
                }
                Err(message) => Some(message),
            }
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
        declared_variables,
        packages_error,
        health: None,
        binding: state.actions().binding_view(),
        model_keys,
    })
    .into_response()
}

/// The package closure of the blueprint in force; `None` when no blueprint is.
pub(crate) async fn current_closure(
    state: &ControlState,
) -> Result<Option<Vec<ClosureEntry>>, String> {
    match blueprint_in_force(state).await? {
        Some(blueprint) => closure_of(state, blueprint).await.map(Some),
        None => Ok(None),
    }
}

async fn blueprint_in_force(
    state: &ControlState,
) -> Result<Option<submilli_blueprint::Blueprint>, String> {
    Ok(state.0.app.blueprint(&state.0.blueprint_name).await)
}

async fn closure_of(
    state: &ControlState,
    blueprint: submilli_blueprint::Blueprint,
) -> Result<Vec<ClosureEntry>, String> {
    let packages = Arc::clone(&state.0.packages);
    tokio::task::spawn_blocking(move || packages.closure(&blueprint))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|failure| failure.to_string())
}

pub(crate) async fn stop(State(state): State<ControlState>) -> Response {
    state.0.held.begin_stopping();
    state.0.server_shutdown.notify_one();
    (StatusCode::ACCEPTED, Json(json!({ "stopping": true }))).into_response()
}

/// The page's built files, by path under its build directory: `index.html`, and the
/// content-hashed files under `assets/` it loads.
const PAGE_FILES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/page_files.rs"));

/// The page and its assets load only from this listener, and nothing else may frame,
/// post from, or rebase them.
const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
                        font-src 'self'; img-src 'self' data:; connect-src 'self'; \
                        base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn page_file(path: &str) -> Option<&'static [u8]> {
    PAGE_FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// The page. It names its assets by content hash, so it is never cached itself: a new
/// build reaches the browser on the next load.
pub(crate) async fn page() -> Response {
    let Some(html) = page_file("index.html") else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "this build has no playground page",
        );
    };
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
        ],
        html,
    )
        .into_response()
}

/// A file the page loads. Its name carries its content hash, so it never changes and
/// the browser may keep it.
pub(crate) async fn page_asset(Path(file): Path<String>) -> Response {
    let Some(bytes) = page_file(&format!("assets/{file}")) else {
        return error(StatusCode::NOT_FOUND, "not_found", "no such page asset");
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type(&file))),
            (
                CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
            (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
        ],
        bytes,
    )
        .into_response()
}

fn content_type(file: &str) -> &'static str {
    match file.rsplit_once('.').map(|(_, extension)| extension) {
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK_LIFETIME_PROJECT: &str = "SUBMILLI_TEST_LOCK_LIFETIME_PROJECT";

    #[test]
    #[ignore = "subprocess for instance_lock_outlives_runtime_teardown"]
    fn instance_lock_lifetime_child() {
        use std::io::Read;

        let root = std::env::var_os(LOCK_LIFETIME_PROJECT).expect("child project");
        let state = StateDir::for_project(std::path::Path::new(&root));
        state.create().unwrap();
        let held = Arc::new(HeldInstance::new(state.instance_lock().unwrap().unwrap()));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.spawn(async move {
            let _held = held;
            std::future::pending::<()>().await;
        });
        runtime.shutdown_timeout(Duration::from_secs(1));
        // No async task or caller retains HeldInstance now. Native database
        // cleanup may still be running, so the instance must remain locked.
        std::fs::write(std::path::Path::new(&root).join("torn-down"), "").unwrap();
        std::io::stdin().read_exact(&mut [0]).unwrap();
    }

    #[test]
    fn instance_lock_outlives_runtime_teardown() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        use std::time::Instant;

        let directory = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(directory.path());
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::playground::host::tests::instance_lock_lifetime_child",
                "--ignored",
            ])
            .env(LOCK_LIFETIME_PROJECT, directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !directory.path().join("torn-down").exists() {
            assert!(child.try_wait().unwrap().is_none(), "child exited early");
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child did not finish runtime teardown");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let holder = state.instance_holder().unwrap().and_then(|held| held.pid());
        let pid = child.id();
        child.stdin.take().unwrap().write_all(&[0]).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(holder, Some(pid), "runtime teardown released the instance");
        assert!(state.instance_holder().unwrap().is_none());
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
        let _: interpreter::runtime::NetworkPolicy = granted.policy();
    }

    /// Text React carries that names a URL without ever requesting it: the XML
    /// namespaces it creates elements in, and the link it puts in a production error
    /// message.
    const NOT_REQUESTED: &[&str] = &[
        "http://www.w3.org/1998/Math/MathML",
        "http://www.w3.org/1999/xhtml",
        "http://www.w3.org/1999/xlink",
        "http://www.w3.org/2000/svg",
        "http://www.w3.org/XML/1998/namespace",
        "https://reactjs.org/docs/error-decoder.html?invariant=",
    ];

    #[test]
    fn no_page_file_names_another_origin() {
        assert!(page_file("index.html").is_some());
        for (name, bytes) in PAGE_FILES {
            // The fonts' license text links to the license's home.
            if name.ends_with(".txt") {
                continue;
            }
            let mut text = String::from_utf8_lossy(bytes).into_owned();
            for allowed in NOT_REQUESTED {
                text = text.replace(allowed, "");
            }
            for scheme in ["http://", "https://"] {
                assert!(
                    !text.contains(scheme),
                    "{name} names {}",
                    text.split(scheme)
                        .nth(1)
                        .map(|rest| rest.chars().take(60).collect::<String>())
                        .unwrap_or_default()
                );
            }
        }
    }
}
