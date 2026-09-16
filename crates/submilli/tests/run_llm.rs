//! `submilli run` wiring for `submilli:llm`.
//!
//! Unlike `submilli:session` — which is deliberately not wired into the CLI,
//! because session state is memory-only and a local run has nothing to carry it
//! across — a blueprint-configured provider is exactly what makes `submilli run`
//! useful for testing a program before it reaches a server, and the credentials
//! already resolve from the blueprint the CLI is loading anyway.
//!
//! The consequence that matters here is the budget: the CLI allocates a
//! per-execution ceiling from the same ladder, with a **finite** default, so it
//! never becomes a path that spends against a provider credential with no
//! ceiling behind it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn stdout(out: &Output) -> &str {
    std::str::from_utf8(&out.stdout).expect("stdout utf-8")
}

fn stderr(out: &Output) -> &str {
    std::str::from_utf8(&out.stderr).expect("stderr utf-8")
}

/// A blueprint declaring one provider and one model, and granting `llm.call`.
/// Declaration is authoritative, so the model has to be here for a call to
/// resolve at all.
const BLUEPRINT: &str = r#"name: llm-cli
permissions:
  main:
    - capability: llm.call
      action: allow
llm:
  providers:
    fake:
      type: anthropic
  models:
    test-model:
      provider: fake
      output_reserve: 1000
"#;

/// Catches, so a refusal prints as a value on stdout rather than as a backtrace.
const CATCH: &str = r#"import llm from "submilli:llm";
function main(): string {
    try {
        const t = llm.call("test-model", "hi").text;
        return "OK:" + (t === null ? "NULL" : (t as string));
    } catch (e: Error) {
        return e.message;
    }
}"#;

struct Fixture {
    _dir: tempfile::TempDir,
    script: PathBuf,
    blueprint: PathBuf,
}

fn fixture(name: &str, source: &str) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join(format!("{name}.subm"));
    fs::write(&script, source).expect("write script");
    let blueprint = dir.path().join("blueprint.yaml");
    fs::write(&blueprint, BLUEPRINT).expect("write blueprint");
    Fixture {
        _dir: dir,
        script,
        blueprint,
    }
}

/// Run with `SUBMILLI_HOME` pointed at a scratch dir, so the local secret store
/// the blueprint path opens does not touch the developer's real one.
fn run(fixture: &Fixture, home: &Path, env: &[(&str, &str)], flags: &[&str]) -> Output {
    let mut cmd = Command::new(submilli_bin());
    cmd.arg("run");
    for flag in flags {
        cmd.arg(flag);
    }
    cmd.arg(&fixture.script)
        .arg("--blueprint")
        .arg(&fixture.blueprint)
        .env("SUBMILLI_HOME", home);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.output().expect("invoke submilli")
}

/// R12 on the CLI path: no dispatch is compiled into the binary, so a program
/// calling a declared model gets the *catchable* configuration error naming the
/// model — not a crash, and not a silent success.
///
/// This is also what proves the provider seam is reached at all: without the
/// wiring the call would fail as an unknown import or an ungated capability
/// instead.
#[test]
fn a_cli_run_with_no_dispatch_gets_the_catchable_configuration_error() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("llm_no_provider", CATCH);
    let out = run(&f, home.path(), &[], &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let printed = stdout(&out);
    assert!(
        printed.contains("test-model"),
        "the refusal must name the model: {printed}"
    );
    assert!(
        !printed.starts_with("OK:"),
        "no dispatch is configured, so the call must not succeed: {printed}"
    );
}

/// A set-but-unparseable ceiling fails the run naming the variable, rather than
/// falling through to the default. Silently ignoring a typo'd ceiling is how a
/// CLI run ends up spending more than the operator asked it to.
#[test]
fn a_malformed_ceiling_fails_the_run_naming_the_variable() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("llm_malformed", CATCH);
    let out = run(
        &f,
        home.path(),
        &[("SUBMILLI_MAX_EXECUTION_LLM_TOKENS", "lots")],
        &[],
    );

    assert!(
        !out.status.success(),
        "a malformed ceiling must fail the run, not fall through to the default"
    );
    let err = stderr(&out);
    assert!(
        err.contains("SUBMILLI_MAX_EXECUTION_LLM_TOKENS"),
        "the error must name the variable: {err}"
    );
}

/// The fan-out bound (KTD4) is rejected at zero rather than clamped: zero reads
/// as "no concurrency", but a zero-permit semaphore deadlocks, and silently
/// treating it as one hides the operator's mistake behind a serial dispatch they
/// did not ask for.
#[test]
fn a_zero_concurrency_bound_is_rejected() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("llm_zero_concurrency", CATCH);
    let out = run(&f, home.path(), &[], &["--max-llm-concurrency", "0"]);

    assert!(
        !out.status.success(),
        "zero would deadlock the fan-out semaphore, so it must be refused"
    );
    assert!(
        stderr(&out).contains("at least 1"),
        "the refusal must say what is allowed: {}",
        stderr(&out)
    );
}
