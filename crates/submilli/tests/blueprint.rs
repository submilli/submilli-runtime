//! End-to-end integration tests for `submilli server blueprint {add,list}`.

#[path = "../../submilli-server/tests/common/in_memory_config.rs"]
mod in_memory_config;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use submilli_server::{AppState, app};

async fn spawn_server() -> String {
    let state = AppState::new(in_memory_config::config()).expect("AppState");
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

fn write_blueprint(name: &str, contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("submilli-blueprint-{name}.yaml"));
    fs::write(&path, contents).expect("write temp blueprint");
    path
}

fn add(server: &str, file: &Path) -> Output {
    Command::new(submilli_bin())
        .args([
            "server",
            "blueprint",
            "add",
            "--server",
            server,
            file.to_str().expect("path utf-8"),
        ])
        .output()
        .expect("invoke submilli server blueprint add")
}

fn apply(server: &str, file: &Path) -> Output {
    Command::new(submilli_bin())
        .args([
            "server",
            "blueprint",
            "apply",
            "--server",
            server,
            file.to_str().expect("path utf-8"),
        ])
        .output()
        .expect("invoke submilli server blueprint apply")
}

fn show(server: &str, name: &str) -> Output {
    Command::new(submilli_bin())
        .args(["server", "blueprint", "show", "--server", server, name])
        .output()
        .expect("invoke submilli server blueprint show")
}

fn remove(server: &str, name: &str) -> Output {
    Command::new(submilli_bin())
        .args(["server", "blueprint", "remove", "--server", server, name])
        .output()
        .expect("invoke submilli server blueprint remove")
}

fn list(server: &str) -> Output {
    Command::new(submilli_bin())
        .args(["server", "blueprint", "list", "--server", server])
        .output()
        .expect("invoke submilli server blueprint list")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_then_list() {
    let server = spawn_server().await;
    let file = write_blueprint("prod", "name: production\n");
    let add_out = tokio::task::spawn_blocking({
        let server = server.clone();
        let file = file.clone();
        move || add(&server, &file)
    })
    .await
    .unwrap();
    assert!(add_out.status.success(), "stderr: {}", stderr(&add_out));
    assert!(stdout(&add_out).contains("production"));

    let list_out = tokio::task::spawn_blocking(move || list(&server))
        .await
        .unwrap();
    assert!(list_out.status.success(), "stderr: {}", stderr(&list_out));
    assert_eq!(stdout(&list_out), "production\n");
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_validation_rejects_unknown_field() {
    let server = spawn_server().await;
    let file = write_blueprint("badfield", "name: foo\nbogus: {}\n");
    let out = tokio::task::spawn_blocking(move || add(&server, &file))
        .await
        .unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("error:"));
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn apply_then_show_then_remove() {
    let server = spawn_server().await;
    let file = write_blueprint("apply", "name: applied\n");

    let apply_out = tokio::task::spawn_blocking({
        let server = server.clone();
        let file = file.clone();
        move || apply(&server, &file)
    })
    .await
    .unwrap();
    assert!(apply_out.status.success(), "stderr: {}", stderr(&apply_out));
    assert!(stdout(&apply_out).contains("applied"));

    // apply again replaces without error
    let reapply_out = tokio::task::spawn_blocking({
        let server = server.clone();
        let file = file.clone();
        move || apply(&server, &file)
    })
    .await
    .unwrap();
    assert!(
        reapply_out.status.success(),
        "stderr: {}",
        stderr(&reapply_out)
    );

    let show_out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || show(&server, "applied")
    })
    .await
    .unwrap();
    assert!(show_out.status.success(), "stderr: {}", stderr(&show_out));
    assert!(stdout(&show_out).contains("name: applied"));

    let remove_out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || remove(&server, "applied")
    })
    .await
    .unwrap();
    assert!(
        remove_out.status.success(),
        "stderr: {}",
        stderr(&remove_out)
    );

    let list_out = tokio::task::spawn_blocking(move || list(&server))
        .await
        .unwrap();
    assert_eq!(stdout(&list_out), "");
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn show_preserves_comments_and_moves_permissions_last() {
    let server = spawn_server().await;
    let file = write_blueprint(
        "comments",
        "\
name: commented

# operator policy
permissions:
  main:
    # allow reads
    - capability: fs.read
      action: allow

mcp:
  linear:
    # endpoint
    url: https://mcp.linear.app/mcp
",
    );

    let apply_out = tokio::task::spawn_blocking({
        let server = server.clone();
        let file = file.clone();
        move || apply(&server, &file)
    })
    .await
    .unwrap();
    assert!(apply_out.status.success(), "stderr: {}", stderr(&apply_out));

    let show_out = tokio::task::spawn_blocking(move || show(&server, "commented"))
        .await
        .unwrap();
    assert!(show_out.status.success(), "stderr: {}", stderr(&show_out));
    let shown = stdout(&show_out);
    assert!(shown.contains("# operator policy"), "{shown}");
    assert!(shown.contains("# allow reads"), "{shown}");
    assert!(shown.contains("# endpoint"), "{shown}");
    assert!(
        shown.rfind("permissions:").unwrap() > shown.rfind("mcp:").unwrap(),
        "{shown}"
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn show_and_remove_unknown_fail() {
    let server = spawn_server().await;

    let show_out = tokio::task::spawn_blocking({
        let server = server.clone();
        move || show(&server, "ghost")
    })
    .await
    .unwrap();
    assert!(!show_out.status.success());
    assert!(stderr(&show_out).contains("not registered"));

    let remove_out = tokio::task::spawn_blocking(move || remove(&server, "ghost"))
        .await
        .unwrap();
    assert!(!remove_out.status.success());
    assert!(stderr(&remove_out).contains("not registered"));
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_name_is_error() {
    let server = spawn_server().await;
    let file = write_blueprint("dup", "name: dup\n");
    let first = tokio::task::spawn_blocking({
        let server = server.clone();
        let file = file.clone();
        move || add(&server, &file)
    })
    .await
    .unwrap();
    assert!(first.status.success());
    let second = tokio::task::spawn_blocking(move || add(&server, &file))
        .await
        .unwrap();
    assert!(!second.status.success());
    assert!(stderr(&second).contains("already"));
}

fn lint(contents: &str, name: &str) -> Output {
    let file = write_blueprint(name, contents);
    Command::new(submilli_bin())
        .args(["blueprint", "lint", file.to_str().expect("path utf-8")])
        .output()
        .expect("invoke submilli blueprint lint")
}

#[test]
fn lint_rejects_duplicate_rule_name_in_one_caller_block() {
    let out = lint(
        "name: dup-rules\npermissions:\n  main:\n    - name: same\n      capability: fs.read\n      action: allow\n    - name: same\n      capability: fs.write\n      action: allow\n",
        "lint-dup-rule-name",
    );
    assert!(!out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("both named `same`"), "{err}");
}

#[test]
fn lint_accepts_same_rule_name_in_different_caller_blocks() {
    let out = lint(
        "name: shared-names\npermissions:\n  main:\n    - name: same\n      capability: fs.read\n      action: allow\n  other:\n    - name: same\n      capability: fs.read\n      action: allow\n",
        "lint-same-name-two-blocks",
    );
    assert!(
        !stderr(&out).contains("both named"),
        "unexpected duplicate-name error: {}",
        stderr(&out)
    );
}

#[test]
fn lint_accepts_blueprint_without_rule_names() {
    let out = lint(
        "name: unnamed\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n    - capability: fs.read\n      action: deny\n",
        "lint-unnamed-rules",
    );
    assert!(!stderr(&out).contains("both named"), "{}", stderr(&out));
}
