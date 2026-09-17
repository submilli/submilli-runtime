//! `submilli upgrade` against a loopback stand-in for GitHub releases. The
//! command replaces its own executable, so each test runs a private copy.
#![cfg(unix)]

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use sha2::{Digest, Sha256};

const NEW_EXECUTABLE: &str = "#!/bin/sh\necho 'submilli 99.0.0'\n";

fn asset_name() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "submilli-x86_64-unknown-linux-musl",
        ("macos", "x86_64") => "submilli-x86_64-apple-darwin",
        ("macos", "aarch64") => "submilli-aarch64-apple-darwin",
        other => panic!("no release asset for {other:?}"),
    }
}

/// Serves `releases/latest` as a redirect to v99.0.0 plus that release's files.
fn serve_release(checksum_of: &str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let source = format!("http://{}/repo", listener.local_addr().unwrap());
    let sums = format!(
        "{:x}  {}\n{:x}  install.sh\n",
        Sha256::digest(checksum_of.as_bytes()),
        asset_name(),
        Sha256::digest(b"installer"),
    );
    let files = HashMap::from([
        (
            format!("/repo/releases/download/v99.0.0/{}", asset_name()),
            NEW_EXECUTABLE.to_owned(),
        ),
        (
            "/repo/releases/download/v99.0.0/SHA256SUMS".to_owned(),
            sums,
        ),
    ]);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = [0u8; 2048];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            let path = request.split(' ').nth(1).unwrap_or_default();
            let response = if path == "/repo/releases/latest" {
                "HTTP/1.1 302 Found\r\nlocation: /repo/releases/tag/v99.0.0\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned()
            } else if let Some(body) = files.get(path) {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else {
                "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    .to_owned()
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    source
}

fn private_copy(directory: &Path) -> PathBuf {
    let executable = directory.join("bin/submilli");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_submilli"), &executable).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    executable
}

fn upgrade(executable: &Path, home: &Path, source: &str, args: &[&str]) -> Output {
    Command::new(executable)
        .arg("upgrade")
        .args(args)
        .current_dir(home)
        .env("SUBMILLI_TELEMETRY", "0")
        .env("HOME", home)
        .env("SUBMILLI_HOME", home.join(".submilli"))
        .env("SUBMILLI_RELEASE_SOURCE", source)
        .output()
        .unwrap()
}

#[test]
fn check_reports_a_newer_release_without_touching_the_executable() {
    let home = tempfile::tempdir().unwrap();
    let executable = private_copy(home.path());
    let before = fs::read(&executable).unwrap();
    let output = upgrade(
        &executable,
        home.path(),
        &serve_release(NEW_EXECUTABLE),
        &["--check"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("v99.0.0 is available"));
    assert_eq!(fs::read(&executable).unwrap(), before);
}

#[test]
fn replaces_the_running_executable_with_the_verified_release() {
    let home = tempfile::tempdir().unwrap();
    let executable = private_copy(home.path());
    let output = upgrade(
        &executable,
        home.path(),
        &serve_release(NEW_EXECUTABLE),
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&executable).unwrap(), NEW_EXECUTABLE);
    assert_eq!(
        fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let leftovers: Vec<_> = fs::read_dir(executable.parent().unwrap())
        .unwrap()
        .collect();
    assert_eq!(leftovers.len(), 1, "staging files must not remain");
}

#[test]
fn a_checksum_mismatch_leaves_the_executable_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let executable = private_copy(home.path());
    let before = fs::read(&executable).unwrap();
    let output = upgrade(
        &executable,
        home.path(),
        &serve_release("something else"),
        &[],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"));
    assert_eq!(fs::read(&executable).unwrap(), before);
}

#[test]
fn an_unreachable_source_fails_without_changes() {
    let home = tempfile::tempdir().unwrap();
    let executable = private_copy(home.path());
    let before = fs::read(&executable).unwrap();
    let output = upgrade(&executable, home.path(), "http://127.0.0.1:1/repo", &[]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&executable).unwrap(), before);
}
