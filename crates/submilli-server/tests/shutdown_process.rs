//! Shutdown as a container runtime sees it: the whole *process* must be gone
//! within its budget, not merely `serve()` returning.
//!
//! `tests/shutdown.rs` covers the drain itself in-process. That is the wrong
//! altitude for the guarantee `--shutdown-grace` actually makes, because two
//! more stages run after `serve()` returns and neither is visible from inside
//! it: axum spawns a task per connection, so the dropped server future leaves
//! work running for the runtime to reap, and the telemetry guard flushes on
//! drop. Both stack on top of the grace, and the container runtime is timing
//! all three.
//!
//! What these cover: the end-to-end budget, that a second signal cuts the wait
//! short, and that the grace is a ceiling rather than a sleep. What they do
//! *not* cover: `shutdown_timeout` specifically. A request parked in an
//! extractor is an async task the runtime cancels cleanly, so removing that
//! bound still passes here. Reaching it needs work the runtime cannot cancel —
//! a `spawn_blocking` package install, or an interpreter run that never yields
//! — which is why the bound stays as defense rather than something these prove.

#![cfg(unix)]

mod common;

use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::process::Child;
use std::time::{Duration, Instant};

use common::{free_port, signal, spawn_server, wait_for_exit, wait_ready};

const GRACE_SECS: u64 = 2;

/// The two post-drain stages the binary bounds internally, plus room for
/// process teardown on a loaded machine.
const TEARDOWN_ALLOWANCE: Duration = Duration::from_secs(4);

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn sigterm_exits_the_process_within_the_grace_budget() {
    let home = tempfile::tempdir().expect("temp home");
    let port = free_port();
    let mut server = spawn(&home, port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    wait_ready(port);

    // Hold a request open so the drain has something to wait for; without this
    // the server exits immediately and the budget is never exercised.
    let mut stalled = TcpStream::connect(addr).expect("connect");
    stalled
        .write_all(
            b"POST /v1/execute HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
              Content-Length: 4096\r\n\r\n{\"blue",
        )
        .expect("write partial request");
    stalled.flush().expect("flush");
    std::thread::sleep(Duration::from_millis(250));

    let start = Instant::now();
    signal(&server, libc::SIGTERM);
    let status = wait_for_exit(&mut server, Duration::from_secs(30));
    let elapsed = start.elapsed();

    let budget = Duration::from_secs(GRACE_SECS) + TEARDOWN_ALLOWANCE;
    assert!(
        elapsed < budget,
        "process took {elapsed:?} to exit with a request in flight; \
         --shutdown-grace is {GRACE_SECS}s and every stage after the drain is \
         supposed to be bounded too"
    );
    assert!(
        elapsed >= Duration::from_secs(GRACE_SECS) - Duration::from_millis(400),
        "process exited after only {elapsed:?}; the in-flight request should \
         have held the drain for the full grace"
    );
    assert!(
        status.success(),
        "expected a clean exit, got {:?}",
        status.code()
    );
}

/// A second signal means the operator has stopped waiting — it must not require
/// reaching for SIGKILL.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn a_second_sigterm_skips_the_remaining_grace() {
    let home = tempfile::tempdir().expect("temp home");
    let port = free_port();
    let mut server = spawn(&home, port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    wait_ready(port);

    let mut stalled = TcpStream::connect(addr).expect("connect");
    stalled
        .write_all(
            b"POST /v1/execute HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
              Content-Length: 4096\r\n\r\n{\"blue",
        )
        .expect("write partial request");
    stalled.flush().expect("flush");
    std::thread::sleep(Duration::from_millis(250));

    let start = Instant::now();
    signal(&server, libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(150));
    signal(&server, libc::SIGTERM);
    wait_for_exit(&mut server, Duration::from_secs(30));
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(GRACE_SECS),
        "second signal took {elapsed:?}; it should have cut the {GRACE_SECS}s \
         grace short rather than being swallowed"
    );
}

/// A long grace the test never waits out: proves the process is not simply
/// sleeping for the full budget regardless of whether work is in flight.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_idle_server_exits_immediately_regardless_of_the_grace() {
    let home = tempfile::tempdir().expect("temp home");
    let port = free_port();
    let mut server = spawn_server(home.path(), port, 60, &[]);
    wait_ready(port);

    let start = Instant::now();
    signal(&server, libc::SIGTERM);
    let status = wait_for_exit(&mut server, Duration::from_secs(30));
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "idle server took {elapsed:?} to exit under a 60s grace; the grace is a \
         ceiling on waiting for work, not a sleep"
    );
    assert!(status.success(), "expected a clean exit");
}

fn spawn(home: &tempfile::TempDir, port: u16) -> Child {
    spawn_server(home.path(), port, GRACE_SECS, &[])
}
