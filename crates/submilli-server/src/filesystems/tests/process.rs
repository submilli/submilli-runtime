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

async fn open(root: &Path) -> (Arc<ServerDatabase>, DurableObjects) {
    let database = Arc::new(ServerDatabase::open(&root.join("state.db")).await.unwrap());
    let objects = DurableObjects::open(
        DurableObjectConfig {
            directory: root.join("objects"),
            ..Default::default()
        },
        database.clone(),
        Arc::new(SecretCipher::from_key([7; 32])),
    )
    .await
    .unwrap();
    (database, objects)
}

#[tokio::test]
async fn crash_child() {
    let Some(root) = std::env::var_os("SUBMILLI_FILESYSTEM_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let (database, objects) = open(&root).await;
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state) VALUES(?,?,'session','request','startup',1,0,'running')")
            .bind(RUN).bind(RUN).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let filesystem = DurableFilesystem::retained_ephemeral(
        &root.join("ephemeral"),
        RUN,
        "owner",
        database.clone(),
        objects.clone(),
    )
    .await
    .unwrap();
    std::fs::write(filesystem.vfs().root().join("file"), b"initial").unwrap();
    let id = ObjectId::parse("019c019c-019c-419c-819c-019c019c019c".into()).unwrap();
    objects
        .reserve(id.clone(), "owner".into(), [1; 32], 6)
        .await
        .unwrap();
    let prepared = objects
        .publish(&id, Cursor::new(b"suffix".to_vec()))
        .await
        .unwrap();
    let reference = objects
        .bind(&prepared, BTreeSet::from([Retention::Run(RUN.into())]))
        .await
        .unwrap();
    let request = Mutation::Append {
        path: "/file".into(),
        content: Content {
            id: reference.id.as_str().into(),
            bytes: reference.content.bytes,
            sha256: reference.content.sha256,
        },
    };
    std::fs::write(root.join("request"), serde_json::to_vec(&request).unwrap()).unwrap();
    filesystem.execute(EFFECT, request).await.unwrap();
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query(
                    "UPDATE runs SET state='finished',completed_at_unix_ms=1 WHERE run_id=?",
                )
                .bind(RUN)
                .execute(connection)
                .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    filesystem.retire("owner").await.unwrap();
    panic!("crash checkpoint not reached");
}

fn kill_at(root: &Path, point: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "filesystems::tests::process::crash_child",
            "--nocapture",
        ])
        .env("SUBMILLI_FILESYSTEM_CRASH_ROOT", root)
        .env("SUBMILLI_FILESYSTEM_CRASH_AT", point)
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
async fn process_restart_recovers_sql_filesystem_and_cleanup_boundaries() {
    for point in [
        "workspace-reserved",
        "workspace-root",
        "workspace-layout",
        "workspace-active",
        "effect-request",
        "effect-intent",
        "effect-applied",
        "effect-receipt",
        "retirement",
        "filesystem-removed",
        "retired",
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("objects")).unwrap();
        std::fs::create_dir(root.path().join("ephemeral")).unwrap();
        let child_root = root.path().to_owned();
        tokio::task::spawn_blocking(move || kill_at(&child_root, point))
            .await
            .unwrap();
        let (database, objects) = open(root.path()).await;
        let id = workspace_id(RUN, objects.store_identity());
        if ["retirement", "filesystem-removed", "retired"].contains(&point) {
            DurableFilesystem::recover_cleanup(&id, "owner", database, objects)
                .await
                .unwrap();
            assert!(
                !root
                    .path()
                    .join("ephemeral")
                    .join(format!("submilli-vfs-run-{id}"))
                    .exists()
            );
            continue;
        }
        let filesystem = DurableFilesystem::retained_ephemeral(
            &root.path().join("ephemeral"),
            RUN,
            "owner",
            database,
            objects,
        )
        .await
        .unwrap();
        if point.starts_with("workspace-") {
            assert!(filesystem.vfs().root().is_dir());
            continue;
        }
        let request: Mutation =
            serde_json::from_slice(&std::fs::read(root.path().join("request")).unwrap()).unwrap();
        filesystem.recover().await.unwrap();
        filesystem.execute(EFFECT, request).await.unwrap();
        assert_eq!(
            std::fs::read(filesystem.vfs().root().join("file")).unwrap(),
            b"initialsuffix"
        );
    }
}
