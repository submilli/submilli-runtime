use super::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const RUN: &str = "019a019a-019a-419a-819a-019a019a019a";
const EFFECT: &str = "019b019b-019b-419b-819b-019b019b019b";

pub(crate) fn checkpoint(point: &str) {
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

async fn open_database(root: &Path) -> (Arc<ServerDatabase>, FilesystemContext) {
    let database = Arc::new(ServerDatabase::open(&root.join("state.db")).await.unwrap());
    let context = FilesystemContext::new(Arc::new(SecretCipher::from_key([7; 32])));
    (database, context)
}

#[tokio::test]
async fn crash_child() {
    submilli_durable::set_test_checkpoint(checkpoint);
    let Some(root) = std::env::var_os("SUBMILLI_FILESYSTEM_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let (database, context) = open_database(&root).await;
    let filesystem = open(
        Vfs::external(root.join("workspace")).unwrap(),
        &RUN.into(),
        database,
        context,
    )
    .await
    .unwrap();
    let kind = std::env::var("SUBMILLI_FILESYSTEM_CRASH_KIND").unwrap_or_default();
    if kind == "reader" {
        use interpreter::runtime::filesystem::FileReader;
        std::fs::write(filesystem.vfs().root().join("file"), b"abcdef").unwrap();
        let mut reader = filesystem
            .operation(None)
            .open_reader(&RUN.into(), "/file", byte_options())
            .await
            .unwrap();
        let invocation = interpreter::runtime::filesystem::Invocation {
            operation: Some(EFFECT.to_owned().into()),
            work: None,
        };
        reader
            .next(
                &invocation,
                &interpreter::runtime::limits::TenantLimits::new(1 << 20),
            )
            .await
            .unwrap();
    } else if kind == "listing" {
        use interpreter::runtime::filesystem::Filesystem;
        std::fs::write(filesystem.vfs().root().join("a"), b"a").unwrap();
        std::fs::write(filesystem.vfs().root().join("b"), b"b").unwrap();
        let opening = interpreter::runtime::filesystem::Invocation {
            operation: Some(RUN.to_owned().into()),
            work: None,
        };
        let mut listing = Filesystem::open_directory(
            &filesystem,
            &opening,
            "/",
            false,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20),
        )
        .await
        .unwrap();
        let invocation = interpreter::runtime::filesystem::Invocation {
            operation: Some(EFFECT.to_owned().into()),
            work: None,
        };
        listing
            .next_entry(
                &invocation,
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
    } else {
        std::fs::write(filesystem.vfs().root().join("file"), b"initial").unwrap();
        filesystem
            .operation(None)
            .write_bytes(&EFFECT.into(), "/file", b"suffix".to_vec(), true)
            .await
            .unwrap();
    }
    panic!("crash checkpoint not reached");
}

fn kill_at(root: &Path, point: &str) {
    kill_at_mode(root, point, "append");
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
    for point in [
        "effect-request",
        "effect-intent",
        "effect-applied",
        "effect-receipt",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("workspace")).unwrap();
        let child_root = root.path().to_owned();
        tokio::task::spawn_blocking(move || kill_at(&child_root, point))
            .await
            .unwrap();
        let (database, context) = open_database(root.path()).await;
        let filesystem = open(
            Vfs::external(root.path().join("workspace")).unwrap(),
            &RUN.into(),
            database,
            context,
        )
        .await
        .unwrap();
        filesystem
            .operation(None)
            .write_bytes(&EFFECT.into(), "/file", b"suffix".to_vec(), true)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
            b"initialsuffix"
        );
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
        let (database, context) = open_database(root.path()).await;
        let filesystem = open(
            Vfs::external(root.path().join("workspace")).unwrap(),
            &RUN.into(),
            database,
            context,
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
    use interpreter::runtime::filesystem::{
        Filesystem, Invocation, ResourceDescriptor, ResourceKind,
    };
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
            let (database, context) = open_database(root.path()).await;
            let filesystem = open(
                Vfs::external(root.path().join("workspace")).unwrap(),
                &RUN.into(),
                database,
                context,
            )
            .await
            .unwrap();
            let invocation = Invocation {
                operation: Some(EFFECT.to_owned().into()),
                work: None,
            };
            if kind == "reader" {
                let descriptor = ResourceDescriptor {
                    run_id: RUN.into(),
                    id: RUN.into(),
                    kind: ResourceKind::Reader(byte_options()),
                };
                let mut reader = Filesystem::restore_reader(&filesystem, descriptor).unwrap();
                let limits = interpreter::runtime::limits::TenantLimits::new(1 << 20);
                assert_eq!(
                    read_bytes(reader.next(&invocation, &limits).await.unwrap()).unwrap(),
                    b"abc",
                    "{point}"
                );
                assert_eq!(
                    read_bytes(reader.next(&resource_invocation(), &limits).await.unwrap())
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
                            &invocation,
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
                            &resource_invocation(),
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
