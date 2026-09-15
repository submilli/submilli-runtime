//! End-to-end integration tests for `submilli server status` / `submilli server
//! stop` against a running server's admin endpoints.

use std::path::PathBuf;
use std::process::{Command, Output};

use submilli_server::{AppState, app};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn status(server: &str) -> Output {
    Command::new(submilli_bin())
        .args(["server", "status", "--server", server])
        .output()
        .expect("invoke submilli server status")
}

fn stop(server: &str) -> Output {
    Command::new(submilli_bin())
        .args(["server", "stop", "--server", server])
        .output()
        .expect("invoke submilli server stop")
}

/// Bind an ephemeral port and serve with graceful shutdown wired, returning the
/// base URL, the bound address, and the shutdown signal so the test can stop it.
async fn spawn_server() -> (String, std::sync::Arc<tokio::sync::Notify>) {
    let state = AppState::new(submilli_server::ServerConfig::default()).expect("AppState");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    state.set_bind_addr(addr);
    let shutdown = state.shutdown_signal();
    let router = app(state);
    let drain = shutdown.clone();
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { drain.notified().await })
            .await
            .expect("axum::serve");
    });
    (format!("http://{addr}"), shutdown)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_reports_running() {
    let (server, shutdown) = spawn_server().await;
    let out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || status(&server)
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("running"), "missing running: {text}");
    assert!(
        text.contains("active sessions:"),
        "missing sessions: {text}"
    );
    shutdown.notify_one();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_drains_and_status_then_reports_stopped() {
    let (server, _shutdown) = spawn_server().await;

    let stop_out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || stop(&server)
    })
    .await
    .unwrap();
    assert!(stop_out.status.success(), "stderr: {}", stderr(&stop_out));
    assert!(stdout(&stop_out).contains("stopped"));

    // After draining, status can no longer reach the server.
    let status_out = tokio::task::spawn_blocking(move || status(&server))
        .await
        .unwrap();
    assert!(!status_out.status.success());
    assert!(stdout(&status_out).contains("stopped"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_against_no_server_reports_stopped() {
    // Nothing bound on this port.
    let out = tokio::task::spawn_blocking(|| status("http://127.0.0.1:1"))
        .await
        .unwrap();
    assert!(!out.status.success());
    assert!(stdout(&out).contains("stopped"));
}
