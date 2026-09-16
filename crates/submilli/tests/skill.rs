use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, verb: &str, agent: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["skill", verb, "--agent", agent, "--project"])
        .arg(root)
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .unwrap()
}

fn ok(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn installs_complete_bundle_for_each_assistant_and_is_idempotent() {
    for (agent, directory) in [
        ("claude", ".claude"),
        ("codex", ".agents"),
        ("cursor", ".cursor"),
    ] {
        let root = tempfile::tempdir().unwrap();
        assert!(!run(root.path(), "status", agent).status.success());
        ok(run(root.path(), "install", agent));
        let path = root.path().join(directory).join("skills/submilli");
        compare_tree(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../skills/submilli")
                .as_path(),
            &path,
        );
        ok(run(root.path(), "install", agent));
        ok(run(root.path(), "update", agent));
        ok(run(root.path(), "status", agent));
    }
}

fn compare_tree(source: &Path, installed: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = installed.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            compare_tree(&entry.path(), &target);
        } else {
            assert_eq!(fs::read(entry.path()).unwrap(), fs::read(target).unwrap());
        }
    }
}

#[test]
fn user_install_uses_assistants_home_discovery_path() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["skill", "install", "--agent", "codex"])
        .env(
            if cfg!(windows) { "USERPROFILE" } else { "HOME" },
            root.path(),
        )
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .unwrap();
    ok(output);
    assert!(
        root.path()
            .join(".agents/skills/submilli/SKILL.md")
            .exists()
    );
}

#[test]
fn update_requires_install_and_install_preserves_unmanaged_content() {
    let root = tempfile::tempdir().unwrap();
    assert!(!run(root.path(), "update", "codex").status.success());
    let path = root.path().join(".agents/skills/submilli");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("SKILL.md"), "my skill").unwrap();
    for verb in ["install", "update", "status"] {
        assert!(!run(root.path(), verb, "codex").status.success());
        assert_eq!(
            fs::read_to_string(path.join("SKILL.md")).unwrap(),
            "my skill"
        );
    }
}

#[test]
fn edits_additions_and_deletions_are_preserved() {
    for change in ["edit", "add", "delete"] {
        let root = tempfile::tempdir().unwrap();
        ok(run(root.path(), "install", "claude"));
        let path = root.path().join(".claude/skills/submilli");
        match change {
            "edit" => fs::write(path.join("SKILL.md"), "custom").unwrap(),
            "add" => fs::write(path.join("custom.md"), "custom").unwrap(),
            _ => fs::remove_file(path.join("SKILL.md")).unwrap(),
        }
        let before = fs::read(path.join("SKILL.md")).ok();
        for verb in ["install", "update", "status"] {
            assert!(!run(root.path(), verb, "claude").status.success());
            assert_eq!(fs::read(path.join("SKILL.md")).ok(), before);
        }
        if change == "add" {
            assert!(path.join("custom.md").exists());
        }
    }
}

#[test]
fn replaces_an_intact_older_bundle_and_removes_retired_files() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    ok(run(root.path(), "install", "cursor"));
    let path = root.path().join(".cursor/skills/submilli");
    let receipt_path = path.join(".submilli-skill.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    fs::write(path.join("SKILL.md"), "old release").unwrap();
    fs::write(path.join("retired.md"), "retired").unwrap();
    receipt["files"]["SKILL.md"] = format!("{:x}", Sha256::digest(b"old release")).into();
    receipt["files"]["retired.md"] = format!("{:x}", Sha256::digest(b"retired")).into();
    fs::write(receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert!(!run(root.path(), "status", "cursor").status.success());
    assert!(!run(root.path(), "install", "cursor").status.success());
    ok(run(root.path(), "update", "cursor"));
    assert!(!path.join("retired.md").exists());
    ok(run(root.path(), "status", "cursor"));
}

#[test]
fn corrupt_and_future_receipts_do_not_authorize_replacement() {
    for content in [
        "not json",
        r#"{"schema":99,"cli_version":"future","files":{}}"#,
    ] {
        let root = tempfile::tempdir().unwrap();
        ok(run(root.path(), "install", "codex"));
        let path = root.path().join(".agents/skills/submilli");
        let original = fs::read(path.join("SKILL.md")).unwrap();
        fs::write(path.join(".submilli-skill.json"), content).unwrap();
        assert!(!run(root.path(), "update", "codex").status.success());
        assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), original);
    }
}

#[test]
fn an_existing_lock_prevents_concurrent_replacement() {
    let root = tempfile::tempdir().unwrap();
    ok(run(root.path(), "install", "codex"));
    let lock = root.path().join(".agents/skills/.submilli-install.lock");
    fs::write(&lock, "").unwrap();
    assert!(!run(root.path(), "update", "codex").status.success());
    assert!(lock.exists());
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_targets_ancestors_and_contents() {
    use std::os::unix::fs::symlink;
    for relative in [".agents", ".agents/skills", ".agents/skills/submilli"] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = root.path().join(relative);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(outside.path(), &link).unwrap();
        assert!(!run(root.path(), "install", "codex").status.success());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
    let root = tempfile::tempdir().unwrap();
    ok(run(root.path(), "install", "codex"));
    let path = root.path().join(".agents/skills/submilli");
    symlink("missing", path.join("extra")).unwrap();
    assert!(!run(root.path(), "update", "codex").status.success());
}

fn code_block<'a>(text: &'a str, language: &str) -> &'a str {
    text.split_once(&format!("```{language}\n"))
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0
}

#[test]
fn skill_frontmatter_and_relative_links_are_valid() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/submilli");
    let core = fs::read_to_string(root.join("SKILL.md")).unwrap();
    let yaml = core
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---")
        .unwrap()
        .0;
    let metadata: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
    assert_eq!(metadata["name"].as_str(), Some("submilli"));
    assert!(!metadata["description"].as_str().unwrap().is_empty());
    assert!(
        core.split_whitespace().count() < 650,
        "keep core small; move details into references"
    );
    let mut files = vec![root.join("SKILL.md")];
    files.extend(
        fs::read_dir(root.join("references"))
            .unwrap()
            .map(|e| e.unwrap().path()),
    );
    for path in files {
        let content = fs::read_to_string(&path).unwrap();
        for suffix in content.split("](").skip(1) {
            let target = suffix.split(')').next().unwrap();
            if !target.starts_with("https://") {
                assert!(
                    path.parent().unwrap().join(target).is_file(),
                    "broken reference in {}: {target}",
                    path.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn documented_package_and_blueprint_enforce_the_bound_customer() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use serde_json::{Value, json};
    use std::sync::Arc;
    use submilli_server::{AppState, ServerConfig, app, blueprint::InMemoryBlueprintStore};
    use tower::ServiceExt;

    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store");
    let cli = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_submilli"))
            .args(args)
            .current_dir(root.path())
            .env("SUBMILLI_HOME", &store)
            .env("SUBMILLI_TELEMETRY", "0")
            .output()
            .unwrap()
    };
    let references = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/submilli/references");
    let package_doc = fs::read_to_string(references.join("packages.md")).unwrap();
    let blueprint_doc = fs::read_to_string(references.join("blueprints.md")).unwrap();
    ok(cli(&["build", "init", "@acme/billing", "package"]));
    fs::write(
        root.path().join("package/src/lib.ts"),
        code_block(&package_doc, "typescript"),
    )
    .unwrap();
    fs::write(root.path().join("package/tests/lib.test.ts"),
        "import { readBalance } from \"@acme/billing\";\nfunction main(): void { assert(readBalance(\"cus_northwind\") === 6150); }\n").unwrap();
    ok(cli(&["build", "check"]));
    ok(cli(&["build", "test"]));
    ok(cli(&["build", "publish-local"]));
    let yaml = code_block(&blueprint_doc, "yaml");
    fs::write(root.path().join("blueprint.yaml"), yaml).unwrap();
    ok(cli(&["blueprint", "lint", "blueprint.yaml"]));
    let blueprint = submilli_blueprint::parse(yaml).unwrap();
    let router = app(AppState::new(ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([blueprint]))),
        package_store_root: Some(store.join("packages")),
        session_storage_root: Some(root.path().join("sessions")),
        ..ServerConfig::default()
    })
    .unwrap());
    let code = code_block(&blueprint_doc, "typescript");
    for (program, variables, expected) in [
        (
            code.to_owned(),
            json!({"customerId":"cus_northwind"}),
            "allowed",
        ),
        (
            code.replace("cus_northwind", "cus_initech"),
            json!({"customerId":"cus_northwind"}),
            "denied",
        ),
        (code.to_owned(), json!({}), "missing"),
    ] {
        let request = Request::post("/v1/execute")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"blueprint":"support-read", "code":program, "variables":variables})
                    .to_string(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        match expected {
            "allowed" => {
                assert!(status.is_success(), "{body}");
                assert_eq!(body["result"], "6150");
            }
            "denied" => {
                assert!(body.to_string().contains("permission denied"), "{body}");
                assert!(body.to_string().contains("acme.com/balance.read"), "{body}");
            }
            _ => {
                assert_eq!(body["error"]["kind"], "invalid_request", "{body}");
                assert!(body["result"].is_null(), "{body}");
                assert!(body.to_string().contains("customerId"), "{body}");
            }
        }
    }
}
