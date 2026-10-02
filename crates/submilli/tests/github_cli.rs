//! `submilli github …` without the network: where the token is stored, and
//! which source package fetches would use. Checking a token with GitHub is
//! covered by the gated live tests in `install_e2e.rs`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn github(home: &Path, args: &[&str]) -> Output {
    Command::new(PathBuf::from(env!("CARGO_BIN_EXE_submilli")))
        .arg("github")
        .args(args)
        // Nothing inherited: no `GH_TOKEN`, and no `gh` on the path.
        .env_clear()
        .env("SUBMILLI_HOME", home)
        .env("HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("invoke submilli github")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn auth_status_without_a_token_says_public_only() {
    let home = tempfile::tempdir().unwrap();
    let output = github(home.path(), &["auth-status"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("no GitHub token"), "{text}");
    assert!(text.contains("submilli github authenticate"), "{text}");
}

#[test]
fn deauthenticate_removes_the_stored_token() {
    let home = tempfile::tempdir().unwrap();
    let stored = home.path().join("github_token");
    std::fs::write(&stored, "ghp_storedtoken\n").unwrap();

    let output = github(home.path(), &["deauthenticate"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("removed the stored GitHub token"));
    assert!(!stored.exists());
    assert!(!stdout(&output).contains("ghp_storedtoken"));

    let again = github(home.path(), &["deauthenticate"]);
    assert!(again.status.success(), "{again:?}");
    assert!(stdout(&again).contains("no GitHub token was stored"));
}

#[test]
fn authenticate_refuses_input_that_is_not_a_token_without_storing_it() {
    use std::io::Write;
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_submilli")))
        .args(["github", "authenticate"])
        .env_clear()
        .env("SUBMILLI_HOME", home.path())
        .env("SUBMILLI_TELEMETRY", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"not a token\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("not a GitHub token"), "{error}");
    assert!(!home.path().join("github_token").exists());
}

#[test]
fn authenticate_refuses_an_owner_that_is_not_a_github_name() {
    let home = tempfile::tempdir().unwrap();
    let output = github(
        home.path(),
        &["authenticate", "--owner", "acme&contents=write"],
    );
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("not a GitHub user or organization"),
        "{error}"
    );
}

#[test]
fn a_malformed_stored_token_is_reported_and_skipped() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("github_token"), "not a token\n").unwrap();
    let output = github(home.path(), &["auth-status"]);
    assert!(output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("ignoring the stored GitHub token"),
        "{error}"
    );
    assert!(!error.contains("not a token\n"), "{error}");
    assert!(stdout(&output).contains("no GitHub token"));
}
