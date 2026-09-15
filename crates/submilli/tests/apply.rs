//! End-to-end tests for `submilli apply` against a running submilli-server
//! using blueprint files and multi-document YAML streams.

use std::path::{Path, PathBuf};
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

fn apply(path: &Path, env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(submilli_bin());
    cmd.args(["apply", "-f"]).arg(path);
    cmd.env_remove("SUBMILLI_SERVER_URL");
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.output().expect("invoke submilli apply")
}

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

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn applies_and_reapplies_a_blueprint() {
    let (server, shutdown) = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(dir.path(), "prod.yaml", "kind: blueprint\nname: prod\n");

    let out = tokio::task::spawn_blocking({
        let (file, server) = (file.clone(), server.clone());
        move || apply(&file, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("Added blueprint 'prod'"));

    let out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || apply(&file, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("Updated blueprint 'prod'"));
    shutdown.notify_one();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn applies_every_document_in_a_multi_doc_file() {
    let (server, shutdown) = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(
        dir.path(),
        "all.yaml",
        "name: alpha\n---\nkind: blueprint\nname: beta\n",
    );

    let out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || apply(&file, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Added blueprint 'alpha'"), "{text}");
    assert!(text.contains("Added blueprint 'beta'"), "{text}");
    shutdown.notify_one();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn applies_a_directory_of_files() {
    let (server, shutdown) = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "b.yaml", "name: bee\n");
    write(dir.path(), "a.yml", "name: ay\n");
    write(dir.path(), "notes.txt", "not yaml\n");

    let out = tokio::task::spawn_blocking({
        let (dir_path, server) = (dir.path().to_path_buf(), server.clone());
        move || apply(&dir_path, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Added blueprint 'ay'"), "{text}");
    assert!(text.contains("Added blueprint 'bee'"), "{text}");
    shutdown.notify_one();
}

#[test]
fn missing_server_env_names_the_variable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(dir.path(), "prod.yaml", "name: prod\n");
    let out = apply(&file, &[]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("SUBMILLI_SERVER_URL"),
        "stderr: {}",
        stderr(&out)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_rejection_names_file_and_document() {
    let (server, shutdown) = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(dir.path(), "bad.yaml", "name: \"bad name!\"\n");

    let out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || apply(&file, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(!out.status.success());
    let text = stderr(&out);
    assert!(text.contains("submilli-server rejected"), "{text}");
    assert!(text.contains("bad.yaml (document 1)"), "{text}");
    shutdown.notify_one();
}

#[test]
fn unknown_kind_is_a_local_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(dir.path(), "odd.yaml", "kind: deployment\nname: x\n");
    let out = apply(&file, &[]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("unknown kind 'deployment'"),
        "stderr: {}",
        stderr(&out)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_kind_prevents_partial_apply() {
    let (server, shutdown) = spawn_server().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(
        dir.path(),
        "mixed.yaml",
        "name: untouched\n---\nkind: deployment\nname: unsupported\n",
    );
    let out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || apply(&file, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("mixed.yaml (document 2)"));
    assert!(stderr(&out).contains("expected 'blueprint'"));
    let status = tokio::task::spawn_blocking(move || {
        ureq::get(&format!("{server}/v1/blueprints/untouched"))
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .unwrap()
            .status()
            .as_u16()
    })
    .await
    .unwrap();
    assert_eq!(status, 404);
    shutdown.notify_one();
}
