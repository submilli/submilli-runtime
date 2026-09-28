//! Graceful shutdown for `submilli-server`: SIGTERM and SIGINT drain the same
//! way `POST /v1/shutdown` does, and the drain is bounded.
//!
//! Every scenario shares one `#[tokio::test]`. Raising a signal is process-wide,
//! so parallel tests would receive each other's signals, and a raise landing
//! before any handler is installed would take the harness down with the default
//! disposition.

#![cfg(unix)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{ServerConfig, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

/// Short enough to keep the bounded-drain scenario quick, long enough that the
/// prompt-shutdown scenarios can't accidentally satisfy the "waited" assertion.
const GRACE: Duration = Duration::from_millis(600);

/// Ceiling for a shutdown that should be prompt — generous relative to the work
/// involved, but far below `GRACE`.
const PROMPT: Duration = Duration::from_millis(250);

struct Server {
    addr: SocketAddr,
    task: JoinHandle<anyhow::Result<()>>,
    _home: tempfile::TempDir,
}

impl Server {
    /// Waits for `serve` to return and reports how long it took.
    async fn joined(self) -> Duration {
        let start = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("serve() did not return within 10s")
            .expect("serve task panicked");
        result.expect("serve() returned an error");
        start.elapsed()
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_paths() {
    sigterm_drains().await;
    sigint_drains().await;
    http_shutdown_still_drains().await;
    in_flight_request_is_drained_then_bounded().await;
}

/// SIGTERM reaches the same drain `POST /v1/shutdown` uses, and an idle server
/// takes nowhere near the grace period to get there.
async fn sigterm_drains() {
    let server = start().await;
    raise(libc::SIGTERM);
    let elapsed = server.joined().await;
    assert!(
        elapsed < PROMPT,
        "idle server took {elapsed:?} to drain; the grace deadline should not be waited out"
    );
}

async fn sigint_drains() {
    let server = start().await;
    raise(libc::SIGINT);
    let elapsed = server.joined().await;
    assert!(elapsed < PROMPT, "idle server took {elapsed:?} to drain");
}

/// The pre-existing HTTP shutdown path still works — folding signals into the
/// same `Notify` must not displace it.
async fn http_shutdown_still_drains() {
    let server = start().await;
    let response = request(
        server.addr,
        "POST /v1/shutdown HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "unexpected /v1/shutdown response: {response}"
    );
    let elapsed = server.joined().await;
    assert!(elapsed < PROMPT, "idle server took {elapsed:?} to drain");
}

/// A request the server is still reading holds the drain open — proving the
/// signal triggers a *graceful* shutdown rather than severing connections — but
/// only until the grace deadline, so a long execution can't outlive Docker's
/// own 10s window and get SIGKILLed instead.
async fn in_flight_request_is_drained_then_bounded() {
    let server = start().await;

    // Declare more body than we send, so the `Json` extractor in the execute
    // handler stays parked waiting for the rest.
    let mut stalled = TcpStream::connect(server.addr).await.expect("connect");
    stalled
        .write_all(
            b"POST /v1/execute HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
              Content-Length: 4096\r\n\r\n{\"blue",
        )
        .await
        .expect("write partial request");
    stalled.flush().await.expect("flush");
    // Give the connection task time to read those bytes, so the signal lands
    // against a busy connection rather than an idle one hyper would just close.
    tokio::time::sleep(Duration::from_millis(150)).await;

    raise(libc::SIGTERM);
    let elapsed = server.joined().await;

    assert!(
        elapsed >= GRACE - PROMPT,
        "shutdown returned after {elapsed:?} with a request still in flight; \
         the drain should have waited for it"
    );
    assert!(
        elapsed < GRACE + Duration::from_secs(2),
        "shutdown took {elapsed:?}; the drain should be bounded by the grace period"
    );
}

fn raise(signal: libc::c_int) {
    // SAFETY: raising at the process level is the only way to exercise the
    // handler; every scenario has already confirmed the server is accepting,
    // which means `serve` installed its handlers.
    let rc = unsafe { libc::raise(signal) };
    assert_eq!(rc, 0, "raise({signal}) failed");
}

/// Starts a server on a free loopback port and returns once it accepts
/// connections. `serve` registers its signal handlers before binding, so an
/// accepted connection is also proof that a raise is safe.
async fn start() -> Server {
    let home = tempfile::tempdir().expect("temp home");
    let config = ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::default())),
        session_storage_root: Some(home.path().join("vfs")),
        package_store_root: Some(home.path().join("packages")),
        ..ServerConfig::default()
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], free_port()));
    let task = tokio::spawn(serve(addr, config, GRACE));

    for _ in 0..500 {
        if TcpStream::connect(addr).await.is_ok() {
            return Server {
                addr,
                task,
                _home: home,
            };
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("server never accepted a connection on {addr}");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind probe listener")
        .local_addr()
        .expect("probe local_addr")
        .port()
}

/// Sends a raw request and reads until the server closes the connection, so the
/// caller must include `Connection: close`.
async fn request(addr: SocketAddr, raw: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream.write_all(raw.as_bytes()).await.expect("write");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    response
}
