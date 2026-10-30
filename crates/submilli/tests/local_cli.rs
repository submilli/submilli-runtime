//! End-to-end integration tests for the local-dev commands: `submilli check`,
//! `submilli blueprint lint`, and `submilli run --blueprint`. None need a server.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn write_temp(name: &str, contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    fs::write(&path, contents).expect("write temp file");
    path
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .output()
        .expect("invoke submilli")
}

fn run_with_home(args: &[&std::ffi::OsStr], home: &std::path::Path) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli")
}

fn run_in_with_home(
    args: &[&std::ffi::OsStr],
    cwd: &std::path::Path,
    home: &std::path::Path,
) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .current_dir(cwd)
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli")
}

fn os(s: &str) -> &std::ffi::OsStr {
    std::ffi::OsStr::new(s)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write_file(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create dirs");
    fs::write(path, contents).expect("write file");
    if let Some(package_dir) = package_dir_for_source(path) {
        let docs = package_dir.join("docs/readme.md");
        if !docs.exists() {
            fs::create_dir_all(docs.parent().expect("docs parent")).expect("create docs dir");
            fs::write(&docs, "# Test package\n").expect("write docs");
        }
    }
}

fn package_dir_for_source(path: &std::path::Path) -> Option<&std::path::Path> {
    let parent = path.parent()?;
    if parent.file_name()? == "src" {
        parent.parent()
    } else {
        None
    }
}

// ---- submilli check ------------------------------------------------------

#[test]
fn check_clean_file_exits_zero() {
    let script = write_temp(
        "submilli-check-clean.subm",
        "function main(): number { return 1; }",
    );
    let out = run(&[os("check"), script.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_type_error_exits_nonzero_with_diagnostic() {
    let script = write_temp(
        "submilli-check-error.subm",
        r#"function main(): number { return "x"; }"#,
    );
    let out = run(&[os("check"), script.as_os_str()]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "missing `error:` in {err}");
    assert!(err.contains('^'), "missing caret in {err}");
}

// ---- submilli blueprint lint ---------------------------------------------

#[test]
fn lint_valid_blueprint_exits_zero() {
    let file = write_temp("submilli-lint-ok.yaml", "name: production\n");
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("valid"));
}

#[test]
fn lint_malformed_yaml_fails() {
    let file = write_temp("submilli-lint-malformed.yaml", "name: [unterminated\n");
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("error:"));
}

#[test]
fn lint_default_allow_accepted() {
    let file = write_temp(
        "submilli-lint-default-allow.yaml",
        "name: x\ndefault: allow\n",
    );
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("valid"));
}

#[test]
fn lint_warns_on_default_allow() {
    let file = write_temp(
        "submilli-lint-default-allow-warning.yaml",
        "name: x\ndefault: allow\n",
    );
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(err.contains("default: allow"), "missing the form in {err}");
    assert!(err.contains("default: deny"), "missing the fix in {err}");
}

#[test]
fn lint_does_not_warn_on_safe_defaults() {
    for (label, yaml) in [
        ("deny", "name: x\ndefault: deny\n"),
        ("omitted", "name: x\n"),
    ] {
        let file = write_temp(&format!("submilli-lint-default-{label}.yaml"), yaml);
        let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let err = stderr(&out);
        assert!(
            !err.contains("default: allow"),
            "unexpected default-allow warning for `{label}` in {err}"
        );
    }
}

#[test]
fn lint_warns_on_the_deprecated_default_and_asks_that_cannot_wait() {
    let file = write_temp(
        "submilli-lint-asks.yaml",
        "name: x\ndefault: ask-human\npermissions:\n  main:\n    - capability: git.commit\n      action: ask-admin\n    - capability: fs.write\n      action: ask-human\n    - capability: http.get\n      action: ask-human\n",
    );
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("`default: ask-human` is deprecated"), "{err}");
    assert!(
        err.contains("`git.commit`, `ask-admin`) can never ask"),
        "{err}"
    );
    assert!(
        err.contains("`fs.write`, `ask-human`): the `submilli:code` library"),
        "{err}"
    );
    assert!(!err.contains("`http.get`, `ask-human`"), "{err}");
}

#[test]
fn lint_undeclared_secret_reference_fails() {
    let file = write_temp(
        "submilli-lint-undeclared-secret.yaml",
        "name: x\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.MISSING}\"\n",
    );
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("error:"));
}

#[test]
fn lint_warns_when_package_permission_caller_is_absent_from_packages() {
    let file = write_temp(
        "submilli-lint-package-caller-warning.yaml",
        "name: x\npermissions:\n  \"@pied-piper/util\":\n    - capability: pied-piper/do\n      action: allow\n",
    );
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(
        err.contains("@pied-piper/util"),
        "missing package name in {err}"
    );
}

#[test]
fn lint_warns_when_package_has_no_permission_block() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/sdk\"\n",
    );
    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(
        err.contains("@pied-piper/sdk"),
        "missing package name in {err}"
    );
}

fn publish_capability_packages(home: &std::path::Path) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@pied-piper/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@pied-piper/sdk"]

[[package]]
name = "@pied-piper/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
"#,
    );
    write_file(
        &project.path().join("sdk/src/lib.ts"),
        r#"
            import { check } from "submilli:security";
            /** @capability pied-piper.example/charge { customer } */
            export function charge(customer: string): void {
                check("pied-piper.example/charge", { customer });
            }
            /** @capability pied-piper.example/refund { customer } */
            export function refund(customer: string): void {
                check("pied-piper.example/refund", { customer });
            }
        "#,
    );
    write_file(
        &project.path().join("app/src/lib.ts"),
        r#"
            import { charge } from "@pied-piper/sdk";
            import { check } from "submilli:security";
            /** @capability pied-piper.example/run { customer } */
            export function run(customer: string): void {
                check("pied-piper.example/run", { customer });
                charge("cus_123");
            }
        "#,
    );
    let out = run_in_with_home(&[os("build"), os("publish-local")], project.path(), home);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    project
}

fn publish_secret_package(home: &std::path::Path) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@pied-piper/secrets"
version = "0.1.0"
description = "Secrets package."
path = "secrets"
"#,
    );
    write_file(
        &project.path().join("secrets/src/lib.ts"),
        r#"
            import { get } from "submilli:secrets";

            export function loadToken(): void {
                get("STRIPE_API_KEY");
            }
        "#,
    );
    let out = run_in_with_home(&[os("build"), os("publish-local")], project.path(), home);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    project
}

/// Leaving a provided capability out of `main` is how a blueprint withholds
/// it; `default:` decides those calls, so lint says nothing about them.
#[test]
fn lint_ignores_omitted_provided_capabilities() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");
    for (package, selection) in [
        ("@pied-piper/app", "--no-capabilities"),
        (
            "@pied-piper/sdk",
            "--capabilities=pied-piper.example/charge",
        ),
    ] {
        let out = run_with_home(
            &[
                os("blueprint"),
                os("add-package"),
                os(package),
                os(selection),
                os("--blueprint"),
                file.as_os_str(),
            ],
            home.path(),
        );
        assert!(out.status.success(), "stderr: {}", stderr(&out));
    }
    let scaffolded = fs::read_to_string(&file).expect("read blueprint");

    for flags in [&[][..], &[os("--fix")][..]] {
        let mut args = vec![os("blueprint"), os("lint")];
        args.extend_from_slice(flags);
        args.push(file.as_os_str());
        let out = run_with_home(&args, home.path());

        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let err = stderr(&out);
        assert!(!err.contains("provides `"), "got: {err}");
        assert!(!err.contains("warning:"), "got: {err}");
        assert_eq!(
            fs::read_to_string(&file).expect("read blueprint"),
            scaffolded,
            "lint {flags:?} must leave the blueprint as scaffolded"
        );
    }
}

#[test]
fn lint_warns_for_missing_package_secret() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_secret_package(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/secrets\"\npermissions:\n  \"@pied-piper/secrets\":\n    - capability: secrets.get\n      filter: name == \"STRIPE_API_KEY\"\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(
        err.contains("requires secret `STRIPE_API_KEY`"),
        "got: {err}"
    );
}

#[test]
fn lint_accepts_declared_package_secret() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_secret_package(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\nsecrets:\n  STRIPE_API_KEY: { store: STRIPE_API_KEY }\npackages:\n  - \"@pied-piper/secrets\"\npermissions:\n  \"@pied-piper/secrets\":\n    - capability: secrets.get\n      filter: name == \"STRIPE_API_KEY\"\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        !stderr(&out).contains("requires secret `STRIPE_API_KEY`"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn lint_errors_for_missing_requires_rule() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/app\"\npermissions:\n  \"@pied-piper/app\": []\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "missing error in {err}");
    assert!(
        err.contains("requires `pied-piper.example/charge`"),
        "got: {err}"
    );
    assert!(err.contains("customer == \"cus_123\""), "got: {err}");
}

#[test]
fn lint_keeps_a_narrowed_requires_rule() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    let blueprint = "name: x\npackages:\n  - \"@pied-piper/app\"\npermissions:\n  \"@pied-piper/app\":\n    - capability: pied-piper.example/charge\n      filter: customer glob \"cus_1*\"\n      action: allow\n";
    write_file(&file, blueprint);

    let out = run_with_home(
        &[os("blueprint"), os("lint"), os("--fix"), file.as_os_str()],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(err.contains("grants it differently"), "got: {err}");
    let updated = fs::read_to_string(&file).expect("read blueprint");
    let parsed = submilli_blueprint::parse(&updated).expect("blueprint parses");
    assert_eq!(parsed.permissions["@pied-piper/app"].len(), 1, "{updated}");
}

#[test]
fn lint_fix_adds_missing_requires_rules_only() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\npermissions: {}\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), os("--fix"), file.as_os_str()],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(&file).expect("read fixed blueprint");
    let blueprint = submilli_blueprint::parse(&updated).expect("fixed blueprint parses");
    assert!(
        !blueprint.permissions.contains_key("main"),
        "--fix must not grant or rule on provided capabilities: {updated}"
    );
    let app_rules = &blueprint.permissions["@pied-piper/app"];
    assert!(app_rules.iter().any(|rule| {
        rule.capability == "pied-piper.example/charge"
            && rule
                .filter
                .as_ref()
                .is_some_and(|filter| filter.to_string() == "customer == \"cus_123\"")
            && rule.action == submilli_blueprint::Action::Allow
    }));
}

#[test]
fn add_package_warns_for_missing_package_secret() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_secret_package(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/secrets"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(
        err.contains("requires secret `STRIPE_API_KEY`"),
        "got: {err}"
    );
}

#[test]
fn add_package_adds_dependency_and_capability_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/app"),
            os("--capabilities"),
            os("pied-piper.example/run"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let out_text = stdout(&out);
    assert!(
        out_text.contains("added @pied-piper/app"),
        "got: {out_text}"
    );
    assert!(
        out_text.contains("allow pied-piper.example/run"),
        "got: {out_text}"
    );
    assert!(
        out_text.contains("allow pied-piper.example/charge"),
        "got: {out_text}"
    );

    let updated = fs::read_to_string(&file).expect("read updated blueprint");
    assert!(
        updated.contains("# @pied-piper/app provides:"),
        "got: {updated}"
    );
    assert!(
        updated.contains("# pied-piper.example/run"),
        "got: {updated}"
    );
    assert!(
        updated.contains("#   { customer: string }"),
        "got: {updated}"
    );
    let blueprint = submilli_blueprint::parse(&updated).expect("updated blueprint parses");
    assert!(blueprint.packages.contains("@pied-piper/app"));
    assert_eq!(
        blueprint.default_action,
        Some(submilli_blueprint::DefaultAction::Deny)
    );
    assert!(
        blueprint.permissions["main"]
            .iter()
            .any(|rule| rule.capability == "pied-piper.example/run"
                && rule.action == submilli_blueprint::Action::Allow)
    );
    assert!(blueprint.permissions["@pied-piper/app"].iter().any(|rule| {
        rule.capability == "pied-piper.example/charge"
            && rule.action == submilli_blueprint::Action::Allow
            && rule
                .filter
                .as_ref()
                .is_some_and(|filter| filter.to_string() == "customer == \"cus_123\"")
    }));

    let lint = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );
    assert!(lint.status.success(), "stderr: {}", stderr(&lint));
}

#[test]
fn add_package_without_selection_adds_no_main_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    // Non-interactive (stdin is not a TTY) and no selection flag: nothing is
    // granted to `main`; `default: deny` covers the provided capabilities.
    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/app"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("no provided capabilities selected"),
        "got: {err}"
    );
    let out_text = stdout(&out);
    assert!(
        out_text.contains("not selected; `default: deny` denies calls to them"),
        "got: {out_text}"
    );

    let updated = fs::read_to_string(&file).expect("read updated blueprint");
    let blueprint = submilli_blueprint::parse(&updated).expect("updated blueprint parses");
    assert!(
        !blueprint.permissions.contains_key("main"),
        "got: {updated}"
    );
    assert!(
        blueprint.permissions["@pied-piper/app"]
            .iter()
            .any(|rule| rule.capability == "pied-piper.example/charge")
    );
}

fn capability_list_unconfigured(
    home: &std::path::Path,
    file: &std::path::Path,
    library: Option<&str>,
) -> Output {
    let mut args = vec![os("blueprint"), os("capability"), os("list")];
    if let Some(library) = library {
        args.push(os(library));
    }
    args.extend([os("--unconfigured"), os("--blueprint"), file.as_os_str()]);
    run_with_home(&args, home)
}

#[test]
fn capability_list_unconfigured_shows_provided_capabilities_without_a_main_rule() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    let blueprint = "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\npermissions:\n  main:\n    - capability: pied-piper.example/run\n      action: allow\n";
    write_file(&file, blueprint);

    // The filtered view speaks for the named package, not the blueprint.
    let out = capability_list_unconfigured(home.path(), &file, Some("@pied-piper/app"));
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: `@pied-piper/app` provides no capability"),
        "got: {text}"
    );

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default: deny` denies"), "got: {text}");
    assert!(
        text.contains(
            "@pied-piper/sdk\n  pied-piper.example/charge\n      fields: customer: string\n"
        ),
        "got: {text}"
    );
    assert!(text.contains("pied-piper.example/refund"), "got: {text}");
    assert!(!text.contains("pied-piper.example/run"), "got: {text}");
    assert!(!text.contains("@pied-piper/app"), "got: {text}");
    assert_eq!(
        fs::read_to_string(&file).expect("read blueprint"),
        blueprint
    );
}

#[test]
fn capability_list_unconfigured_filters_to_one_declared_package() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, Some("@pied-piper/app"));

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default:` is unset"), "got: {text}");
    assert!(
        text.contains("@pied-piper/app\n  pied-piper.example/run"),
        "got: {text}"
    );
    assert!(!text.contains("pied-piper.example/charge"), "got: {text}");
    assert!(!text.contains("@pied-piper/sdk"), "got: {text}");

    let out = capability_list_unconfigured(home.path(), &file, Some("submilli:fs"));
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("not a package"), "got: {err}");
    assert!(
        err.contains("@pied-piper/app, @pied-piper/sdk"),
        "got: {err}"
    );
}

/// An explicit `deny` is a rule, not an omission; under `default: allow` an
/// omitted capability is allowed, and the view must say so.
#[test]
fn capability_list_unconfigured_distinguishes_deny_rules_and_names_the_default() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: allow\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      action: deny\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default: allow` allows"), "got: {text}");
    assert!(text.contains("pied-piper.example/run"), "got: {text}");
    assert!(!text.contains("pied-piper.example/charge"), "got: {text}");
}

#[test]
fn capability_list_unconfigured_reports_when_nothing_is_omitted() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/sdk\"\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      filter: customer == \"cus_1\"\n      action: allow\n    - capability: pied-piper.example/refund\n      action: ask-human\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, None);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: no declared package provides"),
        "got: {text}"
    );

    let bare = home.path().join("bare.yaml");
    write_file(&bare, "name: x\n");
    let out = capability_list_unconfigured(home.path(), &bare, None);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("declares no packages"),
        "got: {}",
        stdout(&out)
    );
}

/// `default: ask-human` is a deprecated spelling of `deny`; rules other callers
/// hold for an omitted name are not `main`'s and stay out of this view.
#[test]
fn capability_list_unconfigured_under_ask_human_hides_other_callers_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: ask-human\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\npermissions:\n  \"@pied-piper/app\":\n    - capability: pied-piper.example/charge\n      action: allow\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default: ask-human`"), "got: {text}");
    assert!(
        text.contains("deprecated spelling of `deny`"),
        "got: {text}"
    );
    assert!(text.contains("pied-piper.example/charge"), "got: {text}");
    assert!(!text.contains("rule["), "got: {text}");
}

#[test]
fn capability_list_unconfigured_with_a_package_that_provides_nothing() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_secret_package(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\npackages:\n  - \"@pied-piper/secrets\"\n");

    let out = capability_list_unconfigured(home.path(), &file, Some("@pied-piper/secrets"));

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: `@pied-piper/secrets` provides no capability"),
        "got: {text}"
    );
}

#[test]
fn capability_list_unconfigured_fails_on_a_package_it_cannot_load() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\npackages:\n  - \"@pied-piper/missing\"\n");

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("@pied-piper/missing"),
        "got: {}",
        stderr(&out)
    );
}

fn add_sdk_with_charge_selected(home: &std::path::Path, blueprint: &str) -> Output {
    let file = home.join("blueprint.yaml");
    write_file(&file, blueprint);
    run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/sdk"),
            os("--capabilities"),
            os("pied-piper.example/charge"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home,
    )
}

/// The summary names what the blueprint's own `default:` does with the
/// capabilities left unselected, not a fixed `default: deny`.
#[test]
fn add_package_summary_names_the_blueprints_default() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    for (blueprint, expected) in [
        (
            "name: x\npermissions:\n  main: []\n",
            "1 provided capabilities not selected; `default:` is unset, so calls to them are denied",
        ),
        (
            "name: x\ndefault: ask-human\n",
            "1 provided capabilities not selected; `default: ask-human`, a deprecated spelling of `deny`, denies calls to them",
        ),
    ] {
        let out = add_sdk_with_charge_selected(home.path(), blueprint);

        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let text = stdout(&out);
        assert!(text.contains(expected), "for {blueprint:?} got: {text}");
    }
}

/// An unselected capability `main` already rules on is neither reported as
/// falling to the default nor given a `deny` that its earlier rule shadows.
#[test]
fn add_package_keeps_an_existing_main_rule_for_an_unselected_capability() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    for default in ["deny", "allow"] {
        let out = add_sdk_with_charge_selected(
            home.path(),
            &format!(
                "name: x\ndefault: {default}\npermissions:\n  main:\n    - capability: pied-piper.example/refund\n      action: allow\n"
            ),
        );

        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let text = stdout(&out);
        assert!(
            !text.contains("not selected"),
            "default {default} got: {text}"
        );
        let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
        let blueprint = submilli_blueprint::parse(&updated).expect("parses");
        let refund: Vec<_> = blueprint.permissions["main"]
            .iter()
            .filter(|rule| rule.capability == "pied-piper.example/refund")
            .collect();
        assert_eq!(refund.len(), 1, "default {default}: {updated}");
        assert_eq!(refund[0].action, submilli_blueprint::Action::Allow);
    }
}

/// A filtered rule decides only the calls it matches; under `default: allow`
/// the rest are allowed unless add-package still appends its `deny`.
#[test]
fn add_package_denies_behind_a_filtered_main_rule_under_default_allow() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let out = add_sdk_with_charge_selected(
        home.path(),
        "name: x\ndefault: allow\npermissions:\n  main:\n    - capability: pied-piper.example/refund\n      filter: customer == \"vip\"\n      action: deny\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    let refund: Vec<_> = blueprint.permissions["main"]
        .iter()
        .filter(|rule| rule.capability == "pied-piper.example/refund")
        .collect();
    assert_eq!(refund.len(), 2, "{updated}");
    assert!(refund[1].filter.is_none(), "{updated}");
    assert_eq!(refund[1].action, submilli_blueprint::Action::Deny);
}

/// Only `main`'s own rules decide `main`'s calls: another caller's unfiltered
/// rule for the same name must not stop the protective `deny`.
#[test]
fn add_package_denies_despite_another_callers_rule_under_default_allow() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let out = add_sdk_with_charge_selected(
        home.path(),
        "name: x\ndefault: allow\npermissions:\n  \"@pied-piper/app\":\n    - capability: pied-piper.example/refund\n      action: allow\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    assert!(
        blueprint.permissions["main"].iter().any(|rule| {
            rule.capability == "pied-piper.example/refund"
                && rule.filter.is_none()
                && rule.action == submilli_blueprint::Action::Deny
        }),
        "{updated}"
    );
}

/// The summary and `capability list --unconfigured` agree on what is left to
/// the default: a capability `main` rules on only with a filter is neither.
#[test]
fn add_package_summary_agrees_with_unconfigured_on_filtered_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let out = add_sdk_with_charge_selected(
        home.path(),
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: pied-piper.example/refund\n      filter: customer == \"vip\"\n      action: allow\n",
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(!text.contains("not selected"), "got: {text}");

    let file = home.path().join("blueprint.yaml");
    let out = capability_list_unconfigured(home.path(), &file, Some("@pied-piper/sdk"));
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: `@pied-piper/sdk` provides no capability"),
        "got: {text}"
    );
}

#[test]
fn add_package_rejects_duplicate_package() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/app\"\npermissions: {}\n",
    );

    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/app"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success());
    assert!(stderr(&out).contains("already declares package"));
}

#[test]
fn add_package_rejects_versioned_package_spec_for_now() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/app@1.0.0"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success());
    assert!(stderr(&out).contains("do not support versioned packages"));
}

#[test]
fn add_package_reports_missing_package() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@pied-piper/missing"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success());
    assert!(stderr(&out).contains("package `@pied-piper/missing` was not found"));
}

/// `@pied-piper/desk` imports `@pied-piper/support`, which imports `@pied-piper/billing`;
/// billing reads a secret and provides the capability support requires.
fn publish_dependency_chain(home: &std::path::Path) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@pied-piper/desk"
version = "0.1.0"
description = "Desk package."
path = "desk"
dependencies = ["@pied-piper/support"]

[[package]]
name = "@pied-piper/support"
version = "0.1.0"
description = "Support package."
path = "support"
dependencies = ["@pied-piper/billing"]

[[package]]
name = "@pied-piper/billing"
version = "0.1.0"
description = "Billing package."
path = "billing"
"#,
    );
    write_file(
        &project.path().join("billing/src/lib.ts"),
        r#"
            import { check } from "submilli:security";
            import secrets from "submilli:secrets";
            import { get } from "submilli:http";
            /** @capability pied-piper.example/credits.apply { customer } */
            export function applyCredit(customer: string): string {
                check("pied-piper.example/credits.apply", { customer });
                return secrets.get("BILLING_API_KEY") ?? "no key";
            }
            /** Never called by the tests: it adds an `http.get` requirement. */
            export function ping(): number {
                return get("https://billing.example.com/ping").status;
            }
        "#,
    );
    write_file(
        &project.path().join("desk/src/lib.ts"),
        r#"
            import { apologize } from "@pied-piper/support";
            export function escalate(customer: string): string {
                return apologize(customer);
            }
        "#,
    );
    write_file(
        &project.path().join("support/src/lib.ts"),
        r#"
            import { applyCredit } from "@pied-piper/billing";
            export function apologize(customer: string): string {
                return applyCredit(customer);
            }
        "#,
    );
    let out = run_in_with_home(&[os("build"), os("publish-local")], project.path(), home);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    project
}

fn add_package(home: &std::path::Path, file: &std::path::Path, args: &[&str]) -> Output {
    let mut command = vec![os("blueprint"), os("add-package")];
    command.extend(args.iter().map(|arg| os(arg)));
    command.extend([os("--blueprint"), file.as_os_str()]);
    run_with_home(&command, home)
}

fn run_script(home: &std::path::Path, blueprint: &std::path::Path, source: &str) -> Output {
    let script = home.join("script.ts");
    write_file(&script, source);
    Command::new(submilli_bin())
        .args([
            os("run"),
            os("--blueprint"),
            blueprint.as_os_str(),
            script.as_os_str(),
        ])
        .env("SUBMILLI_HOME", home)
        .output()
        .expect("invoke submilli")
}

fn declare_billing_secret(home: &std::path::Path, file: &std::path::Path) {
    let out = run_with_home(
        &[
            os("blueprint"),
            os("secret"),
            os("add"),
            os("BILLING_API_KEY"),
            os("--store"),
            os("BILLING_API_KEY"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home,
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
}

/// SUB-1235: one `add-package` yields a blueprint the whole chain runs under.
#[test]
fn add_package_adds_caller_rules_for_the_dependency_chain() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(
            "✓ added caller rules for @pied-piper/billing, a dependency of @pied-piper/support"
        ),
        "got: {text}"
    );
    assert!(
        stderr(&out).contains("package `@pied-piper/billing` requires secret `BILLING_API_KEY`"),
        "got: {}",
        stderr(&out)
    );
    let updated = fs::read_to_string(&file).expect("read blueprint");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    assert!(
        !blueprint.packages.contains("@pied-piper/billing"),
        "only the named package is importable: {updated}"
    );
    assert!(
        blueprint.permissions["@pied-piper/billing"]
            .iter()
            .any(|rule| rule.capability == "secrets.get"),
        "{updated}"
    );
    assert!(!blueprint.permissions.contains_key("main"), "{updated}");

    declare_billing_secret(home.path(), &file);
    let put = secret_put(home.path(), "BILLING_API_KEY", "sk_test");
    assert!(put.status.success(), "{}", stderr(&put));
    let lint = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );
    assert!(lint.status.success(), "stderr: {}", stderr(&lint));
    assert!(
        !stderr(&lint).contains("warning:"),
        "got: {}",
        stderr(&lint)
    );

    let run = run_script(
        home.path(),
        &file,
        r#"import { apologize } from "@pied-piper/support"; function main(): string { return apologize("cus_1"); }"#,
    );
    assert!(run.status.success(), "stderr: {}", stderr(&run));
    assert!(stdout(&run).contains("sk_test"), "got: {}", stdout(&run));
}

/// A program reaches a dependency only through the package that uses it, so
/// the dependency's grants never serve the program's own calls.
#[test]
fn add_package_keeps_dependencies_out_of_main_imports() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\ndefault: allow\n");

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--all-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(&file).expect("read blueprint");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    assert!(!blueprint.permissions.contains_key("main"), "{updated}");
    declare_billing_secret(home.path(), &file);
    let run = run_script(
        home.path(),
        &file,
        r#"import { applyCredit } from "@pied-piper/billing"; function main(): string { return applyCredit("cus_1"); }"#,
    );
    assert!(!run.status.success(), "stdout: {}", stdout(&run));
    assert!(
        stderr(&run).contains("package `@pied-piper/billing` is not a dependency of `main`"),
        "got: {}",
        stderr(&run)
    );
}

/// A dependency the blueprint already declares keeps the operator's rules.
#[test]
fn add_package_leaves_an_already_declared_dependency_alone() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/billing\"\npermissions:\n  \"@pied-piper/billing\":\n    - capability: secrets.get\n      action: deny\n",
    );

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        !stdout(&out).contains("@pied-piper/billing,"),
        "got: {}",
        stdout(&out)
    );
    let blueprint =
        submilli_blueprint::parse(&fs::read_to_string(&file).expect("read")).expect("parses");
    let billing = &blueprint.permissions["@pied-piper/billing"];
    assert_eq!(billing.len(), 1);
    assert_eq!(billing[0].action, submilli_blueprint::Action::Deny);
}

/// A dependency's caller list from an earlier `add-package` (or written by
/// hand) is the operator's: kept, with what it lacks named.
#[test]
fn add_package_keeps_a_dependencys_existing_caller_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npermissions:\n  \"@pied-piper/billing\":\n    - capability: http.get\n      action: deny\n",
    );

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains(
            "✓ kept the existing caller rules for @pied-piper/billing, a dependency of @pied-piper/support\n  they don't cover 1 capabilities it requires; `submilli blueprint lint` reports them:\n    secrets.get"
        ),
        "got: {}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains("package `@pied-piper/billing` requires secret `BILLING_API_KEY`"),
        "got: {}",
        stderr(&out)
    );
    let blueprint =
        submilli_blueprint::parse(&fs::read_to_string(&file).expect("read")).expect("parses");
    assert_eq!(blueprint.permissions["@pied-piper/billing"].len(), 1);
}

/// A rule with another filter leaves the dependency's own calls outside it
/// denied, as lint warns, so it does not cover the requirement.
#[test]
fn add_package_reports_a_dependencys_differently_filtered_rule() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npermissions:\n  \"@pied-piper/billing\":\n    - capability: http.get\n      action: allow\n    - capability: secrets.get\n      filter: name == \"OTHER\"\n      action: allow\n",
    );

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("they don't cover 1 capabilities it requires; `submilli blueprint lint` reports them:\n    secrets.get"),
        "got: {}",
        stdout(&out)
    );
}

/// A complete caller list, such as the one an earlier `add-package` of a
/// dependent wrote, is not reported again.
#[test]
fn add_package_is_silent_about_a_dependencys_complete_caller_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");
    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/desk", "--no-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        !stdout(&out).contains("@pied-piper/billing"),
        "got: {}",
        stdout(&out)
    );
    assert!(
        !stderr(&out).contains("@pied-piper/billing"),
        "got: {}",
        stderr(&out)
    );
}

/// A caller list left behind by an earlier removal keeps its rules; the
/// package's broad rule appended behind them would match what they leave out.
#[test]
fn add_package_keeps_a_callers_existing_rule_for_a_required_capability() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npermissions:\n  \"@pied-piper/billing\":\n    - capability: secrets.get\n      filter: name == \"OTHER\"\n      action: allow\n",
    );

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/billing", "--no-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(
            "added 1 rules to caller `@pied-piper/billing` (default allow):\n    allow http.get"
        ),
        "got: {text}"
    );
    assert!(
        text.contains("`@pied-piper/billing` already has rules for 1 required capabilities; kept them:\n    secrets.get"),
        "got: {text}"
    );
    let blueprint =
        submilli_blueprint::parse(&fs::read_to_string(&file).expect("read")).expect("parses");
    assert_eq!(blueprint.permissions["@pied-piper/billing"].len(), 2);
}

/// The whole closure must load before anything is written.
#[test]
fn add_package_writes_nothing_when_a_dependency_is_missing() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    fs::remove_dir_all(home.path().join("packages/@pied-piper/billing")).expect("remove billing");
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("`@pied-piper/billing` (required by `@pied-piper/support`)"),
        "got: {}",
        stderr(&out)
    );
    assert_eq!(fs::read_to_string(&file).expect("read"), "name: x\n");
}

/// The hint names the command that grants an already-declared package.
#[test]
fn add_package_rejection_of_a_declared_package_points_to_capability_add() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\npackages:\n  - \"@pied-piper/billing\"\n");

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/billing", "--all-capabilities"],
    );

    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("submilli blueprint capability add <name>"),
        "got: {}",
        stderr(&out)
    );
}

/// A selected capability `main` rules on only with a filter still gets its
/// `allow`, for the calls the filter leaves.
#[test]
fn add_package_allows_behind_a_filtered_main_rule() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let out = add_sdk_with_charge_selected(
        home.path(),
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      filter: customer == \"vip\"\n      action: deny\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    let main = &blueprint.permissions["main"];
    assert_eq!(main.len(), 2, "{updated}");
    assert!(main[1].filter.is_none(), "{updated}");
    assert_eq!(main[1].action, submilli_blueprint::Action::Allow);
}

/// SUB-1228: an `allow` behind the operator's unfiltered rule never matches,
/// so the summary reports the kept rule instead of claiming a grant.
#[test]
fn add_package_keeps_an_existing_main_rule_for_a_selected_capability() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let out = add_sdk_with_charge_selected(
        home.path(),
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      action: deny\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        !text.contains("allow pied-piper.example/charge"),
        "got: {text}"
    );
    assert!(
        text.contains("`main` already has rules for 1 selected capabilities; kept them, since the first matching rule wins:\n    pied-piper.example/charge"),
        "got: {text}"
    );
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    assert_eq!(blueprint.permissions["main"].len(), 1, "{updated}");
    assert_eq!(
        blueprint.permissions["main"][0].action,
        submilli_blueprint::Action::Deny
    );
    assert!(!updated.contains("provides:"), "{updated}");
}

/// The `provides` comments go above the rules add-package wrote, not above an
/// earlier operator rule for another of the package's capabilities.
#[test]
fn add_package_annotates_only_the_rules_it_adds() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let out = add_sdk_with_charge_selected(
        home.path(),
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: pied-piper.example/refund\n      action: deny\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    assert!(
        updated.contains(
            "  main:\n  - capability: pied-piper.example/refund\n    action: deny\n    # @pied-piper/sdk provides:\n    # pied-piper.example/charge\n"
        ),
        "{updated}"
    );
    assert!(
        !updated.contains("# pied-piper.example/refund"),
        "{updated}"
    );
}

/// A dependency's calls run as its own caller: lint checks its `requires`
/// like a declared package's, `--fix` adds them, and its caller block needs no
/// `packages:` entry.
#[test]
fn lint_checks_and_fixes_a_dependencys_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\nsecrets:\n  BILLING_API_KEY:\n    store: BILLING_API_KEY\npackages:\n  - \"@pied-piper/support\"\npermissions:\n  \"@pied-piper/support\":\n    - capability: pied-piper.example/credits.apply\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains(
            "package `@pied-piper/billing` requires `secrets.get` with filter `name == \"BILLING_API_KEY\"`, but `permissions.@pied-piper/billing` has no matching rule"
        ),
        "got: {}",
        stderr(&out)
    );

    let fixed = run_with_home(
        &[os("blueprint"), os("lint"), os("--fix"), file.as_os_str()],
        home.path(),
    );
    assert!(fixed.status.success(), "stderr: {}", stderr(&fixed));
    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(!stderr(&out).contains("warning:"), "got: {}", stderr(&out));
}

/// Once nothing declared depends on it, a dependency's caller block is stale.
#[test]
fn lint_warns_for_a_caller_block_outside_every_closure() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npermissions:\n  \"@pied-piper/billing\":\n    - capability: secrets.get\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(
        stderr(&out).contains(
            "caller block for `@pied-piper/billing`, but `@pied-piper/billing` is neither in `packages:` nor a dependency of a package there"
        ),
        "got: {}",
        stderr(&out)
    );
}

/// A dependency that fails to load is an error, but the declared package and
/// every dependency that does load keep their checks, and the missing one's
/// block is not stale.
#[test]
fn lint_checks_what_loads_when_a_dependency_is_missing() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    fs::remove_dir_all(home.path().join("packages/@pied-piper/billing")).expect("remove billing");
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/desk\"\npermissions:\n  \"@pied-piper/desk\": []\n  \"@pied-piper/billing\":\n    - capability: secrets.get\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    let err = stderr(&out);
    assert!(
        err.contains("cannot validate package `@pied-piper/desk` capabilities"),
        "got: {err}"
    );
    assert!(
        err.contains("package `@pied-piper/support` requires `pied-piper.example/credits.apply`"),
        "got: {err}"
    );
    assert!(!err.contains("neither in `packages:`"), "got: {err}");
}

/// Past a missing package, lint cannot tell a dependency's caller block from a
/// stale one, so it does not call it stale.
#[test]
fn lint_does_not_call_blocks_stale_past_a_missing_dependency() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    fs::remove_dir_all(home.path().join("packages/@pied-piper/support")).expect("remove support");
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/desk\"\npermissions:\n  \"@pied-piper/desk\": []\n  \"@pied-piper/billing\":\n    - capability: secrets.get\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    let err = stderr(&out);
    assert!(
        err.contains("cannot validate package `@pied-piper/desk` capabilities"),
        "got: {err}"
    );
    assert!(!err.contains("neither in `packages:`"), "got: {err}");
}

/// Adding a dependency by name after its dependent lists it for `main` and
/// keeps the caller list its dependent's `add-package` wrote.
#[test]
fn add_package_lists_a_dependency_added_after_its_dependent() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");
    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/billing", "--all-capabilities"],
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("`@pied-piper/billing` already has rules for every capability it requires; kept them:\n    http.get\n    secrets.get"),
        "got: {text}"
    );
    assert!(!text.contains("added 0 rules"), "got: {text}");
    let blueprint =
        submilli_blueprint::parse(&fs::read_to_string(&file).expect("read")).expect("parses");
    assert!(blueprint.packages.contains("@pied-piper/billing"));
    assert_eq!(blueprint.permissions["@pied-piper/billing"].len(), 2);
    assert_eq!(blueprint.permissions["main"].len(), 1);
}

#[test]
fn capability_add_suggests_closest_name_without_changing_blueprint() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    let original = "name: typo\ndefault: deny\n";
    write_file(&file, original);
    for (input, expected) in [
        ("fs.cpy", "did you mean: fs.copy"),
        ("http.downlod", "did you mean: http.download"),
        ("FS.Write", "did you mean: fs.write"),
    ] {
        let out = run_with_home(
            &[
                os("blueprint"),
                os("capability"),
                os("add"),
                os(input),
                os("--blueprint"),
                file.as_os_str(),
            ],
            home.path(),
        );
        assert!(!out.status.success());
        let message = stderr(&out);
        assert!(message.contains(expected), "{message}");
        assert_eq!(fs::read_to_string(&file).expect("read blueprint"), original);
    }
}

/// A dependent's caller list names its dependency's provided capabilities,
/// so `capability add` knows them without `--force`.
#[test]
fn capability_add_knows_a_dependencys_provided_capabilities() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");
    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let out = run_with_home(
        &[
            os("blueprint"),
            os("capability"),
            os("add"),
            os("pied-piper.example/credits.apply"),
            os("--caller"),
            os("@pied-piper/support"),
            os("--filter"),
            os("customer == \"cus_1\""),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let list = run_with_home(
        &[
            os("blueprint"),
            os("capability"),
            os("list"),
            os("@pied-piper/billing"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );
    assert!(list.status.success(), "stderr: {}", stderr(&list));
    assert!(
        stdout(&list).contains("pied-piper.example/credits.apply"),
        "got: {}",
        stdout(&list)
    );
}

/// `main` cannot import a dependency, so a `main` rule for what only a
/// dependency provides could never match.
#[test]
fn capability_add_refuses_a_dependencys_capability_for_main() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\n");
    let out = add_package(
        home.path(),
        &file,
        &["@pied-piper/support", "--no-capabilities"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let out = run_with_home(
        &[
            os("blueprint"),
            os("capability"),
            os("add"),
            os("pied-piper.example/credits.apply"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains(
            "'pied-piper.example/credits.apply' is provided by `@pied-piper/billing`, which only the packages that depend on it can call; `main` cannot import it\n  grant it to one of them with `--caller @pied-piper/support`"
        ),
        "got: {}",
        stderr(&out)
    );
}

/// SUB-1258: the first matching rule decides, so a rule behind an unfiltered
/// rule for the same capability and caller never matches.
#[test]
fn lint_warns_on_a_rule_behind_an_unfiltered_rule() {
    let file = write_temp(
        "submilli-lint-shadowed.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: fs.read\n      filter: path == \"/a\"\n      action: allow\n    - capability: fs.read\n      action: deny\n    - capability: fs.write\n      action: allow\n    - capability: fs.read\n      filter: path == \"/x\"\n      action: allow\n  \"@pied-piper/util\":\n    - capability: fs.read\n      action: allow\n    - capability: http.get\n      action: allow\n    - capability: http.get\n      action: deny\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert!(
        out.status.success(),
        "a warning, not an error: {}",
        stderr(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("`permissions.main` rule 4 for `fs.read` (`allow` with filter `path == \"/x\"`) never matches: rule 2 (`deny`, no filter) decides every call to it first"),
        "got: {err}"
    );
    assert!(
        err.contains("`permissions.@pied-piper/util` rule 3 for `http.get` (`deny`, no filter) never matches: rule 2 (`allow`, no filter) decides every call to it first"),
        "got: {err}"
    );
    assert_eq!(err.matches("never matches").count(), 2, "got: {err}");
}

/// The runtime refuses `secrets.get` to `main` outright, which lint reports
/// on its own; no rule there decides anything.
#[test]
fn lint_does_not_call_rules_refused_to_main_shadowed() {
    let file = write_temp(
        "submilli-lint-shadowed-secrets.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: secrets.get\n      action: allow\n    - capability: secrets.get\n      action: deny\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    let err = stderr(&out);
    assert!(err.contains("has no effect"), "got: {err}");
    assert!(!err.contains("never matches"), "got: {err}");
}

/// A filtered rule decides only the calls it matches, so the rules after it
/// still apply to the rest.
#[test]
fn lint_accepts_rules_behind_a_filtered_rule() {
    let file = write_temp(
        "submilli-lint-filtered-first.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: fs.read\n      filter: path == \"/x\"\n      action: deny\n    - capability: fs.read\n      action: allow\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        !stderr(&out).contains("never matches"),
        "got: {}",
        stderr(&out)
    );
}

/// SUB-1283: a condition on a field the check doesn't report is false for
/// every call, so the `allow` rule never matches and the `not` form matches
/// every call.
#[test]
fn lint_rejects_a_package_filter_on_a_field_the_operation_does_not_report() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - '@pied-piper/sdk'\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      filter: customer == \"cus_1\" and customerClass == \"premium\"\n      action: allow\n    - capability: pied-piper.example/refund\n      filter: customer == \"cus_1\" and not (customerClass == \"standard\")\n      action: allow\n  '@pied-piper/sdk': []\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    for (position, capability) in [
        (1, "pied-piper.example/charge"),
        (2, "pied-piper.example/refund"),
    ] {
        assert!(
            err.contains(&format!("error: {}: `permissions.main` rule {position} for `{capability}` tests `customerClass`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: customer", file.display())),
            "got: {err}"
        );
    }
    assert!(!err.contains("is valid"), "got: {err}");
}

#[test]
fn lint_rejects_stdlib_and_mcp_filters_on_unreported_fields() {
    let file = write_temp(
        "submilli-lint-unreported-fields.yaml",
        "name: x\ndefault: deny\nmcp:\n  linear:\n    url: https://example.com/mcp\npermissions:\n  main:\n    - capability: fs.write\n      filter: host == \"x\" or host == \"y\"\n      action: allow\n    - capability: mcp.linear\n      filter: tool == \"save_issue\" and server == \"linear\"\n      action: allow\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("`permissions.main` rule 1 for `fs.write` tests `host`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: diff, length, max_bytes, path"),
        "got: {err}"
    );
    assert_eq!(err.matches("tests `host`").count(), 1, "got: {err}");
    assert!(
        err.contains("`permissions.main` rule 2 for `mcp.linear` tests `server`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: tool, transport"),
        "got: {err}"
    );
}

/// `http.request` reports the method alongside the verbs' fields.
#[test]
fn lint_checks_fields_of_http_request() {
    let file = write_temp(
        "submilli-lint-http-request-fields.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: http.request\n      filter: method == \"GET\" and host == \"a.com\"\n      action: allow\n    - capability: http.request\n      filter: owner == \"ops\"\n      action: deny\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(!err.contains("tests `method`"), "got: {err}");
    assert!(
        err.contains("`permissions.main` rule 2 for `http.request` tests `owner`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: body_size, host, method, path, timeout_ms"),
        "got: {err}"
    );
}

/// A package that checks a standard-library name itself reports its own
/// context, so its fields count alongside the catalog's.
#[test]
fn lint_accepts_a_field_a_package_reports_for_a_stdlib_capability() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        "[[package]]\nname = \"@pied-piper/saver\"\nversion = \"0.1.0\"\ndescription = \"Saver package.\"\npath = \"saver\"\n",
    );
    write_file(
        &project.path().join("saver/src/lib.ts"),
        r#"
            import { check } from "submilli:security";
            /** @capability fs.write { path, purpose } */
            export function save(path: string, purpose: string): void {
                check("fs.write", { path, purpose });
            }
        "#,
    );
    let out = run_in_with_home(
        &[os("build"), os("publish-local")],
        project.path(),
        home.path(),
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - '@pied-piper/saver'\npermissions:\n  main:\n    - capability: fs.write\n      filter: purpose == \"notes\" and path glob \"/out/*\"\n      action: allow\n    - capability: fs.write\n      filter: owner == \"ops\"\n      action: allow\n  '@pied-piper/saver': []\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    let err = stderr(&out);
    assert!(!err.contains("tests `purpose`"), "got: {err}");
    assert!(
        err.contains("rule 2 for `fs.write` tests `owner`, which the operation doesn't report, so a condition on it is false for every call, and true under `not`; its fields are: diff, length, max_bytes, path, purpose"),
        "got: {err}"
    );
}

/// A package that fails to load could report the field itself, so lint
/// reports the load failure and leaves the field lists unjudged.
#[test]
fn lint_skips_field_checks_when_a_package_fails_to_load() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - '@pied-piper/missing'\npermissions:\n  main:\n    - capability: fs.write\n      filter: purpose == \"notes\"\n      action: allow\n  '@pied-piper/missing': []\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("@pied-piper/missing"), "got: {err}");
    assert!(!err.contains("doesn't report"), "got: {err}");
}

/// Fields only some calls supply are still reported, and only the first
/// segment of a dotted path is checked: `path.length` can't match a string
/// `path`, but nothing records what a field contains. A capability nothing
/// provides has no field list; lint warns about its name instead.
#[test]
fn lint_accepts_filters_on_reported_fields() {
    let file = write_temp(
        "submilli-lint-reported-fields.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: fs.write\n      filter: path glob \"/out/*\" and length < 1000\n      action: allow\n    - capability: fs.read\n      filter: path.length == 3\n      action: allow\n    - capability: pied-piper.example/unknown\n      filter: anything == 1\n      action: allow\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(!err.contains("doesn't report"), "got: {err}");
    assert!(
        err.contains("rule 3 for `pied-piper.example/unknown` never matches"),
        "got: {err}"
    );
}

/// A misspelled name is a rule that never matches. It stays a warning: the
/// policy engine matches names verbatim, so the file is still valid.
#[test]
fn lint_warns_on_a_capability_name_nothing_lists() {
    let file = write_temp(
        "submilli-lint-unknown-capability.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: fs.wrte\n      action: allow\n    - capability: mcp.linear\n      action: deny\n    - capability: http.trace\n      action: deny\n    - capability: http.request\n      action: deny\n    - capability: http.dlete\n      action: deny\n  '@pied-piper/util':\n    - capability: pied-piper.example/charge\n      action: allow\nmcp:\n  linear:\n    url: https://example.com/mcp\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("`permissions.main` rule 1 for `fs.wrte` never matches: no standard-library operation, declared package, or declared MCP server provides it; did you mean: "),
        "got: {err}"
    );
    assert!(err.contains("fs.write"), "got: {err}");
    // A method is not a capability of its own: `http.request` checks it.
    assert!(
        err.contains("`permissions.main` rule 3 for `http.trace` never matches"),
        "got: {err}"
    );
    assert!(!err.contains("rule 4 for `http.request`"), "got: {err}");
    assert!(
        err.contains("`permissions.main` rule 5 for `http.dlete` never matches: no standard-library operation, declared package, or declared MCP server provides it; did you mean: http.delete"),
        "got: {err}"
    );
    // A stale caller block's own warning covers its rules.
    assert!(
        err.contains("caller block for `@pied-piper/util`"),
        "got: {err}"
    );
    assert_eq!(err.matches("never matches").count(), 3, "got: {err}");
}

/// Names the catalog, a declared MCP server, or a declared package provide
/// are known, under `main` and under a package.
#[test]
fn lint_accepts_capability_names_something_provides() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\nmcp:\n  linear:\n    url: https://example.com/mcp\npackages:\n  - \"@pied-piper/support\"\npermissions:\n  main:\n    - capability: llm.call\n      action: allow\n    - capability: mcp.linear\n      action: allow\n  \"@pied-piper/support\":\n    - capability: pied-piper.example/credits.apply\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(
        !stderr(&out).contains("never matches"),
        "got: {}",
        stderr(&out)
    );

    let _sdk = publish_capability_packages(home.path());
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/sdk\"\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      action: allow\n  \"@pied-piper/sdk\":\n    - capability: pied-piper.example/chrage\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    let err = stderr(&out);
    assert!(
        !err.contains("`pied-piper.example/charge` never matches"),
        "got: {err}"
    );
    assert!(
        err.contains(
            "`permissions.@pied-piper/sdk` rule 1 for `pied-piper.example/chrage` never matches"
        ),
        "got: {err}"
    );
}

/// `main` imports only declared packages, so a `main` rule for what only an
/// undeclared dependency provides never matches; the packages that can call
/// it can hold the rule.
#[test]
fn lint_warns_on_a_main_rule_for_a_dependency_only_capability() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_dependency_chain(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@pied-piper/desk\"\npermissions:\n  main:\n    - capability: pied-piper.example/credits.apply\n      action: allow\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    let err = stderr(&out);
    assert!(
        err.contains("`permissions.main` rule 1 for `pied-piper.example/credits.apply` never matches: `@pied-piper/billing` provides it, and only the packages that depend on it can call it; move the rule under `permissions.@pied-piper/support`, or add `@pied-piper/billing` to `packages:`"),
        "got: {err}"
    );
}

/// A package that fails to load may provide the name, so lint reports the
/// load failure and leaves names unjudged.
#[test]
fn lint_skips_name_checks_when_a_package_fails_to_load() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - '@pied-piper/missing'\npermissions:\n  main:\n    - capability: pied-piper.example/charge\n      action: allow\n  '@pied-piper/missing': []\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    let err = stderr(&out);
    assert!(err.contains("@pied-piper/missing"), "got: {err}");
    assert!(!err.contains("never matches"), "got: {err}");
}

// ---- submilli run --blueprint --------------------------------------------

const FS_WRITE_SCRIPT: &str = r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/x.txt", "hi"); }"#;

fn run_with_blueprint(script_name: &str, bp_name: &str, bp_yaml: &str) -> Output {
    let script = write_temp(script_name, FS_WRITE_SCRIPT);
    let bp = write_temp(bp_name, bp_yaml);
    run(&[
        os("run"),
        script.as_os_str(),
        os("--blueprint"),
        bp.as_os_str(),
    ])
}

#[test]
fn run_blueprint_denies_ungranted_capability() {
    // Policy-free blueprint ⇒ deny-by-default ⇒ fs.write is denied ⇒ trap.
    let out = run_with_blueprint(
        "submilli-run-bp-deny.subm",
        "submilli-run-bp-deny.yaml",
        "name: t\n",
    );
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
}

#[test]
fn run_blueprint_allows_granted_capability() {
    let out = run_with_blueprint(
        "submilli-run-bp-allow.subm",
        "submilli-run-bp-allow.yaml",
        "name: t\npermissions:\n  main:\n    - capability: fs.write\n      action: allow\n",
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
}

#[test]
fn run_without_blueprint_allows_everything() {
    let script = write_temp("submilli-run-no-bp.subm", FS_WRITE_SCRIPT);
    let out = run(&[os("run"), script.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
}

// ---- submilli secret + store: secret resolution in run ------------------

/// `submilli secret put <key>`, feeding the value on stdin, under `home`.
fn secret_put(home: &std::path::Path, key: &str, value: &str) -> Output {
    let mut child = Command::new(submilli_bin())
        .args([os("secret"), os("put"), os(key)])
        .env("SUBMILLI_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn secret put");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(value.as_bytes())
        .expect("write secret value");
    child.wait_with_output().expect("secret put output")
}

/// The package reports what it saw rather than returning the value: handing the
/// plaintext back to `main` is the shape the carve-out exists to prevent.
const SECRET_PACKAGE_SOURCE: &str = r#"
    import { get } from "submilli:secrets";

    /** Returns "matched" when MY_TOKEN resolves to `expected`; throws otherwise. */
    export function tokenProbe(expected: string): string {
        const value = get("MY_TOKEN");
        if (value === undefined) { throw new Error("MY_TOKEN resolved to nothing"); }
        if (value !== expected) { throw new Error("MY_TOKEN did not match"); }
        return "matched";
    }
"#;

const SECRET_SCRIPT: &str = r#"import { tokenProbe } from "@pied-piper/secrets";
function main(): string {
  return tokenProbe("sekret");
}"#;

const SECRET_BLUEPRINT: &str = "name: t\n\
packages:\n  - \"@pied-piper/secrets\"\n\
secrets:\n  MY_TOKEN: { store: MY_TOKEN }\n\
permissions:\n  \"@pied-piper/secrets\":\n    - capability: secrets.get\n      action: allow\n";

/// Publishes `@pied-piper/secrets` into the store under `home`. Returned so the
/// project tempdir outlives the run.
fn publish_token_probe_package(home: &std::path::Path) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@pied-piper/secrets"
version = "0.1.0"
description = "Secrets probe package."
path = "secrets"
"#,
    );
    write_file(
        &project.path().join("secrets/src/lib.ts"),
        SECRET_PACKAGE_SOURCE,
    );
    let out = run_in_with_home(&[os("build"), os("publish-local")], project.path(), home);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    project
}

#[test]
fn run_blueprint_resolves_store_secret_through_a_package() {
    let home = tempfile::tempdir().expect("home dir");
    let _project = publish_token_probe_package(home.path());

    let put = secret_put(home.path(), "MY_TOKEN", "sekret");
    assert!(put.status.success(), "secret put: {}", stderr(&put));

    let script = write_temp("submilli-run-store-secret.subm", SECRET_SCRIPT);
    let bp = write_temp("submilli-run-store-secret.yaml", SECRET_BLUEPRINT);
    let out = run_with_home(
        &[
            os("run"),
            script.as_os_str(),
            os("--blueprint"),
            bp.as_os_str(),
        ],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "matched");
}

#[test]
fn run_blueprint_store_secret_missing_traps() {
    // Same blueprint, but nothing stored ⇒ `store:` resolves to no value. The
    // null-check that throws now lives in the package, not the script: `main`
    // never sees the value either way.
    let home = tempfile::tempdir().expect("home dir");
    let _project = publish_token_probe_package(home.path());

    let script = write_temp("submilli-run-store-missing.subm", SECRET_SCRIPT);
    let bp = write_temp("submilli-run-store-missing.yaml", SECRET_BLUEPRINT);
    let out = run_with_home(
        &[
            os("run"),
            script.as_os_str(),
            os("--blueprint"),
            bp.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("MY_TOKEN resolved to nothing"),
        "stderr: {}",
        stderr(&out)
    );
}

/// `main` reading a secret directly is refused whatever the blueprint says.
#[test]
fn run_blueprint_refuses_a_direct_secret_read_from_main() {
    let home = tempfile::tempdir().expect("home dir");
    let put = secret_put(home.path(), "MY_TOKEN", "sekret");
    assert!(put.status.success(), "secret put: {}", stderr(&put));

    let script = write_temp(
        "submilli-run-main-secret.subm",
        r#"import { get } from "submilli:secrets";
function main(): string | undefined { return get("MY_TOKEN"); }"#,
    );
    let bp = write_temp(
        "submilli-run-main-secret.yaml",
        "name: t\nsecrets:\n  MY_TOKEN: { store: MY_TOKEN }\n\
         permissions:\n  main:\n    - capability: secrets.get\n      action: allow\n",
    );
    let out = run_with_home(
        &[
            os("run"),
            script.as_os_str(),
            os("--blueprint"),
            bp.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    let err = stderr(&out);
    assert!(err.contains("no policy can grant this"), "stderr: {err}");
    assert!(!stdout(&out).contains("sekret"), "the value must not leak");
}

#[test]
fn run_invalid_blueprint_fails_fast() {
    let script = write_temp("submilli-run-bad-bp.subm", FS_WRITE_SCRIPT);
    let bp = write_temp("submilli-run-bad-bp.yaml", "name: x\ndefault: maybe\n");
    let out = run(&[
        os("run"),
        script.as_os_str(),
        os("--blueprint"),
        bp.as_os_str(),
    ]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("error:"));
}

// ---- submilli build test: caller attribution ------------------------------

/// A project whose package exports a gated call and a thrower, with a test file
/// that exercises both. `AllowAllCheck` logs `caller=` for every gated call, so
/// the run's stderr is the attribution record.
fn attribution_test_project(test_file: &str) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@pied-piper/probe"
version = "0.1.0"
description = "Probe package."
path = "probe"
"#,
    );
    write_file(
        &project.path().join("probe/src/lib.ts"),
        r#"
            import { exists } from "submilli:fs";

            /** Reads through a gated capability. */
            export function probe(): boolean {
                return exists("/from-export");
            }

            /** Always throws, so a caller can catch and continue. */
            export function boom(): void {
                throw new Error("boom");
            }
        "#,
    );
    write_file(&project.path().join("probe/tests/lib.test.ts"), test_file);
    project
}

#[test]
fn build_test_attributes_a_package_test_file_to_the_package() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project = attribution_test_project(
        r#"
            import { exists } from "submilli:fs";
            import { probe } from "@pied-piper/probe";

            function main(): void {
                const _direct = exists("/from-test-file");
                const _viaExport = probe();
            }
        "#,
    );

    let out = run_in_with_home(&[os("build"), os("test")], project.path(), home.path());

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let log = stderr(&out);
    assert_eq!(
        log.matches("caller=@pied-piper/probe capability=fs.stat")
            .count(),
        2,
        "the test file's own call and the one inside the export are both the \
         package's; got: {log}",
    );
    assert!(
        !log.contains("caller=main capability=fs.stat"),
        "no gated call should be attributed to main; got: {log}",
    );
}

#[test]
fn build_test_keeps_package_attribution_after_a_caught_throw() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project = attribution_test_project(
        r#"
            import { exists } from "submilli:fs";
            import { boom } from "@pied-piper/probe";

            function main(): void {
                try {
                    boom();
                } catch (e: Error) {
                    // The export's frame must come off even though it threw.
                }
                const _after = exists("/after-catch");
            }
        "#,
    );

    let out = run_in_with_home(&[os("build"), os("test")], project.path(), home.path());

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let log = stderr(&out);
    assert_eq!(
        log.matches("caller=@pied-piper/probe capability=fs.stat")
            .count(),
        1,
        "the post-catch call stays the package's; got: {log}",
    );
    assert!(
        !log.contains("caller=main capability=fs.stat"),
        "a leaked frame would show up as a different caller; got: {log}",
    );
}

#[test]
fn lint_warns_on_a_main_rule_the_runtime_never_consults() {
    let file = write_temp(
        "submilli-lint-unreachable-main-rule.yaml",
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: secrets.get\n      action: allow\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(
        err.contains("`secrets.get` under `main:` has no effect"),
        "got: {err}"
    );
    assert!(
        err.contains("pass the secret NAME"),
        "the warning must name the fix; got: {err}"
    );
}

#[test]
fn lint_does_not_warn_on_the_same_rule_under_a_package() {
    let file = write_temp(
        "submilli-lint-package-secret-rule.yaml",
        "name: x\ndefault: deny\npermissions:\n  \"@pied-piper/sdk\":\n    - capability: secrets.get\n      action: allow\n",
    );

    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(
        !err.contains("has no effect"),
        "a package may hold this rule; got: {err}"
    );
}

#[test]
fn blueprint_http_denial_is_enforced_by_local_run() {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("probe.ts");
    let blueprint = temp.path().join("blueprint.yaml");
    fs::write(
        &script,
        r#"
        import { get } from "submilli:http";
        function main(): string {
            try { get("http://127.0.0.1:1/?token=never-print-this"); return "allowed"; }
            catch (error) { return (error as Error).message; }
        }
    "#,
    )
    .unwrap();
    for (allow, rule) in [(false, false), (false, true), (true, false)] {
        fs::write(&blueprint, format!("name: gates\ndefault: allow\nallow_insecure_http: {allow}\nauth_proxy:\n- host: 127.0.0.1\n  allow_insecure_http: {rule}\n  headers: {{ X-Test: dummy }}\n")).unwrap();
        let result = run_with_home(
            &[
                os("run"),
                script.as_os_str(),
                os("--blueprint"),
                blueprint.as_os_str(),
            ],
            temp.path(),
        );
        assert!(result.status.success(), "{}", stderr(&result));
        let message = stdout(&result);
        assert!(message.contains("HTTPS required"), "{message}");
        assert!(!message.contains("never-print-this"), "{message}");
        assert!(message.contains(if allow {
            "auth_proxy rule"
        } else {
            "blueprint"
        }));
    }
}

/// A callback re-enters Wasm on the native stack. With a raised `--max-stack`,
/// the run's thread must be sized to match, or deep re-entry overflows the
/// thread and aborts the process instead of ending the run.
#[test]
fn deep_reentry_under_a_raised_stack_ends_the_run_cleanly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("reentry.ts");
    write_file(
        &script,
        "function depth(n: number): number {\n  if (n === 0) { return 0; }\n  return [n].map((x: number) => depth(x - 1))[0] + 1;\n}\nfunction main(): number { return depth(200000); }\n",
    );

    let out = run(&[
        os("run"),
        os("--max-stack"),
        os("1572864"),
        script.as_os_str(),
    ]);

    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("call stack exhausted"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn blueprint_secret_add_accepts_only_store_or_harness_sources() {
    let home = tempfile::tempdir().unwrap();
    let blueprint = home.path().join("blueprint.yaml");
    let original = "name: secret-sources\n";
    fs::write(&blueprint, original).unwrap();
    for args in [
        vec!["--env", "KEY"],
        vec!["--file", "key.txt"],
        vec![],
        vec!["--store", "key", "--harness"],
    ] {
        let mut command = vec![
            os("blueprint"),
            os("secret"),
            os("add"),
            os("TOKEN"),
            os("--blueprint"),
            blueprint.as_os_str(),
        ];
        command.extend(args.iter().map(|arg| os(arg)));
        let out = run_with_home(&command, home.path());
        assert!(!out.status.success(), "accepted {args:?}");
        assert_eq!(fs::read_to_string(&blueprint).unwrap(), original);
    }
    for (name, args) in [
        ("STORED", vec!["--store", "key"]),
        ("BOUND", vec!["--harness", "--required"]),
    ] {
        let mut command = vec![
            os("blueprint"),
            os("secret"),
            os("add"),
            os(name),
            os("--blueprint"),
            blueprint.as_os_str(),
        ];
        command.extend(args.iter().map(|arg| os(arg)));
        let out = run_with_home(&command, home.path());
        assert!(out.status.success(), "{}", stderr(&out));
    }
    let parsed = submilli_blueprint::parse(&fs::read_to_string(&blueprint).unwrap()).unwrap();
    assert_eq!(
        parsed.secrets["STORED"],
        submilli_blueprint::SecretSource::Store("key".into())
    );
    assert_eq!(
        parsed.secrets["BOUND"],
        submilli_blueprint::SecretSource::Harness(submilli_blueprint::HarnessSecret {
            required: true
        })
    );
}

#[test]
fn lint_suggests_closest_capability_names_without_changing_blueprint() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    let original = "name: typo\ndefault: deny\npermissions:\n  main:\n    - capability: fs.cpy\n      action: allow\n    - capability: http.downlod\n      action: deny\n    - capability: FS.Write\n      action: allow\n";
    write_file(&file, original);
    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let message = stderr(&out);
    for expected in [
        "did you mean: fs.copy",
        "did you mean: http.download",
        "did you mean: fs.write",
    ] {
        assert!(message.contains(expected), "{message}");
    }
    assert_eq!(fs::read_to_string(&file).expect("read blueprint"), original);
}

#[test]
fn deny_warnings_lint_flag_and_environment_reject_unsafe_default() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("blueprint.yaml");
    fs::write(&file, "name: strict\ndefault: allow\n").unwrap();
    for environment in [false, true] {
        let mut command = Command::new(submilli_bin());
        command
            .args(["blueprint", "lint"])
            .arg(&file)
            .env("SUBMILLI_HOME", tmp.path());
        if environment {
            command.env("SUBMILLI_DENY_WARNINGS", "1");
        } else {
            command.arg("--deny-warnings");
        }
        let out = command.output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr(&out).contains("1 warning(s) treated as errors"),
            "{}",
            stderr(&out)
        );
        assert!(!stdout(&out).contains("is valid"));
    }
    fs::write(&file, "name: strict\ndefault: deny\n").unwrap();
    let out = run(&[
        os("blueprint"),
        os("lint"),
        os("--deny-warnings"),
        file.as_os_str(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn deny_warnings_lint_fix_uses_remaining_findings() {
    let home = tempfile::tempdir().unwrap();
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\npermissions: { \"@pied-piper/sdk\": [] }\n",
    );
    let out = run_with_home(
        &[
            os("blueprint"),
            os("lint"),
            os("--fix"),
            os("--deny-warnings"),
            file.as_os_str(),
        ],
        home.path(),
    );
    assert!(out.status.success(), "{}", stderr(&out));
    write_file(
        &file,
        "name: x\ndefault: allow\npackages:\n  - \"@pied-piper/app\"\n  - \"@pied-piper/sdk\"\npermissions: { \"@pied-piper/sdk\": [] }\n",
    );
    let out = run_with_home(
        &[
            os("blueprint"),
            os("lint"),
            os("--fix"),
            os("--deny-warnings"),
            file.as_os_str(),
        ],
        home.path(),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("1 warning(s) treated as errors"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn oversized_diagnostic_and_warning_are_abbreviated_without_changing_exit_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("render.ts");
    let source = format!(
        "function main(): number {{ {} return missing; }}",
        " ".repeat(100_000)
    );
    fs::write(&path, source).unwrap();
    let failed = run(&[os("check"), path.as_os_str()]);
    assert!(!failed.status.success());
    assert!(stderr(&failed).contains("[diagnostic output truncated]"));
    let warning = format!(
        "/** @{} */\nfunction main(): void {{}}",
        "unknown".repeat(15_000)
    );
    fs::write(&path, warning).unwrap();
    let warning = run(&[os("check"), path.as_os_str()]);
    assert!(warning.status.success(), "{}", stderr(&warning));
    assert!(stderr(&warning).contains("[diagnostic output truncated]"));
    fs::write(&path, "function main(): number { return 42; }").unwrap();
    assert!(run(&[os("check"), path.as_os_str()]).status.success());
}

#[test]
fn run_without_a_terminal_denies_a_call_that_needs_approval() {
    let dir = tempfile::tempdir().expect("tempdir");
    let blueprint = dir.path().join("blueprint.yaml");
    write_file(
        &blueprint,
        "name: refunds\npermissions:\n  main:\n    - capability: test.refund\n      action: ask-human\n",
    );
    let script = dir.path().join("main.ts");
    write_file(
        &script,
        r#"import { check } from "submilli:security";
function main(): string {
    try {
        check("test.refund", { amount: 250 });
        return "refunded";
    } catch (e) {
        if (e instanceof PermissionDeniedError) { return e.reason; }
        throw e;
    }
}
"#,
    );

    // `output()` gives the child no terminal on either stdin or stderr.
    let out = run(&[
        os("run"),
        os("--blueprint"),
        blueprint.as_os_str(),
        script.as_os_str(),
    ]);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let result = stdout(&out);
    assert!(
        result.contains("approval (ask-human)") && result.contains("approval can't be asked here"),
        "got: {result}"
    );
}
