//! Runs the first-party packages' policy scripts: each is a `main` program
//! executed under a restricted blueprint against the locally published package.
//!
//! `build test` runs package tests under an allow-all policy, so it cannot show
//! that a `main` caller is constrained. These scripts can, and they need no
//! credentials or network: each blueprint denies the package its secret or its
//! HTTP calls, so an allowed call stops at that gated operation.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Packages that ship policy scripts under `tests/policy/`.
const PACKAGES: &[&str] = &[
    "firecrawl",
    "github",
    "gmail",
    "google-calendar",
    "google-drive",
    "jina",
    "notion",
    "slack-bot",
    "slack-user",
];

#[test]
fn policy_scripts_pass_under_their_blueprints() {
    let home = tempfile::tempdir().expect("package store");
    let mut failures = Vec::new();
    for package in PACKAGES {
        let published = submilli(
            &[
                "build",
                "publish-local",
                "-p",
                &format!("@submilli/{package}"),
            ],
            home.path(),
        );
        if !published.status.success() {
            failures.push(format!(
                "publishing @submilli/{package} failed:\n{}",
                String::from_utf8_lossy(&published.stderr)
            ));
            continue;
        }
        let scripts = policy_scripts(package);
        if scripts.is_empty() {
            failures.push(format!("@submilli/{package} has no policy script"));
        }
        for script in scripts {
            let blueprint = script.with_extension("yaml");
            let run = submilli(
                &[
                    "run",
                    &script.to_string_lossy(),
                    "--blueprint",
                    &blueprint.to_string_lossy(),
                ],
                home.path(),
            );
            if !run.status.success() {
                failures.push(format!(
                    "{} failed:\n{}{}",
                    script.display(),
                    String::from_utf8_lossy(&run.stdout),
                    String::from_utf8_lossy(&run.stderr)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Fails on any warning that names `check()`: a repeated read or a misplaced
/// `check()`, and a `check()` call with no `@capability`. These only warn, so a
/// package that regresses still builds.
#[test]
fn first_party_packages_build_without_check_warnings() {
    let home = tempfile::tempdir().expect("package store");
    let checked = submilli(&["build", "check"], home.path());
    let stderr = String::from_utf8_lossy(&checked.stderr);
    assert!(checked.status.success(), "build check failed:\n{stderr}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("warning") && line.contains("`check()`"))
        .collect();
    assert!(warnings.is_empty(), "{}", warnings.join("\n"));
}

/// Every `<name>.ts` with a `<name>.yaml` beside it under the package's `tests/policy/`.
fn policy_scripts(package: &str) -> Vec<PathBuf> {
    let directory = repository_root()
        .join("packages")
        .join(package)
        .join("tests/policy");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("reading {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .filter(|path| path.with_extension("yaml").is_file())
        .collect();
    scripts.sort();
    scripts
}

fn submilli(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(args)
        .current_dir(repository_root())
        .env("SUBMILLI_HOME", home)
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .expect("invoke submilli")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
