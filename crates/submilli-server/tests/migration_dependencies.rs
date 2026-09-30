//! Migration must refuse before relocating configuration needed by this boot.
#![cfg(unix)]

mod common;

use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn key_file_in_legacy_sessions_is_preserved() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let key = sessions.join("key.b64");
    std::fs::write(&key, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
    let output = Command::new(common::BIN)
        .env_clear()
        .env("SUBMILLI_HOME", home.path())
        .arg("--allow-unauthenticated")
        .args(["--secret-store-key-file"])
        .arg(&key)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("migration would relocate"), "{error}");
    assert!(key.is_file());
    assert!(!home.path().join("server").exists());
    assert!(!home.path().join("server.migrating").exists());
}

#[test]
fn config_file_in_legacy_sessions_is_preserved() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let config = sessions.join("config.yaml");
    std::fs::write(&config, "{}").unwrap();
    let mut server = Command::new(common::BIN)
        .env_clear()
        .env("SUBMILLI_HOME", home.path())
        .arg("--allow-unauthenticated")
        .arg("--config")
        .arg(&config)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let status = common::wait_for_exit(&mut server, Duration::from_secs(10));
    assert!(!status.success());
    assert!(config.is_file());
    assert!(!home.path().join("server").exists());
    assert!(!home.path().join("server.migrating").exists());
}
