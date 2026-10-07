use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{Mutex, Notify, oneshot};

use crate::{AppState, ServerConfig, app};

/// The multi-thread runtime a server built from `config` runs on. Programs run on
/// its threads, so their stacks are sized for the configured Wasm stack rather
/// than tokio's 2 MiB default: a program recursing through host callbacks nests
/// frames on them, and overflowing one would abort every session.
pub fn runtime(config: &ServerConfig) -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(config.runtime.native_stack_size())
        .build()
}

/// Serve until shutdown is requested — by `POST /v1/shutdown`, SIGTERM, or
/// SIGINT — then let in-flight requests finish for at most `shutdown_grace`
/// before dropping what remains. A second signal skips the remaining wait.
///
/// `shutdown_grace` bounds the wait after draining starts, including database
/// cleanup. Database cleanup can continue on its own thread after this function
/// returns. Startup failures close the database without a drain deadline.
pub async fn serve(
    addr: SocketAddr,
    mut config: ServerConfig,
    shutdown_grace: Duration,
) -> Result<()> {
    // Registered first: once the handlers are in place a signal is queued rather
    // than killing the process, so a `docker stop` arriving mid-boot drains once
    // boot finishes instead of terminating the process outright.
    let signals = ShutdownSignals::install()?;

    let database = match (config.database.take(), config.database_path.as_deref()) {
        (Some(database), _) => Some(database),
        (None, Some(path)) => Some(Arc::new(crate::database::ServerDatabase::open(path).await?)),
        (None, None) => None,
    };
    config.database = database.clone();

    let result = serve_opened(addr, config, shutdown_grace, signals).await;
    let Some(database) = database else {
        return result.map(|_| ());
    };
    close_database(database, result, shutdown_grace).await
}

enum DrainStatus {
    NoDrain,
    Completed {
        started_at: tokio::time::Instant,
        signals: Arc<Mutex<ShutdownSignals>>,
    },
    Forced,
}

async fn close_database(
    database: Arc<crate::database::ServerDatabase>,
    result: Result<DrainStatus>,
    shutdown_grace: Duration,
) -> Result<()> {
    let drain = match result {
        Ok(drain) => drain,
        Err(error) => {
            database.close().await?;
            return Err(error);
        }
    };
    let (started_at, signals) = match drain {
        DrainStatus::NoDrain => {
            database.close().await?;
            return Ok(());
        }
        DrainStatus::Completed {
            started_at,
            signals,
        } => (started_at, signals),
        DrainStatus::Forced => {
            database.begin_close()?;
            return Ok(());
        }
    };
    let deadline = started_at.checked_add(shutdown_grace);
    database.begin_close()?;
    tokio::select! {
        biased;
        result = database.close() => result?,
        () = wait_until(deadline) => tracing::warn!("stopped waiting for database work during shutdown"),
        () = async {
            let mut signals = signals.lock().await;
            signals.recv_signal().await;
        } => tracing::warn!("second signal stopped database drain wait"),
    }
    Ok(())
}

async fn wait_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn serve_opened(
    addr: SocketAddr,
    mut config: ServerConfig,
    shutdown_grace: Duration,
    signals: ShutdownSignals,
) -> Result<DrainStatus> {
    crate::auth::log_auth_posture(addr.ip(), &config.auth);
    let tls = config.tls.clone();
    let settings_hash = crate::audit::settings_hash(&config, addr, shutdown_grace);
    let allow_unauthenticated = matches!(config.auth, crate::auth::AuthConfig::Disabled);
    config.blueprints = Some(prepare_blueprint_store(&config).await?);
    let state = AppState::new(config)?;
    let audit = state.audit().clone();
    // Rehydrate persisted sessions and sweep orphan directories before serving,
    // so an immediate reconnect resolves instead of 404-ing.
    state.boot().await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    audit.emit(
        "server",
        serde_json::Map::from_iter([
            ("event".into(), serde_json::json!("started")),
            (
                "version".into(),
                serde_json::json!(env!("CARGO_PKG_VERSION")),
            ),
            ("settings_hash".into(), serde_json::json!(settings_hash)),
            (
                "allow_unauthenticated".into(),
                serde_json::json!(allow_unauthenticated),
            ),
            (
                "egress_grants".into(),
                serde_json::json!(egress_environment_grants()),
            ),
        ]),
    );
    let _lifecycle = ServerAuditStop(audit);
    state.set_bind_addr(bound);
    let shutdown = state.shutdown_signal();
    let requests = state.graceful_shutdown();
    let router = app(state);
    crate::metrics::server_start();
    tracing::info!(addr = %bound, protocol = if tls.is_some() { "https" } else { "http" }, "submilli-server listening");

    if let Some(tls) = tls {
        serve_listener(
            crate::tls::Listener::new(listener, tls),
            router,
            signals,
            shutdown,
            requests,
            shutdown_grace,
        )
        .await
    } else {
        serve_listener(
            listener,
            router,
            signals,
            shutdown,
            requests,
            shutdown_grace,
        )
        .await
    }
}

/// SIGTERM and SIGINT, registered for [`serve_embedded`]. An embedder installs them
/// before it announces itself (writes a lock, prints an address), as [`serve`] does
/// before it binds, so a signal sent once it is visible is held for the drain rather
/// than killing the process mid-start.
///
/// A signal this process inherited as ignored (a background job of a
/// non-interactive shell ignores SIGINT; `nohup` ignores SIGHUP) stays ignored: it
/// is not watched, so it neither drains the server nor stops being ignored.
pub struct EmbeddedSignals(ShutdownSignals);

impl EmbeddedSignals {
    /// Register the handlers. Needs a Tokio runtime with signal support.
    pub fn install() -> Result<Self> {
        ShutdownSignals::install_unless_ignored().map(Self)
    }

    /// Whether `signal` is ignored in this process now, as inherited or set. Read
    /// before a handler is installed for it, which would replace the ignoring.
    #[cfg(unix)]
    pub fn is_ignored(signal: libc::c_int) -> Result<bool> {
        // SAFETY: a zeroed sigaction is a valid value for the kernel to fill in; the
        // new action is null, so sigaction only reads the current one into `current`.
        let (read, current) = unsafe {
            let mut current: libc::sigaction = std::mem::zeroed();
            let read = libc::sigaction(signal, std::ptr::null(), &raw mut current);
            (read, current)
        };
        if read != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(current.sa_sigaction == libc::SIG_IGN)
    }
}

/// Serve a state an embedder built and booted, on a listener it bound, until
/// shutdown is requested — by `POST /v1/shutdown`, by notifying
/// [`AppState::shutdown_signal`], or by one of `signals` — then drain as
/// [`serve`] does. For an embedder that runs the server beside listeners of its
/// own, such as the playground; the embedder owns any database it opened.
pub async fn serve_embedded(
    listener: tokio::net::TcpListener,
    state: AppState,
    shutdown_grace: Duration,
    signals: EmbeddedSignals,
) -> Result<()> {
    let EmbeddedSignals(signals) = signals;
    state.set_bind_addr(listener.local_addr()?);
    let shutdown = state.shutdown_signal();
    let requests = state.graceful_shutdown();
    serve_listener(
        listener,
        app(state),
        signals,
        shutdown,
        requests,
        shutdown_grace,
    )
    .await
    .map(|_| ())
}

/// Resolve the blueprint backend and finish migration before application startup.
/// Public so an embedder building its own [`AppState`] selects the same store
/// [`serve`] would for its config.
pub async fn prepare_blueprint_store(
    config: &ServerConfig,
) -> Result<Arc<dyn crate::blueprint::BlueprintStore>> {
    use crate::blueprint::{FileBlueprintStore, InMemoryBlueprintStore, SqliteBlueprintStore};

    if let Some(store) = &config.blueprints {
        return Ok(Arc::clone(store));
    }
    if let Some(database) = &config.database {
        let store = SqliteBlueprintStore::new(Arc::clone(database), config.blueprint_dir.clone());
        store.migrate().await?;
        return Ok(Arc::new(store));
    }
    if let Some(directory) = &config.blueprint_dir {
        return Ok(Arc::new(FileBlueprintStore::new(directory.clone())?));
    }
    Ok(Arc::new(InMemoryBlueprintStore::default()))
}

async fn serve_listener<L>(
    listener: L,
    router: axum::Router,
    signals: ShutdownSignals,
    shutdown: Arc<Notify>,
    requests: Arc<crate::graceful_shutdown::GracefulShutdownTracker>,
    shutdown_grace: Duration,
) -> Result<DrainStatus>
where
    L: axum::serve::Listener<Addr = SocketAddr>,
    for<'a> PeerAddr: axum::extract::connect_info::Connected<axum::serve::IncomingStream<'a, L>>,
{
    let _request_guard = RequestShutdownGuard(Arc::clone(&requests));
    let shutdown_requests = Arc::clone(&requests);
    // The handoff announces that draining has begun. The signal streams stay
    // available for a second signal while database work drains after HTTP.
    let (draining, drain_started) = oneshot::channel();
    let signals = Arc::new(Mutex::new(signals));
    let shutdown_signals = Arc::clone(&signals);
    let force_signals = Arc::clone(&signals);
    let started_at = Arc::new(OnceLock::new());
    let shutdown_started_at = Arc::clone(&started_at);
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<PeerAddr>(),
    )
    .with_graceful_shutdown(async move {
        let mut signals = shutdown_signals.lock().await;
        signals.recv(shutdown).await;
        drop(signals);
        shutdown_requests.close();
        let started_at = tokio::time::Instant::now();
        let _ = shutdown_started_at.set(started_at);
        let _ = draining.send(started_at);
    })
    .into_future();

    let drain = async {
        server.await?;
        requests.close();
        requests.wait().await;
        Ok::<(), std::io::Error>(())
    };
    let forced = tokio::select! {
        // Biased so a drain that finishes as the deadline expires is reported as
        // the clean shutdown it was.
        biased;
        result = drain => { result?; false },
        cause = forced_stop(drain_started, force_signals, shutdown_grace) => {
            tracing::warn!(
                %cause,
                grace_secs = shutdown_grace.as_secs_f64(),
                "stopped waiting for HTTP responses and request tasks"
            );
            true
        }
    };
    if forced {
        return Ok(DrainStatus::Forced);
    }
    Ok(match started_at.get().copied() {
        Some(started_at) => DrainStatus::Completed {
            started_at,
            signals,
        },
        None => DrainStatus::NoDrain,
    })
}

/// Also cancel owned requests if serving fails or its caller drops the future.
struct RequestShutdownGuard(Arc<crate::graceful_shutdown::GracefulShutdownTracker>);

impl Drop for RequestShutdownGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(Clone)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

impl
    axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, tokio::net::TcpListener>>
    for PeerAddr
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, crate::tls::Listener>>
    for PeerAddr
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, crate::tls::Listener>) -> Self {
        Self(*stream.remote_addr())
    }
}

struct ServerAuditStop(crate::audit::AuditLog);

impl Drop for ServerAuditStop {
    fn drop(&mut self) {
        self.0.emit(
            "server",
            serde_json::Map::from_iter([("event".into(), serde_json::json!("stopped"))]),
        );
    }
}

fn egress_environment_grants() -> Vec<&'static str> {
    [
        "SUBMILLI_ALLOW_LOCALHOST",
        "SUBMILLI_ALLOW_PRIVATE",
        "SUBMILLI_ALLOW_IP",
    ]
    .into_iter()
    .filter(|key| {
        let value = std::env::var(key).unwrap_or_default();
        if *key == "SUBMILLI_ALLOW_IP" {
            value.split(',').any(|part| !part.trim().is_empty())
        } else {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }
    })
    .collect()
}

/// Why the drain stopped early.
enum Forced {
    GraceElapsed,
    SecondSignal,
}

impl std::fmt::Display for Forced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GraceElapsed => f.write_str("grace elapsed"),
            Self::SecondSignal => f.write_str("second signal"),
        }
    }
}

/// Resolves once the drain has run out of patience, and never resolves if the
/// drain never begins: the sender is dropped when the server ends for any other
/// reason, and a deadline that fired on that would race the server's own clean
/// return.
async fn forced_stop(
    drain_started: oneshot::Receiver<tokio::time::Instant>,
    signals: Arc<Mutex<ShutdownSignals>>,
    grace: Duration,
) -> Forced {
    let Ok(started_at) = drain_started.await else {
        return std::future::pending().await;
    };
    let mut signals = signals.lock().await;
    tokio::select! {
        () = wait_until(started_at.checked_add(grace)) => Forced::GraceElapsed,
        // An operator signalling twice has stopped waiting; honor that rather
        // than making them reach for SIGKILL.
        () = signals.recv_signal() => Forced::SecondSignal,
    }
}

/// The signals that mean "shut down", held as streams so they are registered
/// before the server starts accepting rather than on first poll.
/// `None` for a signal left ignored ([`EmbeddedSignals`]), which never arrives.
#[cfg(unix)]
struct ShutdownSignals {
    terminate: Option<tokio::signal::unix::Signal>,
    interrupt: Option<tokio::signal::unix::Signal>,
}

#[cfg(unix)]
impl ShutdownSignals {
    fn install() -> Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            terminate: Some(signal(SignalKind::terminate())?),
            interrupt: Some(signal(SignalKind::interrupt())?),
        })
    }

    /// [`Self::install`], leaving a signal this process ignores alone.
    fn install_unless_ignored() -> Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        let watch = |number: libc::c_int, kind: SignalKind| -> Result<_> {
            if EmbeddedSignals::is_ignored(number)? {
                return Ok(None);
            }
            Ok(Some(signal(kind)?))
        };
        Ok(Self {
            terminate: watch(libc::SIGTERM, SignalKind::terminate())?,
            interrupt: watch(libc::SIGINT, SignalKind::interrupt())?,
        })
    }

    async fn recv(&mut self, shutdown: Arc<Notify>) {
        tokio::select! {
            () = shutdown.notified() => tracing::info!("shutdown requested via /v1/shutdown"),
            () = next(&mut self.terminate) => tracing::info!("SIGTERM received"),
            () = next(&mut self.interrupt) => tracing::info!("SIGINT received"),
        }
    }

    async fn recv_signal(&mut self) {
        tokio::select! {
            () = next(&mut self.terminate) => {},
            () = next(&mut self.interrupt) => {},
        }
    }
}

/// The next delivery of a watched signal; never, for one not watched.
#[cfg(unix)]
async fn next(signal: &mut Option<tokio::signal::unix::Signal>) {
    match signal {
        Some(signal) => {
            signal.recv().await;
        }
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;

#[cfg(not(unix))]
struct ShutdownSignals;

#[cfg(not(unix))]
impl ShutdownSignals {
    fn install() -> Result<Self> {
        Ok(Self)
    }

    fn install_unless_ignored() -> Result<Self> {
        Ok(Self)
    }

    async fn recv(&mut self, shutdown: Arc<Notify>) {
        tokio::select! {
            () = shutdown.notified() => tracing::info!("shutdown requested via /v1/shutdown"),
            _ = tokio::signal::ctrl_c() => tracing::info!("interrupt received"),
        }
    }

    async fn recv_signal(&mut self) {
        let _ = tokio::signal::ctrl_c().await;
    }
}
