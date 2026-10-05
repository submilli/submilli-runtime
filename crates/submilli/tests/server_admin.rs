//! End-to-end integration tests for `submilli server status` / `submilli server
//! stop` against a running server's admin endpoints.

#[path = "../../submilli-server/tests/common/in_memory_config.rs"]
mod in_memory_config;

use std::path::PathBuf;
use std::process::{Command, Output};

use submilli_server::{ApiToken, AppState, AuthConfig, Role, ServerConfig, app};

const ADMIN_TOKEN: &str = "admin-token-0123456789abcdef0123456789";
const USER_TOKEN: &str = "user-token-0123456789abcdef01234567890";

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `submilli server <args> --server <server>`, with the token settings a
/// developer's shell may carry cleared so only `env` supplies one.
fn server_command(args: &[&str], server: &str, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(submilli_bin());
    command
        .arg("server")
        .args(args)
        .args(["--server", server])
        .env_remove("SUBMILLI_SERVER_TOKEN")
        .env_remove("SUBMILLI_SERVER_TOKEN_FILE");
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("invoke submilli server")
}

fn status(server: &str) -> Output {
    server_command(&["status"], server, &[])
}

fn stop(server: &str) -> Output {
    server_command(&["stop"], server, &[])
}

async fn spawn_server() -> (String, std::sync::Arc<tokio::sync::Notify>) {
    spawn_server_with(in_memory_config::config()).await
}

/// A server that requires a token: one admin and one user.
async fn spawn_authenticated_server() -> (String, std::sync::Arc<tokio::sync::Notify>) {
    let token = |name, role, token| ApiToken::new(name, role, token).expect("valid token");
    spawn_server_with(ServerConfig {
        auth: AuthConfig::Tokens(vec![
            token("ops", Role::Admin, ADMIN_TOKEN),
            token("app", Role::User, USER_TOKEN),
        ]),
        ..in_memory_config::config()
    })
    .await
}

/// Bind an ephemeral port and serve with graceful shutdown wired, returning the
/// base URL and the shutdown signal so the test can stop it.
async fn spawn_server_with(config: ServerConfig) -> (String, std::sync::Arc<tokio::sync::Notify>) {
    let state = AppState::new(config).expect("AppState");
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_against_no_server_reports_stopped() {
    // Nothing bound on this port.
    let out = tokio::task::spawn_blocking(|| status("http://127.0.0.1:1"))
        .await
        .unwrap();
    assert!(!out.status.success());
    assert!(stdout(&out).contains("stopped"));
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_token_is_reported_as_such_not_as_a_stopped_server() {
    let (server, shutdown) = spawn_authenticated_server().await;
    let run = |env: Vec<(&'static str, &'static str)>| {
        let server = server.clone();
        tokio::task::spawn_blocking(move || server_command(&["status"], &server, &env))
    };

    let anonymous = run(vec![]).await.unwrap();
    assert!(!anonymous.status.success());
    assert!(!stdout(&anonymous).contains("stopped"));
    let message = stderr(&anonymous);
    assert!(message.contains("SUBMILLI_SERVER_TOKEN"), "{message}");
    assert!(message.contains("SUBMILLI_SERVER_TOKEN_FILE"), "{message}");

    let as_user = run(vec![("SUBMILLI_SERVER_TOKEN", USER_TOKEN)])
        .await
        .unwrap();
    assert!(!as_user.status.success());
    let message = stderr(&as_user);
    assert!(message.contains("`admin`"), "{message}");
    assert!(!message.contains(USER_TOKEN), "{message}");

    shutdown.notify_one();
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_token_comes_from_the_environment_or_a_file() {
    let (server, _shutdown) = spawn_authenticated_server().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let token_file = dir.path().join("admin.token");
    std::fs::write(&token_file, format!("{ADMIN_TOKEN}\n")).expect("write token");
    let token_file = token_file.to_str().expect("utf-8 path").to_owned();

    let from_env = tokio::task::spawn_blocking({
        let server = server.clone();
        move || {
            server_command(
                &["status"],
                &server,
                &[("SUBMILLI_SERVER_TOKEN", ADMIN_TOKEN)],
            )
        }
    })
    .await
    .unwrap();
    assert!(from_env.status.success(), "stderr: {}", stderr(&from_env));
    assert!(stdout(&from_env).contains("running"));

    // The file wins over the variable, which here holds a token too weak for
    // the command.
    let from_file = tokio::task::spawn_blocking({
        let server = server.clone();
        let token_file = token_file.clone();
        move || {
            server_command(
                &["status", "--token-file", &token_file],
                &server,
                &[("SUBMILLI_SERVER_TOKEN", USER_TOKEN)],
            )
        }
    })
    .await
    .unwrap();
    assert!(from_file.status.success(), "stderr: {}", stderr(&from_file));

    let stopped = tokio::task::spawn_blocking(move || {
        server_command(
            &["stop"],
            &server,
            &[("SUBMILLI_SERVER_TOKEN_FILE", &token_file)],
        )
    })
    .await
    .unwrap();
    assert!(stopped.status.success(), "stderr: {}", stderr(&stopped));
    assert!(stdout(&stopped).contains("stopped"));
}

/// `status` run directly rather than through `server_command`, which always
/// passes `--server`: the point here is what the variables alone do. A blank
/// URL must mean the default address, and blank token settings must mean "no
/// token" rather than a file to read. Only what must not appear is asserted:
/// the rest of the output depends on whatever is listening at the default
/// address. Skipped with the HTTP tests because it connects there.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn exported_but_blank_settings_count_as_unset() {
    for blank in ["", "  "] {
        let out = Command::new(submilli_bin())
            .args(["server", "status"])
            .env("SUBMILLI_SERVER_URL", blank)
            .env("SUBMILLI_SERVER_TOKEN_FILE", blank)
            .env("SUBMILLI_SERVER_TOKEN", blank)
            .output()
            .expect("invoke submilli server status");
        let text = format!("{}{}", stdout(&out), stderr(&out));
        assert!(!text.contains("token file"), "{blank:?}: {text}");
        assert!(
            !text.contains(&format!("(no server at {blank})")),
            "{blank:?}: {text}"
        );
    }
}

/// The CLI refuses what it cannot send or was pointed at by mistake, instead
/// of calling the server without a token.
#[test]
fn a_bad_token_source_is_reported_before_any_request() {
    let dir = tempfile::tempdir().expect("temp dir");
    let empty = dir.path().join("empty.token");
    std::fs::write(&empty, "\n").expect("write token");
    let out = server_command(
        &[
            "status",
            "--token-file",
            empty.to_str().expect("utf-8 path"),
        ],
        "http://127.0.0.1:1",
        &[("SUBMILLI_SERVER_TOKEN", ADMIN_TOKEN)],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("is empty"), "{}", stderr(&out));

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let out = Command::new(submilli_bin())
            .args(["server", "status", "--server", "http://127.0.0.1:1"])
            .env_remove("SUBMILLI_SERVER_TOKEN_FILE")
            .env(
                "SUBMILLI_SERVER_TOKEN",
                std::ffi::OsStr::from_bytes(b"\xff\xfe"),
            )
            .output()
            .expect("invoke submilli server status");
        assert!(!out.status.success());
        assert!(stderr(&out).contains("cannot be sent"), "{}", stderr(&out));
        assert!(!stdout(&out).contains("stopped"));
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsendable_token_is_an_error_not_a_stopped_server() {
    let (server, shutdown) = spawn_authenticated_server().await;
    let bad = format!("\u{e9}{ADMIN_TOKEN}");
    for command in ["status", "stop"] {
        let out = tokio::task::spawn_blocking({
            let (server, bad) = (server.clone(), bad.clone());
            move || server_command(&[command], &server, &[("SUBMILLI_SERVER_TOKEN", &bad)])
        })
        .await
        .unwrap();
        assert!(!out.status.success(), "{command}");
        assert!(!stdout(&out).contains("stopped"), "{command}");
        assert!(stderr(&out).contains("cannot be sent"), "{command}");
    }
    shutdown.notify_one();
}
