//! Helpers for tests that drive the real `submilli-server` binary as a child
//! process. Each test binary that needs them declares `mod common;`, so the
//! items a given binary does not use are expected to be dead there.

#![allow(dead_code)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const BIN: &str = env!("CARGO_BIN_EXE_submilli-server");

/// A server bound to loopback on `port` with `home` as its `SUBMILLI_HOME`,
/// telemetry off (its flush would otherwise reach for sentry.io from a test),
/// and every ambient variable that could steer the address ladder, name a
/// state directory, point at a config file, or key the store cleared, so the
/// developer's shell cannot change what the server resolves. `extra_env` is
/// applied last.
pub fn spawn_server(
    home: &Path,
    port: u16,
    shutdown_grace_secs: u64,
    extra_env: &[(&str, &str)],
) -> Child {
    let mut command = Command::new(BIN);
    command
        .args(["--bind", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .args(["--shutdown-grace", &shutdown_grace_secs.to_string()])
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .env_remove("HOST")
        .env_remove("PORT")
        .env_remove("SUBMILLI_BIND")
        .env_remove("SUBMILLI_PORT")
        .env_remove("SUBMILLI_CONFIG")
        .env_remove("SUBMILLI_BLUEPRINT_DIR")
        .env_remove("SUBMILLI_BLUEPRINT_SEED_DIR")
        .env_remove("SUBMILLI_SESSION_STORE_DIR")
        .env_remove("SUBMILLI_VFS_SESSION_DIR")
        .env_remove("SUBMILLI_VFS_EPHEMERAL_DIR")
        .env_remove("SUBMILLI_SECRET_STORE_DIR")
        .env_remove("SUBMILLI_PACKAGE_STORE_DIR")
        .env_remove("SUBMILLI_SECRET_STORE_KEY_ENV")
        .env_remove("SUBMILLI_SECRET_STORE_KEY_FILE")
        .env_remove("SUBMILLI_SECRET_KEY")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.spawn().expect("spawn server")
}

#[cfg(unix)]
pub fn signal(child: &Child, sig: libc::c_int) {
    // SAFETY: the pid belongs to a child this test spawned and has not reaped.
    let rc = unsafe { libc::kill(child.id() as libc::pid_t, sig) };
    assert_eq!(rc, 0, "kill({sig}) failed");
}

pub fn wait_for_exit(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    panic!("server did not exit within {timeout:?} of the signal");
}

/// Readiness via the server's own probe, so it proves our binary answered
/// rather than that *something* accepted a connection on the port.
pub fn wait_ready(port: u16) {
    for _ in 0..600 {
        let probed = Command::new(BIN)
            .args(["--health-check", "--bind", "127.0.0.1", "--port"])
            .arg(port.to_string())
            // The probe resolves its address through the same ladder as the
            // server, so the same ambient variables must not steer it.
            .env_remove("HOST")
            .env_remove("PORT")
            .env_remove("SUBMILLI_BIND")
            .env_remove("SUBMILLI_PORT")
            .env_remove("SUBMILLI_CONFIG")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("spawn health check");
        if probed.success() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server never became healthy on port {port}");
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind probe listener")
        .local_addr()
        .expect("probe local_addr")
        .port()
}
