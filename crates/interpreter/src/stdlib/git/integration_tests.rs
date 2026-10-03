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

/// The repository at `root`, opened to write.
fn snapshot(root: &Path) -> storage::Snapshot {
    storage::Snapshot::open(
        &super::location::Location::at(root),
        Arc::new(AtomicBool::new(false)),
        storage::MAX_BYTES,
        true,
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
    state.publish(None).unwrap();
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
    // A reference staged where the repository has a directory: publication
    // refuses it before moving anything.
    std::fs::write(state.repo.refs.git_dir().join("refs/heads/blocked"), "x").unwrap();
    std::fs::create_dir(root.path().join(".git/refs/heads/blocked")).unwrap();
    assert!(state.publish(None).is_err());
    assert_eq!(native(root.path(), &["rev-parse", "HEAD"]), head);
    assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".git-submilli-")
    }));
    snapshot(root.path()).publish(None).unwrap();
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
    // One operation holds a repository at a time.
    drop(state);
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

/// `init` writes the `.git` skeleton before it publishes, so the size limit's count
/// must be taken before either; and a branch name can't make that skeleton large.
#[tokio::test]
async fn init_is_counted_against_the_size_limit_and_bounds_branch_names() {
    let vfs = Vfs::tempdir().unwrap().with_size_limit(1 << 20);
    let quota = vfs.quota().unwrap().clone();
    worker::run(
        &vfs,
        &job(&vfs, "init"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .unwrap();
    assert!(quota.used() > 0, "the new repository counts");
    assert_eq!(quota.used(), vfs.measure_usage().unwrap());

    let long = "a".repeat(300);
    let Err(error) = worker::run(
        &vfs,
        &job(&vfs, "init"),
        "init",
        &[json!({ "branch": long })],
    ) else {
        panic!("a 300-byte branch name was accepted");
    };
    let error = error.to_string();
    assert!(error.contains("at most 250 bytes"), "{error}");
}

/// A 250-byte branch name leaves room for git's `.lock` file within a 255-byte
/// path component, through a commit and a new branch; one byte more is refused.
#[tokio::test]
async fn a_branch_name_of_250_bytes_works_end_to_end() {
    let vfs = Vfs::tempdir().unwrap().with_size_limit(1 << 20);
    let longest = "a".repeat(250);
    worker::run(
        &vfs,
        &job(&vfs, "init"),
        "init",
        &[json!({ "branch": longest })],
    )
    .unwrap();
    std::fs::write(vfs.root().join("notes.txt"), b"notes").unwrap();
    vfs.quota().unwrap().record(5);
    worker::run(&vfs, &job(&vfs, "add"), "add", &[json!(["notes.txt"])]).unwrap();
    worker::run(&vfs, &job(&vfs, "commit"), "commit", &[json!("add notes")]).unwrap();
    worker::run(
        &vfs,
        &job(&vfs, "createBranch"),
        "createBranch",
        &[json!("b".repeat(250)), json!(longest)],
    )
    .unwrap();
    let Err(error) = worker::run(
        &vfs,
        &job(&vfs, "createBranch"),
        "createBranch",
        &[json!("c".repeat(251)), json!(longest)],
    ) else {
        panic!("a 251-byte branch name was accepted");
    };
    let error = error.to_string();
    assert!(error.contains("at most 250 bytes"), "{error}");
}

/// After a commit, the size limit's count matches what the directory holds.
#[tokio::test]
async fn a_commit_is_counted_against_the_size_limit() {
    let vfs = Vfs::tempdir().unwrap().with_size_limit(1 << 20);
    let quota = vfs.quota().unwrap().clone();
    worker::run(
        &vfs,
        &job(&vfs, "init"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .unwrap();
    std::fs::write(vfs.root().join("notes.txt"), vec![b'x'; 5000]).unwrap();
    quota.record(5000);
    worker::run(&vfs, &job(&vfs, "add"), "add", &[json!(["notes.txt"])]).unwrap();
    worker::run(&vfs, &job(&vfs, "commit"), "commit", &[json!("add notes")]).unwrap();
    assert_eq!(quota.used(), vfs.measure_usage().unwrap());
}

/// A VFS that couldn't be measured is treated as full, for git as for every other
/// writer.
#[tokio::test]
async fn git_refuses_to_write_in_an_unmeasured_vfs() {
    let vfs = Vfs::tempdir().unwrap().with_measured_limit(1 << 20, None);
    let Err(error) = worker::run(
        &vfs,
        &job(&vfs, "init"),
        "init",
        &[json!({ "branch": "main" })],
    ) else {
        panic!("init wrote in an unmeasured VFS");
    };
    assert!(
        error
            .downcast_ref::<crate::runtime::host::QuotaExceededError>()
            .is_some(),
        "{error}"
    );
    assert!(
        error.to_string().contains("couldn't be measured"),
        "{error}"
    );
    assert!(!vfs.root().join(".git").exists(), "nothing is left behind");
}

/// A branch switch that replaces a file a handle holds leaves the old copy on
/// disk; the count keeps it until the handle lets go.
#[tokio::test]
async fn a_switch_that_replaces_a_held_file_counts_it_until_released() {
    let vfs = Vfs::tempdir().unwrap().with_size_limit(1 << 20);
    let quota = vfs.quota().unwrap().clone();
    let run = |op: &str, args: &[serde_json::Value]| {
        worker::run(&vfs, &job(&vfs, op), op, args).unwrap();
    };
    let write = |bytes: &[u8]| {
        std::fs::write(vfs.root().join("big.bin"), bytes).unwrap();
        quota.record(bytes.len() as u64);
    };
    run("init", &[json!({ "branch": "main" })]);
    write(&[b'm'; 5000]);
    run("add", &[json!(["big.bin"])]);
    run("commit", &[json!("main")]);
    run("createBranch", &[json!("other"), json!("HEAD")]);
    run("switchBranch", &[json!("other")]);
    quota.release(5000);
    write(&[b'o'; 5000]);
    run("add", &[json!(["big.bin"])]);
    run("commit", &[json!("other")]);
    assert_eq!(quota.used(), vfs.measure_usage().unwrap());

    let held = vfs.dir().unwrap().symlink_metadata("big.bin").unwrap();
    let guard = quota.hold(
        crate::runtime::fs::FileIdentity::of(&held).unwrap(),
        crate::runtime::Holder::Reader,
    );
    run("switchBranch", &[json!("main")]);
    assert_eq!(
        quota.used(),
        vfs.measure_usage().unwrap() + 5000,
        "the replaced copy is still on disk"
    );
    drop(guard);
    assert_eq!(quota.used(), vfs.measure_usage().unwrap());
}

/// Replacing a held file frees nothing until the handle lets go, so a switch
/// over one needs room for both copies.
#[tokio::test]
async fn a_switch_over_a_held_file_needs_room_for_both_copies() {
    let dir = tempfile::tempdir().unwrap();
    let roomy = Vfs::external(dir.path().to_path_buf())
        .unwrap()
        .with_size_limit(1 << 20);
    let quota = roomy.quota().unwrap().clone();
    let run =
        |vfs: &Vfs, op: &str, args: &[serde_json::Value]| worker::run(vfs, &job(vfs, op), op, args);
    let write = |bytes: &[u8]| {
        std::fs::write(dir.path().join("big.bin"), bytes).unwrap();
        quota.record(bytes.len() as u64);
    };
    run(&roomy, "init", &[json!({ "branch": "main" })]).unwrap();
    write(&[b'm'; 5000]);
    run(&roomy, "add", &[json!(["big.bin"])]).unwrap();
    run(&roomy, "commit", &[json!("main")]).unwrap();
    run(&roomy, "createBranch", &[json!("other"), json!("HEAD")]).unwrap();
    run(&roomy, "switchBranch", &[json!("other")]).unwrap();
    quota.release(5000);
    write(&[b'o'; 5000]);
    run(&roomy, "add", &[json!(["big.bin"])]).unwrap();
    run(&roomy, "commit", &[json!("other")]).unwrap();

    let used = roomy.measure_usage().unwrap();
    let tight = Vfs::external(dir.path().to_path_buf())
        .unwrap()
        .with_size_limit(used + 1000);
    let tight_quota = tight.quota().unwrap().clone();
    let held = tight.dir().unwrap().symlink_metadata("big.bin").unwrap();
    let guard = tight_quota.hold(
        crate::runtime::fs::FileIdentity::of(&held).unwrap(),
        crate::runtime::Holder::Reader,
    );
    let Err(error) = run(&tight, "switchBranch", &[json!("main")]) else {
        panic!("a switch over a held file fit without room for its old copy");
    };
    assert!(
        error
            .downcast_ref::<crate::runtime::host::QuotaExceededError>()
            .is_some(),
        "{error}"
    );
    drop(guard);
    run(&tight, "switchBranch", &[json!("main")]).unwrap();
    assert_eq!(tight_quota.used(), tight.measure_usage().unwrap());
}

/// A git change that doesn't grow the files is still refused in a VFS that
/// couldn't be measured: publication can't be counted there.
#[tokio::test]
async fn git_refuses_a_change_that_does_not_grow_an_unmeasured_vfs() {
    let dir = tempfile::tempdir().unwrap();
    let measured = Vfs::external(dir.path().to_path_buf())
        .unwrap()
        .with_size_limit(1 << 20);
    worker::run(
        &measured,
        &job(&measured, "init"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .unwrap();
    let long_url = format!("https://example.com/{}.git", "r".repeat(200));
    worker::run(
        &measured,
        &job(&measured, "addRemote"),
        "addRemote",
        &[json!("origin"), json!(long_url)],
    )
    .unwrap();
    let unmeasured = Vfs::external(dir.path().to_path_buf())
        .unwrap()
        .with_measured_limit(1 << 20, None);
    let Err(error) = worker::run(
        &unmeasured,
        &job(&unmeasured, "setRemoteUrl"),
        "setRemoteUrl",
        &[json!("origin"), json!("https://example.com/r.git")],
    ) else {
        panic!("a git change went through in an unmeasured VFS");
    };
    assert!(
        error
            .downcast_ref::<crate::runtime::host::QuotaExceededError>()
            .is_some(),
        "{error}"
    );
}

fn mounted(vfs: Vfs, volume: &Path, access: crate::runtime::vfs::Access) -> Vfs {
    vfs.with_mount(crate::runtime::vfs::MountSpec {
        guest_path: "/memory".into(),
        host: volume.to_path_buf(),
        volume: "memory".into(),
        access,
        quota: Some(Arc::new(crate::runtime::DiskQuota::new(1 << 20, 0))),
    })
    .unwrap()
}

fn job_at(vfs: &Vfs, op: &str, path: &str) -> Job {
    let mut job = job(vfs, op);
    job.path = path.into();
    job
}

#[tokio::test]
async fn mount_repository_lives_in_the_volume_and_charges_it() {
    let volume = tempfile::tempdir().unwrap();
    let vfs = mounted(
        Vfs::tempdir().unwrap().with_size_limit(1 << 20),
        volume.path(),
        crate::runtime::vfs::Access::ReadWrite,
    );
    let mount_quota = vfs.mounts()[0].quota().unwrap().clone();
    let root_quota = vfs.quota().unwrap().clone();
    worker::run(
        &vfs,
        &job_at(&vfs, "init", "/memory/repo"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .unwrap();
    assert!(volume.path().join("repo/.git/HEAD").is_file());
    assert!(mount_quota.used() > 0, "the volume is charged");
    assert_eq!(root_quota.used(), 0, "the root is not");
}

#[tokio::test]
async fn mount_point_inside_a_repository_is_refused() {
    let volume = tempfile::tempdir().unwrap();
    let vfs = mounted(
        Vfs::tempdir().unwrap(),
        volume.path(),
        crate::runtime::vfs::Access::ReadWrite,
    );
    let error = worker::run(
        &vfs,
        &job_at(&vfs, "init", "/"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        error.contains("overlaps the mount point /memory"),
        "{error}"
    );
    assert!(!vfs.root().join(".git").exists());
}

#[tokio::test]
async fn mount_read_only_refuses_repository_changes_but_not_reads() {
    let volume = tempfile::tempdir().unwrap();
    let writable = mounted(
        Vfs::tempdir().unwrap(),
        volume.path(),
        crate::runtime::vfs::Access::ReadWrite,
    );
    worker::run(
        &writable,
        &job_at(&writable, "init", "/memory"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .unwrap();
    std::fs::write(volume.path().join("note.txt"), "note").unwrap();
    worker::run(
        &writable,
        &job_at(&writable, "add", "/memory"),
        "add",
        &[json!(["note.txt"])],
    )
    .unwrap();
    std::fs::write(volume.path().join("other.txt"), "other").unwrap();
    let before = storage::read_files(
        &Dir::open_ambient_dir(volume.path(), cap_std::ambient_authority()).unwrap(),
        true,
        &AtomicBool::new(false),
        storage::MAX_BYTES,
    )
    .unwrap();
    let vfs = mounted(
        Vfs::tempdir().unwrap(),
        volume.path(),
        crate::runtime::vfs::Access::ReadOnly,
    );
    for (op, args) in [
        ("add", vec![json!(["other.txt"])]),
        ("commit", vec![json!("message")]),
    ] {
        let error = worker::run(&vfs, &job_at(&vfs, op, "/memory"), op, &args)
            .err()
            .unwrap();
        let denied = error
            .downcast_ref::<crate::runtime::host::PermissionDenied>()
            .unwrap_or_else(|| panic!("{op}: {error}"));
        assert_eq!(denied.capability, format!("git.{op}"));
    }
    let error = worker::run(
        &vfs,
        &job_at(&vfs, "init", "/memory/nested"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .err()
    .unwrap();
    assert!(
        error
            .downcast_ref::<crate::runtime::host::PermissionDenied>()
            .is_some()
    );
    assert!(!volume.path().join("nested").exists());
    worker::run(&vfs, &job_at(&vfs, "status", "/memory"), "status", &[]).unwrap();
    let after = storage::read_files(
        &Dir::open_ambient_dir(volume.path(), cap_std::ambient_authority()).unwrap(),
        true,
        &AtomicBool::new(false),
        storage::MAX_BYTES,
    )
    .unwrap();
    assert_eq!(before, after, "nothing in the volume changed");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn mount_non_ascii_alias_cannot_host_a_repository() {
    let volume = tempfile::tempdir().unwrap();
    let vfs = Vfs::tempdir()
        .unwrap()
        .with_mount(crate::runtime::vfs::MountSpec {
            guest_path: "/skills".into(),
            host: volume.path().to_path_buf(),
            volume: "skills".into(),
            access: crate::runtime::vfs::Access::ReadWrite,
            quota: None,
        })
        .unwrap();
    // U+017F (long s) folds to `s` on a case-insensitive APFS volume.
    let error = worker::run(
        &vfs,
        &job_at(&vfs, "init", "/\u{17F}kills"),
        "init",
        &[json!({ "branch": "main" })],
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("mount point /skills"), "{error}");
    assert_eq!(
        std::fs::read_dir(vfs.root().join("skills"))
            .unwrap()
            .count(),
        0
    );
}
