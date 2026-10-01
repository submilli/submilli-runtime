//! Live end-to-end tests against the public test repo
//! `github.com/submilli/test-packages` (a two-package pure-Wasm monorepo):
//! `submilli install <url>`, and `submilli build` resolving a GitHub dependency
//! on one of those packages (SUB-626).
//!
//! Gated: they only run when `SUBMILLI_E2E_GITHUB=1` is set, because they need
//! network access and the repo to exist. CI leaves them skipped.
//!
//! The SSH tests run only with `SUBMILLI_E2E_GITHUB_SSH=1` and use the
//! invoking user's ssh-agent or `~/.ssh` keys, with github.com in
//! `~/.ssh/known_hosts`. `SUBMILLI_E2E_PRIVATE_REPO=<org/repo>` points them at
//! a private mirror of `submilli/test-packages` (`git push --mirror` keeps the
//! commit the fixture pins); without it they fetch the public repo over SSH.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const TEST_REPO: &str = "github.com/submilli/test-packages";

/// The commit the `github-dep` fixture pins `@submilli/greet` to. Keep in sync
/// with `tests/fixtures/github-dep/submilli.toml`.
const GITHUB_DEP_SHA: &str = "09fc925af1d7bdb16c26b123fa3b0b7ff6620eb2";

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn enabled() -> bool {
    std::env::var("SUBMILLI_E2E_GITHUB").is_ok_and(|v| v == "1")
}

fn install(home: &Path, args: &[&str]) -> Output {
    Command::new(submilli_bin())
        .arg("install")
        .args(args)
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli install")
}

fn installed(home: &Path, name: &str) -> bool {
    // Store layout is `$SUBMILLI_HOME/packages/@org/leaf/pkg.wasm`.
    home.join("packages").join(name).join("pkg.wasm").is_file()
}

#[test]
fn install_all_packages_from_repo() {
    if !enabled() {
        return;
    }
    let home = TempDir::new().unwrap();
    let out = install(home.path(), &[TEST_REPO]);
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(installed(home.path(), "@submilli/greet"));
    assert!(installed(home.path(), "@submilli/mathx"));

    // Re-running the same install is an idempotent no-op.
    let again = install(home.path(), &[TEST_REPO]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("up to date"));
}

#[test]
fn install_single_package_from_repo() {
    if !enabled() {
        return;
    }
    let home = TempDir::new().unwrap();
    let out = install(home.path(), &[TEST_REPO, "@submilli/greet"]);
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(installed(home.path(), "@submilli/greet"));
    assert!(!installed(home.path(), "@submilli/mathx"));
}

/// A package project in this repo declares a GitHub dependency on
/// `@submilli/greet`; `submilli build check` fetches it at the pinned commit,
/// installs it into the store, writes `submilli.lock`, and compiles the package
/// against it.
#[test]
fn build_resolves_github_dependency() {
    if !enabled() {
        return;
    }
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github-dep");
    copy_dir(&fixture, project.path());

    let out = Command::new(submilli_bin())
        .args(["build", "check"])
        .current_dir(project.path())
        .env("SUBMILLI_HOME", home.path())
        .output()
        .expect("invoke submilli build check");
    assert!(
        out.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The GitHub dependency was fetched and installed into the store.
    assert!(
        installed(home.path(), "@submilli/greet"),
        "dependency installed into the store"
    );

    // A lockfile pinning the dependency was written next to submilli.toml.
    let lock = std::fs::read_to_string(project.path().join("submilli.lock"))
        .expect("submilli.lock written");
    assert!(
        lock.contains("@submilli/greet"),
        "lock names the dep:\n{lock}"
    );
    assert!(lock.contains(GITHUB_DEP_SHA), "lock pins the SHA:\n{lock}");

    // A second build is reproducible from the lock — no re-fetch needed.
    let again = Command::new(submilli_bin())
        .args(["build", "check"])
        .current_dir(project.path())
        .env("SUBMILLI_HOME", home.path())
        .output()
        .expect("invoke submilli build check");
    assert!(
        again.status.success(),
        "second build failed: {}",
        String::from_utf8_lossy(&again.stderr)
    );
}

/// Recursively copy `src`'s contents into the existing directory `dst`.
fn copy_dir(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            std::fs::create_dir_all(&to).expect("create dir");
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}

fn ssh_enabled() -> bool {
    std::env::var("SUBMILLI_E2E_GITHUB_SSH").is_ok_and(|v| v == "1")
}

/// The SSH URL of the (ideally private) mirror of the test repo.
fn ssh_repo_url() -> String {
    let repo = std::env::var("SUBMILLI_E2E_PRIVATE_REPO")
        .unwrap_or_else(|_| "submilli/test-packages".to_string());
    format!("git@github.com:{repo}.git")
}

#[test]
fn install_over_ssh_with_the_local_identity() {
    if !ssh_enabled() {
        return;
    }
    let home = TempDir::new().unwrap();
    let url = ssh_repo_url();
    let out = install(home.path(), &[&url]);
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(installed(home.path(), "@submilli/greet"));
    let metadata =
        std::fs::read_to_string(home.path().join("packages/@submilli/greet/metadata.json"))
            .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(
        metadata["source"]["github"]["transport"], "ssh",
        "provenance records SSH: {metadata}"
    );
}

/// A dependency declared by SSH URL is fetched at its pinned commit, locked
/// verbatim, and satisfied from the lock on the next build.
#[test]
fn build_resolves_ssh_dependency_and_reuses_the_lock() {
    if !ssh_enabled() {
        return;
    }
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github-dep");
    copy_dir(&fixture, project.path());
    let manifest_path = project.path().join("submilli.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    let url = ssh_repo_url();
    std::fs::write(
        &manifest_path,
        manifest.replace(&format!("\"{TEST_REPO}\""), &format!("\"{url}\"")),
    )
    .unwrap();

    let build = || {
        Command::new(submilli_bin())
            .args(["build", "check"])
            .current_dir(project.path())
            .env("SUBMILLI_HOME", home.path())
            .output()
            .expect("invoke submilli build check")
    };
    let out = build();
    assert!(
        out.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lock = std::fs::read_to_string(project.path().join("submilli.lock")).unwrap();
    assert!(lock.contains(&url), "lock keeps the SSH URL:\n{lock}");
    assert!(lock.contains(GITHUB_DEP_SHA), "lock pins the SHA:\n{lock}");

    let again = build();
    assert!(
        again.status.success(),
        "second build failed: {}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("submilli.lock")).unwrap(),
        lock,
        "the lock is reused unchanged"
    );
}

/// With no agent and no key files, an SSH install fails at once with the fix
/// instead of waiting on a prompt.
#[test]
fn ssh_install_without_an_identity_fails_fast() {
    if !ssh_enabled() {
        return;
    }
    let home = TempDir::new().unwrap();
    let user_home = TempDir::new().unwrap();
    let started = std::time::Instant::now();
    let out = Command::new(submilli_bin())
        .args(["install", &ssh_repo_url()])
        .env("SUBMILLI_HOME", home.path())
        .env("HOME", user_home.path())
        .env("SSH_AUTH_SOCK", "")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("invoke submilli install");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("no SSH identity was available"),
        "names the missing identity: {stderr}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
}
