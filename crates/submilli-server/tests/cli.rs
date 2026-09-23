//! Binary-level flags the container image depends on: `--version` for the
//! conventional image smoke test, `--health-check` for the Docker `HEALTHCHECK`
//! (a distroless image has no shell and no `curl`).

mod common;

use std::net::SocketAddr;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use common::{BIN, free_port};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{ServerConfig, serve};
use tokio::net::TcpStream;

#[test]
fn version_flag_prints_the_crate_version() {
    let output = Command::new(BIN).arg("--version").output().expect("spawn");
    assert!(
        output.status.success(),
        "--version exited {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "--version printed {stdout:?}, expected the crate version"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn health_check_follows_the_resolved_address() {
    let home = tempfile::tempdir().expect("temp home");
    let config = ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::default())),
        session_storage_root: Some(home.path().join("vfs")),
        package_store_root: Some(home.path().join("packages")),
        ..ServerConfig::default()
    };
    let port = free_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let _server = tokio::spawn(serve(addr, config, Duration::from_secs(5)));
    wait_ready(addr).await;

    assert!(
        health_check(port).await.success(),
        "--health-check should succeed against a live server on port {port}"
    );
    // A port the server is not on: the probe must fail rather than report the
    // container healthy because *something* answered somewhere.
    assert!(
        !health_check(free_port()).await.success(),
        "--health-check should fail when nothing is listening"
    );
}

/// The container path: a Docker `HEALTHCHECK` runs its own process and cannot
/// inherit the server's `CMD` flags, so the probe has to reach the right address
/// through the environment and the config file alone. Both directions are
/// asserted — a probe that succeeded regardless of where it was pointed would
/// report every container healthy.
#[tokio::test(flavor = "multi_thread")]
async fn health_check_resolves_from_env_and_config_without_flags() {
    let home = tempfile::tempdir().expect("temp home");
    let config = ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::default())),
        session_storage_root: Some(home.path().join("vfs")),
        package_store_root: Some(home.path().join("packages")),
        ..ServerConfig::default()
    };
    let live = free_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], live));
    let _server = tokio::spawn(serve(addr, config, Duration::from_secs(5)));
    wait_ready(addr).await;

    let dead = free_port();
    let config_file = home.path().join("server.yaml");
    std::fs::write(&config_file, format!("bind: 127.0.0.1\nport: {live}\n")).expect("write config");
    let wrong_file = home.path().join("wrong.yaml");
    std::fs::write(&wrong_file, format!("bind: 127.0.0.1\nport: {dead}\n")).expect("write config");

    for (label, vars, expected) in [
        (
            "SUBMILLI_BIND/PORT",
            vec![
                ("SUBMILLI_BIND", "127.0.0.1".to_string()),
                ("SUBMILLI_PORT", live.to_string()),
            ],
            true,
        ),
        (
            "SUBMILLI_BIND/PORT pointed elsewhere",
            vec![
                ("SUBMILLI_BIND", "127.0.0.1".to_string()),
                ("SUBMILLI_PORT", dead.to_string()),
            ],
            false,
        ),
        (
            "SUBMILLI_CONFIG",
            vec![(
                "SUBMILLI_CONFIG",
                config_file.to_str().expect("utf-8 path").to_string(),
            )],
            true,
        ),
        (
            "SUBMILLI_CONFIG pointed elsewhere",
            vec![(
                "SUBMILLI_CONFIG",
                wrong_file.to_str().expect("utf-8 path").to_string(),
            )],
            false,
        ),
    ] {
        let succeeded = health_check_with_env(vars).await.success();
        assert_eq!(
            succeeded,
            expected,
            "probing via {label} should have {}",
            if expected { "succeeded" } else { "failed" }
        );
    }
}

/// Runs `--health-check` with no address flags at all, so only the environment
/// and config file can steer it.
async fn health_check_with_env(vars: Vec<(&'static str, String)>) -> std::process::ExitStatus {
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(BIN);
        command
            .arg("--health-check")
            .env_remove("HOST")
            .env_remove("PORT");
        for (name, value) in vars {
            command.env(name, value);
        }
        command.status().expect("spawn --health-check")
    })
    .await
    .expect("health-check task panicked")
}

async fn health_check(port: u16) -> std::process::ExitStatus {
    tokio::task::spawn_blocking(move || {
        Command::new(BIN)
            .args(["--health-check", "--bind", "127.0.0.1", "--port"])
            .arg(port.to_string())
            // The address ladder falls back to these, and the test's own
            // environment must not steer the probe.
            .env_remove("HOST")
            .env_remove("PORT")
            .status()
            .expect("spawn --health-check")
    })
    .await
    .expect("health-check task panicked")
}

async fn wait_ready(addr: SocketAddr) {
    for _ in 0..500 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("server never accepted a connection on {addr}");
}
