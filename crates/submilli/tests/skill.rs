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
    code_blocks(text, language)[0]
}

/// Every fenced block tagged exactly `language`, in document order.
fn code_blocks<'a>(text: &'a str, language: &str) -> Vec<&'a str> {
    text.split(&format!("```{language}\n"))
        .skip(1)
        .map(|rest| rest.split_once("\n```").unwrap().0)
        .collect()
}

/// The "real service" package and blueprint documented in the references must
/// keep compiling, deriving the host and secret filters the prose describes, and
/// linting against each other — they are the pattern users copy.
#[test]
fn documented_real_service_package_and_blueprint_agree() {
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
    let source = code_blocks(&package_doc, "typescript")[1];
    assert!(
        source.contains("acme.com/orders.cancel"),
        "second package example moved"
    );
    ok(cli(&["build", "init", "@acme/orders", "package"]));
    fs::write(root.path().join("package/src/lib.ts"), source).unwrap();
    fs::write(
        root.path().join("package/tests/lib.test.ts"),
        "import { buildPageQuery } from \"@acme/orders\";\nfunction main(): void { assert(buildPageQuery(null) === \"?limit=50\"); }\n",
    )
    .unwrap();
    let check = cli(&["build", "check"]);
    ok(check.clone());
    assert!(
        !String::from_utf8_lossy(&check.stderr).contains("cannot statically resolve the host"),
        "the documented URL construction must derive a host filter"
    );
    let schema = fs::read_to_string(root.path().join("package/capabilities.yaml")).unwrap();
    for expected in [
        "host == \"api.acme.com\"",
        "capability: http.get",
        "capability: http.post",
        "name == \"ORDERS_API_TOKEN\"",
        "totalCents:\n      type: number",
    ] {
        assert!(schema.contains(expected), "{schema}");
    }
    ok(cli(&["build", "test"]));
    ok(cli(&["build", "publish-local"]));
    // Every complete blueprint in the reference must lint clean against this
    // CLI, so a schema or catalog change that outdates the skill fails here.
    // The `@acme/billing` fixture blueprint is exercised end to end by
    // `documented_package_and_blueprint_enforce_the_bound_customer` instead.
    let blueprints: Vec<&str> = code_blocks(&blueprint_doc, "yaml")
        .into_iter()
        .filter(|block| block.starts_with("kind: blueprint") && !block.contains("@acme/billing"))
        .collect();
    assert!(
        blueprints
            .iter()
            .any(|b| b.contains("name: support-orders")),
        "the support-orders example moved"
    );
    for (index, yaml) in blueprints.iter().enumerate() {
        let file = format!("blueprint-{index}.yaml");
        fs::write(root.path().join(&file), yaml).unwrap();
        let lint = cli(&["blueprint", "lint", &file]);
        ok(lint.clone());
        assert!(
            !String::from_utf8_lossy(&lint.stderr).contains("warning:"),
            "documented blueprint {index} should lint clean: {}",
            String::from_utf8_lossy(&lint.stderr)
        );
    }
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
                // Installed copies live under an assistant's directory where
                // nothing outside the skill resolves.
                assert!(
                    !target.contains("..") && !target.starts_with('/'),
                    "reference escapes the skill in {}: {target}",
                    path.display()
                );
                assert!(
                    path.parent().unwrap().join(target).is_file(),
                    "broken reference in {}: {target}",
                    path.display()
                );
            }
        }
        // Claude Code expands `@path` at load time, which defeats on-demand
        // references and breaks on other assistants.
        let include = ["@references/", "@./", "@/", "@~"];
        assert!(
            !content
                .split_whitespace()
                .any(|word| include.iter().any(|prefix| word.starts_with(prefix))),
            "@file include in {}",
            path.display()
        );
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

/// Serves a git ref advertisement and one release asset, as GitHub would.
fn serve_release(version: u32, files: serde_json::Value) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let source = format!("http://{}/repo", listener.local_addr().unwrap());
    let refs = format!("0000aaaa refs/tags/v9.9.9\nbbbb refs/tags/skill-v{version}\n");
    let asset = serde_json::json!({"schema": 1, "version": version, "files": files}).to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = [0u8; 2048];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            let body = if request.starts_with("GET /repo.git/info/refs?service=git-upload-pack ") {
                Some(&refs)
            } else if request.starts_with(&format!(
                "GET /repo/releases/download/skill-v{version}/submilli-skill.json "
            )) {
                Some(&asset)
            } else {
                None
            };
            let response = match body {
                Some(body) => format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                ),
                None => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    .to_owned(),
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    source
}

/// `sync` takes no target: it runs from inside the project, as an assistant would.
fn sync(project: &Path, home: &Path, source: Option<&str>) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_submilli"));
    command
        .args(["skill", "sync"])
        .current_dir(project)
        .env("SUBMILLI_TELEMETRY", "0")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUBMILLI_HOME", home.join(".submilli"))
        // Keep the CLI release hint off the real network.
        .env("SUBMILLI_RELEASE_SOURCE", "http://127.0.0.1:1/repo");
    match source {
        Some(source) => command.env("SUBMILLI_SKILL_SOURCE", source),
        None => command.env("SUBMILLI_SKILL_AUTOUPDATE", "0"),
    };
    let output = command.output().unwrap();
    ok(output.clone());
    String::from_utf8(output.stdout).unwrap()
}

fn project_with_install(agent: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".git")).unwrap();
    ok(run(project.path(), "install", agent));
    (project, tempfile::tempdir().unwrap())
}

#[test]
fn sync_finds_installs_without_flags_and_applies_the_bundle_offline() {
    use sha2::{Digest, Sha256};
    let (project, home) = project_with_install("claude");
    let path = project.path().join(".claude/skills/submilli");
    let receipt_path = path.join(".submilli-skill.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    fs::write(path.join("SKILL.md"), "old release").unwrap();
    receipt["files"]["SKILL.md"] = format!("{:x}", Sha256::digest(b"old release")).into();
    fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();

    let nested = project.path().join("src/deep");
    fs::create_dir_all(&nested).unwrap();
    let output = sync(&nested, home.path(), None);
    assert!(output.contains("SKILL.md changed: re-read it"), "{output}");
    compare_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/submilli"),
        &path,
    );
    assert!(sync(&nested, home.path(), None).contains(": current"));
}

#[test]
fn sync_installs_a_newer_release_then_answers_from_its_cache() {
    let (project, home) = project_with_install("codex");
    let path = project.path().join(".agents/skills/submilli");
    let source = serve_release(
        900,
        serde_json::json!({"SKILL.md": "released", "VERSION": "900\n", "references/new.md": "new"}),
    );
    let output = sync(project.path(), home.path(), Some(&source));
    assert!(
        output.contains("updated to skill v900 from skill release v900"),
        "{output}"
    );
    assert_eq!(
        fs::read_to_string(path.join("references/new.md")).unwrap(),
        "new"
    );
    assert!(!path.join("references/setup.md").exists());

    // A release newer than this CLI's bundle is healthy, not drift.
    ok(run(project.path(), "status", "codex"));
    ok(run(project.path(), "update", "codex"));
    assert_eq!(
        fs::read_to_string(path.join("SKILL.md")).unwrap(),
        "released"
    );

    // Within the check interval a second installation converges on the same
    // release without the network: this source refuses connections.
    ok(run(project.path(), "install", "cursor"));
    let output = sync(project.path(), home.path(), Some("http://127.0.0.1:1/repo"));
    assert!(
        output.contains(".cursor/skills/submilli: updated to skill v900"),
        "{output}"
    );
}

#[test]
fn sync_preserves_local_edits_and_survives_an_unreachable_source() {
    let (project, home) = project_with_install("claude");
    let skill_md = project.path().join(".claude/skills/submilli/SKILL.md");
    fs::write(&skill_md, "custom").unwrap();
    let output = sync(project.path(), home.path(), Some("http://127.0.0.1:1/repo"));
    assert!(output.contains("Skill release check skipped"), "{output}");
    assert!(output.contains("locally modified; preserved"), "{output}");
    assert_eq!(fs::read_to_string(skill_md).unwrap(), "custom");
}

#[test]
fn sync_rejects_a_release_that_writes_outside_the_skill() {
    let (project, home) = project_with_install("claude");
    let source = serve_release(
        901,
        serde_json::json!({"SKILL.md": "x", "VERSION": "901", "../../escape.md": "x"}),
    );
    let output = sync(project.path(), home.path(), Some(&source));
    assert!(output.contains("unsafe path"), "{output}");
    assert!(!project.path().join(".claude/escape.md").exists());
    ok(run(project.path(), "status", "claude"));
}

#[test]
fn installs_the_verifier_subagent_where_each_assistant_discovers_agents() {
    let root = tempfile::tempdir().unwrap();
    for (agent, companion) in [
        ("claude", ".claude/agents/submilli-verifier.md"),
        ("cursor", ".cursor/agents/submilli-verifier.md"),
        ("codex", ".codex/agents/submilli-verifier.toml"),
    ] {
        ok(run(root.path(), "install", agent));
        let installed = fs::read_to_string(root.path().join(companion)).unwrap();
        assert!(installed.contains("submilli-verifier"), "{agent}");
        assert!(installed.contains("verification.md"), "{agent}");
    }
    // Codex custom agents live under .codex even though its skills use .agents.
    assert!(!root.path().join(".agents/agents").exists());
}

#[test]
fn update_refreshes_an_unmodified_verifier_and_preserves_an_edited_one() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    ok(run(root.path(), "install", "claude"));
    let verifier = root.path().join(".claude/agents/submilli-verifier.md");
    let bundled = fs::read_to_string(&verifier).unwrap();

    // Make the skill look like an older release so update has work to do.
    let path = root.path().join(".claude/skills/submilli");
    let receipt_path = path.join(".submilli-skill.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    fs::write(path.join("SKILL.md"), "old release").unwrap();
    receipt["files"]["SKILL.md"] = format!("{:x}", Sha256::digest(b"old release")).into();
    // An unmodified verifier from the older release is replaced.
    fs::write(&verifier, "older verifier").unwrap();
    receipt["companions"][".claude/agents/submilli-verifier.md"] =
        format!("{:x}", Sha256::digest(b"older verifier")).into();
    fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    ok(run(root.path(), "update", "claude"));
    assert_eq!(fs::read_to_string(&verifier).unwrap(), bundled);

    // An edited verifier is kept, and the update still succeeds.
    fs::write(path.join("SKILL.md"), "old release").unwrap();
    fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    fs::write(&verifier, "my own verifier").unwrap();
    let output = run(root.path(), "update", "claude");
    ok(output.clone());
    assert!(String::from_utf8_lossy(&output.stdout).contains("not written"));
    assert_eq!(fs::read_to_string(&verifier).unwrap(), "my own verifier");
}

/// `build init` points a developer at `skill install`, naming the assistant
/// the project shows signs of, and says nothing once the skill is installed.
#[test]
fn build_init_suggests_the_skill_for_detected_assistants() {
    let init = |project: &Path, home: &Path| {
        let output = Command::new(env!("CARGO_BIN_EXE_submilli"))
            .args(["build", "init", "@acme/demo", "packages/demo"])
            .current_dir(project)
            .env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }, home)
            .env("SUBMILLI_HOME", home.join(".submilli"))
            .env("SUBMILLI_TELEMETRY", "0")
            .output()
            .unwrap();
        ok(output.clone());
        String::from_utf8(output.stderr).unwrap()
    };
    let home = tempfile::tempdir().unwrap();

    let bare = tempfile::tempdir().unwrap();
    let stderr = init(bare.path(), home.path());
    assert!(stderr.contains("Claude Code, Codex or Cursor"), "{stderr}");

    let claude = tempfile::tempdir().unwrap();
    fs::write(claude.path().join("CLAUDE.md"), "").unwrap();
    let stderr = init(claude.path(), home.path());
    assert!(
        stderr.contains("skill install --agent claude") && stderr.contains("Claude Code"),
        "{stderr}"
    );

    // Markers are found up to the repository root, and several assistants
    // produce one line naming each flag.
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join(".cursor")).unwrap();
    fs::create_dir_all(repo.path().join(".codex")).unwrap();
    let nested = repo.path().join("services/billing");
    fs::create_dir_all(&nested).unwrap();
    let stderr = init(&nested, home.path());
    assert!(stderr.contains("--agent <codex|cursor>"), "{stderr}");

    // An installation anywhere the CLI manages (here the user home) silences it.
    let installed = tempfile::tempdir().unwrap();
    fs::write(installed.path().join("CLAUDE.md"), "").unwrap();
    ok(Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(["skill", "install", "--agent", "claude"])
        .env(
            if cfg!(windows) { "USERPROFILE" } else { "HOME" },
            home.path(),
        )
        .env("SUBMILLI_TELEMETRY", "0")
        .output()
        .unwrap());
    let stderr = init(installed.path(), home.path());
    assert!(!stderr.contains("skill install"), "{stderr}");
}
