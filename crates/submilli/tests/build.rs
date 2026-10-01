//! End-to-end integration tests for `submilli build`.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn run_with_home(args: &[&OsStr], home: &Path) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli")
}

fn stdout(out: &Output) -> &str {
    std::str::from_utf8(&out.stdout).expect("stdout utf-8")
}

fn stderr(out: &Output) -> &str {
    std::str::from_utf8(&out.stderr).expect("stderr utf-8")
}

fn write_file(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create dirs");
    fs::write(path, text).expect("write file");
    if let Some(package_dir) = package_dir_for_source(path) {
        let docs = package_dir.join("docs/readme.md");
        if !docs.exists() {
            fs::create_dir_all(docs.parent().expect("docs parent")).expect("create docs dir");
            fs::write(&docs, "# Test package\n").expect("write docs");
        }
    }
}

fn package_dir_for_source(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    if parent.file_name()? == "src" {
        parent.parent()
    } else {
        None
    }
}

fn build(project: &Path, home: &Path, extra: &[&str]) -> Output {
    build_subcommand("publish-local", project, home, extra)
}

fn build_subcommand(subcommand: &str, cwd: &Path, home: &Path, extra: &[&str]) -> Output {
    Command::new(submilli_bin())
        .args(["build", subcommand])
        .args(extra)
        .current_dir(cwd)
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli")
}

fn artifact_files_exist(home: &Path, name: &str) -> bool {
    let (scope, package) = name.split_once('/').expect("scoped name");
    let dir = home.join("packages").join(scope).join(package);
    [
        "pkg.wasm",
        "capabilities.yaml",
        "package-declaration.json",
        "metadata.json",
        "docs/readme.md",
    ]
    .iter()
    .all(|file| dir.join(file).is_file())
}

#[test]
fn single_package_builds_installs_and_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );

    let out = build(&project, tmp.path(), &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(artifact_files_exist(tmp.path(), "@acme/util"));
    assert!(
        stderr(&out).contains("installed @acme/util v0.1.0"),
        "stderr: {}",
        stderr(&out)
    );

    let script = tmp.path().join("consumer.subm");
    write_file(
        &script,
        r#"
            import { answer } from "@acme/util";
            function main(): number { return answer(); }
        "#,
    );
    let blueprint = tmp.path().join("blueprint.yaml");
    write_file(
        &blueprint,
        "name: build-test\npackages:\n  - \"@acme/util\"\n",
    );

    let out = run_with_home(
        &[
            OsStr::new("run"),
            script.as_os_str(),
            OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
}

#[test]
fn ts_and_subm_packages_build_and_run_together() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "Test package."
path = "app"
dependencies = ["@acme/util"]

[[package]]
name = "@acme/util"
version = "0.1.0"
description = "Test package."
path = "util"
"#,
    );
    write_file(
        &project.join("util/src/lib.subm"),
        "export function forty(): number { return 40; }",
    );
    write_file(
        &project.join("app/src/wrap.ts"),
        r#"
            import { forty } from "@acme/util";
            export function fortyTwo(): number { return forty() + 2; }
        "#,
    );
    write_file(
        &project.join("app/src/lib.ts"),
        "export { fortyTwo } from \"./wrap.ts\";\n",
    );

    let out = build_subcommand("check", &project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let out = build(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let script = tmp.path().join("consumer.ts");
    write_file(
        &script,
        r#"
            import { fortyTwo } from "@acme/app";
            function main(): number { return fortyTwo(); }
        "#,
    );
    let blueprint = tmp.path().join("blueprint.yaml");
    write_file(
        &blueprint,
        "name: build-test\npackages:\n  - \"@acme/app\"\n",
    );

    let out = run_with_home(
        &[
            OsStr::new("run"),
            script.as_os_str(),
            OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
}

#[test]
fn monorepo_links_in_dependency_order_with_transitive_load() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    // Names chosen so alphabetical order is wrong: @a/app must instantiate
    // after its dependency @z/util.
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@a/app"
version = "0.1.0"
description = "Test package."
path = "app"
dependencies = ["@z/util"]

[[package]]
name = "@z/util"
version = "0.1.0"
description = "Test package."
path = "util"
"#,
    );
    write_file(
        &project.join("util/src/lib.subm"),
        "export function forty(): number { return 40; }",
    );
    write_file(
        &project.join("app/src/lib.subm"),
        r#"
            import { forty } from "@z/util";
            export function fortyTwo(): number { return forty() + 2; }
        "#,
    );

    let out = build(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(artifact_files_exist(tmp.path(), "@a/app"));
    assert!(artifact_files_exist(tmp.path(), "@z/util"));

    // The blueprint lists only the root; @z/util must be loaded transitively
    // from artifact metadata and instantiated first.
    let script = tmp.path().join("consumer.subm");
    write_file(
        &script,
        r#"
            import { fortyTwo } from "@a/app";
            function main(): number { return fortyTwo(); }
        "#,
    );
    let blueprint = tmp.path().join("blueprint.yaml");
    write_file(&blueprint, "name: build-test\npackages:\n  - \"@a/app\"\n");

    let out = run_with_home(
        &[
            OsStr::new("run"),
            script.as_os_str(),
            OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
}

#[test]
fn package_calls_sibling_and_checks_provided_capability_under_main() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@a/app"
version = "0.1.0"
description = "Test package."
path = "app"
dependencies = ["@z/util"]

[[package]]
name = "@z/util"
version = "0.1.0"
description = "Test package."
path = "util"
"#,
    );
    write_file(
        &project.join("util/src/lib.subm"),
        "export function forty(): number { return 40; }",
    );
    write_file(
        &project.join("app/src/lib.subm"),
        r#"
            import { forty } from "@z/util";
            import { check } from "submilli:security";
            /** @capability test.com/op { amount: number } */
            export function fortyTwo(): number {
                check("test.com/op", { amount: 100 });
                return forty() + 2;
            }
        "#,
    );
    let out = build(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let script = tmp.path().join("consumer.subm");
    write_file(
        &script,
        r#"
            import { fortyTwo } from "@a/app";
            function main(): number { return fortyTwo(); }
        "#,
    );
    // Deny-by-default: the run only succeeds if the `check()` inside `@a/app`
    // is attributed to the package's caller, not the transparent security shim.
    let blueprint = tmp.path().join("blueprint.yaml");
    write_file(
        &blueprint,
        "\
name: build-test
default: deny
packages:
  - \"@a/app\"
permissions:
  main:
    - capability: test.com/op
      action: allow
",
    );

    let out = run_with_home(
        &[
            OsStr::new("run"),
            script.as_os_str(),
            OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "42\n");
}

#[test]
fn closure_only_dependency_is_not_importable_by_the_script() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@a/app"
version = "0.1.0"
description = "Test package."
path = "app"
dependencies = ["@z/util"]

[[package]]
name = "@z/util"
version = "0.1.0"
description = "Test package."
path = "util"
"#,
    );
    write_file(
        &project.join("util/src/lib.subm"),
        "export function forty(): number { return 40; }",
    );
    write_file(
        &project.join("app/src/lib.subm"),
        r#"
            import { forty } from "@z/util";
            export function fortyTwo(): number { return forty() + 2; }
        "#,
    );
    let out = build(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let script = tmp.path().join("consumer.subm");
    write_file(
        &script,
        r#"
            import { forty } from "@z/util";
            function main(): number { return forty(); }
        "#,
    );
    let blueprint = tmp.path().join("blueprint.yaml");
    write_file(&blueprint, "name: build-test\npackages:\n  - \"@a/app\"\n");

    let out = run_with_home(
        &[
            OsStr::new("run"),
            script.as_os_str(),
            OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).contains("@z/util"), "stderr: {}", stderr(&out));
}

#[test]
fn package_flag_scopes_to_the_dependency_closure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "Test package."
path = "app"
dependencies = ["@acme/util"]

[[package]]
name = "@acme/util"
version = "0.1.0"
description = "Test package."
path = "util"

[[package]]
name = "@acme/other"
version = "0.1.0"
description = "Test package."
path = "other"
"#,
    );
    write_file(
        &project.join("util/src/lib.subm"),
        "export const u: number = 1;",
    );
    write_file(
        &project.join("app/src/lib.subm"),
        "import { u } from \"@acme/util\";\nexport const a: number = u;",
    );
    write_file(
        &project.join("other/src/lib.subm"),
        "export const o: number = 1;",
    );

    let out = build(&project, tmp.path(), &["-p", "@acme/app"]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(artifact_files_exist(tmp.path(), "@acme/app"));
    assert!(artifact_files_exist(tmp.path(), "@acme/util"));
    assert!(!artifact_files_exist(tmp.path(), "@acme/other"));
}

#[test]
fn manifest_errors_render_with_source_context() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(&project.join("submilli.toml"), "[[package]\n");

    let out = build(&project, tmp.path(), &[]);

    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "stderr: {err}");
    assert!(err.contains("submilli.toml"), "stderr: {err}");
}

#[test]
fn dependency_cycle_fails_the_build() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@acme/a"
version = "0.1.0"
description = "Test package."
path = "a"
dependencies = ["@acme/b"]

[[package]]
name = "@acme/b"
version = "0.1.0"
description = "Test package."
path = "b"
dependencies = ["@acme/a"]
"#,
    );
    write_file(&project.join("a/src/lib.subm"), "export const x = 1;");
    write_file(&project.join("b/src/lib.subm"), "export const y = 1;");

    let out = build(&project, tmp.path(), &[]);

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("circular package dependency"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn compile_errors_render_with_caret() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.subm"),
        "export function bad(): number { return \"x\"; }",
    );

    let out = build(&project, tmp.path(), &[]);

    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "stderr: {err}");
    assert!(err.contains("--> src/lib.subm:1:"), "stderr: {err}");
    assert!(err.contains('^'), "stderr: {err}");
}

#[test]
fn ambiguous_ts_and_subm_module_fails_check() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export { x } from \"./wrap\";\n",
    );
    write_file(&project.join("src/wrap.ts"), "export const x = 1;\n");
    write_file(&project.join("src/wrap.subm"), "export const x = 2;\n");

    let out = build_subcommand("check", &project, tmp.path(), &[]);

    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("delete or rename one source file"),
        "stderr: {err}"
    );
    assert!(err.contains("wrap.ts"), "stderr: {err}");
    assert!(err.contains("wrap.subm"), "stderr: {err}");
}

#[test]
fn build_help_lists_subcommands() {
    let out = Command::new(submilli_bin())
        .args(["build", "--help"])
        .output()
        .expect("invoke submilli");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    for subcommand in ["init", "new", "check", "publish-local"] {
        assert!(
            text.contains(subcommand),
            "help missing {subcommand}: {text}"
        );
    }

    let out = Command::new(submilli_bin())
        .args(["build", "publish-local", "--help"])
        .output()
        .expect("invoke submilli");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("--package"), "help missing --package: {text}");
}

#[test]
fn bare_build_requires_a_subcommand() {
    let out = Command::new(submilli_bin())
        .args(["build"])
        .output()
        .expect("invoke submilli");
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("publish-local"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn check_walks_up_to_find_the_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\npath = \"util\"\n",
    );
    write_file(
        &project.join("util/src/lib.subm"),
        "export function answer(): number { return 42; }",
    );

    let out = build_subcommand("check", &project.join("util/src"), tmp.path(), &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("checked @acme/util v0.1.0"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn check_without_manifest_points_at_init() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let empty = tmp.path().join("empty");
    fs::create_dir_all(&empty).expect("create dir");

    let out = build_subcommand("check", &empty, tmp.path(), &[]);

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("submilli build init"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn check_compiles_without_installing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.subm"),
        "export function answer(): number { return 42; }",
    );

    let out = build_subcommand("check", &project, tmp.path(), &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("checked @acme/util v0.1.0"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(!artifact_files_exist(tmp.path(), "@acme/util"));
}

#[test]
fn check_refreshes_generated_editor_declarations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    write_file(&project.join(".submilli/types/stdlib.d.ts"), "stale\n");
    write_file(
        &project.join(".submilli/types/lib.submilli.d.ts"),
        "stale\n",
    );

    let out = build_subcommand("check", &project, tmp.path(), &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let stdlib =
        fs::read_to_string(project.join(".submilli/types/stdlib.d.ts")).expect("read stdlib");
    let lib =
        fs::read_to_string(project.join(".submilli/types/lib.submilli.d.ts")).expect("read lib");
    assert!(stdlib.contains("declare module \"submilli:uuid\" {"));
    assert!(stdlib.contains("declare module \"submilli:http\" {"));
    assert!(lib.contains("namespace JSON {"));
    assert!(lib.contains("interface Array<"));
}

/// Publish `@acme/leaf`, which exports `Page`, and `@acme/mid`, which returns
/// leaf's `Page`, into `home`'s store.
fn publish_leaf_and_mid(root: &Path, home: &Path) {
    let leaf = root.join("store-leaf");
    write_file(&leaf.join("submilli.toml"), LEAF_MANIFEST);
    write_file(&leaf.join("leaf/src/lib.ts"), LEAF_SOURCE);
    let published = build(&leaf, home, &[]);
    assert!(published.status.success(), "stderr: {}", stderr(&published));

    let mid = root.join("store-mid");
    write_file(
        &mid.join("submilli.toml"),
        "[dependencies]\n\"@acme/leaf\" = \"0.1.0\"\n\n\
         [[package]]\nname = \"@acme/mid\"\nversion = \"0.1.0\"\ndescription = \"Mid.\"\n\
         dependencies = [\"@acme/leaf\"]\n",
    );
    write_file(
        &mid.join("src/lib.ts"),
        "import { first, Page } from \"@acme/leaf\";\n\
         /** The first page, from the leaf. */\nexport function page(): Page { return first(); }\n",
    );
    let published = build(&mid, home, &[]);
    assert!(published.status.success(), "stderr: {}", stderr(&published));
}

const LEAF_MANIFEST: &str = "[[package]]\nname = \"@acme/leaf\"\nversion = \"0.1.0\"\n\
    description = \"Leaf.\"\npath = \"leaf\"\n";

const LEAF_SOURCE: &str = "/** A page of items. */\nexport interface Page {\n    /** Items. */\n    \
    items: number[];\n}\n/** The first page. */\nexport function first(): Page { return { items: [] }; }\n";

#[test]
fn check_declares_store_dependencies_for_the_editor() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    publish_leaf_and_mid(tmp.path(), &home);
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[dependencies]\n\"@acme/mid\" = \"0.1.0\"\n\"@acme/missing\" = \"0.1.0\"\n\n\
         [[package]]\nname = \"@acme/app\"\nversion = \"0.1.0\"\ndescription = \"App.\"\n\
         dependencies = [\"@acme/mid\", \"@acme/missing\"]\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "/** Nothing. */\nexport function nothing(): number { return 0; }\n",
    );

    // `@acme/missing` fails the build, but not the other dependencies' types.
    let out = build_subcommand("check", &project, &home, &[]);

    assert!(!out.status.success());
    let packages =
        fs::read_to_string(project.join(".submilli/types/packages.d.ts")).expect("read packages");
    assert!(
        packages.contains("declare module \"@acme/mid\" {"),
        "{packages}"
    );
    assert!(
        packages.contains("declare module \"@acme/leaf\" {"),
        "{packages}"
    );
    assert!(
        packages.contains("  import type { Page } from \"@acme/leaf\";"),
        "{packages}"
    );
    assert!(!packages.contains("@acme/missing"), "{packages}");
}

#[test]
fn a_project_package_in_a_dependencys_closure_is_left_to_its_source() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    publish_leaf_and_mid(tmp.path(), &home);
    // The project has its own `@acme/leaf`, which `@acme/mid` also depends on.
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        &format!(
            "[dependencies]\n\"@acme/mid\" = \"0.1.0\"\n\n{LEAF_MANIFEST}\n\
             [[package]]\nname = \"@acme/app\"\nversion = \"0.1.0\"\ndescription = \"App.\"\n\
             path = \"app\"\ndependencies = [\"@acme/leaf\", \"@acme/mid\"]\n"
        ),
    );
    write_file(&project.join("leaf/src/lib.ts"), LEAF_SOURCE);
    write_file(
        &project.join("app/src/lib.ts"),
        "/** Nothing. */\nexport function nothing(): number { return 0; }\n",
    );

    let out = build_subcommand("check", &project, &home, &[]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let packages =
        fs::read_to_string(project.join(".submilli/types/packages.d.ts")).expect("read packages");
    assert!(
        packages.contains("declare module \"@acme/mid\" {"),
        "{packages}"
    );
    assert!(
        !packages.contains("declare module \"@acme/leaf\""),
        "{packages}"
    );
    assert!(
        packages.contains("  import type { Page } from \"@acme/leaf\";"),
        "{packages}"
    );
}

#[test]
fn check_reports_compile_errors_with_caret() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.subm"),
        "export function bad(): number { return \"x\"; }",
    );

    let out = build_subcommand("check", &project, tmp.path(), &[]);

    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "stderr: {err}");
    assert!(err.contains('^'), "stderr: {err}");
}

#[test]
fn init_then_new_then_publish_installs_both_packages() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).expect("create project dir");

    let out = Command::new(submilli_bin())
        .args(["build", "init", "@acme/app", "app"])
        .current_dir(&project)
        .output()
        .expect("invoke submilli");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(project.join("submilli.toml").is_file());
    assert!(project.join("app/src/lib.ts").is_file());
    assert!(project.join(".submilli/tsconfig.submilli.json").is_file());
    assert!(project.join(".submilli/types/lib.submilli.d.ts").is_file());
    assert!(project.join(".submilli/types/stdlib.d.ts").is_file());
    assert!(project.join("tsconfig.json").is_file());
    assert!(project.join(".vscode/tasks.json").is_file());

    let out = Command::new(submilli_bin())
        .args(["build", "new", "@acme/util", "util"])
        .current_dir(&project)
        .output()
        .expect("invoke submilli");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(project.join("util/src/lib.ts").is_file());

    let out = build(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(artifact_files_exist(tmp.path(), "@acme/app"));
    assert!(artifact_files_exist(tmp.path(), "@acme/util"));
}

#[test]
fn init_prompts_for_name_on_stdin() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).expect("create project dir");

    let mut child = Command::new(submilli_bin())
        .args(["build", "init"])
        .current_dir(&project)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn submilli");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(b"@acme/app\n")
        .expect("write name to stdin");
    let out = child.wait_with_output().expect("wait for submilli");

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("package name (@scope/name):"),
        "stderr: {}",
        stderr(&out)
    );
    let manifest = fs::read_to_string(project.join("submilli.toml")).expect("read manifest");
    assert!(manifest.contains("name = \"@acme/app\""), "got: {manifest}");
}

#[test]
fn init_rejects_invalid_piped_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).expect("create project dir");

    let mut child = Command::new(submilli_bin())
        .args(["build", "init"])
        .current_dir(&project)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn submilli");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(b"unscoped\n")
        .expect("write name to stdin");
    let out = child.wait_with_output().expect("wait for submilli");

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("invalid package name"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(!project.join("submilli.toml").exists());
}

#[test]
fn new_without_manifest_names_the_fix() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).expect("create project dir");

    let out = Command::new(submilli_bin())
        .args(["build", "new", "@acme/util", "util"])
        .current_dir(&project)
        .output()
        .expect("invoke submilli");

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("submilli build init"),
        "stderr: {}",
        stderr(&out)
    );
}

fn build_test(project: &Path, home: &Path, extra: &[&str]) -> Output {
    build_subcommand("test", project, home, extra)
}

#[test]
fn build_commands_report_unresolved_http_hosts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/http\"\nversion = \"0.1.0\"\ndescription = \"HTTP client.\"\n",
    );
    for prefix in ["https://api.example.com", "https://api.example.com/"] {
        write_file(
            &project.join("src/lib.ts"),
            &format!(
                "import {{ get }} from \"submilli:http\";\n\
                 /** Fetch a relative API path. */\n\
                 export function fetch(path: string): string {{\n\
                     return get(\"{prefix}\" + path).body;\n\
                 }}\n"
            ),
        );
        let host_is_fixed = prefix.ends_with('/');
        for command in ["check", "publish-local", "test"] {
            let out = build_subcommand(command, &project, tmp.path(), &[]);
            assert!(out.status.success(), "{command}: {}", stderr(&out));
            let diagnostics = stderr(&out);
            let warning = "cannot statically resolve the host in the URL passed to `http.get`";
            assert_eq!(
                diagnostics.matches(warning).count(),
                usize::from(!host_is_fixed),
                "{command} with {prefix}: {diagnostics}"
            );
            if !host_is_fixed {
                assert!(diagnostics.contains("src/lib.ts:4:"), "{diagnostics}");
                assert!(diagnostics.contains("no host capability filter was derived"));
                assert!(diagnostics.contains("call the HTTP function directly"));
                assert!(diagnostics.contains("include `/` after the host in the constant prefix"));
                assert!(diagnostics.contains("`\"https://api.example.com/\" + path`"));
            }
            let capabilities = fs::read_to_string(project.join("capabilities.yaml"))
                .expect("generated capabilities");
            assert_eq!(
                capabilities.contains("host == \"api.example.com\""),
                host_is_fixed,
                "{command}: {capabilities}"
            );
        }
    }
}

#[test]
fn build_test_runs_segments_and_reports_summary() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    // Uses the canonical `.test.ts` extension — the runner discovers it too.
    write_file(
        &project.join("tests/basic.test.ts"),
        r#"
            import { label, expectException } from "submilli:test";
            import { answer } from "@acme/util";
            function main(): void {
                label("answer is 42");
                assert(answer() === 42, "answer");
                label("throwing closure is caught");
                const e = expectException(() => { assert(false, "boom"); });
                assert(e.message === "boom", "message round-trips");
            }
        "#,
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let stdout = stdout(&out);
    assert!(stdout.contains("answer is 42"), "stdout: {stdout}");
    assert!(
        stdout.contains("throwing closure is caught"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("2 passed, 0 failed across 1 files"),
        "stdout: {stdout}"
    );
}

/// A test file's top-level statements run before `main`; their failure fails
/// the file with the thrown message, as a failure in `main` does.
#[test]
fn build_test_reports_a_top_level_failure_as_the_files_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    write_file(
        &project.join("tests/setup.test.ts"),
        r#"
            import { answer } from "@acme/util";
            if (answer() === 42) { throw new RangeError("set-up refused"); }
            function main(): void { }
        "#,
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stdout(&out).contains("0 passed, 1 failed across 1 files"),
        "stdout: {}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains("error: RangeError: set-up refused"),
        "stderr: {}",
        stderr(&out)
    );
}

/// The package's own top-level statements run when it is installed for a test
/// file; their failure fails the file with a frame at the package's statement.
#[test]
fn build_test_reports_a_package_initializer_failure_with_its_statement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "const table: number[] = [1];\nconst picked: number = table[7];\n/** The pick. */\nexport function answer(): number { return picked; }",
    );
    write_file(
        &project.join("tests/setup.test.ts"),
        "import { answer } from \"@acme/util\";\nfunction main(): void { assert(answer() === 1); }",
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stdout(&out).contains("0 passed, 1 failed across 1 files"),
        "stdout: {}",
        stdout(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains(
            "error: package `@acme/util` failed to initialize: RangeError: index out of range\n  at <top level> (@acme/util/lib:2:"
        ),
        "stderr: {err}"
    );
    assert!(
        err.contains("2 | const picked: number = table[7];\n  |"),
        "the package's statement: {err}"
    );
}

/// Every package's entry module is `lib`; each frame still shows its own
/// package's source.
#[test]
fn build_test_package_frames_show_their_own_packages_source() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/base\"\nversion = \"0.1.0\"\ndescription = \"Base.\"\npath = \"packages/base\"\n\n\
         [[package]]\nname = \"@acme/app\"\nversion = \"0.1.0\"\ndescription = \"App.\"\ndependencies = [\"@acme/base\"]\npath = \"packages/app\"\n",
    );
    write_file(
        &project.join("packages/base/src/lib.ts"),
        "// base line 1\n/** Refuses. */\nexport function refuse(): number { throw new Error(\"from base\"); }",
    );
    write_file(
        &project.join("packages/app/src/lib.ts"),
        "import { refuse } from \"@acme/base\";\nconst x: number = refuse();\n/** The value. */\nexport function value(): number { return x; }",
    );
    write_file(
        &project.join("packages/app/tests/value.test.ts"),
        "import { value } from \"@acme/app\";\nfunction main(): void { assert(value() === 1); }",
    );

    let out = build_test(&project, tmp.path(), &["-p", "@acme/app"]);
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    let err = stderr(&out);
    assert!(
        err.contains("  at refuse (@acme/base/lib:3:")
            && err.contains(
                "3 | export function refuse(): number { throw new Error(\"from base\"); }"
            ),
        "the dependency's frame and source: {err}"
    );
    assert!(
        err.contains("  at <top level> (@acme/app/lib:2:")
            && err.contains("2 | const x: number = refuse();"),
        "the tested package's frame and source: {err}"
    );
}

#[test]
fn build_test_http_skip_preserves_local_tests_and_docs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = http_skip_project(tmp.path());
    for name in ["network.test.ts", "nested/network_read.test.subm"] {
        write_file(&project.join("tests").join(name), "not valid TypeScript");
    }
    write_file(
        &project.join("tests/networking.test.ts"),
        "function main(): void { assert(true); }",
    );
    write_file(
        &project.join("docs/readme.md"),
        "# Util\n```ts\nfunction main(): number { return 42; }\n```\n",
    );
    let out = run_http_skip_test(&project, tmp.path(), Some("1"));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("2 passed, 0 failed across 2 files"));
    assert!(stdout(&out).contains("2 HTTP test files skipped"));
    assert!(stdout(&out).contains("networking.test.ts"));
    assert!(stdout(&out).contains("docs/readme.md :: example 1 (compile)"));

    write_file(
        &project.join("docs/readme.md"),
        "# Util\n```ts\nfunction main(): number { return missing(); }\n```\n",
    );
    let out = run_http_skip_test(&project, tmp.path(), Some("1"));
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(stdout(&out).contains("1 passed, 1 failed across 2 files"));
}

#[test]
fn build_test_http_skip_requires_an_explicit_one() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = http_skip_project(tmp.path());
    write_file(
        &project.join("tests/network_probe.test.ts"),
        "function main(): void { assert(false, \"network test ran\"); }",
    );
    for setting in [None, Some("0"), Some("true")] {
        let out = run_http_skip_test(&project, tmp.path(), setting);
        assert!(!out.status.success(), "{setting:?}: {}", stdout(&out));
        assert!(
            stderr(&out).contains("network test ran"),
            "{}",
            stderr(&out)
        );
        assert!(!stdout(&out).contains("HTTP test files skipped"));
    }
    let out = run_http_skip_test(&project, tmp.path(), Some("1"));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("0 passed, 0 failed across 0 files"));
    assert!(stdout(&out).contains("1 HTTP test files skipped"));
    assert!(!stderr(&out).contains("no test files found"));
}

fn http_skip_project(home: &Path) -> PathBuf {
    let project = home.join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    project
}

fn run_http_skip_test(project: &Path, home: &Path, setting: Option<&str>) -> Output {
    let mut command = Command::new(submilli_bin());
    command
        .args(["build", "test"])
        .current_dir(project)
        .env("SUBMILLI_HOME", home)
        .env_remove("SUBMILLI_SKIP_HTTP_TESTS");
    if let Some(setting) = setting {
        command.env("SUBMILLI_SKIP_HTTP_TESTS", setting);
    }
    command.output().expect("invoke build test")
}

#[test]
fn build_test_compiles_doc_examples() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    write_file(
        &project.join("docs/readme.md"),
        "# Util\n\n```ts\nimport { answer } from \"@acme/util\";\nfunction main(): number {\n    return answer();\n}\n```\n",
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let stdout = stdout(&out);
    assert!(
        stdout.contains("docs/readme.md :: example 1 (compile)"),
        "stdout: {stdout}"
    );
}

#[test]
fn build_test_fails_a_broken_doc_example_at_its_readme_line() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    // The bad line is line 5 of the readme; the diagnostic must point there.
    write_file(
        &project.join("docs/readme.md"),
        "# Util\n\n```ts\nfunction main(): number {\n    return missing();\n}\n```\n",
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    let stdout_text = stdout(&out);
    assert!(
        stdout_text.contains("FAIL") && stdout_text.contains("example 1 (compile)"),
        "stdout: {stdout_text}"
    );
    assert!(
        stderr(&out).contains("docs/readme.md:5"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn build_test_ignores_non_ts_doc_fences() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(
        &project.join("src/lib.ts"),
        "export function answer(): number { return 42; }",
    );
    write_file(
        &project.join("docs/readme.md"),
        "# Util\n\n```text\nthis is not code\n```\n\n```yaml\na: b\n```\n",
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        !stdout(&out).contains("(compile)"),
        "stdout: {}",
        stdout(&out)
    );
}

#[test]
fn build_test_failing_assert_fails_only_its_segment() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(&project.join("src/lib.ts"), "export const u: number = 1;");
    write_file(
        &project.join("tests/seg.test.subm"),
        r#"
            import { label } from "submilli:test";
            function main(): void {
                label("first passes");
                assert(1 + 1 === 2, "math");
                label("second fails");
                assert(1 + 1 === 3, "bad math");
            }
        "#,
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(!out.status.success(), "should exit non-zero");
    let stdout = stdout(&out);
    assert!(
        stdout.contains("ok") && stdout.contains("first passes"),
        "first segment should pass: {stdout}"
    );
    assert!(
        stdout.contains("FAIL") && stdout.contains("second fails"),
        "second segment should fail: {stdout}"
    );
    assert!(
        stdout.contains("1 passed, 1 failed across 1 files"),
        "stdout: {stdout}"
    );
    // The failure message + backtrace go to stderr.
    assert!(
        stderr(&out).contains("bad math"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn build_test_uncaught_throw_fails_file_and_others_still_run() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(&project.join("src/lib.ts"), "export const u: number = 1;");
    // Alphabetical discovery: a_throws runs before b_passes.
    write_file(
        &project.join("tests/a_throws.test.subm"),
        r#"
            import { label } from "submilli:test";
            function main(): void {
                label("explodes");
                throw new Error("kaboom");
            }
        "#,
    );
    write_file(
        &project.join("tests/b_passes.test.subm"),
        r#"
            function main(): void {
                assert(true, "fine");
            }
        "#,
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(!out.status.success(), "should exit non-zero");
    let stdout = stdout(&out);
    assert!(
        stdout.contains("FAIL") && stdout.contains("explodes"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("1 passed, 1 failed across 2 files"),
        "the passing file must still run: {stdout}"
    );
    assert!(stderr(&out).contains("kaboom"), "stderr: {}", stderr(&out));
}

#[test]
fn build_test_anonymous_file_with_bare_asserts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        "[[package]]\nname = \"@acme/util\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n",
    );
    write_file(&project.join("src/lib.ts"), "export const u: number = 1;");
    write_file(
        &project.join("tests/anon.test.subm"),
        "function main(): void { assert(2 + 2 === 4, \"arithmetic\"); }",
    );

    let out = build_test(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("1 passed, 0 failed across 1 files"),
        "stdout: {}",
        stdout(&out)
    );
}

#[test]
fn build_test_package_flag_scopes_to_one_package() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "Test package."
path = "app"

[[package]]
name = "@acme/other"
version = "0.1.0"
description = "Test package."
path = "other"
"#,
    );
    write_file(
        &project.join("app/src/lib.ts"),
        "export const a: number = 1;",
    );
    write_file(
        &project.join("other/src/lib.ts"),
        "export const o: number = 1;",
    );
    write_file(
        &project.join("app/tests/app.test.subm"),
        "function main(): void { assert(true, \"app\"); }",
    );
    write_file(
        &project.join("other/tests/other.test.subm"),
        "function main(): void { assert(true, \"other\"); }",
    );

    let out = build_test(&project, tmp.path(), &["-p", "@acme/app"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let stdout = stdout(&out);
    assert!(stdout.contains("app.test.subm"), "stdout: {stdout}");
    assert!(
        !stdout.contains("other.test.subm"),
        "other should be skipped: {stdout}"
    );
    assert!(
        stdout.contains("1 passed, 0 failed across 1 files"),
        "stdout: {stdout}"
    );
}

#[test]
fn submilli_run_rejects_test_module_import() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let script = tmp.path().join("uses_test.subm");
    write_file(
        &script,
        r#"
            import { label } from "submilli:test";
            function main(): void { label("x"); }
        "#,
    );

    let out = run_with_home(&[OsStr::new("run"), script.as_os_str()], tmp.path());
    assert!(!out.status.success(), "should fail to compile");
    let stderr = stderr(&out);
    assert!(
        stderr.contains("submilli:test") && stderr.contains("not found"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("submilli build test"),
        "help should name the runner: {stderr}"
    );
}

#[test]
fn cross_package_const_import_links_and_runs() {
    // Regression: a package's top-level globals are exported mutable (so its
    // `_start` can initialize them), so a consumer importing another package's
    // `const` must import it as a mutable global too — otherwise instantiation
    // fails with "incompatible import type".
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    write_file(
        &project.join("submilli.toml"),
        r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "Test package."
path = "app"
dependencies = ["@acme/util"]

[[package]]
name = "@acme/util"
version = "0.1.0"
description = "Test package."
path = "util"
"#,
    );
    write_file(
        &project.join("util/src/lib.ts"),
        "export const SEVEN: number = 7;",
    );
    write_file(
        &project.join("app/src/lib.ts"),
        r#"
            import { SEVEN } from "@acme/util";
            export function plusOne(): number { return SEVEN + 1; }
        "#,
    );

    let out = build(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let script = tmp.path().join("consumer.subm");
    write_file(
        &script,
        r#"
            import { plusOne } from "@acme/app";
            function main(): number { return plusOne(); }
        "#,
    );
    let blueprint = tmp.path().join("blueprint.yaml");
    write_file(&blueprint, "name: t\npackages:\n  - \"@acme/app\"\n");

    let out = run_with_home(
        &[
            OsStr::new("run"),
            script.as_os_str(),
            OsStr::new("--blueprint"),
            blueprint.as_os_str(),
        ],
        tmp.path(),
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "8\n");
}

#[test]
fn init_scaffolds_a_sample_test_that_passes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).expect("create project dir");

    let out = build_subcommand("init", &project, tmp.path(), &["@demo/app", "."]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        project.join("tests/lib.test.ts").is_file(),
        "init should scaffold a sample .ts test file"
    );
    assert!(
        stderr(&out).contains("tests/lib.test.ts"),
        "init should report the created test file: {}",
        stderr(&out)
    );

    // The scaffolded sample test runs green out of the box.
    let out = build_test(&project, tmp.path(), &[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("1 passed, 0 failed across 1 files"),
        "stdout: {}",
        stdout(&out)
    );
}
