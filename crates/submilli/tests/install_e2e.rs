//! Live end-to-end tests against the public test repo
//! `github.com/submilli/test-packages` (a two-package pure-Wasm monorepo):
//! `submilli install <url>`, and `submilli build` resolving a GitHub dependency
//! on one of those packages (SUB-626).
//!
//! Gated: they only run when `SUBMILLI_E2E_GITHUB=1` is set, because they need
//! network access and the repo to exist. CI leaves them skipped.

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
