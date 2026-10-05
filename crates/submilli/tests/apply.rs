//! End-to-end tests for `submilli server apply` against a running submilli-server
//! using blueprint files and multi-document YAML streams.

#[path = "../../submilli-server/tests/common/in_memory_config.rs"]
mod in_memory_config;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use submilli_server::{ApiToken, AppState, AuthConfig, Role, ServerConfig, app};

const ADMIN_TOKEN: &str = "admin-token-0123456789abcdef0123456789";

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
    cmd.args(["server", "apply", "-f"]).arg(path);
    cmd.env_remove("SUBMILLI_SERVER_URL");
    cmd.env_remove("SUBMILLI_SERVER_TOKEN");
    cmd.env_remove("SUBMILLI_SERVER_TOKEN_FILE");
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.output().expect("invoke submilli server apply")
}

async fn spawn_server() -> (String, std::sync::Arc<tokio::sync::Notify>) {
    spawn_server_with(in_memory_config::config()).await
}

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

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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
fn server_apply_has_standard_target_options_and_no_top_level_alias() {
    let out = Command::new(submilli_bin())
        .args(["server", "apply", "--help"])
        .output()
        .expect("help");
    assert!(out.status.success());
    let help = stdout(&out);
    assert!(help.contains("--server <URL>"));
    assert!(help.contains("--token-file <PATH>"));
    assert!(help.contains("http://127.0.0.1:8128"));
    let out = Command::new(submilli_bin())
        .args(["apply", "--help"])
        .output()
        .expect("old command");
    assert!(!out.status.success());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn apply_sends_the_admin_token_from_the_environment() {
    let admin = ApiToken::new("ops", Role::Admin, ADMIN_TOKEN).expect("valid token");
    let (server, shutdown) = spawn_server_with(ServerConfig {
        auth: AuthConfig::Tokens(vec![admin]),
        ..in_memory_config::config()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "prod.yaml", "name: prod\ndefault: deny\n");

    let refused = tokio::task::spawn_blocking({
        let (file, server) = (file.clone(), server.clone());
        move || apply(&file, &[("SUBMILLI_SERVER_URL", &server)])
    })
    .await
    .unwrap();
    assert!(!refused.status.success());
    let message = stderr(&refused);
    assert!(message.contains("SUBMILLI_SERVER_TOKEN"), "{message}");

    let file_for_flags = file.clone();
    let accepted = tokio::task::spawn_blocking({
        let server = server.clone();
        move || {
            apply(
                &file,
                &[
                    ("SUBMILLI_SERVER_URL", &server),
                    ("SUBMILLI_SERVER_TOKEN", ADMIN_TOKEN),
                ],
            )
        }
    })
    .await
    .unwrap();
    assert!(accepted.status.success(), "stderr: {}", stderr(&accepted));
    assert!(stdout(&accepted).contains("Added blueprint 'prod'"));
    let token_file = write(dir.path(), "token", ADMIN_TOKEN);
    let accepted = tokio::task::spawn_blocking(move || {
        Command::new(submilli_bin())
            .args(["server", "apply", "-f"])
            .arg(file_for_flags)
            .args(["--server", &server, "--token-file"])
            .arg(token_file)
            .env("SUBMILLI_SERVER_URL", "http://127.0.0.1:1")
            .env("SUBMILLI_SERVER_TOKEN", "wrong-token")
            .output()
            .expect("apply with explicit target")
    })
    .await
    .expect("join");
    assert!(accepted.status.success(), "stderr: {}", stderr(&accepted));
    assert!(stdout(&accepted).contains("Updated blueprint 'prod'"));
    shutdown.notify_one();
}
