//! `submilli run` exit status for an uncaught permission denial.
//!
//! A denial the runtime threw that escapes the program exits 3, so a script can
//! tell a refused capability from a bug. Everything else that fails stays 1, and
//! the printed text is the same either way.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn stderr(out: &Output) -> &str {
    std::str::from_utf8(&out.stderr).expect("stderr utf-8")
}

const DENY_ALL: &str = "name: denial\ndefault: deny\n";

const DENIED: &str = r#"
import { check } from "submilli:security";
function main(): number { check("test.com/op", { amount: 100 }); return 1; }
"#;

fn run(dir: &Path, name: &str, source: &str, blueprint: Option<&str>) -> Output {
    let script = dir.join(format!("{name}.ts"));
    fs::write(&script, source).expect("write script");
    let mut cmd = Command::new(submilli_bin());
    cmd.arg("run").arg(&script).env("SUBMILLI_HOME", dir);
    if let Some(yaml) = blueprint {
        let path = dir.join("blueprint.yaml");
        fs::write(&path, yaml).expect("write blueprint");
        cmd.arg("--blueprint").arg(&path);
    }
    cmd.output().expect("invoke submilli")
}

#[test]
fn an_uncaught_denial_exits_3_with_the_usual_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), "denied", DENIED, Some(DENY_ALL));

    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains(
            "PermissionDeniedError: permission denied: caller=main capability=test.com/op"
        ),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn a_denial_from_a_top_level_statement_exits_3() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
import { check } from "submilli:security";
check("test.com/op", { amount: 100 });
function main(): number { return 1; }
"#;
    let out = run(dir.path(), "top_level", source, Some(DENY_ALL));

    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
}

#[test]
fn a_program_built_permission_denied_error_exits_1() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
function main(): number {
    throw new PermissionDeniedError("permission denied: forged", "main", "fs.read", "forged");
}
"#;
    let out = run(dir.path(), "forged", source, Some(DENY_ALL));

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("PermissionDeniedError"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn another_runtime_error_exits_1() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = "function main(): number { throw new Error(\"boom\"); }";
    let out = run(dir.path(), "boom", source, None);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("Error: boom"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn a_caught_denial_exits_0() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"
import { check } from "submilli:security";
function main(): string {
    try { check("test.com/op", { amount: 100 }); return "allowed"; }
    catch (e: PermissionDeniedError) { return "caught"; }
}
"#;
    let out = run(dir.path(), "caught", source, Some(DENY_ALL));

    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
}
