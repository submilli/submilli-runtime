//! The playground's server listener: its runtime, its routes, and how it stops.
//!
//! The stop semantics follow `submilli-server`'s `serve.rs` (`serve_embedded`,
//! `EmbeddedSignals`, `WatchedSignal`): a stop or the first SIGTERM or SIGINT
//! begins a drain, in-flight requests get the grace period to finish, and a
//! second signal stops waiting at once. A signal this process inherited as
//! ignored stays ignored.

use std::future::IntoFuture;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use tokio::sync::{Notify, mpsc, oneshot};

use super::super::log::warn;

/// The multi-thread runtime the host runs on. Programs run on its threads, so
/// their stacks are sized for the Wasm stack rather than tokio's 2 MiB default.
pub(crate) fn runtime(native_stack_size: usize) -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(native_stack_size)
        .build()
}

/// Bind the server listener, on loopback only.
pub(crate) async fn bind() -> Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .context("binding the server listener")
}

/// The routes every server listener has: `/healthz`, public, because `submilli
/// playground status` probes it without a token.
pub(crate) fn router() -> Router {
    Router::new().route("/healthz", get(|| async { StatusCode::OK }))
}

/// [`router`] with the MCP endpoint, `/mcp/{blueprint}`, mounted behind the
/// playground's tokens.
pub(crate) fn router_with_mcp(mcp: Arc<super::mcp::McpHost>) -> Router {
    router().merge(super::mcp::routes(mcp))
}

/// What asks the listener to stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The playground was told to stop.
    Requested,
    Signal(libc::c_int),
}

/// Every stop, in the order it arrived: requests through a [`Notify`], and SIGTERM
/// and SIGINT, which are registered when this is made.
pub(crate) struct Stops {
    received: mpsc::UnboundedReceiver<Stop>,
}

impl Stops {
    /// Watch `requested` and the stop signals. Installed before the instance
    /// announces itself, so a signal from then on drains it rather than killing
    /// it with its record on disk. Needs a Tokio runtime with signal support.
    pub(crate) fn install(requested: Arc<Notify>) -> Result<Self> {
        use tokio::signal::unix::SignalKind;
        let (sender, stops) = Self::channel();
        for (kind, signal) in [
            (SignalKind::terminate(), libc::SIGTERM),
            (SignalKind::interrupt(), libc::SIGINT),
        ] {
            let mut watched = WatchedSignal::unless_ignored(kind)?;
            let sender = sender.clone();
            tokio::spawn(async move {
                loop {
                    watched.next().await;
                    if sender.send(Stop::Signal(signal)).is_err() {
                        return;
                    }
                }
            });
        }
        tokio::spawn(async move {
            loop {
                requested.notified().await;
                if sender.send(Stop::Requested).is_err() {
                    return;
                }
            }
        });
        Ok(stops)
    }

    pub(crate) fn channel() -> (mpsc::UnboundedSender<Stop>, Self) {
        let (sender, received) = mpsc::unbounded_channel();
        (sender, Self { received })
    }

    /// The next stop; never, once nothing can send one.
    async fn next(&mut self) -> Stop {
        match self.received.recv().await {
            Some(stop) => stop,
            None => std::future::pending().await,
        }
    }

    async fn next_signal(&mut self) {
        while self.next().await == Stop::Requested {}
    }
}

/// How serving ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drained {
    /// Every request in flight finished.
    Completed,
    /// The drain stopped waiting for what remained.
    Forced(Forced),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Forced {
    GraceElapsed,
    SecondSignal,
}

/// Serve `router` on `listener` until the first stop, then let in-flight requests
/// finish for at most `grace` before dropping what remains. A second signal
/// skips the remaining wait.
pub(crate) async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    mut stops: Stops,
    grace: Duration,
) -> Result<Drained> {
    let (draining, drain_started) = oneshot::channel();
    let (stops_back, stops_returned) = oneshot::channel();
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            stops.next().await;
            let _ = draining.send(());
            let _ = stops_back.send(stops);
        })
        .into_future();
    tokio::select! {
        // Biased so a drain that finishes as the deadline expires is reported as
        // the clean shutdown it was.
        biased;
        result = server => {
            result.context("serving the server listener")?;
            Ok(Drained::Completed)
        }
        forced = forced_stop(drain_started, stops_returned, grace) => {
            warn(&format!(
                "stopped waiting for requests in flight after {}",
                match forced {
                    Forced::GraceElapsed => "the grace period",
                    Forced::SecondSignal => "a second signal",
                }
            ));
            Ok(Drained::Forced(forced))
        }
    }
}

/// Resolves once the drain has run out of patience, and never if the drain never
/// begins: a deadline that fired then would race the server's own return.
async fn forced_stop(
    drain_started: oneshot::Receiver<()>,
    stops: oneshot::Receiver<Stops>,
    grace: Duration,
) -> Forced {
    let (Ok(()), Ok(mut stops)) = (drain_started.await, stops.await) else {
        return std::future::pending().await;
    };
    tokio::select! {
        () = tokio::time::sleep(grace) => Forced::GraceElapsed,
        // Someone signalling twice has stopped waiting; honor that rather than
        // making them reach for SIGKILL.
        () = stops.next_signal() => Forced::SecondSignal,
    }
}

/// One signal, held as a stream registered when it is made, or not watched at all
/// when this process ignores it and should keep ignoring it.
pub(crate) struct WatchedSignal(Option<tokio::signal::unix::Signal>);

impl WatchedSignal {
    /// Watch `kind` unless this process ignores it now, which is then left ignored:
    /// a background job of a non-interactive shell ignores SIGINT, and installing a
    /// handler would undo that.
    pub(crate) fn unless_ignored(kind: tokio::signal::unix::SignalKind) -> Result<Self> {
        if is_ignored(kind.as_raw_value())? {
            return Ok(Self(None));
        }
        Ok(Self(Some(
            tokio::signal::unix::signal(kind).context("watching for a stop signal")?,
        )))
    }

    /// The next delivery of the signal; never, for one left ignored.
    pub(crate) async fn next(&mut self) {
        match &mut self.0 {
            Some(signal) => {
                signal.recv().await;
            }
            None => std::future::pending().await,
        }
    }
}

/// Whether `signal` is ignored in this process now, as inherited or set. Read
/// before a handler is installed for it, which would replace the ignoring.
fn is_ignored(signal: libc::c_int) -> Result<bool> {
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

#[cfg(test)]
mod tests;
