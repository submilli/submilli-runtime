use super::{GitConfig, Job, storage, worker};
use crate::runtime::{StoreData, Vfs};
use cap_std::fs::Dir;
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, Mutex, atomic::AtomicBool, atomic::AtomicU64};

fn native(repo: &Path, args: &[&str]) -> Vec<u8> {
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

fn repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    native(root.path(), &["init", "-b", "main"]);
    native(root.path(), &["config", "user.name", "Native"]);
    native(root.path(), &["config", "user.email", "native@example.com"]);
    std::fs::write(root.path().join("base"), "base").unwrap();
    native(root.path(), &["add", "."]);
    native(root.path(), &["commit", "-m", "base"]);
    root
}

fn snapshot(root: &Path) -> storage::Snapshot {
    storage::Snapshot::open(
        Arc::new(Dir::open_ambient_dir(root, cap_std::ambient_authority()).unwrap()),
        Arc::new(AtomicBool::new(false)),
        storage::MAX_BYTES,
    )
    .unwrap()
}

#[cfg(unix)]
#[test]
fn metadata_publication_preserves_native_hook_execution() {
    use std::os::unix::fs::PermissionsExt;
    let root = repository();
    let hook = root.path().join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nprintf ran > hook-ran\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut state = snapshot(root.path());
    super::operations::set_remote(&mut state, "origin", "https://example.com/repo.git", true)
        .unwrap();
    state.publish().unwrap();
    assert_ne!(
        std::fs::metadata(&hook).unwrap().permissions().mode() & 0o111,
        0
    );
    native(
        root.path(),
        &["commit", "--allow-empty", "-m", "native hook"],
    );
    assert_eq!(std::fs::read(root.path().join("hook-ran")).unwrap(), b"ran");
}

#[cfg(unix)]
#[test]
fn failed_metadata_staging_does_not_leave_a_recovery_blocker() {
    let root = repository();
    let head = native(root.path(), &["rev-parse", "HEAD"]);
    let state = snapshot(root.path());
    // A metadata path rejected by publication fails before the backup exists.
    std::fs::write(state.repo.git_dir().join("hooks/invalid\\name"), "invalid").unwrap();
    assert!(state.publish().is_err());
    assert_eq!(native(root.path(), &["rev-parse", "HEAD"]), head);
    assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".git-submilli-")
    }));
    snapshot(root.path()).publish().unwrap();
}

fn job(vfs: &Vfs, op: &str) -> Job {
    let data = StoreData::with_vfs(vfs.clone());
    Job {
        op: op.into(),
        path: "/".into(),
        caller: "main".into(),
        config: GitConfig {
            name: "Agent".into(),
            email: "agent@example.com".into(),
            username: None,
        },
        security: data.security_check.clone(),
        secrets: data.secret_provider.clone(),
        http: data.http_client.clone(),
        runtime: tokio::runtime::Handle::current(),
        cancelled: Arc::new(AtomicBool::new(false)),
        max_bytes: storage::MAX_BYTES,
        transferred: Arc::new(AtomicU64::new(0)),
        denial: Arc::new(Mutex::new(None)),
    }
}

#[tokio::test]
async fn native_merge_in_progress_refuses_mutations_without_changing_history() {
    let root = repository();
    native(root.path(), &["checkout", "-b", "topic"]);
    std::fs::write(root.path().join("topic"), "topic").unwrap();
    native(root.path(), &["add", "."]);
    native(root.path(), &["commit", "-m", "topic"]);
    native(root.path(), &["checkout", "main"]);
    native(root.path(), &["merge", "--no-commit", "--no-ff", "topic"]);
    let before = storage::read_files(
        &Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap(),
        false,
        &AtomicBool::new(false),
        storage::MAX_BYTES,
    )
    .unwrap();
    let vfs = Vfs::external(root.path().to_owned()).unwrap();
    for (op, args) in [
        ("commit", vec![json!("incorrect merge")]),
        ("add", vec![json!(["."])]),
    ] {
        let result = worker::run(&vfs, &job(&vfs, op), op, &args);
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("in-progress native Git")
        );
    }
    let after = storage::read_files(
        &Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap(),
        false,
        &AtomicBool::new(false),
        storage::MAX_BYTES,
    )
    .unwrap();
    assert_eq!(before, after);
}

#[tokio::test]
async fn empty_native_state_directories_also_prevent_mutations() {
    let root = repository();
    let vfs = Vfs::external(root.path().to_owned()).unwrap();
    for marker in ["rebase-apply", "rebase-merge", "sequencer"] {
        let path = root.path().join(".git").join(marker);
        std::fs::create_dir(&path).unwrap();
        let result = worker::run(
            &vfs,
            &job(&vfs, "createBranch"),
            "createBranch",
            &[json!("topic"), json!("HEAD")],
        );
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("in-progress native Git")
        );
        std::fs::remove_dir(path).unwrap();
    }
}

struct RemotePrefix;

impl crate::runtime::SecurityCheck for RemotePrefix {
    fn check(
        &self,
        _: &str,
        capability: &str,
        context: &serde_json::Value,
    ) -> crate::runtime::security::CheckOutcome {
        use crate::runtime::security::CheckOutcome;
        if matches!(capability, "git.clone" | "git.fetch")
            && !context["remote"]
                .as_str()
                .is_some_and(|url| url.starts_with("https://example.com/allowed/"))
        {
            return CheckOutcome::Deny {
                reason: "remote outside allowed prefix".into(),
            };
        }
        CheckOutcome::Allow
    }
}

#[tokio::test]
async fn remote_grants_check_canonical_paths_before_creation_or_network() {
    let root = repository();
    let vfs = Vfs::external(root.path().to_owned()).unwrap();
    for (index, url) in [
        "https://example.com/allowed/../private",
        "https://example.com/allowed/%2e%2e/private",
    ]
    .into_iter()
    .enumerate()
    {
        let configure = if index == 0 {
            "addRemote"
        } else {
            "setRemoteUrl"
        };
        let mut request = job(&vfs, configure);
        request.security = Arc::new(RemotePrefix);
        // Configuring a URL does not authorize contacting it.
        worker::run(&vfs, &request, configure, &[json!("origin"), json!(url)]).unwrap();
        for op in ["fetch", "clone"] {
            let mut request = job(&vfs, op);
            request.security = Arc::new(RemotePrefix);
            let args = if op == "clone" {
                request.path = "/new".into();
                vec![json!(url), json!("/new"), serde_json::Value::Null]
            } else {
                vec![json!("origin"), json!("main")]
            };
            let error = worker::run(&vfs, &request, op, &args).err().unwrap();
            assert!(
                error.to_string().contains("remote outside allowed prefix"),
                "{error}"
            );
        }
    }
    assert!(!root.path().join("new").exists());
    let mut request = job(&vfs, "setRemoteUrl");
    request.security = Arc::new(RemotePrefix);
    worker::run(
        &vfs,
        &request,
        "setRemoteUrl",
        &[
            json!("origin"),
            json!("https://EXAMPLE.com:443/allowed/repo"),
        ],
    )
    .unwrap();
    assert_eq!(
        snapshot(root.path()).remotes().unwrap()["origin"],
        "https://example.com/allowed/repo"
    );
}

#[tokio::test]
async fn commit_rejects_symbolic_branch_alias_and_nonbranch_head() {
    for target in ["refs/heads/allowed", "refs/tags/allowed"] {
        let root = repository();
        let original = native(root.path(), &["rev-parse", "refs/heads/main"]);
        if target.starts_with("refs/heads/") {
            native(root.path(), &["symbolic-ref", target, "refs/heads/main"]);
        } else {
            native(root.path(), &["tag", "allowed"]);
        }
        native(root.path(), &["symbolic-ref", "HEAD", target]);
        std::fs::write(root.path().join("base"), "staged change").unwrap();
        native(root.path(), &["add", "base"]);
        let vfs = Vfs::external(root.path().to_owned()).unwrap();
        let error = worker::run(&vfs, &job(&vfs, "commit"), "commit", &[json!("forbidden")])
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains(if target.starts_with("refs/heads/") {
                    "symbolic local branch"
                } else {
                    "HEAD must target a local branch"
                })
        );
        assert_eq!(
            native(root.path(), &["rev-parse", "refs/heads/main"]),
            original
        );
        assert_eq!(native(root.path(), &["rev-parse", target]), original);
    }
}

struct OnlyAliasBranch;

impl crate::runtime::SecurityCheck for OnlyAliasBranch {
    fn check(
        &self,
        _: &str,
        capability: &str,
        context: &serde_json::Value,
    ) -> crate::runtime::security::CheckOutcome {
        use crate::runtime::security::CheckOutcome;
        if capability == "git.commit" && context["branch"] != "MAIN" {
            return CheckOutcome::Deny {
                reason: "only MAIN is authorized".into(),
            };
        }
        CheckOutcome::Allow
    }
}

#[tokio::test]
async fn filesystem_ref_aliases_cannot_bypass_branch_grants() {
    let root = repository();
    let vfs = Vfs::external(root.path().to_owned()).unwrap();
    let state = snapshot(root.path());
    if !state.repo.git_dir().join("refs/heads/MAIN").exists() {
        // Distinct names remain valid on case-sensitive scratch filesystems.
        super::operations::create_branch(&state, "MAIN", "main").unwrap();
        return;
    }
    drop(state);
    let metadata = vfs.dir().unwrap().open_dir(".git").unwrap();
    for operation in ["switchBranch", "commit"] {
        if operation == "commit" {
            std::fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/MAIN\n").unwrap();
            std::fs::write(root.path().join("base"), "staged change").unwrap();
            native(root.path(), &["add", "base"]);
        }
        let before = storage::read_files(
            &metadata,
            false,
            &AtomicBool::new(false),
            storage::MAX_BYTES,
        )
        .unwrap();
        let mut request = job(&vfs, operation);
        request.security = Arc::new(OnlyAliasBranch);
        let argument = if operation == "switchBranch" {
            "MAIN"
        } else {
            "forbidden update"
        };
        let error = worker::run(&vfs, &request, operation, &[json!(argument)])
            .err()
            .expect("filesystem alias must not use MAIN's branch grant");
        assert!(error.to_string().contains("reference spelling"), "{error}");
        let after = storage::read_files(
            &metadata,
            false,
            &AtomicBool::new(false),
            storage::MAX_BYTES,
        )
        .unwrap();
        assert_eq!(before, after);
    }
}

#[test]
fn branch_listing_follows_native_symbolic_aliases_without_panicking() {
    let root = repository();
    native(
        root.path(),
        &["symbolic-ref", "refs/heads/alias", "refs/heads/main"],
    );
    let state = snapshot(root.path());
    let branches = super::operations::read(&state, "branches", &[]).unwrap();
    assert_eq!(branches.as_array().unwrap().len(), 2);
    assert_eq!(branches[0]["id"], branches[1]["id"]);
    assert!(
        super::operations::checkout(&state, "alias")
            .unwrap_err()
            .to_string()
            .contains("symbolic local branch")
    );
    native(
        root.path(),
        &["symbolic-ref", "refs/heads/alias", "refs/heads/alias"],
    );
    let state = snapshot(root.path());
    assert!(
        super::operations::read(&state, "branches", &[])
            .unwrap_err()
            .to_string()
            .contains("reference nesting")
    );
}
