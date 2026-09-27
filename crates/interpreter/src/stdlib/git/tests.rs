use super::*;
use crate::runtime::{RuntimeConfig, Vfs, dispatch_main_async, install_runtime_async};
use serde_json::json;

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
async fn repository_class_construction_and_dispatch() {
    run_source(
        r#"
        import { Repository } from "submilli:git";
        class Checkout extends Repository {
            constructor(path: string) { super(path); }
            clean(): boolean { return this.status().clean; }
        }
        function main(): void {
            const created: Repository = Repository.init("/repo");
            assert(created instanceof Repository);
            const opened = new Repository("/repo");
            assert(opened instanceof Repository);
            const status = () => opened.status();
            assert(status().clean);
            const checkout = new Checkout("/repo");
            assert(checkout instanceof Repository);
            assert(checkout instanceof Checkout);
            assert(checkout.clean());
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
    let mut data = StoreData::with_vfs(vfs);
    data.git = Some(GitConfig {
        name: "Agent".into(),
        email: "agent@example.com".into(),
        username: Some("agent".into()),
    });
    data
}

async fn run_source(source: &str, mut data: StoreData) {
    let compiled = crate::compile_script(source, "git.ts", crate::FileId(0), &[], &[])
        .unwrap_or_else(|error| panic!("{error:#?}"));
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().unwrap();
    data.install_type_info(compiled.type_info.clone());
    let mut store = cfg.store(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    dispatch_main_async(&mut store, &instance).await.unwrap();
    assert_eq!(store.data().tenant_limits.host_attached_bytes(), 0);
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
            try { fs.remove("/repo", true); } catch (e: Error) { refused = true; }
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
    let dir =
        Arc::new(cap_std::fs::Dir::open_ambient_dir(repo, cap_std::ambient_authority()).unwrap());
    let snapshot =
        storage::Snapshot::open(dir, Arc::new(AtomicBool::new(false)), storage::MAX_BYTES).unwrap();
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
    async fn send_without_redirects(
        &self,
        request: &crate::stdlib::http::transport::HttpRequest,
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
        Ok(crate::stdlib::http::transport::HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), content_type.into())],
            body,
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
    });
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.http_client = http.clone();
    data.security_check = Arc::new(GitOnly {
        capability: "git.clone",
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
    data.security_check = Arc::new(GitOnly {
        capability: "git.fetch",
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            const update = repo.pull();
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

struct GitOnly {
    capability: &'static str,
}
impl SecurityCheck for GitOnly {
    fn check(&self, caller: &str, capability: &str, context: &Value) -> CheckOutcome {
        assert_eq!(caller, "main");
        if capability == self.capability && context["path"] == "/repo" {
            CheckOutcome::Allow
        } else {
            CheckOutcome::Deny {
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
    });
    let secret = Arc::new(Token(std::sync::atomic::AtomicU64::new(0)));
    let mut data = test_data(Vfs::tempdir().unwrap());
    data.http_client = http.clone();
    data.secret_provider = secret.clone();
    data.security_check = Arc::new(GitOnly {
        capability: "git.fetch",
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
    data.security_check = Arc::new(GitOnly {
        capability: "git.clone",
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.clone("https://example.com/repo.git", "/repo");
            assert(repo.status().clean);
            assert(repo.log().commits.length === 1);
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
    assert!(secret.0.load(Ordering::Relaxed) > 0);
}

#[tokio::test]
async fn init_and_commit_grants_leave_local_work_ungated() {
    let vfs = Vfs::tempdir().unwrap();
    let mut data = test_data(vfs.clone());
    data.security_check = Arc::new(GitOnly {
        capability: "git.init",
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
    data.security_check = Arc::new(GitOnly { capability: "" });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            assert(repo.status().clean);
            assert(repo.log().commits.length === 1);
            repo.diff();
            repo.show("HEAD", "hello");
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
    let mut data = test_data(vfs);
    data.security_check = Arc::new(GitOnly {
        capability: "git.commit",
    });
    run_source(
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            const repo = Repository.open("/repo");
            repo.add(["hello"]);
            repo.commit("two");
            assert(repo.log().commits[0].message === "two");
        }
    "#,
        data,
    )
    .await;
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
    let budget = WorkingBudget::reserve(&data.tenant_limits).unwrap();
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
    let budget = WorkingBudget::reserve(&data.tenant_limits).unwrap();
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
    cleanup.await;
    assert_eq!(data.tenant_limits.host_attached_bytes(), 0);
    drop(data);
    assert!(!root.exists());
}

#[test]
fn repository_docs_expose_class_factories_and_capabilities() {
    let docs = crate::packages::docs_with_git(MODULE_NAME, true).unwrap();
    for declaration in [
        "class Repository",
        "constructor(path: string)",
        "static open(",
        "static init(",
        "static clone(",
        "@capability git.init",
        "@capability git.clone",
        "@capability git.fetch",
        "@capability git.commit",
    ] {
        assert!(
            docs.declarations.contains(declaration),
            "missing {declaration}: {}",
            docs.declarations
        );
    }
    assert!(!docs.declarations.contains("function init("));
    assert!(crate::packages::docs(MODULE_NAME).is_none());
}
