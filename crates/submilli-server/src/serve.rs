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
    config: ServerConfig,
    shutdown_grace: Duration,
    signals: ShutdownSignals,
) -> Result<DrainStatus> {
    crate::auth::log_auth_posture(addr.ip(), &config.auth);
    let tls = config.tls.clone();
    let settings_hash = crate::audit::settings_hash(&config, addr, shutdown_grace);
    let allow_unauthenticated = matches!(config.auth, crate::auth::AuthConfig::Disabled);
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
    let router = app(state);
    crate::metrics::server_start();
    tracing::info!(addr = %bound, protocol = if tls.is_some() { "https" } else { "http" }, "submilli-server listening");

    if let Some(tls) = tls {
        serve_listener(
            crate::tls::Listener::new(listener, tls),
            router,
            signals,
            shutdown,
            shutdown_grace,
        )
        .await
    } else {
        serve_listener(listener, router, signals, shutdown, shutdown_grace).await
    }
}

async fn serve_listener<L>(
    listener: L,
    router: axum::Router,
    signals: ShutdownSignals,
    shutdown: Arc<Notify>,
    shutdown_grace: Duration,
) -> Result<DrainStatus>
where
    L: axum::serve::Listener<Addr = SocketAddr>,
    for<'a> PeerAddr: axum::extract::connect_info::Connected<axum::serve::IncomingStream<'a, L>>,
{
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
        let started_at = tokio::time::Instant::now();
        let _ = shutdown_started_at.set(started_at);
        let _ = draining.send(started_at);
    })
    .into_future();

    let forced = tokio::select! {
        // Biased so a drain that finishes as the deadline expires is reported as
        // the clean shutdown it was.
        biased;
        result = server => { result?; false },
        cause = forced_stop(drain_started, force_signals, shutdown_grace) => {
            tracing::warn!(
                %cause,
                grace_secs = shutdown_grace.as_secs_f64(),
                "stopped waiting with requests still in flight; their connections are being dropped"
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
#[cfg(unix)]
struct ShutdownSignals {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl ShutdownSignals {
    fn install() -> Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
        })
    }

    async fn recv(&mut self, shutdown: Arc<Notify>) {
        tokio::select! {
            () = shutdown.notified() => tracing::info!("shutdown requested via /v1/shutdown"),
            _ = self.terminate.recv() => tracing::info!("SIGTERM received"),
            _ = self.interrupt.recv() => tracing::info!("SIGINT received"),
        }
    }

    async fn recv_signal(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {},
            _ = self.interrupt.recv() => {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn forced_database_drain_releases_lock_after_work_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.db");
        let database = Arc::new(crate::database::ServerDatabase::open(&path).await.unwrap());
        close_database(database, Ok(DrainStatus::Forced), Duration::ZERO)
            .await
            .unwrap();
        let reopened = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match crate::database::ServerDatabase::open(&path).await {
                    Ok(database) => break database,
                    Err(crate::database::DatabaseError::AlreadyOpen(_)) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("unexpected database error: {error}"),
                }
            }
        })
        .await
        .unwrap();
        reopened.close().await.unwrap();
    }
}

#[cfg(not(unix))]
struct ShutdownSignals;

#[cfg(not(unix))]
impl ShutdownSignals {
    fn install() -> Result<Self> {
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
