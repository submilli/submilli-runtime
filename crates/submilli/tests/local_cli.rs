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
        ("ask-human", "name: x\ndefault: ask-human\n"),
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
        "name: x\npermissions:\n  \"@acme/util\":\n    - capability: acme/do\n      action: allow\n",
    );
    let out = run(&[os("blueprint"), os("lint"), file.as_os_str()]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(err.contains("@acme/util"), "missing package name in {err}");
}

#[test]
fn lint_warns_when_package_has_no_permission_block() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: deny\npackages:\n  - \"@acme/sdk\"\n",
    );
    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("warning:"), "missing warning in {err}");
    assert!(err.contains("@acme/sdk"), "missing package name in {err}");
}

fn publish_capability_packages(home: &std::path::Path) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@acme/sdk"]

[[package]]
name = "@acme/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
"#,
    );
    write_file(
        &project.path().join("sdk/src/lib.ts"),
        r#"
            import { check } from "submilli:security";
            /** @capability acme.com/charge { customer } */
            export function charge(customer: string): void {
                check("acme.com/charge", { customer });
            }
            /** @capability acme.com/refund { customer } */
            export function refund(customer: string): void {
                check("acme.com/refund", { customer });
            }
        "#,
    );
    write_file(
        &project.path().join("app/src/lib.ts"),
        r#"
            import { charge } from "@acme/sdk";
            import { check } from "submilli:security";
            /** @capability acme.com/run { customer } */
            export function run(customer: string): void {
                check("acme.com/run", { customer });
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
name = "@acme/secrets"
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
        ("@acme/app", "--no-capabilities"),
        ("@acme/sdk", "--capabilities=acme.com/charge"),
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
        "name: x\npackages:\n  - \"@acme/secrets\"\npermissions:\n  \"@acme/secrets\":\n    - capability: secrets.get\n      filter: name == \"STRIPE_API_KEY\"\n      action: allow\n",
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
        "name: x\nsecrets:\n  STRIPE_API_KEY: { env: STRIPE_API_KEY }\npackages:\n  - \"@acme/secrets\"\npermissions:\n  \"@acme/secrets\":\n    - capability: secrets.get\n      filter: name == \"STRIPE_API_KEY\"\n      action: allow\n",
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
        "name: x\npackages:\n  - \"@acme/app\"\npermissions:\n  \"@acme/app\": []\n",
    );

    let out = run_with_home(
        &[os("blueprint"), os("lint"), file.as_os_str()],
        home.path(),
    );

    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("error:"), "missing error in {err}");
    assert!(err.contains("requires `acme.com/charge`"), "got: {err}");
    assert!(err.contains("customer == \"cus_123\""), "got: {err}");
}

#[test]
fn lint_keeps_a_narrowed_requires_rule() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    let blueprint = "name: x\npackages:\n  - \"@acme/app\"\npermissions:\n  \"@acme/app\":\n    - capability: acme.com/charge\n      filter: customer == \"cus_123\" and amount < 500\n      action: allow\n";
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
    assert_eq!(parsed.permissions["@acme/app"].len(), 1, "{updated}");
}

#[test]
fn lint_fix_adds_missing_requires_rules_only() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@acme/app\"\n  - \"@acme/sdk\"\npermissions: {}\n",
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
    let app_rules = &blueprint.permissions["@acme/app"];
    assert!(app_rules.iter().any(|rule| {
        rule.capability == "acme.com/charge"
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
            os("@acme/secrets"),
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
            os("@acme/app"),
            os("--capabilities"),
            os("acme.com/run"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let out_text = stdout(&out);
    assert!(out_text.contains("added @acme/app"), "got: {out_text}");
    assert!(out_text.contains("allow acme.com/run"), "got: {out_text}");
    assert!(
        out_text.contains("allow acme.com/charge"),
        "got: {out_text}"
    );

    let updated = fs::read_to_string(&file).expect("read updated blueprint");
    assert!(updated.contains("# @acme/app provides:"), "got: {updated}");
    assert!(updated.contains("# acme.com/run"), "got: {updated}");
    assert!(
        updated.contains("#   { customer: string }"),
        "got: {updated}"
    );
    let blueprint = submilli_blueprint::parse(&updated).expect("updated blueprint parses");
    assert!(blueprint.packages.contains("@acme/app"));
    assert_eq!(
        blueprint.default_action,
        Some(submilli_blueprint::DefaultAction::Deny)
    );
    assert!(
        blueprint.permissions["main"]
            .iter()
            .any(|rule| rule.capability == "acme.com/run"
                && rule.action == submilli_blueprint::Action::Allow)
    );
    assert!(blueprint.permissions["@acme/app"].iter().any(|rule| {
        rule.capability == "acme.com/charge"
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
            os("@acme/app"),
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
        blueprint.permissions["@acme/app"]
            .iter()
            .any(|rule| rule.capability == "acme.com/charge")
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
    let blueprint = "name: x\ndefault: deny\npackages:\n  - \"@acme/app\"\n  - \"@acme/sdk\"\npermissions:\n  main:\n    - capability: acme.com/run\n      action: allow\n";
    write_file(&file, blueprint);

    // The filtered view speaks for the named package, not the blueprint.
    let out = capability_list_unconfigured(home.path(), &file, Some("@acme/app"));
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: `@acme/app` provides no capability"),
        "got: {text}"
    );

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default: deny` denies"), "got: {text}");
    assert!(
        text.contains("@acme/sdk\n  acme.com/charge\n      fields: customer: string\n"),
        "got: {text}"
    );
    assert!(text.contains("acme.com/refund"), "got: {text}");
    assert!(!text.contains("acme.com/run"), "got: {text}");
    assert!(!text.contains("@acme/app"), "got: {text}");
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
        "name: x\npackages:\n  - \"@acme/app\"\n  - \"@acme/sdk\"\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, Some("@acme/app"));

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default:` is unset"), "got: {text}");
    assert!(text.contains("@acme/app\n  acme.com/run"), "got: {text}");
    assert!(!text.contains("acme.com/charge"), "got: {text}");
    assert!(!text.contains("@acme/sdk"), "got: {text}");

    let out = capability_list_unconfigured(home.path(), &file, Some("submilli:fs"));
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("not a package"), "got: {err}");
    assert!(err.contains("@acme/app, @acme/sdk"), "got: {err}");
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
        "name: x\ndefault: allow\npackages:\n  - \"@acme/app\"\n  - \"@acme/sdk\"\npermissions:\n  main:\n    - capability: acme.com/charge\n      action: deny\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default: allow` allows"), "got: {text}");
    assert!(text.contains("acme.com/run"), "got: {text}");
    assert!(!text.contains("acme.com/charge"), "got: {text}");
}

#[test]
fn capability_list_unconfigured_reports_when_nothing_is_omitted() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\npackages:\n  - \"@acme/sdk\"\npermissions:\n  main:\n    - capability: acme.com/charge\n      filter: customer == \"cus_1\"\n      action: allow\n    - capability: acme.com/refund\n      action: ask-human\n",
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

/// `ask-human` is accepted but enforced as `deny`; rules other callers hold
/// for an omitted name are not `main`'s and stay out of this view.
#[test]
fn capability_list_unconfigured_under_ask_human_hides_other_callers_rules() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_capability_packages(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(
        &file,
        "name: x\ndefault: ask-human\npackages:\n  - \"@acme/app\"\n  - \"@acme/sdk\"\npermissions:\n  \"@acme/app\":\n    - capability: acme.com/charge\n      action: allow\n",
    );

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("`default: ask-human`"), "got: {text}");
    assert!(text.contains("currently denies"), "got: {text}");
    assert!(text.contains("acme.com/charge"), "got: {text}");
    assert!(!text.contains("rule["), "got: {text}");
}

#[test]
fn capability_list_unconfigured_with_a_package_that_provides_nothing() {
    let home = tempfile::tempdir().expect("home tempdir");
    let _project = publish_secret_package(home.path());
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\npackages:\n  - \"@acme/secrets\"\n");

    let out = capability_list_unconfigured(home.path(), &file, Some("@acme/secrets"));

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: `@acme/secrets` provides no capability"),
        "got: {text}"
    );
}

#[test]
fn capability_list_unconfigured_fails_on_a_package_it_cannot_load() {
    let home = tempfile::tempdir().expect("home tempdir");
    let file = home.path().join("blueprint.yaml");
    write_file(&file, "name: x\npackages:\n  - \"@acme/missing\"\n");

    let out = capability_list_unconfigured(home.path(), &file, None);

    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("@acme/missing"),
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
            os("@acme/sdk"),
            os("--capabilities"),
            os("acme.com/charge"),
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
            "1 provided capabilities not selected; `default: ask-human` applies to them",
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
                "name: x\ndefault: {default}\npermissions:\n  main:\n    - capability: acme.com/refund\n      action: allow\n"
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
            .filter(|rule| rule.capability == "acme.com/refund")
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
        "name: x\ndefault: allow\npermissions:\n  main:\n    - capability: acme.com/refund\n      filter: customer == \"vip\"\n      action: deny\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    let refund: Vec<_> = blueprint.permissions["main"]
        .iter()
        .filter(|rule| rule.capability == "acme.com/refund")
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
        "name: x\ndefault: allow\npermissions:\n  \"@acme/app\":\n    - capability: acme.com/refund\n      action: allow\n",
    );

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let updated = fs::read_to_string(home.path().join("blueprint.yaml")).expect("read");
    let blueprint = submilli_blueprint::parse(&updated).expect("parses");
    assert!(
        blueprint.permissions["main"].iter().any(|rule| {
            rule.capability == "acme.com/refund"
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
        "name: x\ndefault: deny\npermissions:\n  main:\n    - capability: acme.com/refund\n      filter: customer == \"vip\"\n      action: allow\n",
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(!text.contains("not selected"), "got: {text}");

    let file = home.path().join("blueprint.yaml");
    let out = capability_list_unconfigured(home.path(), &file, Some("@acme/sdk"));
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("none: `@acme/sdk` provides no capability"),
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
        "name: x\npackages:\n  - \"@acme/app\"\npermissions: {}\n",
    );

    let out = run_with_home(
        &[
            os("blueprint"),
            os("add-package"),
            os("@acme/app"),
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
            os("@acme/app@1.0.0"),
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
            os("@acme/missing"),
            os("--blueprint"),
            file.as_os_str(),
        ],
        home.path(),
    );

    assert!(!out.status.success());
    assert!(stderr(&out).contains("package `@acme/missing` was not found"));
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
        if (value === null) { throw new Error("MY_TOKEN resolved to nothing"); }
        if (value !== expected) { throw new Error("MY_TOKEN did not match"); }
        return "matched";
    }
"#;

const SECRET_SCRIPT: &str = r#"import { tokenProbe } from "@acme/secrets";
function main(): string {
  return tokenProbe("sekret");
}"#;

const SECRET_BLUEPRINT: &str = "name: t\n\
packages:\n  - \"@acme/secrets\"\n\
secrets:\n  MY_TOKEN: { store: MY_TOKEN }\n\
permissions:\n  \"@acme/secrets\":\n    - capability: secrets.get\n      action: allow\n";

/// Publishes `@acme/secrets` into the store under `home`. Returned so the
/// project tempdir outlives the run.
fn publish_token_probe_package(home: &std::path::Path) -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project tempdir");
    write_file(
        &project.path().join("submilli.toml"),
        r#"
[[package]]
name = "@acme/secrets"
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
function main(): string | null { return get("MY_TOKEN"); }"#,
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
name = "@acme/probe"
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
            import { probe } from "@acme/probe";

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
        log.matches("caller=@acme/probe capability=fs.stat").count(),
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
            import { boom } from "@acme/probe";

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
        log.matches("caller=@acme/probe capability=fs.stat").count(),
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
        "name: x\ndefault: deny\npermissions:\n  \"@acme/sdk\":\n    - capability: secrets.get\n      action: allow\n",
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
