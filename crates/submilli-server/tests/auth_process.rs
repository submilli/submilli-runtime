//! Authentication at the process boundary: what the binary refuses to start
//! with, and that a token declared in the config file is the one it enforces.

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::{BIN, free_port, wait_for_exit, wait_ready};

const ADMIN: &str = "admin-token-0123456789abcdef0123456789";
const USER: &str = "user-token-0123456789abcdef01234567890";

/// The binary with nothing inherited that could configure it.
fn server(home: &Path) -> Command {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

#[test]
fn a_server_with_no_tokens_refuses_to_start() {
    let home = tempfile::tempdir().expect("temp home");
    let output = server(home.path()).output().expect("run server");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("SUBMILLI_SERVER_TOKEN"), "{error}");
    assert!(error.contains("api_tokens"), "{error}");
    assert!(error.contains("--allow-unauthenticated"), "{error}");
    // Refused before any store touched the disk.
    assert!(!home.path().join("server").exists());
}

#[test]
fn a_missing_token_file_refuses_to_start() {
    let home = tempfile::tempdir().expect("temp home");
    let config = home.path().join("server.yaml");
    let token_file = home.path().join("absent.token");
    std::fs::write(
        &config,
        format!(
            "api_tokens:\n  - name: app\n    role: user\n    token_file: {}\n",
            token_file.display()
        ),
    )
    .expect("write config");
    let output = server(home.path())
        .arg("--config")
        .arg(&config)
        .output()
        .expect("run server");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("absent.token"), "{error}");
}

#[test]
fn a_missing_github_token_file_refuses_to_start() {
    let home = tempfile::tempdir().expect("temp home");
    let config = home.path().join("server.yaml");
    let token_file = home.path().join("absent-github.token");
    std::fs::write(
        &config,
        format!(
            "allow_unauthenticated: true\ngithub_token_file: {}\n",
            token_file.display()
        ),
    )
    .expect("write config");
    let output = server(home.path())
        .arg("--config")
        .arg(&config)
        .output()
        .expect("run server");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("github_token_file"), "{error}");
    assert!(error.contains("absent-github.token"), "{error}");
    assert!(!home.path().join("server").exists());
}

#[cfg(unix)]
#[test]
fn a_server_token_that_is_not_unicode_refuses_to_start() {
    use std::os::unix::ffi::OsStrExt;
    let home = tempfile::tempdir().expect("temp home");
    let output = server(home.path())
        .env(
            "SUBMILLI_SERVER_TOKEN",
            std::ffi::OsStr::from_bytes(b"\xff\xfe0123456789abcdef0123456789abcdef"),
        )
        .output()
        .expect("run server");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("SUBMILLI_SERVER_TOKEN"), "{error}");
    assert!(error.contains("valid Unicode"), "{error}");
}

/// The admin token comes from `$SUBMILLI_SERVER_TOKEN` alone, the way a server
/// and a CLI sharing a shell are set up; the user token from the config file.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn the_server_token_and_config_file_tokens_are_enforced() {
    let home = tempfile::tempdir().expect("temp home");
    let user_file = home.path().join("user.token");
    std::fs::write(&user_file, format!("{USER}\n")).expect("write token");
    let config = home.path().join("server.yaml");
    std::fs::write(
        &config,
        format!(
            "api_tokens:\n  - name: app\n    role: user\n    token_file: {}\n",
            user_file.display()
        ),
    )
    .expect("write config");
    let port = free_port();
    let mut child = server(home.path())
        .args(["--bind", "127.0.0.1", "--port", &port.to_string()])
        .args(["--shutdown-grace", "1"])
        .arg("--config")
        .arg(&config)
        .env("SUBMILLI_SERVER_TOKEN", ADMIN)
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    // The probe carries no token, so this also proves `/healthz` needs none.
    wait_ready(port);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let status_with = |token: Option<&str>| {
        let mut request = agent.get(format!("http://127.0.0.1:{port}/v1/status"));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        request.call().expect("request").status().as_u16()
    };
    assert_eq!(status_with(None), 401);
    assert_eq!(status_with(Some(USER)), 403);
    assert_eq!(status_with(Some(ADMIN)), 200);

    let stopped = agent
        .post(format!("http://127.0.0.1:{port}/v1/shutdown"))
        .header("authorization", format!("Bearer {ADMIN}"))
        .send_empty()
        .expect("shutdown")
        .status()
        .as_u16();
    assert_eq!(stopped, 200);
    assert!(wait_for_exit(&mut child, Duration::from_secs(30)).success());
}
