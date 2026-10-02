//! Live end-to-end tests against the public test repo
//! `github.com/submilli/test-packages` (a two-package pure-Wasm monorepo):
//! `submilli install <url>`, and `submilli build` resolving a GitHub dependency
//! on one of those packages (SUB-626).
//!
//! Gated: they only run when `SUBMILLI_E2E_GITHUB=1` is set, because they need
//! network access and the repo to exist. CI leaves them skipped.
//!
//! The private-repository tests also need `SUBMILLI_E2E_PRIVATE_REPO`
//! (`org/repo`, a private mirror of the test repo, so it holds
//! [`GITHUB_DEP_SHA`]) and `SUBMILLI_E2E_GITHUB_TOKEN`, a token with Contents:
//! Read-only on it. They pass the token to each child process explicitly and
//! clear every other source.

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

/// The private mirror and a token that can read it, when both are configured.
fn private_repo() -> Option<(String, String)> {
    if !enabled() {
        return None;
    }
    let repo = std::env::var("SUBMILLI_E2E_PRIVATE_REPO").ok()?;
    let token = std::env::var("SUBMILLI_E2E_GITHUB_TOKEN").ok()?;
    Some((repo, token))
}

/// `submilli <args>` with no GitHub token from anywhere but `token`: the
/// environment is cleared, so neither `GH_TOKEN` nor `gh` on the path leaks in.
fn isolated(home: &Path, args: &[&str], token: Option<&str>) -> Command {
    let mut command = Command::new(submilli_bin());
    command
        .args(args)
        .env_clear()
        .env("SUBMILLI_HOME", home)
        .env("HOME", home)
        .env("SUBMILLI_TELEMETRY", "0");
    if let Some(token) = token {
        command.env("GH_TOKEN", token);
    }
    command
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn private_install_needs_a_token_and_uses_it() {
    let Some((repo, token)) = private_repo() else {
        return;
    };
    let home = TempDir::new().unwrap();
    let url = format!("github.com/{repo}");

    // No token, and no terminal to offer one at: a clear error, no prompt.
    let anonymous = isolated(home.path(), &["install", &url], None)
        .output()
        .unwrap();
    assert!(!anonymous.status.success());
    let error = stderr(&anonymous);
    assert!(error.contains("no public repository"), "{error}");
    assert!(error.contains("submilli github authenticate"), "{error}");
    assert!(!installed(home.path(), "@submilli/greet"));

    let out = isolated(home.path(), &["install", &url], Some(&token))
        .output()
        .unwrap();
    assert!(out.status.success(), "install failed: {}", stderr(&out));
    assert!(installed(home.path(), "@submilli/greet"));
    assert!(installed(home.path(), "@submilli/mathx"));
    assert!(!stderr(&out).contains(&token));
}

/// The stored token (`submilli github authenticate`) reaches a private
/// dependency, and a second build is satisfied from the lock.
#[test]
fn private_dependency_builds_with_the_stored_token() {
    use std::io::Write;
    let Some((repo, token)) = private_repo() else {
        return;
    };
    let home = TempDir::new().unwrap();

    let mut authenticate = isolated(home.path(), &["github", "authenticate"], None)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    authenticate
        .stdin
        .take()
        .unwrap()
        .write_all(token.as_bytes())
        .unwrap();
    let stored = authenticate.wait_with_output().unwrap();
    assert!(
        stored.status.success(),
        "authenticate failed: {}",
        stderr(&stored)
    );
    assert!(!String::from_utf8_lossy(&stored.stdout).contains(&token));

    let project = TempDir::new().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github-dep");
    copy_dir(&fixture, project.path());
    let manifest = project.path().join("submilli.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace(
            "github.com/submilli/test-packages",
            &format!("github.com/{repo}"),
        ),
    )
    .unwrap();

    for attempt in ["first", "second"] {
        let out = isolated(home.path(), &["build", "check"], None)
            .current_dir(project.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{attempt} build failed: {}",
            stderr(&out)
        );
    }
    assert!(installed(home.path(), "@submilli/greet"));
    let lock = std::fs::read_to_string(project.path().join("submilli.lock")).unwrap();
    assert!(lock.contains(GITHUB_DEP_SHA), "lock pins the SHA:\n{lock}");
}

/// A server installs a private repository with its own token file, and
/// without one says what it needs.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_server_install_uses_the_server_token() {
    let Some((repo, token)) = private_repo() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let token_file = dir.path().join("github.token");
    std::fs::write(&token_file, format!("{token}\n")).unwrap();
    let url = format!("github.com/{repo}");

    for (with_token, store) in [(false, "store-anonymous"), (true, "store")] {
        let home = dir.path().join(format!("home-{store}"));
        let config = submilli_server::ServerConfig {
            package_store_root: Some(dir.path().join(store)),
            github_token_file: with_token.then(|| token_file.clone()),
            ..submilli_server::ServerConfig::default()
        };
        let state = submilli_server::AppState::new(config).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, submilli_server::app(state)).into_future());

        let server = format!("http://{addr}");
        let args = [
            "server",
            "packages",
            "install",
            url.as_str(),
            "--server",
            server.as_str(),
        ];
        let out = tokio::task::spawn_blocking({
            let args = args.map(str::to_string);
            move || {
                std::fs::create_dir_all(&home).unwrap();
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                isolated(&home, &args, None).output().unwrap()
            }
        })
        .await
        .unwrap();
        let text = format!("{}{}", stderr(&out), String::from_utf8_lossy(&out.stdout));
        assert!(!text.contains(&token), "{text}");
        if with_token {
            assert!(out.status.success(), "server install failed: {text}");
            let wasm = dir.path().join(store).join("@submilli/greet/pkg.wasm");
            assert!(wasm.is_file(), "{text}");
        } else {
            assert!(!out.status.success(), "{text}");
            assert!(text.contains("github_token_file"), "{text}");
        }
    }
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
