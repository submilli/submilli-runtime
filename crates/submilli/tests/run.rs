//! End-to-end integration tests for `submilli run`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use interpreter::{ModulePath, PackageSourceModule, compile_package};
use submilli_build::{
    ArtifactMetadata, ArtifactSource, derive_capability_schema,
    write_package_artifact_with_docs_and_sources,
};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn write_script(name: &str, source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("submilli-test-{name}.subm"));
    fs::write(&path, source).expect("write temp script");
    path
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .output()
        .expect("invoke submilli")
}

fn run_with_submilli_home(args: &[&std::ffi::OsStr], home: &Path) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli")
}

fn run_script(name: &str, source: &str, extra: &[&str]) -> Output {
    let path = write_script(name, source);
    let mut args: Vec<&std::ffi::OsStr> = Vec::new();
    args.push(std::ffi::OsStr::new("run"));
    for arg in extra {
        args.push(std::ffi::OsStr::new(*arg));
    }
    args.push(path.as_os_str());
    run(&args)
}

fn stdout(out: &Output) -> &str {
    std::str::from_utf8(&out.stdout).expect("stdout utf-8")
}

fn stderr(out: &Output) -> &str {
    std::str::from_utf8(&out.stderr).expect("stderr utf-8")
}

fn write_blueprint(path: &Path, source: &str) {
    fs::write(path, source).expect("write blueprint");
}

#[test]
fn git_identity_and_permissions_use_cli_variable_bindings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let script = tmp.path().join("git.ts");
    fs::write(
        &script,
        r#"import { Repository } from "submilli:git";
import { writeText } from "submilli:fs";
function main(): string {
    const repo = Repository.init("/repo", { branch: "main" });
    writeText("/repo/note.txt", "hello");
    repo.add(["note.txt"]);
    repo.commit("CLI variables");
    const author = repo.log().commits[0];
    return author.authorName + " <" + author.authorEmail + ">";
}"#,
    )
    .expect("write script");
    let blueprint = tmp.path().join("blueprint.yaml");
    write_blueprint(
        &blueprint,
        r#"name: git-cli
variables:
  author: { default: Default Author }
  branch: { required: true }
git:
  identity:
    name: '${vars.author}'
    email: cli@example.com
permissions:
  main:
    - { capability: git.init, action: allow }
    - { capability: fs.write, action: allow }
    - capability: git.commit
      action: allow
      filter: branch == ${vars.branch}
"#,
    );
    let out = run_with_submilli_home(
        &[
            "run".as_ref(),
            "--blueprint".as_ref(),
            blueprint.as_os_str(),
            "--var".as_ref(),
            "author=CLI Author".as_ref(),
            "--var".as_ref(),
            "branch=main".as_ref(),
            script.as_os_str(),
        ],
        tmp.path(),
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "CLI Author <cli@example.com>\n");
}

fn write_acme_util_package(home: &Path) {
    const SOURCE: &str = "export function answer(): number { return 41; }\nexport function plusOne(n: number): number { return n + 1; }\nexport function explode(): void { assert(false, \"pkg boom\"); }";
    let package = compile_package(
        "@acme/util",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: SOURCE,
        }],
        &[],
    )
    .expect("compile synthetic package");
    let dir = home.join("packages").join("@acme").join("util");
    write_package_artifact_with_docs_and_sources(
        &dir,
        &package.wasm,
        &package.type_info,
        &derive_capability_schema(&package.declaration, &package.required_capabilities),
        &package.declaration,
        &ArtifactMetadata::new("@acme/util", "0.0.0-test", Vec::new()),
        "",
        &[ArtifactSource {
            path: ModulePath::from("lib"),
            text: SOURCE.to_string(),
        }],
    )
    .expect("write package artifact");
}

#[test]
fn string_return_with_console() {
    let out = run_script(
        "string_return_with_console",
        r#"function main(): string { console.log("hi"); return "ok"; }"#,
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // A `string` return prints verbatim — no JSON quoting.
    assert_eq!(stdout(&out), "ok\n");
    assert_eq!(stderr(&out), "hi\n");
}

#[test]
fn number_return() {
    let out = run_script(
        "number_return",
        "function main(): number { return 42; }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
    assert_eq!(stderr(&out), "");
}

#[test]
fn blueprint_package_dependency_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_acme_util_package(tmp.path());
    let script = tmp.path().join("consumer.subm");
    fs::write(
        &script,
        r#"
            import { answer, plusOne } from "@acme/util";
            function main(): number { return plusOne(answer()); }
        "#,
    )
    .expect("write script");
    let blueprint = tmp.path().join("blueprint.yaml");
    write_blueprint(
        &blueprint,
        "name: package-test\npackages:\n  - \"@acme/util\"\n",
    );

    let out = run_with_submilli_home(
        &[
            std::ffi::OsStr::new("run"),
            script.as_os_str(),
            std::ffi::OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
}

#[test]
fn package_error_renders_package_source_context() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_acme_util_package(tmp.path());
    let script = tmp.path().join("package-error.subm");
    fs::write(
        &script,
        r#"
            import { explode } from "@acme/util";
            function main(): void { explode(); }
        "#,
    )
    .expect("write script");
    let blueprint = tmp.path().join("blueprint.yaml");
    write_blueprint(
        &blueprint,
        "name: package-test\npackages:\n  - \"@acme/util\"\n",
    );

    let out = run_with_submilli_home(
        &[
            std::ffi::OsStr::new("run"),
            script.as_os_str(),
            std::ffi::OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    let err = stderr(&out);
    assert!(err.contains("error: Error: pkg boom"), "stderr: {err}");
    assert!(err.contains("lib:"), "missing package frame in {err}");
    assert!(
        err.contains("export function explode(): void"),
        "missing package source in {err}",
    );
    assert!(err.contains("^"), "missing caret in {err}");
}

#[test]
fn package_in_store_but_omitted_from_blueprint_is_not_importable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_acme_util_package(tmp.path());
    let script = tmp.path().join("consumer-missing-blueprint.subm");
    fs::write(
        &script,
        r#"
            import { answer } from "@acme/util";
            function main(): number { return answer(); }
        "#,
    )
    .expect("write script");
    let blueprint = tmp.path().join("blueprint.yaml");
    write_blueprint(&blueprint, "name: package-test\n");

    let out = run_with_submilli_home(
        &[
            std::ffi::OsStr::new("run"),
            script.as_os_str(),
            std::ffi::OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("package `@acme/util` not found"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn boolean_return() {
    let out = run_script(
        "boolean_return",
        "function main(): boolean { return false; }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "false\n");
}

#[test]
fn void_main_no_stdout() {
    let out = run_script(
        "void_main_no_stdout",
        r#"function main(): void { console.log("done"); }"#,
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "done\n");
}

#[test]
fn compile_error_to_stderr() {
    let out = run_script(
        "compile_error",
        r#"function main(): number { return "x"; }"#,
        &[],
    );
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
    let err = stderr(&out);
    assert!(err.contains("error:"), "missing `error:` in {err}");
    assert!(err.contains("^"), "missing caret in {err}");
}

#[test]
fn failed_assert_renders_backtrace() {
    let out = run_script(
        "assert_backtrace",
        r#"function main(): void { assert(false, "boom"); }"#,
        &[],
    );
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
    let err = stderr(&out);
    assert!(
        err.contains("error: Error: boom"),
        "missing thrown message in {err}",
    );
    assert!(
        err.contains("thrown here"),
        "missing throw-site label in {err}",
    );
    assert!(err.contains("at main"), "missing frame header in {err}");
}

#[test]
fn timeout_interrupts_infinite_loop() {
    let out = run_script(
        "timeout_loop",
        "function main(): void { while (true) { } }",
        // u64::MAX fuel so the loop outlives the 50 ms watchdog rather than hitting OutOfFuel first.
        &["--timeout", "50", "--fuel", "18446744073709551615"],
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("timeout exceeded"),
        "missing timeout label in {err}",
    );
}

#[test]
fn boolean_to_string_method_call() {
    let out = run_script(
        "sub71_bool_to_string",
        "function main(): void { console.log(true.toString()); }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "true\n");
}

#[test]
#[allow(non_snake_case)]
fn boolean_to_string_via_String_alias() {
    let out = run_script(
        "sub71_bool_String_alias",
        "function main(): void { console.log(String(false)); }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "false\n");
}

#[test]
fn string_to_string_is_identity() {
    let out = run_script(
        "sub71_string_identity",
        r#"function main(): void { console.log("hi".toString()); }"#,
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "hi\n");
}

#[test]
fn array_of_numbers_to_string_joins_with_comma() {
    let out = run_script(
        "sub71_array_numbers_to_string",
        "function main(): void { console.log([1, 2, 3].toString()); }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "1,2,3\n");
}

#[test]
fn array_of_booleans_to_string_dispatches_per_element() {
    let out = run_script(
        "sub71_array_bools_to_string",
        "function main(): void { console.log([true, false].toString()); }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "true,false\n");
}

#[test]
fn empty_array_to_string_is_empty() {
    let out = run_script(
        "sub71_empty_array",
        r#"function main(): void { let xs: number[] = []; console.log("[" + xs.toString() + "]"); }"#,
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "[]\n");
}

#[test]
fn array_join_with_custom_separator() {
    let out = run_script(
        "sub71_array_join",
        r#"function main(): void { console.log([1, 2, 3].join("-")); }"#,
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "1-2-3\n");
}

#[test]
fn object_to_string_returns_object_object() {
    let out = run_script(
        "sub71_object_to_string",
        "function main(): void { console.log({ a: 1, b: 2 }.toString()); }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "[object Object]\n");
}

#[test]
#[allow(non_snake_case)]
fn object_to_string_via_String_alias() {
    let out = run_script(
        "sub71_object_String_alias",
        r#"function main(): void { let o = { x: 1 }; console.log(String(o)); }"#,
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "[object Object]\n");
}

#[test]
fn nested_array_to_string_dispatches_recursively() {
    let out = run_script(
        "sub71_nested_array",
        "function main(): void { console.log([[1, 2], [3]].toString()); }",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "1,2,3\n");
}

#[test]
fn to_string_field_signature_enforced() {
    let out = run_script(
        "sub136_tostring_signature",
        r#"function main(): void { let o = { toString: 1 }; console.log("ok"); }"#,
        &[],
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("field `toString` must have type `() => string`"),
        "missing toString signature diagnostic in {err}",
    );
}

#[test]
fn to_string_on_null_diagnoses_narrow_first() {
    let out = run_script(
        "sub71_null_narrow_first",
        "function main(): void { let n = null; n.toString(); }",
        &[],
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("narrow to a non-null type first"),
        "missing narrow-first diagnostic in {err}",
    );
}

#[test]
fn help_lists_run_subcommand() {
    let out = run(&[std::ffi::OsStr::new("--help")]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("run"), "help missing run: {text}");
}

#[test]
fn run_help_lists_flags() {
    let out = run(&[std::ffi::OsStr::new("run"), std::ffi::OsStr::new("--help")]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    for flag in ["--fuel", "--max-stack", "--timeout"] {
        assert!(text.contains(flag), "help missing {flag}: {text}");
    }
}
