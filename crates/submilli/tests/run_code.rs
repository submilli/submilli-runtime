//! End-to-end integration tests for `submilli server run-code`.

#[path = "../../submilli-server/tests/common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;

use submilli_blueprint::{Action, Blueprint, PermissionRule};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};

const BLUEPRINT_NAME: &str = "test";

async fn spawn_server() -> String {
    // Session state is granted so a program in an opened session can leave
    // something for the next one.
    let rules = ["session.read", "session.write"]
        .into_iter()
        .map(|capability| PermissionRule {
            capability: capability.into(),
            filter: None,
            action: Action::Allow,
        })
        .collect();
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([Blueprint {
            name: BLUEPRINT_NAME.into(),
            permissions: BTreeMap::from([("main".to_string(), rules)]),
            ..Default::default()
        }])
        .expect("seed blueprints"),
    );
    let config = ServerConfig {
        blueprints: Some(blueprints),
        ..in_memory_config::config()
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

fn submilli(args: &[&str]) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .output()
        .expect("invoke submilli")
}

fn run_in_session(server: &str, session: &str, name: &str, source: &str) -> Output {
    let path = write_script(name, source);
    submilli(&[
        "server",
        "run-code",
        "--server",
        server,
        "--session",
        session,
        path.to_str().expect("path utf-8"),
    ])
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

const REMEMBER: &str = r#"import * as session from "submilli:session";
function main(): string { session.set("progress", "cus_initech"); return "saved"; }"#;
const RECALL: &str = r#"import * as session from "submilli:session";
function main(): string { const v = session.get<string | null>("progress"); return v ?? "nothing"; }"#;

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opened_session_keeps_state_between_runs_until_closed() {
    let server = spawn_server().await;
    tokio::task::spawn_blocking(move || {
        let open = submilli(&[
            "server",
            "session",
            "open",
            "--server",
            &server,
            "--blueprint",
            BLUEPRINT_NAME,
        ]);
        assert!(open.status.success(), "stderr: {}", stderr(&open));
        let session = stdout(&open).trim().to_owned();
        assert!(!session.is_empty());

        let saved = run_in_session(&server, &session, "session_remember", REMEMBER);
        assert!(saved.status.success(), "stderr: {}", stderr(&saved));
        assert_eq!(stdout(&saved), "saved\n");

        let recalled = run_in_session(&server, &session, "session_recall", RECALL);
        assert!(recalled.status.success(), "stderr: {}", stderr(&recalled));
        assert_eq!(stdout(&recalled), "cus_initech\n");

        let close = submilli(&["server", "session", "close", "--server", &server, &session]);
        assert!(close.status.success(), "stderr: {}", stderr(&close));

        let after = run_in_session(&server, &session, "session_after_close", RECALL);
        assert!(!after.status.success());
        assert!(
            stderr(&after).contains("unknown session"),
            "stderr: {}",
            stderr(&after)
        );

        let again = submilli(&["server", "session", "close", "--server", &server, &session]);
        assert!(!again.status.success());
        assert!(
            stderr(&again).contains("no session"),
            "stderr: {}",
            stderr(&again)
        );
    })
    .await
    .unwrap();
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_a_session_for_an_unknown_blueprint_fails() {
    let server = spawn_server().await;
    let out = tokio::task::spawn_blocking(move || {
        submilli(&[
            "server",
            "session",
            "open",
            "--server",
            &server,
            "--blueprint",
            "missing",
        ])
    })
    .await
    .unwrap();
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
    assert!(stderr(&out).contains("error:"), "stderr: {}", stderr(&out));
}

#[test]
fn session_excludes_blueprint_and_vars() {
    for extra in [["--blueprint", "x"], ["--var", "a=b"]] {
        let out = submilli(&[
            "server",
            "run-code",
            "--session",
            "s",
            extra[0],
            extra[1],
            "script.ts",
        ]);
        assert!(!out.status.success());
        assert!(
            stderr(&out).contains("cannot be used with"),
            "stderr: {}",
            stderr(&out)
        );
    }
}
