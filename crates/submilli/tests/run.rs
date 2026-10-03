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
    write_acme_util_package_with_source(
        home,
        "export function answer(): number { return 41; }\nexport function plusOne(n: number): number { return n + 1; }\nexport function explode(): void { assert(false, \"pkg boom\"); }",
    );
}

fn write_acme_util_package_with_source(home: &Path, source: &str) {
    let package = compile_package(
        "@acme/util",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source,
        }],
        &[],
    )
    .expect("compile synthetic package");
    let dir = home.join("packages").join("@acme").join("util");
    write_package_artifact_with_docs_and_sources(
        &dir,
        &package.wasm,
        &package.type_info,
        &derive_capability_schema(&package.declaration, &[], &package.required_capabilities),
        &package.declaration,
        &ArtifactMetadata::new("@acme/util", "0.0.0-test", Vec::new()),
        "",
        &[ArtifactSource {
            path: ModulePath::from("lib"),
            text: source.to_string(),
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
fn blueprint_transitive_ancestor_is_available_but_not_importable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = compile_package(
        "@acme/base", ModulePath::from("lib"),
        &[PackageSourceModule { path: ModulePath::from("lib"), source:
            "export class Top { static readonly F: (n: number) => number = (n: number): number => n + 1; }" }], &[],
    ).expect("compile base");
    let mid = compile_package(
        "@acme/mid",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "import { Top } from \"@acme/base\"; export class Mid extends Top {}",
        }],
        &[&base.declaration],
    )
    .expect("compile mid");
    for (name, package, dependencies) in [
        ("base", &base, Vec::new()),
        (
            "mid",
            &mid,
            vec![submilli_build::ArtifactDependency::new(
                "@acme/base",
                "0.0.0-test",
            )],
        ),
    ] {
        write_package_artifact_with_docs_and_sources(
            tmp.path().join("packages/@acme").join(name),
            &package.wasm,
            &package.type_info,
            &derive_capability_schema(&package.declaration, &[], &package.required_capabilities),
            &package.declaration,
            &ArtifactMetadata::new(format!("@acme/{name}"), "0.0.0-test", dependencies),
            "",
            &[],
        )
        .expect("write artifact");
    }
    let blueprint = tmp.path().join("blueprint.yaml");
    write_blueprint(
        &blueprint,
        "name: transitive-test\npackages:\n  - \"@acme/mid\"\n",
    );
    let script = tmp.path().join("consumer.ts");
    fs::write(
        &script,
        r#"
        import { Mid } from "@acme/mid";
        class Local extends Mid {}
        function main(): number {
            assert(new Local() instanceof Mid, "subclass");
            return Mid.F(1);
        }
    "#,
    )
    .expect("write script");
    let args = [
        "run".as_ref(),
        script.as_os_str(),
        "--blueprint".as_ref(),
        blueprint.as_os_str(),
    ];
    let out = run_with_submilli_home(&args, tmp.path());
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "2\n");
    fs::write(
        &script,
        r#"
        import { Top } from "@acme/base";
        function main(): number { return Top.F(1); }
    "#,
    )
    .expect("write forbidden import");
    let out = run_with_submilli_home(&args, tmp.path());
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("package `@acme/base` is not a dependency"),
        "{}",
        stderr(&out)
    );
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
    assert!(
        err.contains("(@acme/util/lib:"),
        "missing package frame in {err}"
    );
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

/// Top-level statements run while the module is instantiated, before `main`.
/// A limit they reach is reported as the same limit reached in `main` is.
#[test]
fn a_limit_reached_by_top_level_statements_is_named() {
    const SPIN: &str = "let total = 0;\n\
        while (true) { total = total + 1; }\n\
        function main(): number { return total; }";
    // 2^26 code units = 128 MB, against the CLI's fixed 50 MB.
    const ALLOCATE: &str = "let s = \"x\";\n\
        for (let i = 0; i < 26; i++) { s = s + s; }\n\
        function main(): number { return s.length; }";
    let unbounded_fuel = "18446744073709551615";
    let cases: [(&str, &str, &[&str], &str); 3] = [
        (
            "top_level_fuel",
            SPIN,
            &["--fuel", "100000"],
            "error: fuel exhausted",
        ),
        ("top_level_memory", ALLOCATE, &[], "error: memory exhausted"),
        (
            "top_level_timeout",
            SPIN,
            &["--timeout", "50", "--fuel", unbounded_fuel],
            "error: timeout exceeded",
        ),
    ];
    for (name, source, flags, expected) in cases {
        let out = run_script(name, source, flags);
        assert!(!out.status.success(), "{name} should fail");
        let err = stderr(&out);
        assert!(
            err.contains(expected),
            "{name}: expected `{expected}`, got: {err}"
        );
    }
}

#[test]
fn a_top_level_throw_keeps_its_message_and_earlier_output() {
    let out = run_script(
        "top_level_throw",
        "const limits: number[] = [1];\n\
         console.log(\"before the throw\");\n\
         if (limits.length === 1) { throw new RangeError(\"refused at the top level\"); }\n\
         function main(): number { return 1; }",
        &[],
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.starts_with("before the throw\nerror: RangeError: refused at the top level\n"),
        "got: {err}"
    );
}

/// A failure raised by a top-level statement points at that statement, as one
/// raised in `main` points at its line.
#[test]
fn a_top_level_failure_renders_the_statement_that_raised_it() {
    const SPIN: &str = "let total = 0;\n\
        while (true) { total = total + 1; }\n\
        function main(): number { return total; }";
    let unbounded_fuel = "18446744073709551615";
    let cases: [(&str, &str, &[&str], &str, &str); 4] = [
        (
            "top_level_throw_frame",
            "const limits: number[] = [1];\n\
             if (limits.length === 1) { throw new RangeError(\"refused\"); }\n\
             function main(): number { return 1; }",
            &[],
            "[thrown here]",
            "2 | if (limits.length === 1) { throw new RangeError(\"refused\"); }\n  |",
        ),
        (
            "top_level_assert_frame",
            "const n: number = 2;\n\
             assert(n === 3, \"n is three\");\n\
             function main(): number { return n; }",
            &[],
            "[thrown here]",
            "2 | assert(n === 3, \"n is three\");\n  |",
        ),
        (
            "top_level_fuel_frame",
            SPIN,
            &["--fuel", "100000"],
            "[fuel exhausted]",
            "2 | while (true) { total = total + 1; }\n  |",
        ),
        (
            "top_level_timeout_frame",
            SPIN,
            &["--timeout", "50", "--fuel", unbounded_fuel],
            "[timeout exceeded]",
            "2 | while (true) { total = total + 1; }\n  |",
        ),
    ];
    for (name, source, flags, label, context) in cases {
        let out = run_script(name, source, flags);
        assert!(!out.status.success(), "{name} should fail");
        let err = stderr(&out);
        assert!(
            err.contains("\n  at <top level> (") && err.contains(&format!("{name}.subm:2:")),
            "{name}: expected a top-level frame on line 2, got: {err}"
        );
        assert!(
            err.contains(label),
            "{name}: expected `{label}`, got: {err}"
        );
        assert!(
            err.contains(context),
            "{name}: expected the statement and a caret, got: {err}"
        );
    }
}

/// A package's top-level statements run when it is installed, before the
/// program's. The deadline bounds them, and their failure points at the
/// package's statement.
#[test]
fn a_package_top_level_failure_renders_the_packages_statement() {
    // Seconds of spinning: well past the 200 ms deadline, yet a run the
    // deadline does not bound still ends (out of fuel) instead of hanging.
    let fuel_past_the_deadline = "100000000";
    let cases: [(&str, &[&str], &str, &str); 2] = [
        (
            "const table: number[] = [1];\nconst picked: number = table[7];\nexport function answer(): number { return picked; }",
            &[],
            "error: package `@acme/util` failed to initialize: RangeError: index out of range\n  at <top level> (@acme/util/lib:2:",
            "2 | const picked: number = table[7];\n  |",
        ),
        (
            "let spins = 0;\nwhile (true) { spins = spins + 1; }\nexport function answer(): number { return spins; }",
            &["--timeout", "200", "--fuel", fuel_past_the_deadline],
            "error: timeout exceeded\n  at <top level> (@acme/util/lib:2:",
            "2 | while (true) { spins = spins + 1; }\n  |",
        ),
    ];
    for (package_source, flags, header, context) in cases {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_acme_util_package_with_source(tmp.path(), package_source);
        let script = tmp.path().join("consumer.subm");
        fs::write(
            &script,
            "import { answer } from \"@acme/util\";\nfunction main(): number { return answer(); }",
        )
        .expect("write script");
        let blueprint = tmp.path().join("blueprint.yaml");
        write_blueprint(
            &blueprint,
            "name: package-test\npackages:\n  - \"@acme/util\"\n",
        );
        let mut args: Vec<&std::ffi::OsStr> = vec!["run".as_ref()];
        args.extend(flags.iter().map(std::ffi::OsStr::new));
        args.extend([
            script.as_os_str(),
            "--blueprint".as_ref(),
            blueprint.as_os_str(),
        ]);

        let out = run_with_submilli_home(&args, tmp.path());
        assert!(!out.status.success(), "stdout: {}", stdout(&out));
        let err = stderr(&out);
        assert!(err.contains(header), "expected `{header}`, got: {err}");
        assert!(err.contains(context), "expected the statement, got: {err}");
    }
}

#[test]
fn reaching_the_memory_limit_ends_the_run_past_a_catch() {
    // 2^26 code units = 128 MB, against the CLI's fixed 50 MB.
    let out = run_script(
        "memory_exhausted",
        r#"function main(): string {
            let s = "x";
            try {
                for (let i = 0; i < 26; i++) { s = s + s; }
            } catch (e) {
                return "caught";
            }
            return "done";
        }"#,
        &[],
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("error: memory exhausted"),
        "expected memory exhaustion in stderr, got: {err}"
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

#[test]
fn report_is_opt_in_and_keeps_the_result_on_stdout() {
    let source = "function main(): number { return 42; }";
    let plain = run_script("usage-plain", source, &[]);
    assert!(plain.status.success());
    assert_eq!(stdout(&plain), "42\n");
    assert!(!stderr(&plain).contains("fuel:"));
    let reported = run_script("usage-report", source, &["--report"]);
    assert!(reported.status.success(), "{}", stderr(&reported));
    assert_eq!(stdout(&reported), "42\n");
    let line = stderr(&reported);
    assert!(line.starts_with("fuel: "), "{line}");
    let (total, split) = line
        .trim_start_matches("fuel: ")
        .split_once(" (wasm ")
        .unwrap();
    let (wasm, rest) = split.split_once(", host ").unwrap();
    let (host, _) = rest.split_once(')').unwrap();
    let number = |grouped: &str| -> u64 { grouped.replace(',', "").parse().unwrap() };
    assert_eq!(number(total), number(wasm) + number(host), "{line}");
    assert!(number(wasm) > 0, "{line}");
    assert!(line.contains("memory peak:"), "{line}");
    assert!(line.contains(" ms (compile "), "{line}");
    assert!(line.contains(" ms, run "), "{line}");
}

/// The host fuel a program reports under `--report`.
fn host_fuel(name: &str, source: &str) -> u64 {
    let out = run_script(name, source, &["--report"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let line = stderr(&out);
    let (_, rest) = line.split_once(", host ").unwrap();
    let (host, _) = rest.split_once(')').unwrap();
    host.replace(',', "").parse().unwrap()
}

#[test]
fn each_host_call_is_charged_its_flat_cost() {
    use interpreter::runtime::fuel::CALL;
    const N: u64 = 10_000;
    // The same loop, once in Wasm alone and once calling a host function
    // that marshals nothing, so the difference is the flat charge per call.
    let plain = host_fuel(
        "usage-plain-loop",
        "function main(): number {
            let sum = 0;
            for (let i = 0; i < 10000; i++) { sum += i; }
            return sum;
        }",
    );
    let calling = host_fuel(
        "usage-calling-loop",
        "function main(): number {
            let sum = 0;
            for (let i = 0; i < 10000; i++) { sum += Math.abs(i); }
            return sum;
        }",
    );
    assert_eq!(calling - plain, N * CALL);
}

#[test]
fn accessors_do_not_pay_for_the_whole_receiver() {
    use interpreter::runtime::fuel::CALL;
    const N: u64 = 10_000;
    // Each loop reads one unit, element or length from a large receiver; if
    // an accessor copied the receiver first, the copy would show as fuel far
    // above the flat charge per call.
    let plain = host_fuel(
        "usage-accessor-plain",
        "function main(): number {
            const s = \"x\".repeat(100000);
            const a: number[] = [];
            for (let i = 0; i < 10000; i++) { a.push(i); }
            const b = Uint8Array.alloc(100000);
            let sum = 0;
            for (let i = 0; i < 10000; i++) { sum += i; }
            // One digit out, whatever the loop summed, so the result's own
            // marshalling costs the same in every case.
            return (sum + s.length + a.length + b.length) % 10;
        }",
    );
    // One case per accessor path.
    let cases = [
        ("usage-char-at", "sum += s.charAt(i).length;"),
        ("usage-string-at", "if (s.at(i) !== null) { sum += 1; }"),
        ("usage-char-code-at", "sum += s.charCodeAt(i);"),
        ("usage-code-point-at", "sum += s.codePointAt(i);"),
        ("usage-string-slice", "sum += s.slice(i, i + 1).length;"),
        ("usage-substring", "sum += s.substring(i, i + 1).length;"),
        (
            "usage-starts-with",
            "if (s.startsWith(\"x\", i)) { sum += 1; }",
        ),
        (
            "usage-ends-with",
            "if (s.endsWith(\"x\", i + 1)) { sum += 1; }",
        ),
        (
            "usage-array-at",
            "const v = a.at(i); if (v !== null) { sum += v; }",
        ),
        (
            "usage-array-pop",
            "const v = a.pop(); if (v !== null) { sum += v; }",
        ),
        ("usage-array-slice", "sum += a.slice(i, i + 1).length;"),
        ("usage-bytes-length", "sum += b.length;"),
        (
            "usage-bytes-at",
            "const v = b.at(i); if (v !== null) { sum += v; }",
        ),
        ("usage-bytes-slice", "sum += b.slice(i, i + 1).length;"),
    ];
    for (name, body) in cases {
        let source = format!(
            "function main(): number {{
                const s = \"x\".repeat(100000);
                const a: number[] = [];
                for (let i = 0; i < 10000; i++) {{ a.push(i); }}
                const b = Uint8Array.alloc(100000);
                let sum = 0;
                for (let i = 0; i < 10000; i++) {{ {body} }}
                return (sum + s.length + a.length + b.length) % 10;
            }}"
        );
        let extra = host_fuel(name, &source) - plain;
        // At least the call itself; at most that, the copy of a one-unit
        // result, and the unboxing of a nullable result or the building of a
        // one-element array, each one more host call. A copied receiver would
        // add ELEM(10,000) or COPY(100,000) per iteration on top.
        assert!(
            (N * CALL..=3 * N * CALL + 8 * CALL).contains(&extra),
            "{body}: {extra}"
        );
    }
}

#[test]
fn operations_charge_for_the_input_they_process() {
    use interpreter::runtime::fuel::{
        CALL, COPY, ELEM, IO, PARSE, REGEX, SCAN, SYSCALL, TZ, sort_cost,
    };
    // Each case builds a large input (the baseline) and then runs one
    // operation over it; the host fuel the operation adds must cover at
    // least its class charge for that input. The floors include what the
    // operation's own callbacks and allocations cost, so that dropping the
    // class charge itself would fall short.
    let text_input = "const s = \"x\".repeat(100000);";
    let json_input = "const s = \"[\" + \"1,\".repeat(50000) + \"1]\";";
    let sort_input = "const a: number[] = []; for (let i = 0; i < 8192; i++) { a.push(8192 - i); }";
    // Bottom-up merge sort of a descending power-of-two array: n/2 * log2(n)
    // comparisons, each one callback.
    let sort_comparisons: u64 = 8192 / 2 * 13;
    let cases = [
        (
            "regex",
            text_input,
            "return /y/.test(s) ? 1 : s.length % 10;",
            REGEX.cost(100_000),
        ),
        (
            "json-parse",
            json_input,
            "return JSON.parse(s) === null ? 0 : s.length % 10;",
            PARSE.cost(2 * 50_000 + 2) + ELEM.cost(50_001),
        ),
        (
            "sort",
            sort_input,
            "a.sort((x: number, y: number) => x - y); return a[0] % 10;",
            sort_cost(8192) + sort_comparisons * CALL,
        ),
        (
            "upper",
            text_input,
            "return s.toUpperCase().length % 10;",
            SCAN.cost(2 * 100_000),
        ),
    ];
    for (name, input, operation, floor) in cases {
        let baseline = host_fuel(
            &format!("usage-{name}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let with_operation = host_fuel(
            &format!("usage-{name}"),
            &format!("function main(): number {{ {input} {operation} }}"),
        );
        let added = with_operation - baseline;
        assert!(added >= floor, "{name}: {added} < {floor}");
    }

    for n in [128_u64, 256] {
        for (method, per_call) in [
            ("exec", CALL + REGEX.cost(1) + SCAN.cost(1) + COPY.cost(1)),
            ("test", CALL + REGEX.cost(1)),
        ] {
            let input = format!("const s = \"x\".repeat({n}); const r = /x/g;");
            let baseline = host_fuel(
                &format!("usage-regex-{method}-{n}-base"),
                &format!("function main(): number {{ {input} return 0; }}"),
            );
            let actual = host_fuel(
                &format!("usage-regex-{method}-{n}"),
                &format!(
                    "function main(): number {{ {input} for (let i = 0; i < {n}; i++) {{ r.{method}(s); }} return 0; }}"
                ),
            ) - baseline;
            let decoding = COPY.cost(n) + SCAN.cost(n);
            assert_eq!(actual, decoding + n * per_call);
            // Old exec cost 21,760/80,384: every call paid full decoding.
            assert!(actual < n * (decoding + per_call));
        }
        for (pattern, slots) in [("/x/g", 0_u64), ("/x|()()()()()()()()/g", 8)] {
            let input = format!("const s = \"x\".repeat({n}); const r = {pattern};");
            let baseline = host_fuel(
                &format!("usage-matchall-{slots}-{n}-base"),
                &format!("function main(): number {{ {input} return 0; }}"),
            );
            let actual = host_fuel(
                &format!("usage-matchall-{slots}-{n}"),
                &format!("function main(): number {{ {input} s.matchAll(r); return 0; }}"),
            ) - baseline;
            // The empty alternative also matches once at the end of the input.
            let end_match = if slots == 0 {
                0
            } else {
                ELEM.cost(slots) + ELEM.cost(1)
            };
            // Input was already shared; only capture-array slots were missing.
            assert_eq!(
                actual,
                CALL + COPY.cost(n)
                    + SCAN.cost(n)
                    + n * (REGEX.cost(1) + SCAN.cost(1) + COPY.cost(1) + ELEM.cost(slots))
                    + ELEM.cost(n)
                    + end_match
            );
        }
    }

    // A structural visit budget does not change the cost of accepted walks:
    // two array snapshots, one hook per element, and the outer call + hook.
    for n in [128_u64, 256] {
        let input = format!(
            "const a: number[] = []; const b: number[] = [];
             for (let i = 0; i < {n}; i++) {{ a.push(i); b.push(i); }}"
        );
        let baseline = host_fuel(
            &format!("usage-structural-{n}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let actual = host_fuel(
            &format!("usage-structural-{n}"),
            &format!("function main(): number {{ {input} Object.is(a, b); return 0; }}"),
        ) - baseline;
        assert_eq!(actual, 2 * CALL + ELEM.cost(2 * n) + n * CALL);
    }

    let mut empty_json_cost = 0;
    for n in [0_u64, 128, 256] {
        let input =
            format!("const a: number[] = []; for (let i = 0; i < {n}; i++) {{ a.push(7); }}");
        let baseline = host_fuel(
            &format!("usage-typed-json-{n}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let actual = host_fuel(
            &format!("usage-typed-json-{n}"),
            &format!(
                "function main(): number {{ {input} JSON.stringify({{items:a}}); return 0; }}"
            ),
        ) - baseline;
        if n == 0 {
            empty_json_cost = actual;
            continue;
        }
        // Preflight and serialization both visit each element. Previously only
        // output marshalling grew: this exact delta catches that undercharge.
        let output_len = 2 * n + 11;
        assert_eq!(
            actual - empty_json_cost,
            ELEM.cost(2 * n) + SCAN.cost(output_len - 12) + COPY.cost(output_len) - COPY.cost(12),
        );
    }

    for (name, operation, expected_delta) in [
        (
            "file-copy",
            "copy(\"/input\", \"/output\", false);",
            IO.cost(128),
        ),
        (
            "line-read",
            "for (const line of lines(\"/input\")) { const n = line.length; }",
            IO.cost(128) + SCAN.cost(2 * 128) + COPY.cost(128),
        ),
    ] {
        let mut costs = Vec::new();
        for n in [128, 256] {
            let input = format!("writeText(\"/input\", \"x\".repeat({n}));");
            let source = |operation: &str| {
                format!(
                    "import {{writeText, copy, lines}} from \"submilli:fs\";
                 function main(): number {{ {input} {operation} return 0; }}"
                )
            };
            let baseline = host_fuel(&format!("usage-{name}-{n}-baseline"), &source(""));
            costs.push(host_fuel(&format!("usage-{name}-{n}"), &source(operation)) - baseline);
        }
        assert_eq!(costs[1] - costs[0], expected_delta, "{name}");
    }

    let mut copy_costs = Vec::new();
    for n in [2, 4] {
        let source = |operation: &str| {
            format!(
                "import {{mkdir, writeText, copy}} from \"submilli:fs\";
             function main(): number {{ mkdir(\"/input\", false);
             for (let i = 0; i < {n}; i++) {{ writeText(\"/input/\" + i.toString(), \"\"); }}
             {operation} return 0; }}"
            )
        };
        let baseline = host_fuel(&format!("usage-copy-entries-{n}-baseline"), &source(""));
        copy_costs.push(
            host_fuel(
                &format!("usage-copy-entries-{n}"),
                &source("copy(\"/input\", \"/output\", true);"),
            ) - baseline,
        );
    }
    assert_eq!(copy_costs[1] - copy_costs[0], SYSCALL.cost(2));

    for n in [128_u64, 256] {
        let input = format!("const s = \"x\".repeat({n});");
        let baseline = host_fuel(
            &format!("usage-console-{n}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let actual = host_fuel(
            &format!("usage-console-{n}"),
            &format!("function main(): number {{ {input} console.log(s); return 0; }}"),
        ) - baseline;
        // Previously only 2 CALL + COPY(n): conversion and output were free.
        assert_eq!(
            actual,
            2 * CALL + COPY.cost(n) + SCAN.cost(n) + IO.cost(n + 1)
        );
    }

    for (digits, limbs) in [(128_u64, 7_u64), (256, 14)] {
        let input = format!("const s = \"7\".repeat({digits});");
        let baseline = host_fuel(
            &format!("usage-bigint-parse-{digits}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let actual = host_fuel(
            &format!("usage-bigint-parse-{digits}"),
            &format!("function main(): number {{ {input} BigInt(s); return 0; }}"),
        ) - baseline;
        // Previously 230/444 fuel: only marshalling, no decimal conversion.
        assert_eq!(
            actual,
            CALL + COPY.cost(digits)
                + SCAN.cost(digits)
                + ELEM.cost(limbs)
                + ELEM.cost(digits * digits.div_ceil(19))
        );
    }

    for limbs in [8_u64, 16] {
        let input = format!("const n = 2n ** {}n - 1n;", limbs * 64);
        let baseline = host_fuel(
            &format!("usage-bigint-hex-{limbs}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let actual = host_fuel(
            &format!("usage-bigint-hex-{limbs}"),
            &format!("function main(): number {{ {input} n.toString(16); return 0; }}"),
        ) - baseline;
        // Formatting these power-of-two radices extracts bits linearly.
        assert_eq!(
            actual,
            CALL + 2 * ELEM.cost(limbs) + SCAN.cost(16 * limbs) + COPY.cost(16 * limbs)
        );
    }

    for n in [1_u64, 2] {
        let input =
            "const a = Temporal.ZonedDateTime.from(\"2024-03-09T12:00:00-05:00[US/Eastern]\");
                     const b = a.withTimeZone(\"America/New_York\");";
        let baseline = host_fuel(
            &format!("usage-zone-{n}-baseline"),
            &format!("function main(): number {{ {input} return 0; }}"),
        );
        let actual = host_fuel(
            &format!("usage-zone-{n}"),
            &format!(
                "function main(): number {{ {input}
                      for (let i = 0; i < {n}; i++) {{ a.equals(b); }} return 0; }}"
            ),
        ) - baseline;
        // Old transition walks added 162,056 fuel per comparison. Identity
        // lookup now costs exactly two bounded lookups plus argument reads.
        assert_eq!(
            actual,
            n * (CALL + 2 * TZ + SCAN.cost(10) + COPY.cost(10) + SCAN.cost(16) + COPY.cost(16))
        );
    }
}

#[test]
fn report_captures_fuel_exhaustion_in_top_level_code() {
    let out = run_script(
        "usage-top-level",
        "while (true) {} function main(): void {}",
        &["--report", "--fuel", "100000"],
    );
    assert!(!out.status.success());
    assert!(stdout(&out).is_empty());
    assert!(stderr(&out).contains("fuel exhausted"), "{}", stderr(&out));
    assert!(stderr(&out).contains("fuel: 100,000"), "{}", stderr(&out));
}
