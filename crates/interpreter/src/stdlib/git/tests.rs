use super::*;
use crate::runtime::{
    CheckOutcome, RuntimeConfig, Vfs, dispatch_main_async, install_runtime_async,
};
use serde_json::json;
use wasmtime::Linker;

#[tokio::test]
async fn repositories_use_selected_volume_subdirectories() {
    use crate::runtime::vfs::{Access, MountSpec};
    for named_root in [true, false] {
        let volume = tempfile::tempdir().unwrap();
        let vfs = if named_root {
            Vfs::external(volume.path().to_path_buf())
                .unwrap()
                .with_volume_name("shared")
                .with_subpath(Some("users/ada"))
                .unwrap()
        } else {
            Vfs::tempdir()
                .unwrap()
                .with_mount_subpath(
                    MountSpec {
                        guest_path: "/workspace".into(),
                        host: volume.path().to_path_buf(),
                        volume: "shared".into(),
                        access: Access::ReadWrite,
                        quota: None,
                    },
                    Some("users/ada"),
                )
                .unwrap()
                .with_cwd("/workspace")
                .unwrap()
        };
        run_source(
            r#"
            import { Repository } from "submilli:git";
            import * as fs from "submilli:fs";
            function main(): void {
                const repo = Repository.init("repo");
                fs.writeText("repo/note.txt", "hello");
                repo.add(["note.txt"]);
                repo.commit("initial");
                assert(Repository.open("repo").status().clean);
            }
            "#,
            test_data(vfs),
        )
        .await;
        assert!(volume.path().join("users/ada/repo/.git").is_dir());
        assert!(!volume.path().join("repo").exists());
    }
}

#[tokio::test]
async fn relative_repository_paths_use_vfs_cwd() {
    let vfs = Vfs::tempdir().unwrap().with_cwd("/work").unwrap();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("repo");
            fs.writeText("repo/note.txt", "hello");
            repo.add(["note.txt"]);
            repo.commit("initial");
            assert(Repository.open("/work/repo").status().clean);
            assert(Repository.open("repo").log().commits[0].message === "initial");
            assert(!fs.exists("/repo"));
        }
        "#,
        test_data(vfs),
    )
    .await;
}

#[tokio::test]
async fn local_repository_round_trip() {
    let source = r#"
        import { Repository } from "submilli:git";
        import { writeText } from "submilli:fs";
        function main(): string {
            const repo = Repository.init("/repo");
            assert(repo.status().clean);
            writeText("/repo/hello.txt", "hello\n");
            assert(repo.status().entries.length === 1);
            repo.add(["hello.txt"]);
            const id = repo.commit("initial");
            assert(repo.status().clean);
            assert(repo.log().commits[0].message === "initial");
            assert(new TextDecoder().decode(repo.show("HEAD", "hello.txt")) === "hello\n");
            repo.createBranch("topic");
            repo.switchBranch("topic");
            assert(repo.status().branch === "topic");
            repo.addRemote("origin", "https://example.com/repo.git");
            assert(repo.remotes()[0].name === "origin");
            assert(Repository.open("/repo").branches().length === 2);
            return id;
        }
    "#;
    let vfs = Vfs::tempdir().unwrap();
    run_source(source, test_data(vfs.clone())).await;
    let native = std::process::Command::new("git")
        .args(["-C"])
        .arg(vfs.root().join("repo"))
        .args(["fsck", "--full"])
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
}

#[tokio::test]
async fn empty_commits_need_allow_empty() {
    let source = r#"
        import { Repository } from "submilli:git";
        import { writeText } from "submilli:fs";
        function refusal(commit: () => string): string {
            try { commit(); } catch (e: Error) { return e.message; }
            return "";
        }
        function main(): void {
            const repo = Repository.init("/repo");
            assert(refusal(() => repo.commit("root")).includes("no staged changes"));
            assert(refusal(() => repo.commit("root", {})).includes("no staged changes"));
            assert(refusal(() => repo.commit("root", { allowEmpty: undefined })).includes("no staged changes"));
            assert(repo.log().commits.length === 0);
            const root = repo.commit("root", { allowEmpty: true });
            assert(repo.ls("HEAD").length === 0);
            writeText("/repo/notes.md", "notes\n");
            repo.add(["notes.md"]);
            const notes = repo.commit("notes", { allowEmpty: false });
            assert(refusal(() => repo.commit("unchanged", { allowEmpty: false })).includes("no staged changes"));
            assert(refusal(() => repo.commit("  ", { allowEmpty: true })).includes("message must not be empty"));
            writeText("/repo/notes.md", "unstaged\n");
            const decision = repo.commit("decision: keep notes", { allowEmpty: true });
            assert(!repo.status().clean);
            assert(new TextDecoder().decode(repo.show("HEAD", "notes.md")) === "notes\n");
            const log = repo.log().commits;
            assert(log.length === 3);
            assert(log[0].id === decision && log[0].message === "decision: keep notes");
            assert(log[1].id === notes && log[2].id === root);
            assert(repo.diff({ mode: "refs", from: notes, to: decision }).patch === "");
        }
    "#;
    let vfs = Vfs::tempdir().unwrap();
    run_source(source, test_data(vfs.clone())).await;
    let root = vfs.root().join("repo");
    let tree = |revision: &str| native(&root, &["rev-parse", &format!("{revision}^{{tree}}")]);
    assert_eq!(tree("HEAD"), tree("HEAD~1"));
    assert_eq!(
        tree("HEAD~2"),
        native(&root, &["hash-object", "-t", "tree", "/dev/null"])
    );
    assert_eq!(
        native(&root, &["rev-list", "--max-parents=0", "HEAD"]),
        native(&root, &["rev-parse", "HEAD~2"])
    );
    native(&root, &["fsck", "--full"]);
}

#[test]
fn allow_empty_reads_only_a_boolean() {
    use serde_json::json;
    assert!(!super::allow_empty(&Value::Null).unwrap());
    assert!(!super::allow_empty(&json!({})).unwrap());
    assert!(!super::allow_empty(&json!({ "allowEmpty": null })).unwrap());
    assert!(!super::allow_empty(&json!({ "allowEmpty": false })).unwrap());
    assert!(super::allow_empty(&json!({ "allowEmpty": true })).unwrap());
    let error = super::allow_empty(&json!({ "allowEmpty": "yes" })).unwrap_err();
    assert_eq!(
        error.to_string(),
        "git.commit: options.allowEmpty must be a boolean"
    );
}

#[tokio::test]
async fn allow_empty_commits_still_need_the_commit_grant() {
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.init", "git.open", "git.read", "git.write"],
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.init("/repo");
            let denied = "";
            try { repo.commit("root", { allowEmpty: true }); }
            catch (e: PermissionDeniedError) { denied = e.capability + "|" + e.reason; }
            assert(denied === "git.commit|not granted");
            assert(repo.log().commits.length === 0);
        }
    "#,
        data,
    )
    .await;
}

#[tokio::test]
async fn list_and_file_history_use_the_commit_not_the_worktree() {
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import { mkdir, remove, writeText } from "submilli:fs";
        function paths(entries: { path: string; kind: string }[]): string {
            return entries.map((entry) => entry.kind + ":" + entry.path).join(",");
        }
        function main(): void {
            const repo = Repository.init("/repo");
            mkdir("/repo/notes");
            writeText("/repo/notes/a.md", "one\n");
            writeText("/repo/notes/b.md", "keep\n");
            writeText("/repo/other.txt", "other\n");
            repo.add(["."]);
            const first = repo.commit("first");
            writeText("/repo/notes/a.md", "two\n");
            repo.add(["notes/a.md"]);
            const second = repo.commit("second");
            writeText("/repo/other.txt", "changed\n");
            repo.add(["other.txt"]);
            repo.commit("third");
            remove("/repo/notes/a.md");
            repo.add(["notes/a.md"]);
            repo.commit("delete a");
            writeText("/repo/notes/staged.md", "staged\n");
            repo.add(["notes/staged.md"]);
            writeText("/repo/notes/dirty.md", "dirty\n");

            assert(paths(repo.ls(first, "notes")) === "file:notes/a.md,file:notes/b.md");
            assert(paths(repo.ls(first)) === "directory:notes,file:other.txt");
            assert(new TextDecoder().decode(repo.show(first, "notes/a.md")) === "one\n");
            assert(paths(repo.ls("HEAD", "notes")) === "file:notes/b.md", "deleted, staged, and dirty files stay out");
            const atHead = repo.log({ path: "notes/a.md", limit: 1 }).commits[0].message;
            assert(atHead === "delete a");
            assert(repo.log({ path: "notes/a.md", ref: second, limit: 1 }).commits[0].id === second);
            assert(repo.log({ path: "notes/a.md", ref: first, limit: 1 }).commits[0].id === first);
            const filtered = repo.log({ path: "notes/a.md" }).commits.map((commit) => commit.message).join(",");
            assert(filtered === "delete a,second,first");
            assert(repo.log().commits[0].message === "delete a");
            assert(repo.log({ limit: 1, offset: 1 }).commits[0].message === "third");
        }
        "#,
        test_data(Vfs::tempdir().unwrap()),
    )
    .await;
}

#[tokio::test]
async fn path_filtered_log_skips_a_merge_that_did_not_change_the_file() {
    let vfs = Vfs::tempdir().unwrap();
    let repo = vfs.root().join("repo");
    std::fs::create_dir(&repo).unwrap();
    native(&repo, &["init", "-b", "main"]);
    native(&repo, &["config", "user.name", "Agent"]);
    native(&repo, &["config", "user.email", "agent@example.com"]);
    std::fs::create_dir(repo.join("notes")).unwrap();
    std::fs::write(repo.join("notes/a.md"), "one\n").unwrap();
    std::fs::write(repo.join("notes/b.md"), "keep\n").unwrap();
    native(&repo, &["add", "."]);
    native(&repo, &["commit", "-m", "first"]);
    std::fs::write(repo.join("notes/a.md"), "two\n").unwrap();
    native(&repo, &["commit", "-am", "second"]);
    native(&repo, &["checkout", "-b", "side", "HEAD~1"]);
    std::fs::write(repo.join("notes/b.md"), "side\n").unwrap();
    native(&repo, &["commit", "-am", "side"]);
    native(&repo, &["checkout", "main"]);
    native(&repo, &["merge", "--no-ff", "side", "-m", "merge"]);
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function messages(path: string): string {
            return Repository.open("/repo").log({ path, limit: 10 }).commits.map((commit) => commit.message).join(",");
        }
        function main(): void {
            assert(messages("notes/a.md") === "second\n,first\n");
            assert(messages("notes/b.md") === "side\n,first\n");
            assert(Repository.open("/repo").log({ path: "notes/a.md", limit: 1 }).commits[0].message === "second\n");
        }
        "#,
        test_data(vfs),
    )
    .await;
}

#[tokio::test]
async fn file_history_keeps_a_shallow_boundary_and_an_explicit_ref_without_head() {
    let vfs = Vfs::tempdir().unwrap();
    let repo = vfs.root().join("repo");
    std::fs::create_dir(&repo).unwrap();
    native(&repo, &["init", "-b", "main"]);
    native(&repo, &["config", "user.name", "Agent"]);
    native(&repo, &["config", "user.email", "agent@example.com"]);
    std::fs::write(repo.join("README"), "notes\n").unwrap();
    native(&repo, &["add", "."]);
    native(&repo, &["commit", "-m", "notes"]);
    let id = String::from_utf8(native(&repo, &["rev-parse", "HEAD"])).unwrap();
    std::fs::write(repo.join(".git/shallow"), &id).unwrap();
    native(&repo, &["checkout", "--orphan", "temp"]);
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            assert(repo.log().commits.length === 0, "the orphan branch has no commits");
            const kept = repo.log({ path: "README", ref: "main", limit: 1 });
            assert(kept.commits.length === 1);
            assert(kept.commits[0].message === "notes\n");
            assert(repo.ls("main").length === 1);
        }
        "#,
        test_data(vfs),
    )
    .await;
}

#[tokio::test]
async fn undefined_method_completion_and_optional_options() {
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import { writeText } from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo", undefined);
            writeText("/repo/hello.txt", "hello\n");
            assert(Object.is(repo.add(["hello.txt"]), undefined));
            repo.commit("initial");
            assert(Object.is(repo.createBranch("topic", undefined), undefined));
            assert(Object.is(repo.switchBranch("topic"), undefined));
            assert(repo.status().branch === "topic");
            assert(Object.is(repo.addRemote("origin", "https://example.com/old.git"), undefined));
            const changeUrl = () => repo.setRemoteUrl("origin", "https://example.com/new.git");
            assert(Object.is(changeUrl(), undefined));
            assert(repo.remotes()[0].url === "https://example.com/new.git");
            assert(repo.log(undefined).commits[0].message === "initial");
        }
        "#,
        test_data(Vfs::tempdir().unwrap()),
    )
    .await;
}

#[tokio::test]
async fn repository_class_construction_and_dispatch() {
    run_source(
        r#"
        import { Repository } from "submilli:git";
        class Checkout extends Repository {
            constructor(path: string) { super(path); }
            clean(): boolean { return this.status().clean; }
        }
        function optionalClean(repo: Repository | null | undefined): boolean | undefined {
            return repo?.status?.().clean;
        }
        function main(): void {
            const created: Repository = Repository.init("/repo");
            assert(created instanceof Repository);
            const opened = new Repository("/repo");
            assert(opened instanceof Repository);
            assert(opened.status?.().clean === true);
            assert(optionalClean(opened) === true);
            assert(optionalClean(null) === undefined);
            assert(optionalClean(undefined) === undefined);
            const status = () => opened.status();
            assert(status().clean);
            const checkout = new Checkout("/repo");
            assert(checkout instanceof Repository);
            assert(checkout instanceof Checkout);
            assert(checkout.clean());
            assert(checkout.status?.().clean === true);
            const nativeBase: Repository = checkout;
            assert(nativeBase.status?.().clean === true);
            assert(optionalClean(nativeBase) === true);
            let rejected = false;
            try { new Repository("/missing"); } catch (error) { rejected = true; }
            assert(rejected);
            assert(JSON.stringify(created) === "{}", "host repository omits private path");
            assert(JSON.stringify(created) === JSON.stringify(checkout), "guest subclass preserves privacy");
            const restored: unknown = JSON.parse(JSON.stringify(created));
            assert(!(restored instanceof Repository));
            const forged: unknown = { path: "/repo" };
            assert(!(forged instanceof Repository));
        }
        "#,
        test_data(Vfs::tempdir().unwrap()),
    )
    .await;
}

fn test_data(vfs: Vfs) -> StoreData {
    let mut data = StoreData::with_vfs("test-run".into(), vfs);
    data.git = Some(GitConfig {
        name: "Agent".into(),
        email: "agent@example.com".into(),
        username: Some("agent".into()),
    });
    data
}

/// Runs `source` with `fuel` to spend, returning how it ended and the host
/// fuel it was charged.
async fn run_program(source: &str, mut data: StoreData, fuel: u64) -> (wasmtime::Result<()>, u64) {
    let compiled = crate::compile_script(source, "git.ts", crate::FileId(0), &[], &[])
        .unwrap_or_else(|error| panic!("{error:#?}"));
    let cfg = RuntimeConfig {
        fuel,
        ..RuntimeConfig::default()
    };
    let engine = cfg.engine().unwrap();
    data.install_type_info(compiled.type_info.clone());
    let mut store = cfg.store(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    let outcome = dispatch_main_async(&mut store, &instance).await.map(drop);
    if store.data().git_history.is_some() {
        assert_eq!(
            store.data().tenant_limits.host_attached_bytes(),
            4 * 1024 * 1024
        );
        store.data_mut().git_history = None;
    }
    assert_eq!(store.data().tenant_limits.host_attached_bytes(), 0);
    (outcome, store.data().host_fuel)
}

/// Runs `source` to completion, returning the host fuel it was charged.
async fn host_fuel_of(source: &str, data: StoreData) -> u64 {
    let (outcome, host_fuel) = run_program(source, data, RuntimeConfig::default().fuel).await;
    outcome.unwrap();
    host_fuel
}

async fn run_source_fuel(source: &str, data: StoreData, fuel: Option<u64>) -> Result<()> {
    run_program(source, data, fuel.unwrap_or(RuntimeConfig::default().fuel))
        .await
        .0
}

async fn run_source(source: &str, data: StoreData) {
    run_program(source, data, RuntimeConfig::default().fuel)
        .await
        .0
        .unwrap();
}

#[tokio::test]
async fn ignored_files_and_metadata_protection() {
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            fs.writeText("/repo/.gitignore", "*.tmp\n");
            fs.writeText("/repo/ignored.tmp", "private");
            fs.writeText("/repo/kept", "kept");
            assert(repo.status().entries.length === 2);
            repo.add(["."]);
            repo.commit("initial");
            assert(repo.status().clean);
            let refused = false;
            try { fs.writeText("/repo/.git/HEAD", "corrupt"); } catch (e: Error) { refused = true; }
            assert(refused);
            refused = false;
            try { fs.remove("/repo", { recursive: true }); } catch (e: Error) { refused = true; }
            assert(refused);
            repo.createBranch("topic");
            refused = false;
            try { repo.switchBranch("topic"); } catch (e: Error) { refused = true; }
            assert(refused, "ignored local files are preserved by clean-only checkout");
            assert(fs.readText("/repo/ignored.tmp") === "private");
        }
    "#,
        test_data(Vfs::tempdir().unwrap()),
    )
    .await;
}

fn native(repo: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[tokio::test]
async fn remote_url_replacement_preserves_native_configuration() {
    let vfs = Vfs::tempdir().unwrap();
    let repo = vfs.root().join("repo");
    std::fs::create_dir(&repo).unwrap();
    native(&repo, &["init", "-b", "main"]);
    let config = repo.join(".git/config");
    let mut original = std::fs::read_to_string(&config).unwrap();
    original.push_str("\n# Operator settings\n[remote \"origin\"]\nurl = https://example.com/old.git\nfetch = +refs/heads/main:refs/remotes/origin/main\npushurl = https://example.com/push.git\n[remote \"origin\"]\nurl = https://example.com/another.git\n[custom]\nvalue = preserved\n");
    std::fs::write(&config, original).unwrap();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            repo.setRemoteUrl("origin", "https://example.com/new.git");
            assert(repo.remotes()[0].url === "https://example.com/new.git");
        }
    "#,
        test_data(vfs.clone()),
    )
    .await;
    assert_eq!(
        native(&repo, &["remote", "get-url", "origin"]),
        b"https://example.com/new.git\n"
    );
    assert_eq!(
        native(&repo, &["config", "--get-all", "remote.origin.url"]),
        b"https://example.com/new.git\n"
    );
    assert_eq!(
        native(&repo, &["config", "--get-all", "remote.origin.fetch"]),
        b"+refs/heads/main:refs/remotes/origin/main\n"
    );
    assert_eq!(
        native(&repo, &["config", "--get", "remote.origin.pushurl"]),
        b"https://example.com/push.git\n"
    );
    assert_eq!(
        native(&repo, &["config", "--get", "custom.value"]),
        b"preserved\n"
    );
    assert!(
        std::fs::read_to_string(config)
            .unwrap()
            .contains("# Operator settings")
    );
}

#[cfg(unix)]
#[test]
fn patches_apply_file_lifecycle_and_mode_changes_with_native_git() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path();
    native(repo, &["init", "-b", "main"]);
    native(repo, &["config", "user.name", "Agent"]);
    native(repo, &["config", "user.email", "agent@example.com"]);
    for (name, text) in [
        ("deleted", "old\n"),
        ("deleted-empty", ""),
        ("executable", "script\n"),
        ("type-change", "target"),
        ("modified", "before\r\n"),
    ] {
        std::fs::write(repo.join(name), text).unwrap();
    }
    native(repo, &["add", "."]);
    native(repo, &["commit", "-m", "before"]);
    std::fs::remove_file(repo.join("deleted")).unwrap();
    std::fs::remove_file(repo.join("deleted-empty")).unwrap();
    std::fs::write(repo.join("added"), "new without newline").unwrap();
    std::fs::write(repo.join("added-empty"), "").unwrap();
    std::fs::write(repo.join("added-é"), "quoted path\n").unwrap();
    std::fs::write(repo.join("modified"), "after\r\n").unwrap();
    std::fs::set_permissions(
        repo.join("executable"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::remove_file(repo.join("type-change")).unwrap();
    symlink("target", repo.join("type-change")).unwrap();
    native(repo, &["add", "."]);
    let expected_tree = native(repo, &["write-tree"]);
    let snapshot = storage::Snapshot::open_unmetered(
        &location::Location::at(repo),
        Arc::new(AtomicBool::new(false)),
        storage::MAX_WORKING_BYTES,
        false,
    )
    .unwrap();
    let diff = operations::read(&snapshot, "diff", &[json!({"mode":"staged"})]).unwrap();
    let patch = directory.path().join(".git/review.patch");
    std::fs::write(&patch, diff["patch"].as_str().unwrap()).unwrap();
    native(repo, &["reset", "--hard", "HEAD"]);
    native(
        repo,
        &["apply", "--check", "--index", patch.to_str().unwrap()],
    );
    native(repo, &["apply", "--index", patch.to_str().unwrap()]);
    assert_eq!(native(repo, &["write-tree"]), expected_tree);
}

#[test]
fn checkout_rejects_case_folded_file_directory_aliases() {
    for paths in [["A", "a/HEAD"], ["A/x", "a/y"], ["A/X", "a/x"]] {
        let files = storage::Files::from([
            (paths[0].into(), (0o120000, b".git".to_vec())),
            (paths[1].into(), (0o100644, b"replacement".to_vec())),
        ]);
        assert!(operations::validate_file_set(&files).is_err(), "{paths:?}");
    }
}

struct GitServer {
    repo: std::path::PathBuf,
    authentication: bool,
    oversized_response: bool,
}

#[async_trait::async_trait]
impl HttpClient for GitServer {
    async fn send(
        &self,
        _: &crate::stdlib::http::transport::HttpRequest,
    ) -> std::result::Result<
        crate::stdlib::http::transport::HttpResponse,
        crate::stdlib::http::transport::HttpError,
    > {
        panic!("Git must not use a redirect-following transport")
    }
    async fn send_without_redirects_to(
        &self,
        request: &crate::stdlib::http::transport::HttpRequest,
        body_out: &mut (dyn std::io::Write + Send),
    ) -> std::result::Result<
        crate::stdlib::http::transport::HttpResponse,
        crate::stdlib::http::transport::HttpError,
    > {
        use std::io::Write;
        if self.authentication {
            let auth = request
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("Authorization"));
            if let Some((_, value)) = auth {
                assert_eq!(value, "Basic YWdlbnQ6aG9zdC1vbmx5LXRva2Vu");
            } else {
                return Ok(crate::stdlib::http::transport::HttpResponse {
                    status: 401,
                    status_text: "Unauthorized".into(),
                    headers: vec![("www-authenticate".into(), "Basic realm=git".into())],
                    body: vec![],
                    final_url: request.url.clone(),
                });
            }
        }
        let (body, content_type) = if request.method == "GET" {
            let mut body = b"001e# service=git-upload-pack\n0000".to_vec();
            body.extend(native(
                &self.repo,
                &["upload-pack", "--stateless-rpc", "--advertise-refs", "."],
            ));
            if self.oversized_response {
                body.resize(4 * 1024 * 1024, b'x');
            }
            (body, "application/x-git-upload-pack-advertisement")
        } else {
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["upload-pack", "--stateless-rpc", "."])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&request.body)
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            (output.stdout, "application/x-git-upload-pack-result")
        };
        body_out.write_all(&body).unwrap();
        Ok(crate::stdlib::http::transport::HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), content_type.into())],
            body: Vec::new(),
            final_url: request.url.clone(),
        })
    }
    async fn download(
        &self,
        _: &crate::stdlib::http::transport::HttpRequest,
        _: &mut (dyn std::io::Write + Send),
    ) -> std::result::Result<
        crate::stdlib::http::transport::DownloadMeta,
        crate::stdlib::http::transport::HttpError,
    > {
        unreachable!()
    }
}

#[tokio::test]
async fn smart_http_clone_and_fast_forward_pull() {
    let upstream = tempfile::tempdir().unwrap();
    native(upstream.path(), &["init", "-b", "trunk"]);
    native(upstream.path(), &["config", "user.name", "Upstream"]);
    native(
        upstream.path(),
        &["config", "user.email", "upstream@example.com"],
    );
    std::fs::write(upstream.path().join("hello"), "one").unwrap();
    native(upstream.path(), &["add", "."]);
    native(upstream.path(), &["commit", "-m", "one"]);
    let http = Arc::new(GitServer {
        repo: upstream.path().to_owned(),
        authentication: false,
        oversized_response: false,
    });
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.clone", "git.read"],
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.clone("https://example.com/repo.git", "/repo");
            assert(repo.status().clean);
            assert(repo.status().branch === "trunk");
            assert(repo.log().commits[0].message === "one\n");
        }
    "#,
        data,
    )
    .await;
    std::fs::write(upstream.path().join("hello"), "two").unwrap();
    native(upstream.path(), &["commit", "-am", "two"]);
    let mut data = test_data(vfs.clone());
    data.http_client = http;
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.fetch", "git.open", "git.read"],
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            repo.fetch(undefined, undefined);
            const update = repo.pull(undefined, undefined);
            assert(update.previous !== update.current);
            assert(repo.status().clean);
            assert(repo.status().branch === "trunk");
            assert(new TextDecoder().decode(repo.show("HEAD", "hello")) === "two");
        }
    "#,
        data,
    )
    .await;
    native(&vfs.root().join("repo"), &["fsck", "--full"]);
}

struct Token(std::sync::atomic::AtomicU64);
impl SecretProvider for Token {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<Option<String>, String>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            assert_eq!(name, "GIT_TOKEN");
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Some("host-only-token".into()))
        })
    }
}

struct GitGrants {
    allowed: &'static [&'static str],
}
impl SecurityCheck for GitGrants {
    fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
        assert_eq!(caller, "main");
        if self.allowed.contains(&capability) && context["path"] == "/repo" {
            CheckOutcome::Allow { rule: None }
        } else {
            CheckOutcome::Deny {
                rule: None,
                reason: "not granted".into(),
            }
        }
    }
}

#[tokio::test]
async fn clone_grant_includes_authentication_without_other_grants() {
    let upstream = tempfile::tempdir().unwrap();
    native(upstream.path(), &["init", "-b", "main"]);
    native(upstream.path(), &["config", "user.name", "Upstream"]);
    native(
        upstream.path(),
        &["config", "user.email", "upstream@example.com"],
    );
    std::fs::write(upstream.path().join("hello"), "one").unwrap();
    native(upstream.path(), &["add", "."]);
    native(upstream.path(), &["commit", "-m", "one"]);
    let http = Arc::new(GitServer {
        repo: upstream.path().to_owned(),
        authentication: true,
        oversized_response: false,
    });
    let secret = Arc::new(Token(std::sync::atomic::AtomicU64::new(0)));
    let mut data = test_data(Vfs::tempdir().unwrap());
    data.http_client = http.clone();
    data.secret_provider = secret.clone();
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.fetch"],
    });
    let denied_vfs = data.vfs.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            let denied = false;
            try { Repository.clone("https://example.com/repo.git", "/repo"); }
            catch (e: PermissionDeniedError) { denied = true; }
            assert(denied);
        }
    "#,
        data,
    )
    .await;
    assert_eq!(secret.0.load(Ordering::Relaxed), 0);
    assert!(!denied_vfs.root().join("repo/.git").exists());
    let mut data = test_data(Vfs::tempdir().unwrap());
    data.http_client = http;
    data.secret_provider = secret.clone();
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.clone"],
    });
    let cloned = data.vfs.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.clone("https://example.com/repo.git", "/repo");
            let readDenied = "";
            try { repo.status(); }
            catch (e: PermissionDeniedError) {
                readDenied = e.caller + "|" + e.capability + "|" + e.reason;
            }
            if (readDenied !== "main|git.read|not granted") throw new Error("read after clone: " + readDenied);
            let fetchDenied = false;
            try { repo.fetch(); }
            catch (e: PermissionDeniedError) { fetchDenied = true; }
            assert(fetchDenied);
            let initDenied = false;
            try { Repository.init("/repo"); }
            catch (e: PermissionDeniedError) { initDenied = true; }
            assert(initDenied);
        }
    "#,
        data,
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(cloned.root().join("repo/hello")).unwrap(),
        "one"
    );
    assert!(secret.0.load(Ordering::Relaxed) > 0);
}

#[tokio::test]
async fn init_and_commit_remain_separate_from_repository_access() {
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.init"],
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            Repository.init("/repo");
            let denied = false;
            try { Repository.init("/elsewhere"); }
            catch (e: PermissionDeniedError) { denied = true; }
            assert(denied);
        }
    "#,
        data,
    )
    .await;
    assert!(!vfs.root().join("elsewhere").exists());
    let root = vfs.root().join("repo");
    native(&root, &["config", "user.name", "Native"]);
    native(&root, &["config", "user.email", "native@example.com"]);
    std::fs::write(root.join("hello"), "one").unwrap();
    native(&root, &["add", "."]);
    native(&root, &["commit", "-m", "one"]);
    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.open", "git.read", "git.write"],
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            assert(repo.status().clean);
            assert(repo.log().commits.length === 1);
            repo.diff();
            repo.show("HEAD", "hello");
            repo.ls("HEAD", "");
            repo.createBranch("topic");
            repo.switchBranch("topic");
            assert(repo.branches().length === 2);
            repo.addRemote("origin", "https://example.com/repo.git");
            repo.setRemoteUrl("origin", "https://example.com/other.git");
            assert(repo.remotes().length === 1);
            repo.add(["hello"]);
            let denied = false;
            try { repo.commit("denied"); }
            catch (e: PermissionDeniedError) { denied = true; }
            assert(denied);
        }
    "#,
        data,
    )
    .await;
    std::fs::write(root.join("hello"), "two").unwrap();
    native(&root, &["add", "hello"]);
    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(GitGrants {
        allowed: &["git.open", "git.commit"],
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            let writeDenied = "";
            try { repo.add(["hello"]); }
            catch (e: PermissionDeniedError) {
                writeDenied = e.caller + "|" + e.capability + "|" + e.reason;
            }
            if (writeDenied !== "main|git.write|not granted") throw new Error("stage before commit: " + writeDenied);
            repo.commit("two");
        }
    "#,
        data,
    )
    .await;
    let committed = String::from_utf8(native(&root, &["log", "-1", "--format=%s"])).unwrap();
    assert_eq!(committed.trim(), "two");
}

const MEMORY_PACKAGE: &str = "@submilli/memory";

struct MemoryPolicy;

impl SecurityCheck for MemoryPolicy {
    fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
        let path = context.get("path").and_then(Value::as_str).unwrap_or("");
        let allowed = caller == MEMORY_PACKAGE
            && path == "/memory"
            && matches!(
                capability,
                "git.open" | "git.read" | "git.write" | "git.init" | "git.commit"
            );
        if allowed {
            CheckOutcome::Allow { rule: None }
        } else {
            CheckOutcome::Deny {
                rule: None,
                reason: "not granted".into(),
            }
        }
    }
}

#[tokio::test]
async fn repository_access_follows_the_current_caller_and_path() {
    let vfs = Vfs::tempdir().unwrap();
    let memory = vfs.root().join("memory");
    std::fs::create_dir(&memory).unwrap();
    native(&memory, &["init", "-q", "-b", "main"]);
    native(&memory, &["config", "user.name", "Native"]);
    native(&memory, &["config", "user.email", "native@example.com"]);
    std::fs::write(memory.join("hello"), "one").unwrap();
    native(&memory, &["add", "."]);
    native(&memory, &["commit", "-m", "one"]);

    let package = crate::compile_package(
        MEMORY_PACKAGE,
        crate::ModulePath::from("lib"),
        &[crate::PackageSourceModule {
            path: crate::ModulePath::from("lib"),
            source: r#"
                import { Repository } from "submilli:git";
                export function open(path: string): Repository {
                    return Repository.open(path);
                }
                export function deniedOpen(path: string): string {
                    try { Repository.open(path); return "allowed"; }
                    catch (error: PermissionDeniedError) {
                        return error.caller + "|" + error.capability + "|" + error.reason + "|" + error.message;
                    }
                }
                export function inspect(repo: Repository): string {
                    const status = repo.status();
                    const history = repo.log({ limit: 1 });
                    const diff = repo.diff();
                    const shown = new TextDecoder().decode(repo.show("HEAD", "hello"));
                    const listed = repo.ls("HEAD", "");
                    return status.clean + ":" + history.commits.length + ":" + diff.patch.length
                        + ":" + shown + ":" + listed.length + ":" + repo.branches().length
                        + ":" + repo.remotes().length;
                }
                export function change(repo: Repository): void {
                    repo.add(["hello"]);
                    repo.createBranch("topic");
                    repo.switchBranch("topic");
                    repo.addRemote("origin", "https://example.com/repo.git");
                    repo.setRemoteUrl("origin", "https://example.com/other.git");
                }
                export function branchCount(repo: Repository): number {
                    return repo.branches().length;
                }
            "#,
        }],
        &[],
    )
    .unwrap_or_else(|error| panic!("{error:#?}"));

    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(MemoryPolicy);
    run_with_package(
        r#"
        import { Repository } from "submilli:git";
        import { open, deniedOpen, inspect, change, branchCount } from "@submilli/memory";
        function denial(path: string): string {
            try { Repository.open(path); return "allowed"; }
            catch (error: PermissionDeniedError) {
                return error.caller + "|" + error.capability + "|" + error.reason + "|" + error.message;
            }
        }
        function main(): void {
            const missing = denial("/missing");
            const existing = denial("/memory");
            let constructed = "";
            try { new Repository("/memory"); constructed = "allowed"; }
            catch (error: PermissionDeniedError) {
                constructed = error.caller + "|" + error.capability + "|" + error.reason + "|" + error.message;
            }
            let constructedMissing = "";
            try { new Repository("/absent"); constructedMissing = "allowed"; }
            catch (error: PermissionDeniedError) {
                constructedMissing = error.caller + "|" + error.capability + "|" + error.reason + "|" + error.message;
            }
            if (missing !== existing) throw new Error("open errors differ: [" + missing + "] [" + existing + "]");
            if (constructed !== existing) throw new Error("constructor error differs: [" + constructed + "]");
            if (constructedMissing !== missing) throw new Error("absent constructor differs: [" + constructedMissing + "]");
            if (missing.indexOf("main|git.open|not granted|") !== 0) throw new Error("main denial: " + missing);
            if (missing.indexOf("not found") >= 0 || missing.indexOf("does not exist") >= 0) {
                throw new Error("denial reveals existence: " + missing);
            }

            const packageMissing = deniedOpen("/missing");
            if (packageMissing.indexOf("@submilli/memory|git.open|not granted|") !== 0) throw new Error("package denial: " + packageMissing);
            if (packageMissing !== deniedOpen("/other")) throw new Error("package path errors differ: " + deniedOpen("/other"));

            const repo = open("/memory");
            assert(open("memory") instanceof Repository);
            function denied(capability: string, call: () => void): void {
                try { call(); throw new Error("allowed " + capability); }
                catch (error: PermissionDeniedError) {
                    const actual = error.caller + "|" + error.capability + "|" + error.reason;
                    if (actual !== "main|" + capability + "|not granted") throw new Error(capability + ": " + actual);
                }
            }
            denied("git.read", () => { repo.status(); });
            denied("git.read", () => { repo.log(); });
            denied("git.read", () => { repo.diff(); });
            denied("git.read", () => { repo.show("HEAD", "hello"); });
            denied("git.read", () => { repo.ls("HEAD", ""); });
            denied("git.read", () => { repo.branches(); });
            denied("git.read", () => { repo.remotes(); });
            denied("git.write", () => { repo.add(["hello"]); });
            denied("git.write", () => { repo.createBranch("smuggled"); });
            denied("git.write", () => { repo.switchBranch("topic"); });
            denied("git.write", () => { repo.addRemote("origin", "https://example.com/repo.git"); });
            denied("git.write", () => { repo.setRemoteUrl("origin", "https://example.com/other.git"); });

            const report = inspect(repo);
            if (report !== "true:1:0:one:1:1:0") throw new Error("inspect: " + report);
            change(repo);
            if (branchCount(repo) !== 2) throw new Error("branches: " + branchCount(repo));
        }
        "#,
        &package,
        data,
    )
    .await;
    assert!(!vfs.root().join("missing").exists());
    assert!(!vfs.root().join("absent").exists());
    assert!(!vfs.root().join("other").exists());
    native(&memory, &["rev-parse", "--verify", "topic"]);
}

async fn run_with_package(source: &str, package: &crate::CompiledPackage, mut data: StoreData) {
    let compiled = crate::compile_script(
        source,
        "git.ts",
        crate::FileId(0),
        &[&package.declaration],
        &[],
    )
    .unwrap_or_else(|error| panic!("{error:#?}"));
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().unwrap();
    data.install_type_info(compiled.type_info.clone());
    let mut store = cfg.store(&engine, data).unwrap();
    let script = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let package_module = wasmtime::Module::new(&engine, &package.wasm).unwrap();
    let mut linker = Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let linked = [crate::runtime::LinkedPackageModule {
        module: &package_module,
        declaration: &package.declaration,
        type_info: &package.type_info,
    }];
    crate::runtime::install_package_modules_async(&mut linker, &mut store, &linked)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &script).await.unwrap();
    dispatch_main_async(&mut store, &instance).await.unwrap();
}

#[test]
fn unsafe_urls_and_metadata_paths_are_rejected() {
    for url in [
        "http://example.com/repo",
        "ssh://example.com/repo",
        "file:///tmp/repo",
        "https://agent:token@example.com/repo",
        "https://example.com/repo?token=secret",
        "https://example.com/repo#fragment",
    ] {
        assert!(transport::validate_url(url).is_err(), "{url}");
    }
    for path in [
        ".git/HEAD",
        ".GiT/config",
        "../outside",
        "/absolute",
        "a/../../outside",
        "git~1/HEAD",
        "a/.git/config",
    ] {
        assert!(storage::validate_path(path).is_err(), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn metadata_writes_through_aliases_and_hardlinks_are_refused() {
    use crate::runtime::fs::{resolve_content, resolve_link};
    let vfs = Vfs::tempdir().unwrap();
    let root = vfs.dir().unwrap();
    for alias in ["git~1", ".git ", ".g\u{200c}it"] {
        assert!(
            resolve_content(&vfs, "/", alias)
                .unwrap()
                .create_dir()
                .is_err(),
            "{alias}"
        );
    }
    root.create_dir_all("repo/.git").unwrap();
    root.write("repo/.git/HEAD", "protected").unwrap();
    root.symlink("repo/.git", "alias").unwrap();
    root.hard_link("repo/.git/HEAD", root, "hardlink").unwrap();
    assert!(
        resolve_content(&vfs, "/", "/alias/HEAD")
            .unwrap()
            .create()
            .is_err()
    );
    assert!(
        resolve_content(&vfs, "/", "/alias/new")
            .unwrap()
            .create()
            .is_err()
    );
    assert!(
        resolve_content(&vfs, "/", "/hardlink")
            .unwrap()
            .append()
            .is_err()
    );
    assert!(
        resolve_link(&vfs, "/", "/repo")
            .unwrap()
            .remove_dir_all()
            .is_err()
    );
    assert_eq!(root.read("repo/.git/HEAD").unwrap(), b"protected");
}

#[tokio::test]
async fn expired_deadline_does_not_start_ready_work() {
    let polled = AtomicBool::new(false);
    let result = before_deadline(tokio::time::Instant::now(), async {
        polled.store(true, Ordering::Relaxed);
    })
    .await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("operation timed out")
    );
    assert!(!polled.load(Ordering::Relaxed));
}

#[tokio::test]
async fn timeout_waits_for_worker_cleanup_and_resource_release() {
    let vfs = Vfs::tempdir().unwrap();
    let data = test_data(vfs.clone());
    let budget = WorkingBudget::reserve(&data.tenant_limits, "status").unwrap();
    let cancelled = AtomicBool::new(false);
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finish, cleanup) = std::sync::mpsc::channel();
    let mut workers = crate::runtime::blocking::BlockingWork::default();
    let mut worker = Box::pin(workers.spawn(move || {
        started.send(()).unwrap();
        // Hold resources until the test permits cleanup, even after timeout.
        cleanup
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok(((), budget))
    }));
    tokio::select! {
        _ = &mut worker => panic!("worker finished before cleanup"),
        result = ready => result.unwrap(),
    }

    let worker = async { worker.await.map_err(blocking_worker_failure)? };
    let result = finish_worker(worker, &cancelled, tokio::time::Instant::now());
    tokio::pin!(result);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut result)
            .await
            .is_err(),
        "the timeout must not return before worker cleanup"
    );
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(data.tenant_limits.host_attached_bytes() > 0);
    finish.send(()).unwrap();
    let Err(error) = result.await else {
        panic!("expected timeout");
    };
    assert!(error.to_string().contains("operation timed out"));
    assert_eq!(data.tenant_limits.host_attached_bytes(), 0);
}

#[tokio::test]
async fn cancellation_keeps_resources_until_worker_cleanup() {
    let vfs = Vfs::tempdir().unwrap();
    let root = vfs.root().to_owned();
    let data = test_data(vfs.clone());
    let budget = WorkingBudget::reserve(&data.tenant_limits, "status").unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let guard = CancelOnDrop(cancelled.clone());
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finish, cleanup) = std::sync::mpsc::channel();
    let mut workers = crate::runtime::blocking::BlockingWork::default();
    let mut worker = Box::pin(workers.spawn(move || {
        let _budget = budget;
        let _vfs = vfs;
        started.send(()).unwrap();
        cleanup
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        Ok::<_, wasmtime::Error>(())
    }));
    tokio::select! {
        _ = &mut worker => panic!("worker finished before cleanup"),
        result = ready => result.unwrap(),
    }

    drop(worker);
    drop(guard);
    assert!(cancelled.load(Ordering::Relaxed));
    let cleanup = workers.finish();
    tokio::pin!(cleanup);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut cleanup)
            .await
            .is_err()
    );
    assert!(root.exists());
    assert!(data.tenant_limits.host_attached_bytes() > 0);
    finish.send(()).unwrap();
    cleanup.await.unwrap();
    assert_eq!(data.tenant_limits.host_attached_bytes(), 0);
    drop(data);
    assert!(!root.exists());
}

#[test]
fn repository_docs_expose_class_factories_and_capabilities() {
    let docs = crate::packages::docs(MODULE_NAME).unwrap();
    for declaration in [
        "class Repository",
        "constructor(path: string)",
        "static open(",
        "static init(",
        "static clone(",
        "@capability git.open",
        "@capability git.read",
        "@capability git.write",
        "@capability git.init",
        "@capability git.clone",
        "@capability git.fetch",
        "@capability git.push",
        "@capability git.commit",
        "push(",
    ] {
        assert!(
            docs.declarations.contains(declaration),
            "missing {declaration}: {}",
            docs.declarations
        );
    }
    assert!(!docs.declarations.contains("function init("));
}

/// A Git operation that needs more fuel than the run has stops before it
/// publishes, and the run ends in the runtime's own out-of-fuel trap, charged
/// no more than it had.
#[tokio::test]
async fn git_runs_out_of_fuel_like_any_other_work() {
    let source = r#"
        import { Repository } from "submilli:git";
        function main(): void {
            Repository.open("/repo").add(["."]);
        }
    "#;
    let repository = || {
        let vfs = Vfs::tempdir().unwrap();
        let repo = vfs.root().join("repo");
        std::fs::create_dir(&repo).unwrap();
        native(&repo, &["init", "-q", "-b", "main"]);
        for number in 0..2_000 {
            let path = repo.join(format!("d{:02}/f{number:05}", number % 50));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("file {number}\n")).unwrap();
        }
        vfs
    };
    let needed = host_fuel_of(source, test_data(repository())).await;
    // With a quarter of what it needs, `add` stops before it publishes.
    let vfs = repository();
    let (outcome, host_fuel) = run_program(source, test_data(vfs.clone()), needed / 4).await;
    let error = outcome.unwrap_err();
    assert_eq!(
        error.downcast_ref::<wasmtime::Trap>(),
        Some(&wasmtime::Trap::OutOfFuel),
        "{error:?}"
    );
    assert!(host_fuel <= needed / 4, "{host_fuel}");
    assert!(
        !vfs.root().join("repo/.git/index").exists(),
        "nothing was staged into the repository"
    );
}

/// A fetch pays for what it inflates, not for what it transfers: two packs
/// of about the same size on the wire cost what their contents do.
#[tokio::test]
async fn fetch_fuel_follows_what_the_pack_inflates_to() {
    let mut fuel = Vec::new();
    // Repetitive, and random bytes of about the same compressed size.
    let repetitive = vec![b'a'; 5_000_000];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let random: Vec<u8> = (0..8_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    for contents in [repetitive, random] {
        let upstream = tempfile::tempdir().unwrap();
        native(upstream.path(), &["init", "-q", "-b", "main"]);
        native(upstream.path(), &["config", "user.name", "Upstream"]);
        native(
            upstream.path(),
            &["config", "user.email", "upstream@example.com"],
        );
        std::fs::write(upstream.path().join("data"), &contents).unwrap();
        native(upstream.path(), &["add", "."]);
        native(upstream.path(), &["commit", "-q", "-m", "data"]);
        let vfs = Vfs::tempdir().unwrap();
        let mut data = test_data(vfs.clone());
        data.http_client = Arc::new(GitServer {
            repo: upstream.path().to_owned(),
            authentication: false,
            oversized_response: false,
        });
        fuel.push(
            host_fuel_of(
                r#"
                import { Repository } from "submilli:git";
                function main(): void {
                    const repo = Repository.init("/repo");
                    repo.addRemote("origin", "https://example.com/repo.git");
                    repo.fetch("origin", "main");
                }
            "#,
                data,
            )
            .await,
        );
    }
    assert!(
        fuel[0] > 10 * fuel[1],
        "5 MB inflated cost {}, 8 KB cost {}",
        fuel[0],
        fuel[1]
    );
}

#[tokio::test]
async fn timeout_drains_worker_and_preserves_invariant_trap() {
    let cancelled = AtomicBool::new(false);
    let cleaned = AtomicBool::new(false);
    let worker = async {
        tokio::task::yield_now().await;
        cleaned.store(true, Ordering::Relaxed);
        Err::<(), _>(crate::runtime::host::invariant_trap(
            "git: injected worker invariant",
        ))
    };
    let error = finish_worker(worker, &cancelled, tokio::time::Instant::now())
        .await
        .unwrap_err();
    assert!(error.is::<wasmtime::Trap>());
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(cleaned.load(Ordering::Relaxed));
    finish_worker(
        async { Ok(()) },
        &AtomicBool::new(false),
        tokio::time::Instant::now() + std::time::Duration::from_secs(1),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn worker_panic_bypasses_guest_catch_and_allows_same_store_follow_up() {
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    struct PanicOnce(AtomicBool);
    impl SecurityCheck for PanicOnce {
        fn check(&self, _: &str, _: &str, _: &Value) -> CheckOutcome {
            if self.0.swap(false, Ordering::SeqCst) {
                panic!("injected worker failure");
            }
            CheckOutcome::Allow { rule: None }
        }
    }
    let source = r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): number {
            try { Repository.init("/repo"); }
            catch (error) { fs.writeText("/caught", "must not run"); }
            return 42;
        }
    "#;
    let compiled = crate::compile_script(source, "git.ts", crate::FileId(0), &[], &[]).unwrap();
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(PanicOnce(AtomicBool::new(true)));
    data.install_type_info(compiled.type_info.clone());
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut store = config.store_async(&engine, data).unwrap();
    crate::runtime::install_tenant_limits(&mut store);
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    let error = dispatch_main_async(&mut store, &instance)
        .await
        .unwrap_err();
    assert!(
        error.is::<crate::runtime::host::FatalHostError>(),
        "{error:#}"
    );
    assert!(
        error.is::<crate::runtime::blocking::BlockingWorkError>(),
        "{error:#}"
    );
    assert!(error.to_string().contains("blocking worker panicked"));
    assert!(!vfs.root().join("caught").exists());
    store.data_mut().blocking_work.finish().await.unwrap();
    assert_eq!(store.data().tenant_limits.host_attached_bytes(), 0);
    assert_eq!(
        dispatch_main_async(&mut store, &instance).await.unwrap(),
        Some("42".into())
    );
    assert!(!vfs.root().join("caught").exists());
}

#[tokio::test]
async fn worker_panic_after_deadline_remains_fatal() {
    let mut workers = crate::runtime::blocking::BlockingWork::default();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let mut worker = Box::pin(async {
        workers
            .spawn(move || -> Result<()> {
                started.send(()).unwrap();
                gate.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                panic!("worker failed during timeout cleanup");
            })
            .await
            .map_err(blocking_worker_failure)?
    });
    tokio::select! {
        _ = &mut worker => panic!("worker ended early"),
        result = ready => result.unwrap(),
    }
    let cancelled = AtomicBool::new(false);
    let mut result = Box::pin(finish_worker(
        worker,
        &cancelled,
        tokio::time::Instant::now(),
    ));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut result)
            .await
            .is_err()
    );
    assert!(cancelled.load(Ordering::Relaxed));
    release.send(()).unwrap();
    let error = result.await.unwrap_err();
    assert!(crate::runtime::host::ends_the_run(&error));
    assert!(error.is::<crate::runtime::blocking::BlockingWorkError>());
    assert_eq!(workers.spawn(|| 42).await.unwrap(), 42);
}

#[tokio::test]
async fn failed_remote_conversion_can_exhaust_fuel_after_worker_settlement() {
    let vfs = Vfs::tempdir().unwrap();
    let repo = vfs.root().join("repo");
    std::fs::create_dir(&repo).unwrap();
    native(&repo, &["init", "-b", "main"]);
    native(
        &repo,
        &["remote", "add", "origin", "https://example.com/repo.git"],
    );
    let http = Arc::new(GitServer {
        repo,
        authentication: false,
        oversized_response: true,
    });
    let source = r#"
        import { Repository } from "submilli:git";
        function main(): void { Repository.open("/repo").fetch(); }
    "#;
    let mut errors = Vec::new();
    for fuel in [None, Some(50_000)] {
        let mut data = test_data(vfs.clone());
        data.http_client = http.clone();
        errors.push(run_source_fuel(source, data, fuel).await.unwrap_err());
    }
    assert!(
        errors[0].to_string().contains("Transport handshake failed"),
        "{}",
        errors[0]
    );
    // Exception construction is outside the effect and uses normal fuel checks.
    assert_eq!(
        errors[1].downcast_ref::<wasmtime::Trap>(),
        Some(&wasmtime::Trap::OutOfFuel)
    );
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void { assert(Repository.open("/repo").status().clean); }
    "#,
        test_data(vfs),
    )
    .await;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PushAuthMode {
    None,
    Always,
    PostOnly,
}

struct PushServer {
    repo: std::path::PathBuf,
    auth: PushAuthMode,
    get_count: std::sync::atomic::AtomicUsize,
    post_count: std::sync::atomic::AtomicUsize,
    post_401_count: std::sync::atomic::AtomicUsize,
}

impl PushServer {
    fn new(repo: std::path::PathBuf, auth: PushAuthMode) -> Self {
        Self {
            repo,
            auth,
            get_count: std::sync::atomic::AtomicUsize::new(0),
            post_count: std::sync::atomic::AtomicUsize::new(0),
            post_401_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl HttpClient for PushServer {
    async fn send(
        &self,
        _: &crate::stdlib::http::transport::HttpRequest,
    ) -> std::result::Result<
        crate::stdlib::http::transport::HttpResponse,
        crate::stdlib::http::transport::HttpError,
    > {
        panic!("Git must not use a redirect-following transport")
    }

    async fn send_without_redirects_to(
        &self,
        request: &crate::stdlib::http::transport::HttpRequest,
        body_out: &mut (dyn std::io::Write + Send),
    ) -> std::result::Result<
        crate::stdlib::http::transport::HttpResponse,
        crate::stdlib::http::transport::HttpError,
    > {
        use std::io::Write;
        let is_post = request.method == "POST";
        let needs_auth = match self.auth {
            PushAuthMode::None => false,
            PushAuthMode::Always => true,
            PushAuthMode::PostOnly => is_post,
        };
        if needs_auth {
            let auth = request
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("Authorization"));
            if let Some((_, value)) = auth {
                assert_eq!(value, "Basic YWdlbnQ6aG9zdC1vbmx5LXRva2Vu");
            } else {
                if is_post {
                    self.post_401_count.fetch_add(1, Ordering::SeqCst);
                }
                return Ok(crate::stdlib::http::transport::HttpResponse {
                    status: 401,
                    status_text: "Unauthorized".into(),
                    headers: vec![("www-authenticate".into(), "Basic realm=git".into())],
                    body: vec![],
                    final_url: request.url.clone(),
                });
            }
        }
        let is_receive = request.url.contains("git-receive-pack");
        let (service, subcmd, advertise_prefix) = if is_receive {
            (
                "git-receive-pack",
                "receive-pack",
                b"001f# service=git-receive-pack\n0000".as_slice(),
            )
        } else {
            (
                "git-upload-pack",
                "upload-pack",
                b"001e# service=git-upload-pack\n0000".as_slice(),
            )
        };
        let (body, content_type) = if request.method == "GET" {
            self.get_count.fetch_add(1, Ordering::SeqCst);
            let mut body = advertise_prefix.to_vec();
            body.extend(native(
                &self.repo,
                &[subcmd, "--stateless-rpc", "--advertise-refs", "."],
            ));
            (body, format!("application/x-{service}-advertisement"))
        } else {
            self.post_count.fetch_add(1, Ordering::SeqCst);
            let mut child = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args([subcmd, "--stateless-rpc", "."])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&request.body)
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            (output.stdout, format!("application/x-{service}-result"))
        };
        body_out.write_all(&body).unwrap();
        Ok(crate::stdlib::http::transport::HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), content_type)],
            body: Vec::new(),
            final_url: request.url.clone(),
        })
    }

    async fn download(
        &self,
        _: &crate::stdlib::http::transport::HttpRequest,
        _: &mut (dyn std::io::Write + Send),
    ) -> std::result::Result<
        crate::stdlib::http::transport::DownloadMeta,
        crate::stdlib::http::transport::HttpError,
    > {
        unreachable!()
    }
}

fn bare_upstream() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    native(dir.path(), &["init", "--bare", "-b", "main"]);
    native(dir.path(), &["config", "http.receivepack", "true"]);
    dir
}

#[tokio::test]
async fn smart_http_push_create_fast_forward_up_to_date_and_explicit_branch() {
    let upstream = bare_upstream();
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/a.txt", "one\n");
            repo.add(["a.txt"]);
            const c1 = repo.commit("first");

            // 1. Create main on empty bare remote.
            const r1 = repo.push();
            assert(r1.branch === "main");
            assert(r1.previous === undefined);
            assert(r1.current === c1);
            assert(r1.upToDate === false);

            // 2. Push again: upToDate, no POST.
            const r2 = repo.push();
            assert(r2.branch === "main");
            assert(r2.previous === c1);
            assert(r2.current === c1);
            assert(r2.upToDate === true);

            // 3. Fast-forward update.
            fs.writeText("/repo/b.txt", "two\n");
            repo.add(["b.txt"]);
            const c2 = repo.commit("second");
            const r3 = repo.push("origin", "main");
            assert(r3.branch === "main");
            assert(r3.previous === c1);
            assert(r3.current === c2);
            assert(r3.upToDate === false);

            // 4. Push an explicit non-current branch.
            repo.createBranch("feature");
            repo.switchBranch("feature");
            fs.writeText("/repo/feat.txt", "feat\n");
            repo.add(["feat.txt"]);
            const c3 = repo.commit("feature commit");
            repo.switchBranch("main");
            const r4 = repo.push("origin", "feature");
            assert(r4.branch === "feature");
            assert(r4.previous === undefined);
            assert(r4.current === c3);
            assert(r4.upToDate === false);
        }
    "#,
        data,
    )
    .await;

    // 4 GETs (one per push), 3 POSTs (upToDate skipped POST).
    assert_eq!(http.get_count.load(Ordering::SeqCst), 4);
    assert_eq!(http.post_count.load(Ordering::SeqCst), 3);

    native(upstream.path(), &["fsck", "--full", "--strict"]);
    let local_repo = vfs.root().join("repo");
    let main_id =
        String::from_utf8(native(&local_repo, &["rev-parse", "refs/heads/main"])).unwrap();
    let feat_id =
        String::from_utf8(native(&local_repo, &["rev-parse", "refs/heads/feature"])).unwrap();
    assert_eq!(
        String::from_utf8(native(upstream.path(), &["rev-parse", "refs/heads/main"])).unwrap(),
        main_id
    );
    assert_eq!(
        String::from_utf8(native(
            upstream.path(),
            &["rev-parse", "refs/heads/feature"]
        ))
        .unwrap(),
        feat_id
    );
    // Remote-tracking refs updated locally.
    assert_eq!(
        String::from_utf8(native(
            &local_repo,
            &["rev-parse", "refs/remotes/origin/main"]
        ))
        .unwrap(),
        main_id
    );
    assert_eq!(
        String::from_utf8(native(
            &local_repo,
            &["rev-parse", "refs/remotes/origin/feature"]
        ))
        .unwrap(),
        feat_id
    );
}

#[tokio::test]
async fn push_authorization_is_scoped_and_checked_before_network_or_secrets() {
    struct ScopedPushGrant;
    impl SecurityCheck for ScopedPushGrant {
        fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
            assert_eq!(caller, "main");
            if matches!(
                capability,
                "git.init"
                    | "git.open"
                    | "git.read"
                    | "git.write"
                    | "git.commit"
                    | "git.fetch"
                    | "fs.write"
            ) {
                return CheckOutcome::Allow { rule: None };
            }
            if capability == "git.push"
                && context["path"] == "/repo"
                && context["remoteName"] == "origin"
                && context["remote"] == "https://example.com/repo.git"
                && context["branch"] == "allowed"
            {
                return CheckOutcome::Allow { rule: None };
            }
            CheckOutcome::Deny {
                rule: None,
                reason: "branch push denied".into(),
            }
        }
    }

    let upstream = bare_upstream();
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::Always,
    ));
    let secret = Arc::new(Token(std::sync::atomic::AtomicU64::new(0)));
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    data.secret_provider = secret.clone();
    data.security_check = Arc::new(ScopedPushGrant);
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/f.txt", "hello\n");
            repo.add(["f.txt"]);
            repo.commit("init");
            let denied = "";
            try { repo.push(); }
            catch (e: PermissionDeniedError) {
                denied = e.capability + "|" + e.reason;
            }
            assert(denied === "git.push|branch push denied");
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.get_count.load(Ordering::SeqCst), 0);
    assert_eq!(http.post_count.load(Ordering::SeqCst), 0);
    assert_eq!(secret.0.load(Ordering::Relaxed), 0);

    // Allowed branch succeeds.
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    data.secret_provider = secret.clone();
    data.security_check = Arc::new(ScopedPushGrant);
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            repo.createBranch("allowed");
            const res = repo.push("origin", "allowed");
            assert(res.branch === "allowed");
            assert(res.upToDate === false);
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.get_count.load(Ordering::SeqCst), 1);
    assert_eq!(http.post_count.load(Ordering::SeqCst), 1);
    // GET authenticated once on 401; POST sent auth on first attempt (no duplicate pack upload).
    assert_eq!(http.post_401_count.load(Ordering::SeqCst), 0);
    assert!(secret.0.load(Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn push_handles_post_only_authentication_challenge() {
    let upstream = bare_upstream();
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::PostOnly,
    ));
    let secret = Arc::new(Token(std::sync::atomic::AtomicU64::new(0)));
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    data.secret_provider = secret.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/f.txt", "post-auth\n");
            repo.add(["f.txt"]);
            repo.commit("init");
            const res = repo.push();
            assert(res.branch === "main");
            assert(res.upToDate === false);
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.get_count.load(Ordering::SeqCst), 1);
    assert_eq!(http.post_401_count.load(Ordering::SeqCst), 1);
    assert_eq!(http.post_count.load(Ordering::SeqCst), 1);
    assert_eq!(secret.0.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn push_refuses_non_fast_forward_unborn_detached_symbolic_alias_and_hook_rejection() {
    let upstream = bare_upstream();
    // Seed upstream with an initial commit from a separate working tree.
    let other = tempfile::tempdir().unwrap();
    native(other.path(), &["init", "-b", "main"]);
    native(other.path(), &["config", "user.name", "Other"]);
    native(other.path(), &["config", "user.email", "other@example.com"]);
    std::fs::write(other.path().join("base.txt"), "base\n").unwrap();
    native(other.path(), &["add", "."]);
    native(other.path(), &["commit", "-m", "base"]);
    native(
        other.path(),
        &["remote", "add", "origin", upstream.path().to_str().unwrap()],
    );
    native(other.path(), &["push", "origin", "main"]);

    // 1. Diverged local history: refused locally before POST.
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/local.txt", "diverged\n");
            repo.add(["local.txt"]);
            repo.commit("diverged");
            let msg = "";
            try { repo.push(); }
            catch (e) { msg = String(e); }
            assert(msg.includes("non-fast-forward"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.get_count.load(Ordering::SeqCst), 1);
    assert_eq!(http.post_count.load(Ordering::SeqCst), 0);

    // 2. Unborn branch: refused before any HTTP request.
    let http_unborn = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(Vfs::tempdir().unwrap());
    data.http_client = http_unborn.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.init("/empty");
            repo.addRemote("origin", "https://example.com/repo.git");
            let failed = false;
            try { repo.push(); } catch (e) { failed = true; }
            assert(failed);
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http_unborn.get_count.load(Ordering::SeqCst), 0);
    assert_eq!(http_unborn.post_count.load(Ordering::SeqCst), 0);

    // 3. Detached HEAD: refused before any HTTP request.
    let vfs_detached = Vfs::tempdir().unwrap();
    let detached_dir = vfs_detached.root().join("repo");
    std::fs::create_dir(&detached_dir).unwrap();
    native(&detached_dir, &["init", "-b", "main"]);
    native(&detached_dir, &["config", "user.name", "Detached"]);
    native(
        &detached_dir,
        &["config", "user.email", "detached@example.com"],
    );
    native(
        &detached_dir,
        &["remote", "add", "origin", "https://example.com/repo.git"],
    );
    std::fs::write(detached_dir.join("a"), "a").unwrap();
    native(&detached_dir, &["add", "."]);
    native(&detached_dir, &["commit", "-m", "init"]);
    let head = String::from_utf8(native(&detached_dir, &["rev-parse", "HEAD"])).unwrap();
    native(&detached_dir, &["checkout", "--detach", head.trim()]);
    let http_detached = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs_detached);
    data.http_client = http_detached.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            let msg = "";
            try { Repository.open("/repo").push(); }
            catch (e) { msg = String(e); }
            assert(msg.includes("detached HEAD"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http_detached.get_count.load(Ordering::SeqCst), 0);

    // 3b. Symbolic local branch alias: refused before any HTTP request.
    let vfs_sym = Vfs::tempdir().unwrap();
    let sym_dir = vfs_sym.root().join("repo");
    std::fs::create_dir(&sym_dir).unwrap();
    native(&sym_dir, &["init", "-b", "main"]);
    native(&sym_dir, &["config", "user.name", "Sym"]);
    native(&sym_dir, &["config", "user.email", "sym@example.com"]);
    native(
        &sym_dir,
        &["remote", "add", "origin", "https://example.com/repo.git"],
    );
    std::fs::write(sym_dir.join("a"), "a").unwrap();
    native(&sym_dir, &["add", "."]);
    native(&sym_dir, &["commit", "-m", "init"]);
    native(
        &sym_dir,
        &["symbolic-ref", "refs/heads/alias", "refs/heads/main"],
    );
    let http_sym = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs_sym);
    data.http_client = http_sym.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            let msg = "";
            try { Repository.open("/repo").push("origin", "alias"); }
            catch (e) { msg = String(e); }
            assert(msg.includes("symbolic local branch aliases"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http_sym.get_count.load(Ordering::SeqCst), 0);

    // 4. Server-side pre-receive hook rejection: returns error and leaves local tracking ref untouched.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook_upstream = bare_upstream();
        let hook = hook_upstream.path().join("hooks/pre-receive");
        std::fs::write(&hook, "#!/bin/sh\necho 'hook blocked' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let http_hook = Arc::new(PushServer::new(
            hook_upstream.path().to_owned(),
            PushAuthMode::None,
        ));
        let vfs_hook = Vfs::tempdir().unwrap();
        let mut data = test_data(vfs_hook.clone());
        data.http_client = http_hook;
        run_source(
            r#"
            import { Repository } from "submilli:git";
            import * as fs from "submilli:fs";
            function main(): void {
                const repo = Repository.init("/repo");
                repo.addRemote("origin", "https://example.com/repo.git");
                fs.writeText("/repo/x.txt", "x\n");
                repo.add(["x.txt"]);
                repo.commit("blocked");
                let msg = "";
                try { repo.push(); }
                catch (e) { msg = String(e); }
                assert(msg.includes("remote rejected"));
            }
        "#,
            data,
        )
        .await;
        assert!(
            !vfs_hook
                .root()
                .join("repo/.git/refs/remotes/origin/main")
                .exists(),
            "rejected push must not create remote-tracking ref"
        );
    }
}

#[tokio::test]
async fn push_fuel_exhaustion_aborts_before_post() {
    let upstream = bare_upstream();
    let vfs = Vfs::tempdir().unwrap();
    let repo_dir = vfs.root().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    native(&repo_dir, &["init", "-q", "-b", "main"]);
    native(&repo_dir, &["config", "user.name", "Fuel"]);
    native(&repo_dir, &["config", "user.email", "fuel@example.com"]);
    native(
        &repo_dir,
        &["remote", "add", "origin", "https://example.com/repo.git"],
    );
    // Write a 1 MB payload so pack generation charges substantial fuel.
    std::fs::write(repo_dir.join("big.bin"), vec![b'z'; 1_000_000]).unwrap();
    native(&repo_dir, &["add", "."]);
    native(&repo_dir, &["commit", "-q", "-m", "big"]);

    let source = r#"
        import { Repository } from "submilli:git";
        function main(): void {
            Repository.open("/repo").push();
        }
    "#;

    // Measure fuel on a disposable upstream.
    let measure_upstream = bare_upstream();
    let mut measure_data = test_data(vfs.clone());
    measure_data.http_client = Arc::new(PushServer::new(
        measure_upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let needed = host_fuel_of(source, measure_data).await;

    // Run against fresh upstream with a quarter of the required fuel: must trap OutOfFuel before POST.
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs);
    data.http_client = http.clone();
    let (outcome, host_fuel) = run_program(source, data, needed / 4).await;
    assert_eq!(
        outcome.unwrap_err().downcast_ref::<wasmtime::Trap>(),
        Some(&wasmtime::Trap::OutOfFuel)
    );
    assert!(host_fuel <= needed / 4, "{host_fuel} vs {}", needed / 4);
    assert_eq!(
        http.post_count.load(Ordering::SeqCst),
        0,
        "fuel exhaustion must abort before POST /git-receive-pack"
    );
}

#[tokio::test]
async fn push_slash_branches_and_tracking_ref_df_conflicts_are_caught_before_network() {
    let upstream = bare_upstream();
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();

    // 1. Pushing a slash branch `feature/foo` succeeds.
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/a.txt", "a\n");
            repo.add(["a.txt"]);
            repo.commit("init");
            repo.createBranch("feature/foo");
            const r = repo.push("origin", "feature/foo");
            assert(r.branch === "feature/foo");
            assert(r.previous === undefined);
            assert(!r.upToDate);
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.get_count.load(Ordering::SeqCst), 1);
    assert_eq!(http.post_count.load(Ordering::SeqCst), 1);

    // 2. Pushing `feature` when loose `refs/remotes/origin/feature/foo` exists fails BEFORE network.
    // Remove the local `feature/foo` branch so local `refs/heads/feature` can be created while
    // `refs/remotes/origin/feature/foo` remains as a loose remote-tracking ref.
    let local_repo = vfs.root().join("repo");
    native(&local_repo, &["branch", "-D", "feature/foo"]);
    let http_df_loose = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs.clone());
    data.http_client = http_df_loose.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            repo.createBranch("feature");
            let msg = "";
            try { repo.push("origin", "feature"); }
            catch (e) { msg = String(e); }
            assert(msg.includes("cannot update remote-tracking branch"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http_df_loose.get_count.load(Ordering::SeqCst), 0);
    assert_eq!(http_df_loose.post_count.load(Ordering::SeqCst), 0);

    // 3. Pushing `topic/sub` when packed `refs/remotes/origin/topic` exists fails BEFORE network.
    let head = String::from_utf8(native(&local_repo, &["rev-parse", "HEAD"])).unwrap();
    native(
        &local_repo,
        &["update-ref", "refs/remotes/origin/topic", head.trim()],
    );
    native(&local_repo, &["pack-refs", "--all"]);
    let http_df_packed = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs.clone());
    data.http_client = http_df_packed.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            repo.createBranch("topic/sub");
            let msg = "";
            try { repo.push("origin", "topic/sub"); }
            catch (e) { msg = String(e); }
            assert(msg.includes("cannot update remote-tracking branch"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http_df_packed.get_count.load(Ordering::SeqCst), 0);
    assert_eq!(http_df_packed.post_count.load(Ordering::SeqCst), 0);

    // 4. Pushing `main` to a remote `other` when packed `refs/remotes/other` exists fails BEFORE network.
    native(
        &local_repo,
        &["remote", "add", "other", "https://example.com/other.git"],
    );
    native(
        &local_repo,
        &["update-ref", "refs/remotes/other", head.trim()],
    );
    native(&local_repo, &["pack-refs", "--all"]);
    let http_df_remote = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs);
    data.http_client = http_df_remote.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            let msg = "";
            try { Repository.open("/repo").push("other", "main"); }
            catch (e) { msg = String(e); }
            assert(msg.includes("cannot update remote-tracking branch"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http_df_remote.get_count.load(Ordering::SeqCst), 0);
    assert_eq!(http_df_remote.post_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn push_refuses_remote_without_report_status_before_post() {
    struct NoReportStatusServer {
        post_count: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl HttpClient for NoReportStatusServer {
        async fn send(
            &self,
            _: &crate::stdlib::http::transport::HttpRequest,
        ) -> std::result::Result<
            crate::stdlib::http::transport::HttpResponse,
            crate::stdlib::http::transport::HttpError,
        > {
            unreachable!()
        }
        async fn send_without_redirects_to(
            &self,
            request: &crate::stdlib::http::transport::HttpRequest,
            body_out: &mut (dyn std::io::Write + Send),
        ) -> std::result::Result<
            crate::stdlib::http::transport::HttpResponse,
            crate::stdlib::http::transport::HttpError,
        > {
            if request.method == "POST" {
                self.post_count.fetch_add(1, Ordering::SeqCst);
                panic!("POST must not be sent when remote lacks report-status");
            }
            // Valid empty-repo receive-pack advertisement omitting `report-status`.
            let line = b"0000000000000000000000000000000000000000 capabilities^{}\0side-band-64k\n";
            let mut adv = b"001f# service=git-receive-pack\n0000".to_vec();
            adv.extend_from_slice(format!("{:04x}", line.len() + 4).as_bytes());
            adv.extend_from_slice(line);
            adv.extend_from_slice(b"0000");
            body_out.write_all(&adv).unwrap();
            Ok(crate::stdlib::http::transport::HttpResponse {
                status: 200,
                status_text: "OK".into(),
                headers: vec![(
                    "content-type".into(),
                    "application/x-git-receive-pack-advertisement".into(),
                )],
                body: Vec::new(),
                final_url: request.url.clone(),
            })
        }
        async fn download(
            &self,
            _: &crate::stdlib::http::transport::HttpRequest,
            _: &mut (dyn std::io::Write + Send),
        ) -> std::result::Result<
            crate::stdlib::http::transport::DownloadMeta,
            crate::stdlib::http::transport::HttpError,
        > {
            unreachable!()
        }
    }

    let http = Arc::new(NoReportStatusServer {
        post_count: std::sync::atomic::AtomicUsize::new(0),
    });
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs);
    data.http_client = http.clone();
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/a.txt", "a\n");
            repo.add(["a.txt"]);
            repo.commit("init");
            let msg = "";
            try { repo.push(); }
            catch (e) { msg = String(e); }
            assert(msg.includes("report-status"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.post_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn push_with_read_only_token_fails_cleanly_on_post_401_without_retrying() {
    struct ReadOnlyTokenServer {
        repo: std::path::PathBuf,
        get_attempts: std::sync::atomic::AtomicUsize,
        post_attempts: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl HttpClient for ReadOnlyTokenServer {
        async fn send(
            &self,
            _: &crate::stdlib::http::transport::HttpRequest,
        ) -> std::result::Result<
            crate::stdlib::http::transport::HttpResponse,
            crate::stdlib::http::transport::HttpError,
        > {
            unreachable!()
        }
        async fn send_without_redirects_to(
            &self,
            request: &crate::stdlib::http::transport::HttpRequest,
            body_out: &mut (dyn std::io::Write + Send),
        ) -> std::result::Result<
            crate::stdlib::http::transport::HttpResponse,
            crate::stdlib::http::transport::HttpError,
        > {
            let has_auth = request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
            if request.method == "GET" {
                self.get_attempts.fetch_add(1, Ordering::SeqCst);
                if !has_auth {
                    return Ok(crate::stdlib::http::transport::HttpResponse {
                        status: 401,
                        status_text: "Unauthorized".into(),
                        headers: Vec::new(),
                        body: Vec::new(),
                        final_url: request.url.clone(),
                    });
                }
                let mut body = b"001f# service=git-receive-pack\n0000".to_vec();
                body.extend(native(
                    &self.repo,
                    &["receive-pack", "--stateless-rpc", "--advertise-refs", "."],
                ));
                body_out.write_all(&body).unwrap();
                return Ok(crate::stdlib::http::transport::HttpResponse {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![(
                        "content-type".into(),
                        "application/x-git-receive-pack-advertisement".into(),
                    )],
                    body: Vec::new(),
                    final_url: request.url.clone(),
                });
            }
            self.post_attempts.fetch_add(1, Ordering::SeqCst);
            assert!(
                has_auth,
                "POST should carry pre-authenticated header after GET 401 retry"
            );
            Ok(crate::stdlib::http::transport::HttpResponse {
                status: 401,
                status_text: "Unauthorized".into(),
                headers: Vec::new(),
                body: Vec::new(),
                final_url: request.url.clone(),
            })
        }
        async fn download(
            &self,
            _: &crate::stdlib::http::transport::HttpRequest,
            _: &mut (dyn std::io::Write + Send),
        ) -> std::result::Result<
            crate::stdlib::http::transport::DownloadMeta,
            crate::stdlib::http::transport::HttpError,
        > {
            unreachable!()
        }
    }

    let upstream = bare_upstream();
    let http = Arc::new(ReadOnlyTokenServer {
        repo: upstream.path().to_owned(),
        get_attempts: std::sync::atomic::AtomicUsize::new(0),
        post_attempts: std::sync::atomic::AtomicUsize::new(0),
    });
    let secret = Arc::new(Token(std::sync::atomic::AtomicU64::new(0)));
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    data.secret_provider = secret;
    run_source(
        r#"
        import { Repository } from "submilli:git";
        import * as fs from "submilli:fs";
        function main(): void {
            const repo = Repository.init("/repo");
            repo.addRemote("origin", "https://example.com/repo.git");
            fs.writeText("/repo/a.txt", "a\n");
            repo.add(["a.txt"]);
            repo.commit("init");
            let msg = "";
            try { repo.push(); }
            catch (e) { msg = String(e); }
            assert(msg.includes("401"));
            assert(!msg.includes("host-only-token"));
        }
    "#,
        data,
    )
    .await;
    assert_eq!(http.get_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(http.post_attempts.load(Ordering::SeqCst), 1);
    assert!(
        !vfs.root()
            .join("repo/.git/refs/remotes/origin/main")
            .exists()
    );
}

#[tokio::test]
async fn push_settles_result_when_fuel_runs_out_after_post() {
    let upstream = bare_upstream();
    let vfs = Vfs::tempdir().unwrap();
    let repo_dir = vfs.root().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    native(&repo_dir, &["init", "-q", "-b", "main"]);
    native(&repo_dir, &["config", "user.name", "Fuel"]);
    native(&repo_dir, &["config", "user.email", "fuel@example.com"]);
    native(
        &repo_dir,
        &["remote", "add", "origin", "https://example.com/repo.git"],
    );
    std::fs::write(repo_dir.join("small.txt"), "hello\n").unwrap();
    native(&repo_dir, &["add", "."]);
    native(&repo_dir, &["commit", "-q", "-m", "small"]);

    // Probe total fuel for `push()` on a fresh upstream, then give slightly less
    // than total so pack build succeeds (before_effect runs) while publish/encoding
    // drains the store to 0; the push must still succeed and return its value
    // (or trap only at the next guest instruction after push() returns).
    let source = r#"
        import { Repository } from "submilli:git";
        function main(): string {
            const res = Repository.open("/repo").push();
            return res.branch + ":" + String(res.upToDate);
        }
    "#;
    let probe_upstream = bare_upstream();
    let mut probe_data = test_data(vfs.clone());
    probe_data.http_client = Arc::new(PushServer::new(
        probe_upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let total = host_fuel_of(source, probe_data).await;

    // Trim just enough fuel (~200 fuel) so `encode_json` hits 0 remaining fuel
    // after `POST` + `publish` settle. `fuel::settle_result` lets `push()` finish
    // marshalling without discarding the remote push; the run then traps on the
    // following guest bytecode (`res.branch + ...`).
    let http = Arc::new(PushServer::new(
        upstream.path().to_owned(),
        PushAuthMode::None,
    ));
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    let (outcome, _) = run_program(source, data, total.saturating_sub(200)).await;
    assert_eq!(http.post_count.load(Ordering::SeqCst), 1);
    assert!(
        vfs.root()
            .join("repo/.git/refs/remotes/origin/main")
            .exists(),
        "local tracking ref must be published once POST succeeded"
    );
    // Remote received the commit; run either returned cleanly or trapped only after push().
    assert!(
        outcome.is_ok()
            || outcome
                .as_ref()
                .unwrap_err()
                .downcast_ref::<wasmtime::Trap>()
                == Some(&wasmtime::Trap::OutOfFuel)
    );
}
