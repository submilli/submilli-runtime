use super::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUN: &str = "019a019a-019a-419a-819a-019a019a019a";
const EFFECT: &str = "019b019b-019b-419b-819b-019b019b019b";

pub(crate) fn test_checkpoint(point: &str) {
    if std::env::var("SUBMILLI_FILESYSTEM_CRASH_AT")
        .ok()
        .as_deref()
        != Some(point)
    {
        return;
    }
    let root = PathBuf::from(std::env::var_os("SUBMILLI_FILESYSTEM_CRASH_ROOT").unwrap());
    std::fs::write(root.join("checkpoint"), point).unwrap();
    loop {
        std::thread::park();
    }
}

async fn open_database(root: &Path) -> (Arc<ServerDatabase>, Arc<SecretCipher>) {
    let database = Arc::new(ServerDatabase::open(&root.join("state.db")).await.unwrap());
    let cipher = Arc::new(SecretCipher::from_key([7; 32]));
    (database, cipher)
}

#[tokio::test]
async fn crash_child() {
    submilli_durable::set_test_checkpoint(test_checkpoint);
    let Some(root) = std::env::var_os("SUBMILLI_FILESYSTEM_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let (database, cipher) = open_database(&root).await;
    let kind = std::env::var("SUBMILLI_FILESYSTEM_CRASH_KIND").unwrap_or_default();
    let mut vfs = Vfs::external(root.join("workspace")).unwrap();
    if kind.starts_with("quota-") {
        vfs = vfs.with_size_limit(if kind == "quota-append" { 131_073 } else { 8 });
    }
    let filesystem = open(vfs, &RUN.into(), database, cipher).await.unwrap();
    if let Some(operation) = kind.strip_prefix("quota-") {
        retry_quota_operation(&filesystem, operation).await;
    } else if kind == "reader" {
        use interpreter::runtime::filesystem::FileReader;
        std::fs::write(filesystem.vfs().root().join("file"), b"abcdef").unwrap();
        let mut reader = filesystem
            .operation(None)
            .open_reader(&RUN.into(), "/file", byte_options())
            .await
            .unwrap();
        let effect_id: EffectId = EFFECT.to_owned().into();
        reader
            .next(
                &effect_id,
                None,
                &interpreter::runtime::limits::TenantLimits::new(1 << 20),
            )
            .await
            .unwrap();
    } else if kind == "listing" {
        use interpreter::runtime::filesystem::Filesystem;
        std::fs::write(filesystem.vfs().root().join("a"), b"a").unwrap();
        std::fs::write(filesystem.vfs().root().join("b"), b"b").unwrap();
        let opening: EffectId = RUN.to_owned().into();
        let mut listing = Filesystem::open_directory(
            &filesystem,
            &opening,
            None,
            "/",
            false,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20),
        )
        .await
        .unwrap();
        let effect_id: EffectId = EFFECT.to_owned().into();
        listing
            .next_entry(
                &effect_id,
                None,
                &interpreter::runtime::limits::TenantLimits::new(64 << 20),
            )
            .await
            .unwrap();
    } else if std::env::var("SUBMILLI_FILESYSTEM_CRASH_KIND").as_deref() == Ok("writer") {
        filesystem
            .operation(None)
            .open_writer(&RUN.into(), "/file")
            .await
            .unwrap();
        filesystem
            .operation(None)
            .write_chunk(&RUN.into(), &RUN.into(), b"suffix".to_vec())
            .await
            .unwrap();
        filesystem
            .operation(None)
            .close_writer(&RUN.into(), &EFFECT.into())
            .await
            .unwrap();
    } else if matches!(kind.as_str(), "move" | "remove" | "mkdir" | "rmdir") {
        let workspace = filesystem.vfs().root();
        std::fs::create_dir(workspace.join("a")).unwrap();
        std::fs::create_dir(workspace.join("b")).unwrap();
        std::fs::create_dir(workspace.join("empty")).unwrap();
        std::fs::write(workspace.join("a/source"), b"source").unwrap();
        std::fs::write(workspace.join("b/destination"), b"old").unwrap();
        std::fs::write(workspace.join("file"), b"remove").unwrap();
        retry_namespace(&filesystem, &kind).await;
    } else if kind == "write" {
        std::fs::write(filesystem.vfs().root().join("file"), b"initial").unwrap();
        filesystem
            .operation(None)
            .write_bytes(&EFFECT.into(), "/file", b"replacement".to_vec())
            .await
            .unwrap();
    } else {
        if kind != "append-create" {
            std::fs::write(filesystem.vfs().root().join("file"), b"initial").unwrap();
        }
        filesystem
            .operation(None)
            .append_bytes(&EFFECT.into(), "/file", b"suffix".to_vec())
            .await
            .unwrap();
    }
    panic!("crash checkpoint not reached");
}

fn kill_at_mode(root: &Path, point: &str, kind: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "filesystems::tests::process::crash_child",
            "--nocapture",
        ])
        .env("SUBMILLI_FILESYSTEM_CRASH_ROOT", root)
        .env("SUBMILLI_FILESYSTEM_CRASH_AT", point)
        .env("SUBMILLI_FILESYSTEM_CRASH_KIND", kind)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("checkpoint").exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before {point}: {status}");
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("checkpoint timeout: {point}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
}

#[tokio::test]
async fn explicit_retries_after_process_restart_finish_filesystem_operations() {
    for (kind, expected) in [
        ("append", b"initialsuffix".as_slice()),
        ("append-create", b"suffix"),
    ] {
        for point in [
            "effect-request",
            "effect-intent",
            "effect-applied",
            "effect-receipt",
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("workspace")).unwrap();
            let child_root = root.path().to_owned();
            tokio::task::spawn_blocking(move || kill_at_mode(&child_root, point, kind))
                .await
                .unwrap();
            let (database, cipher) = open_database(root.path()).await;
            let filesystem = open(
                Vfs::external(root.path().join("workspace")).unwrap(),
                &RUN.into(),
                database,
                cipher,
            )
            .await
            .unwrap();
            let operation = filesystem.operation(None);
            let effect = EFFECT.into();
            operation
                .append_bytes(&effect, "/file", b"suffix".to_vec())
                .await
                .unwrap();
            assert_eq!(
                std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
                expected,
                "{kind} at {point}"
            );
            // Once completed, append must not add bytes to a later file.
            std::fs::write(filesystem.vfs().root().join("file"), b"later").unwrap();
            operation
                .append_bytes(&effect, "/file", b"suffix".to_vec())
                .await
                .unwrap();
            assert_eq!(
                std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
                b"later",
                "{kind} at {point}"
            );
        }
    }
}

#[tokio::test]
async fn writer_restart_repairs_unacknowledged_output_tail_and_close() {
    for point in [
        "writer-staged",
        "writer-chunk-receipt",
        "writer-closed",
        "effect-request",
        "effect-intent",
        "effect-applied",
        "effect-receipt",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("workspace")).unwrap();
        let child_root = root.path().to_owned();
        tokio::task::spawn_blocking(move || kill_at_mode(&child_root, point, "writer"))
            .await
            .unwrap();
        let (database, cipher) = open_database(root.path()).await;
        if matches!(point, "effect-intent" | "effect-applied") {
            let units =
                crate::adapters::unit_of_work::durable::units(database.clone(), cipher.clone());
            let mut unit = units().await.unwrap();
            let journal_entry = unit
                .get_journal_entry(&RUN.into(), &EFFECT.into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&journal_entry.state.unwrap()).unwrap(),
                serde_json::json!({"Prepared": {}})
            );
        }
        let filesystem = open(
            Vfs::external(root.path().join("workspace")).unwrap(),
            &RUN.into(),
            database,
            cipher,
        )
        .await
        .unwrap();
        filesystem
            .operation(None)
            .open_writer(&RUN.into(), "/file")
            .await
            .unwrap();
        filesystem
            .operation(None)
            .write_chunk(&RUN.into(), &RUN.into(), b"suffix".to_vec())
            .await
            .unwrap();
        filesystem
            .operation(None)
            .close_writer(&RUN.into(), &EFFECT.into())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
            b"suffix",
            "{point}"
        );
        std::fs::write(filesystem.vfs().root().join("file"), b"later").unwrap();
        filesystem
            .operation(None)
            .close_writer(&RUN.into(), &EFFECT.into())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
            b"later"
        );
    }
}

#[tokio::test]
async fn resource_progress_survives_process_death_before_framework_completion() {
    use interpreter::runtime::filesystem::{Filesystem, ResourceDescriptor, ResourceKind};
    for (kind, points) in [
        ("reader", ["reader-open", "reader-read", "reader-progress"]),
        (
            "listing",
            ["listing-open", "listing-next", "listing-progress"],
        ),
    ] {
        for point in points {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("workspace")).unwrap();
            let child_root = root.path().to_owned();
            tokio::task::spawn_blocking(move || kill_at_mode(&child_root, point, kind))
                .await
                .unwrap();
            let (database, cipher) = open_database(root.path()).await;
            let filesystem = open(
                Vfs::external(root.path().join("workspace")).unwrap(),
                &RUN.into(),
                database,
                cipher,
            )
            .await
            .unwrap();
            let effect_id: EffectId = EFFECT.to_owned().into();
            if kind == "reader" {
                let descriptor = ResourceDescriptor {
                    run_id: RUN.into(),
                    id: RUN.into(),
                    kind: ResourceKind::Reader(byte_options()),
                };
                let mut reader = Filesystem::restore_reader(&filesystem, descriptor).unwrap();
                let limits = interpreter::runtime::limits::TenantLimits::new(1 << 20);
                assert_eq!(
                    read_bytes(reader.next(&effect_id, None, &limits).await.unwrap()).unwrap(),
                    b"abc",
                    "{point}"
                );
                assert_eq!(
                    read_bytes(
                        reader
                            .next(&resource_effect_id(), None, &limits)
                            .await
                            .unwrap()
                    )
                    .unwrap(),
                    b"def"
                );
            } else {
                let descriptor = ResourceDescriptor {
                    run_id: RUN.into(),
                    id: RUN.into(),
                    kind: ResourceKind::Directory,
                };
                let mut listing = Filesystem::restore_directory(&filesystem, descriptor).unwrap();
                assert_eq!(
                    listing
                        .next_entry(
                            &effect_id,
                            None,
                            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
                        )
                        .await
                        .unwrap()
                        .unwrap()
                        .path,
                    "/a",
                    "{point}"
                );
                assert_eq!(
                    listing
                        .next_entry(
                            &resource_effect_id(),
                            None,
                            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
                        )
                        .await
                        .unwrap()
                        .unwrap()
                        .path,
                    "/b"
                );
            }
        }
    }
}

async fn retry_namespace(filesystem: &DurableFilesystem, kind: &str) {
    let operation = filesystem.operation(None);
    let effect = EFFECT.into();
    match kind {
        "move" => operation
            .move_path(&effect, "/a/source", "/b/destination")
            .await
            .unwrap(),
        "remove" => operation.remove_path(&effect, "/file").await.unwrap(),
        "mkdir" => operation
            .create_directory(&effect, "/created")
            .await
            .unwrap(),
        "rmdir" => operation.remove_path(&effect, "/empty").await.unwrap(),
        _ => panic!("unknown namespace test operation"),
    };
}

#[tokio::test]
async fn namespace_retries_survive_process_death_without_original_backups() {
    for (kind, points) in [
        (
            "move",
            &[
                "effect-intent",
                "destination-published",
                "source-removed",
                "effect-receipt",
            ][..],
        ),
        (
            "remove",
            &["effect-intent", "source-removed", "effect-receipt"][..],
        ),
        (
            "mkdir",
            &["effect-intent", "directory-created", "effect-receipt"][..],
        ),
        (
            "rmdir",
            &["effect-intent", "directory-removed", "effect-receipt"][..],
        ),
    ] {
        for &point in points {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("workspace")).unwrap();
            let child_root = root.path().to_owned();
            tokio::task::spawn_blocking(move || kill_at_mode(&child_root, point, kind))
                .await
                .unwrap();
            let (database, cipher) = open_database(root.path()).await;
            let filesystem = open(
                Vfs::external(root.path().join("workspace")).unwrap(),
                &RUN.into(),
                database,
                cipher,
            )
            .await
            .unwrap();
            retry_namespace(&filesystem, kind).await;
            let workspace = filesystem.vfs().root();
            match kind {
                "move" => {
                    assert!(!workspace.join("a/source").exists());
                    assert_eq!(
                        std::fs::read(workspace.join("b/destination")).unwrap(),
                        b"source"
                    );
                    std::fs::write(workspace.join("a/source"), b"later source").unwrap();
                    std::fs::write(workspace.join("b/destination"), b"later destination").unwrap();
                    retry_namespace(&filesystem, kind).await;
                    assert_eq!(
                        std::fs::read(workspace.join("a/source")).unwrap(),
                        b"later source"
                    );
                    assert_eq!(
                        std::fs::read(workspace.join("b/destination")).unwrap(),
                        b"later destination"
                    );
                }
                "mkdir" => {
                    assert!(workspace.join("created").is_dir());
                    std::fs::remove_dir(workspace.join("created")).unwrap();
                    std::fs::write(workspace.join("created"), b"later file").unwrap();
                    retry_namespace(&filesystem, kind).await;
                    assert_eq!(
                        std::fs::read(workspace.join("created")).unwrap(),
                        b"later file"
                    );
                }
                "remove" | "rmdir" => {
                    let path = if kind == "remove" { "file" } else { "empty" };
                    assert!(!workspace.join(path).exists());
                    std::fs::write(workspace.join(path), b"later file").unwrap();
                    retry_namespace(&filesystem, kind).await;
                    assert_eq!(std::fs::read(workspace.join(path)).unwrap(), b"later file");
                }
                _ => unreachable!(),
            }
        }
    }
}

async fn retry_quota_operation(filesystem: &DurableFilesystem, kind: &str) {
    let operation = filesystem.operation(None);
    let effect = EFFECT.into();
    match kind {
        "write" => {
            operation
                .write_bytes(&effect, "/file", b"replaced".to_vec())
                .await
                .unwrap();
        }
        "append" => {
            operation
                .append_bytes(&effect, "/file", vec![b'x'; 131_073])
                .await
                .unwrap();
        }
        "move" => {
            operation
                .move_path(&effect, "/source", "/destination")
                .await
                .unwrap();
        }
        "remove" => {
            operation.remove_path(&effect, "/file").await.unwrap();
        }
        "writer" => {
            let writer = RUN.into();
            operation.open_writer(&writer, "/file").await.unwrap();
            operation
                .write_chunk(&writer, &writer, b"replaced".to_vec())
                .await
                .unwrap();
            operation.close_writer(&writer, &effect).await.unwrap();
        }
        _ => panic!("unknown quota case: {kind}"),
    }
}

#[tokio::test]
async fn quota_restart_ignores_private_files_and_repairs_unaccounted_public_changes() {
    for (kind, points, expected) in [
        (
            "write",
            vec![
                "candidate-sync",
                "replacement-published-unaccounted",
                "write-synced",
            ],
            8,
        ),
        (
            "append",
            vec!["append-written-unaccounted", "effect-receipt"],
            131_073,
        ),
        (
            "move",
            vec![
                "move-published-unaccounted",
                "source-removed",
                "effect-receipt",
            ],
            6,
        ),
        (
            "remove",
            vec!["remove-unlinked-unaccounted", "effect-receipt"],
            0,
        ),
        (
            "writer",
            vec![
                "writer-staged",
                "effect-intent",
                "replacement-published-unaccounted",
                "effect-receipt",
            ],
            8,
        ),
    ] {
        for point in points {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            if kind == "move" {
                std::fs::write(workspace.join("source"), b"source").unwrap();
                std::fs::write(workspace.join("destination"), b"xx").unwrap();
            } else if kind != "append" {
                std::fs::write(workspace.join("file"), b"old").unwrap();
            }
            let child_root = root.path().to_owned();
            tokio::task::spawn_blocking(move || {
                kill_at_mode(&child_root, point, &format!("quota-{kind}"));
            })
            .await
            .unwrap();
            let public_bytes = if kind == "move" {
                6 // Every selected move point has replaced the old destination.
            } else {
                match std::fs::metadata(workspace.join("file")) {
                    Ok(metadata) => metadata.len(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                    Err(error) => panic!("read public file size: {error}"),
                }
            };
            let (database, cipher) = open_database(root.path()).await;
            let limit = if kind == "append" { 131_073 } else { 8 };
            let filesystem = open(
                Vfs::external(workspace).unwrap().with_size_limit(limit),
                &RUN.into(),
                database,
                cipher,
            )
            .await
            .unwrap();
            let quota = filesystem.vfs().quota().unwrap();
            assert_eq!(quota.used(), public_bytes, "rebuild {kind} at {point}");
            retry_quota_operation(&filesystem, kind).await;
            assert_eq!(quota.used(), expected, "retry {kind} at {point}");
            retry_quota_operation(&filesystem, kind).await;
            assert_eq!(quota.used(), expected, "duplicate {kind} at {point}");
        }
    }
}

#[tokio::test]
async fn byte_write_restarts_from_scratch_without_a_filesystem_receipt() {
    for point in [
        "write-temp-created",
        "write-temp-written",
        "candidate-sync",
        "replacement-published-unaccounted",
        "published",
        "write-synced",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("workspace")).unwrap();
        let child_root = root.path().to_owned();
        tokio::task::spawn_blocking(move || kill_at_mode(&child_root, point, "write"))
            .await
            .unwrap();
        let (database, cipher) = open_database(root.path()).await;
        let filesystem = open(
            Vfs::external(root.path().join("workspace")).unwrap(),
            &RUN.into(),
            database,
            cipher,
        )
        .await
        .unwrap();
        // The outer completion is still missing, so replay supplies the same input.
        // Completed effects would be skipped by the framework (WriteRecovery.tla).
        for _ in 0..2 {
            filesystem
                .operation(None)
                .write_bytes(&EFFECT.into(), "/file", b"replacement".to_vec())
                .await
                .unwrap();
            assert_eq!(
                std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
                b"replacement",
                "{point}"
            );
        }
    }
}
