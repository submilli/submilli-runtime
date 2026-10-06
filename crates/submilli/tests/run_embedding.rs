//! `submilli run` wiring for `submilli:embedding`.
//!
//! The binary cannot be handed a fake dispatch, so these cover what is visible
//! from outside: alias resolution, the settings ladder's refusals, and (against
//! a non-routable address) that a declared alias reaches a real request. The
//! per-run ceiling against a fake dispatch is covered by unit tests beside
//! `commands::run`.

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

/// One Hugging Face alias on a non-routable address. `192.0.2.1` is RFC 5737
/// TEST-NET-1, so a run that reaches the network fails in the connector rather
/// than at somebody's API.
const BLUEPRINT: &str = r#"name: embedding-cli
permissions:
  main:
    - capability: embedding.embed
      action: allow
embedding:
  providers:
    unreachable:
      type: huggingface
      base_url: https://192.0.2.1
  models:
    docs:
      provider: unreachable
      model: bge
      dimensions: 4
"#;

const MODELS: &str = r#"import embedding from "submilli:embedding";
function main(): string {
    const names: string[] = [];
    for (const m of embedding.models()) { names.push(m.name + ":" + m.dimensions.toString()); }
    return names.join(",");
}"#;

const EMBED: &str = r#"import embedding from "submilli:embedding";
function main(): string {
    try {
        return "OK:" + embedding.embed("docs", ["hi"], "document").count.toString();
    } catch (e: Error) {
        return e.message;
    }
}"#;

const UNDECLARED: &str = r#"import embedding from "submilli:embedding";
function main(): string {
    try {
        return "OK:" + embedding.embed("ghost", ["hi"], "document").count.toString();
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
    let script = dir.path().join(format!("{name}.ts"));
    fs::write(&script, source).expect("write script");
    let blueprint = dir.path().join("blueprint.yaml");
    fs::write(&blueprint, BLUEPRINT).expect("write blueprint");
    Fixture {
        _dir: dir,
        script,
        blueprint,
    }
}

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

/// `models()` does no I/O, so it proves the provider is installed from the
/// blueprint without touching a socket.
#[test]
fn models_lists_the_declared_aliases() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("embed_models", MODELS);
    let out = run(&f, home.path(), &[], &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "docs:4");
}

#[test]
fn an_undeclared_alias_is_refused_naming_it() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("embed_undeclared", UNDECLARED);
    let out = run(&f, home.path(), &[], &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let printed = stdout(&out);
    assert!(printed.contains("ghost"), "{printed}");
    assert!(!printed.starts_with("OK:"), "{printed}");
}

/// A declared alias reaches a real outbound request: the call fails in the
/// connector (`transport` or `timeout`), not with a configuration refusal.
#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn a_declared_alias_reaches_a_real_outbound_request() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("embed_wire", EMBED);
    let out = run(&f, home.path(), &[], &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let printed = stdout(&out);
    assert!(
        printed.contains("transport") || printed.contains("timeout"),
        "the call must fail in the connector: {printed}"
    );
}

#[test]
fn a_malformed_ceiling_fails_the_run_naming_the_variable() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("embed_malformed", EMBED);
    for name in [
        "SUBMILLI_MAX_EXECUTION_EMBEDDING_TOKENS",
        "SUBMILLI_MAX_EXECUTION_EMBEDDING_REQUESTS",
        "SUBMILLI_MAX_EMBEDDING_CONCURRENCY",
    ] {
        let out = run(&f, home.path(), &[(name, "lots")], &[]);
        assert!(!out.status.success(), "{name}");
        assert!(stderr(&out).contains(name), "{name}: {}", stderr(&out));
    }
}

#[test]
fn zero_settings_are_rejected() {
    let home = tempfile::tempdir().expect("home");
    let f = fixture("embed_zero", EMBED);
    for flag in [
        "--max-execution-embedding-tokens",
        "--max-execution-embedding-requests",
        "--max-embedding-concurrency",
    ] {
        let out = run(&f, home.path(), &[], &[flag, "0"]);
        assert!(!out.status.success(), "{flag}");
        assert!(
            stderr(&out).contains("at least 1"),
            "{flag}: {}",
            stderr(&out)
        );
    }
}
