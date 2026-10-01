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
        &derive_capability_schema(&package.declaration, &package.required_capabilities),
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
            &derive_capability_schema(&package.declaration, &package.required_capabilities),
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
