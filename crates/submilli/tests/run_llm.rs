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

/// A blueprint whose declared model names a provider the CLI cannot reach.
///
/// Since the real HTTP dispatch is wired, a blueprint naming a *first-party*
/// provider would send a live request to that provider's real endpoint the
/// moment this test ran. `openai-compatible` is the one kind whose endpoint is
/// operator-supplied, so pointing it at a reserved-for-documentation address
/// keeps the run hermetic: the call fails in the connector, not at somebody's
/// API.
///
/// `TEST-NET-1` (RFC 5737) is reserved for exactly this and is not routable, and
/// the blueprint validator accepts it because it is neither loopback,
/// link-local, nor private.
const UNREACHABLE_BLUEPRINT: &str = r#"name: llm-cli
permissions:
  main:
    - capability: llm.call
      action: allow
llm:
  providers:
    unreachable:
      type: openai-compatible
      base_url: https://192.0.2.1
  models:
    test-model:
      provider: unreachable
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

/// Names a model the blueprint does not declare, so the refusal is raised before
/// any request is built.
const UNDECLARED: &str = r#"import llm from "submilli:llm";
function main(): string {
    try {
        const t = llm.call("ghost-model", "hi").text;
        return "OK:" + (t === null ? "NULL" : (t as string));
    } catch (e: Error) {
        return e.message;
    }
}"#;

/// Reports the branch the outcome actually took.
///
/// Deliberately **not** `CATCH`: that script reads `.text` without consulting
/// `ok`, so a failed element — whose `text` is `null` — prints `OK:NULL` and
/// reads as a success. That is KTD1's silent-bug class reproduced in a test, and
/// it is why this file needs a script that branches on `ok` first.
const REPORT: &str = r#"import llm from "submilli:llm";
function main(): string {
    try {
        const c = llm.call("test-model", "hi");
        if (c.ok) { return "OK"; }
        const r = c.reason;
        return "FAIL:" + (r === null ? "NULL" : (r as string));
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
    fixture_with(name, source, BLUEPRINT)
}

fn fixture_with(name: &str, source: &str, blueprint_yaml: &str) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join(format!("{name}.ts"));
    fs::write(&script, source).expect("write script");
    let blueprint = dir.path().join("blueprint.yaml");
    fs::write(&blueprint, blueprint_yaml).expect("write blueprint");
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

/// R12 on the CLI path: a model the blueprint does not declare is refused with a
/// *catchable* error naming the model — not a crash, and not a silent success.
///
/// This is also what proves the provider seam is reached at all: without the
/// wiring the call would fail as an unknown import or an ungated capability
/// instead.
///
/// Declaration — not the presence of a dispatch — is what this now turns on. The
/// real HTTP dispatch is compiled into the binary, so "no provider configured"
/// is no longer a state a CLI run can be in; a deployment that configures no
/// model reaches the guest as this refusal, which names the block to add and is
/// the more actionable of the two.
#[test]
fn a_call_to_an_undeclared_model_is_refused_naming_the_model() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("llm_undeclared", UNDECLARED);
    let out = run(&f, home.path(), &[], &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let printed = stdout(&out);
    assert!(
        printed.contains("ghost-model"),
        "the refusal must name the model: {printed}"
    );
    assert!(
        !printed.starts_with("OK:"),
        "an undeclared model must not resolve: {printed}"
    );
}

#[test]
fn models_lists_only_candidates_allowed_by_blueprint_rules() {
    let source = r#"import llm from "submilli:llm";
function main(): string {
    const names: string[] = [];
    for (const model of llm.models()) { names.push(model.name); }
    return JSON.stringify(names);
}"#;
    for (rule, expected) in [
        (
            "filter: model glob \"claude-*\"\n      action: allow",
            "[\"claude-haiku-4-5\",\"claude-sonnet-5\"]",
        ),
        (
            "filter: model == \"claude-sonnet-5\"\n      action: allow",
            "[\"claude-sonnet-5\"]",
        ),
        ("filter: model glob \"absent-*\"\n      action: allow", "[]"),
        (
            "action: allow",
            "[\"claude-haiku-4-5\",\"claude-sonnet-5\",\"internal-secret-model\"]",
        ),
        ("action: deny", "[]"),
        (
            "filter: model == \"claude-haiku-4-5\"\n      action: deny\n    - capability: llm.call\n      action: allow",
            "[\"claude-sonnet-5\",\"internal-secret-model\"]",
        ),
    ] {
        let yaml = format!(
            r#"name: llm-listing
default: deny
permissions:
  main:
    - capability: llm.call
      {rule}
llm:
  providers:
    fake:
      type: anthropic
  models:
    claude-haiku-4-5:
      provider: fake
    claude-sonnet-5:
      provider: fake
    internal-secret-model:
      provider: fake
"#
        );
        let home = tempfile::tempdir().expect("home");
        let fixture = fixture_with("llm_models", source, &yaml);
        let out = run(&fixture, home.path(), &[], &[]);
        assert!(out.status.success(), "rule {rule}: {}", stderr(&out));
        assert_eq!(stdout(&out).trim(), expected, "rule {rule}");
    }
}

/// The wiring this unit exists to add, proven from the CLI end: a declared model
/// now reaches a **real outbound HTTP request**.
///
/// It is proven by where the call fails. The blueprint points an
/// `openai-compatible` provider at a non-routable address, so a run that reached
/// the network comes back `transport` — the connector's own failure — whereas
/// one that never built a request would come back with the configuration
/// refusal instead. The two are distinguishable, which is what makes this an
/// assertion rather than a smoke test.
///
/// **No live API is contacted.** `192.0.2.1` is RFC 5737 TEST-NET-1, reserved
/// for documentation and not routable on the public internet.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_declared_model_reaches_a_real_outbound_request() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture_with("llm_reaches_wire", REPORT, UNREACHABLE_BLUEPRINT);
    let out = run(&f, home.path(), &[], &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let printed = stdout(&out);
    assert!(
        printed.contains("transport"),
        "the call must fail in the connector, which is what proves it built and \
         sent a request: {printed}"
    );
    // And it is the connector's failure, not the configuration refusal that
    // would appear if no request had been built.
    assert!(
        !printed.contains("test-model"),
        "an undeclared-model refusal means no request was ever sent: {printed}"
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
