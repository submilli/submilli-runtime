//! End-to-end integration tests for `submilli server run-code`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;

use submilli_blueprint::Blueprint;
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};

const BLUEPRINT_NAME: &str = "test";

async fn spawn_server() -> String {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        ..Default::default()
    }]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        ..ServerConfig::default()
    };
    let state = AppState::new(config).expect("AppState");
    let router = app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("axum::serve");
    });
    format!("http://{addr}")
}

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn write_script(name: &str, source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("submilli-runcode-{name}.subm"));
    fs::write(&path, source).expect("write temp script");
    path
}

fn run_code(server: &str, name: &str, source: &str) -> Output {
    let path = write_script(name, source);
    Command::new(submilli_bin())
        .args([
            "server",
            "run-code",
            "--server",
            server,
            "--blueprint",
            BLUEPRINT_NAME,
            path.to_str().expect("path utf-8"),
        ])
        .output()
        .expect("invoke submilli server run-code")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn string_return() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "string_return",
            r#"function main(): string { return "hello"; }"#,
        )
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "hello\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn number_return() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "number_return",
            "function main(): number { return 42; }",
        )
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boolean_return() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "boolean_return",
            "function main(): boolean { return true; }",
        )
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "true\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn void_with_stdio() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "void_with_stdio",
            r#"function main(): void { console.log("hi"); console.log("there"); }"#,
        )
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // Result is null for void main; console reaches us via
    // /v1/last-run and lands on stderr.
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "hi\nthere\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn void_empty() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(&server, "void_empty", "function main(): void { }")
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compile_error() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "compile_error",
            "function main(): string { return 1; }",
        )
    })
    .await
    .unwrap();
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
    let err = stderr(&out);
    assert!(err.contains("error:"), "stderr: {err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_trap_with_console() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "runtime_trap_with_console",
            r#"function main(): void { console.log("before"); assert(false, "boom"); }"#,
        )
    })
    .await
    .unwrap();
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("before"), "stderr missing console line: {err}");
    assert!(
        err.contains("assert") || err.contains("boom"),
        "stderr missing trap detail: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreachable_server() {
    // Port 1 is privileged — nothing can bind to it in tests, so connection refuses immediately.
    let server = "http://127.0.0.1:1".to_string();
    let out = tokio::task::spawn_blocking(move || {
        run_code(
            &server,
            "unreachable_server",
            "function main(): number { return 1; }",
        )
    })
    .await
    .unwrap();
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "stderr: {err}");
}
