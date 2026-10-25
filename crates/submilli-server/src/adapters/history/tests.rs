use super::*;
use interpreter::runtime::effects::{CODEC_VERSION, EffectHistory, NativeError, OutcomeBytes};
use submilli_durable::history::History;

pub(super) fn checkpoint(point: &str) {
    if std::env::var("SUBMILLI_HISTORY_CRASH").as_deref() == Ok(point) {
        let root = std::env::var("SUBMILLI_HISTORY_ROOT").unwrap();
        std::fs::write(std::path::Path::new(&root).join("checkpoint"), point).unwrap();
        loop {
            std::thread::park();
        }
    }
}

async fn fixture(
    root: &std::path::Path,
    threshold: usize,
) -> (Arc<ServerDatabase>, Arc<HistoryObjects>, ExecutionAttempt) {
    let database = Arc::new(ServerDatabase::open(&root.join("db")).await.unwrap());
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,attempt,admission_order,created_at_unix_ms,state,recovery_mode) VALUES('run','run','session','fp','owner',1,1,0,'running','recorded')")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let path = root.join("objects");
    std::fs::create_dir_all(&path).unwrap();
    let objects = HistoryObjects::open(
        database.clone(),
        path,
        Arc::new(SecretCipher::from_key([9; 32])),
        DurableObjectConfig {
            inline_threshold: threshold,
        },
    )
    .await
    .unwrap();
    (
        database,
        objects,
        ExecutionAttempt {
            run_id: "run".into(),
            owner_generation: "owner".into(),
            attempt: 1,
        },
    )
}
fn entry(size: usize) -> HistoryEntry {
    HistoryEntry {
        sequence: 0,
        operation: "value".into(),
        version: CODEC_VERSION,
        arguments_fingerprint: [3; 32],
        outcome: Ok(OutcomeBytes(vec![42; size])),
    }
}
async fn kind(db: &ServerDatabase) -> i64 {
    db.read(|c| {
        Box::pin(async move {
            Ok(sqlx::query_scalar(
                "SELECT storage_kind FROM durable_outcomes WHERE run_id='run' AND sequence=0",
            )
            .fetch_one(c)
            .await?)
        })
    })
    .await
    .unwrap()
}
fn object_ids(root: &std::path::Path) -> Vec<String> {
    match std::fs::read_dir(root.join("objects/session/run")) {
        Ok(entries) => entries
            .map(|e| {
                e.unwrap()
                    .path()
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{error}"),
    }
}
async fn mark_terminal(db: &ServerDatabase) {
    db.transaction(|c| {
        Box::pin(async move {
            sqlx::query(
                "UPDATE runs SET state='completed',completed_at_unix_ms=1 WHERE run_id='run'",
            )
            .execute(c)
            .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
}
async fn retire(db: &ServerDatabase, objects: &HistoryObjects) -> Result<(), StoreError> {
    mark_terminal(db).await;
    objects.remove_run("session", &"run".into()).await?;
    db.transaction(|c| Box::pin(crate::adapters::run_recovery::retire(c, "run")))
        .await
        .map_err(invalid)?;
    checkpoint("retired");
    Ok(())
}

#[tokio::test]
async fn threshold_boundaries_preserve_payload_and_accounting() {
    let sample = entry(2048);
    let bytes = entries::encode(
        &Sealer::new(Some(Arc::new(SecretCipher::from_key([9; 32])))),
        &"run".into(),
        &sample,
    )
    .unwrap();
    for (threshold, expected) in [
        (bytes.plaintext_bytes - 1, 1),
        (bytes.plaintext_bytes, 0),
        (bytes.plaintext_bytes + 1, 0),
        (0, 1),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (db, objects, attempt) = fixture(root.path(), threshold).await;
        let history = objects.history();
        history
            .append(&attempt, Arc::new(sample.clone()))
            .await
            .unwrap();
        assert_eq!(kind(&db).await, expected);
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(sample.clone())
        );
        let progress = history.progress(&attempt).await.unwrap();
        assert_eq!(
            (progress.position, progress.bytes),
            (1, bytes.content.len() as u64)
        );
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(sample.clone())
        );
        retire(&db, &objects).await.unwrap();
        assert!(object_ids(root.path()).is_empty());
        assert_eq!(
            std::fs::read_dir(root.path().join("objects"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn mixed_legacy_and_external_history_reopens_with_a_different_threshold() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    let old = entry(16);
    let mut unit = crate::adapters::unit_of_work::durable::units(
        db.clone(),
        Arc::new(SecretCipher::from_key([9; 32])),
    )()
    .await
    .unwrap();
    unit.append_history(&attempt, Arc::new(old.clone()))
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let mut next = entry(4096);
    next.sequence = 1;
    let history = objects.history();
    history
        .append(&attempt, Arc::new(next.clone()))
        .await
        .unwrap();
    drop(history);
    drop(objects);
    db.close().await.unwrap();
    drop(db);
    let (_db, objects, attempt) = fixture(root.path(), MAX_ENTRY_BYTES).await;
    let history = History::with_store(attempt, objects.history());
    let first = history.begin("value", [3; 32]).await.unwrap();
    assert_eq!(first.recorded, Some(old.outcome));
    let second = history.begin("value", [3; 32]).await.unwrap();
    assert_eq!(second.recorded, Some(next.outcome));
    history.ensure_history_consumed().await.unwrap();
    assert_eq!(object_ids(root.path()).len(), 1);
}

#[tokio::test]
async fn large_errors_are_replayed_without_becoming_successes() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    let outcome = Err(NativeError::Type("native failure".repeat(1024)));
    let live = History::with_store(attempt.clone(), objects.history());
    let call = live.begin("call", [0; 32]).await.unwrap();
    live.complete(&call, outcome.clone()).await.unwrap();
    let replay = History::with_store(attempt, objects.history());
    assert_eq!(
        replay.begin("call", [0; 32]).await.unwrap().recorded,
        Some(outcome)
    );
    assert_eq!(kind(&db).await, 1);
}

#[tokio::test]
async fn encryption_covers_sql_wal_and_object_files() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    let canary = b"SECRET-PAYLOAD-CANARY-1846";
    let mut value = entry(0);
    value.outcome = Ok(OutcomeBytes(canary.repeat(1000)));
    objects
        .history()
        .append(&attempt, Arc::new(value.clone()))
        .await
        .unwrap();
    for directory in [
        root.path().to_path_buf(),
        root.path().join("objects/session/run"),
    ] {
        for path in std::fs::read_dir(directory).unwrap() {
            let path = path.unwrap().path();
            if path.is_file() {
                let bytes = std::fs::read(&path).unwrap();
                assert!(
                    !bytes.windows(canary.len()).any(|window| window == canary),
                    "{} contains plaintext",
                    path.display()
                );
            }
        }
    }
    let size: i64 = db
        .read(|c| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT length(content) FROM durable_outcomes")
                        .fetch_one(c)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert!(size < 4096);
    assert_eq!(
        objects.history().get(&attempt.run_id, 0).await.unwrap(),
        Some(value)
    );
}

#[tokio::test]
async fn missing_corrupt_and_truncated_objects_never_become_missing_outcomes() {
    for damage in ["missing", "corrupt", "truncate"] {
        let root = tempfile::tempdir().unwrap();
        let (db, objects, attempt) = fixture(root.path(), 0).await;
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        let id = object_ids(root.path()).pop().unwrap();
        let path = root
            .path()
            .join("objects/session/run")
            .join(format!("{id}.object"));
        let mut content = std::fs::read(&path).unwrap();
        match damage {
            "missing" => std::fs::remove_file(path).unwrap(),
            "corrupt" => {
                content[60] ^= 1;
                std::fs::write(path, content).unwrap();
            }
            _ => {
                content.truncate(60);
                std::fs::write(path, content).unwrap();
            }
        }
        let replay = History::with_store(attempt, objects.history());
        assert!(replay.begin("value", [3; 32]).await.is_err());
        assert_eq!(kind(&db).await, 1);
    }
}

#[tokio::test]
async fn wrong_key_and_concurrent_store_owners_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    objects
        .history()
        .append(&attempt, Arc::new(entry(4096)))
        .await
        .unwrap();
    let directory = root.path().join("objects");
    assert!(
        HistoryObjects::open(
            db.clone(),
            directory.clone(),
            Arc::new(SecretCipher::from_key([9; 32])),
            DurableObjectConfig::default()
        )
        .await
        .is_err()
    );
    drop(objects);
    let wrong_key = HistoryObjects::open(
        db.clone(),
        directory.clone(),
        Arc::new(SecretCipher::from_key([8; 32])),
        DurableObjectConfig::default(),
    )
    .await
    .unwrap();
    assert!(wrong_key.history().get(&attempt.run_id, 0).await.is_err());
    drop(wrong_key);
}

#[tokio::test]
async fn collection_retries_failed_deletion_and_keeps_recoverable_history() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    let sample = entry(4096);
    objects
        .history()
        .append(&attempt, Arc::new(sample.clone()))
        .await
        .unwrap();
    db.transaction(|c| {
        Box::pin(async move {
            sqlx::query("UPDATE runs SET state='blocked' WHERE run_id='run'")
                .execute(c)
                .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(
        objects.history().get(&attempt.run_id, 0).await.unwrap(),
        Some(sample)
    );
    let id = object_ids(root.path()).pop().unwrap();
    let path = root
        .path()
        .join("objects/session/run")
        .join(format!("{id}.object"));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(retire(&db, &objects).await.is_err());
    assert_eq!(kind(&db).await, 1);
    assert_eq!(object_ids(root.path()), [id]);
    std::fs::remove_dir(&path).unwrap();
    retire(&db, &objects).await.unwrap();
    assert!(object_ids(root.path()).is_empty());
}

#[tokio::test]
async fn stale_attempt_and_history_limit_do_not_publish_a_reference() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, mut attempt) = fixture(root.path(), 0).await;
    attempt.attempt = 2;
    assert!(
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .is_err()
    );
    assert!(object_ids(root.path()).is_empty());
    attempt.attempt = 1;
    db.transaction(|c| {
        Box::pin(async move {
            sqlx::query("UPDATE runs SET history_bytes=1073741824 WHERE run_id='run'")
                .execute(c)
                .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert!(
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .is_err()
    );
    assert_eq!(
        objects.history().progress(&attempt).await.unwrap().position,
        0
    );
    assert!(
        objects
            .history()
            .get(&attempt.run_id, 0)
            .await
            .unwrap()
            .is_none()
    );
    retire(&db, &objects).await.unwrap();
    assert!(object_ids(root.path()).is_empty());
    assert!(
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .is_err()
    );
    assert!(!root.path().join("objects/session").exists());
}

#[test]
fn object_process_child() {
    let Ok(root) = std::env::var("SUBMILLI_HISTORY_ROOT") else {
        return;
    };
    let phase = std::env::var("SUBMILLI_HISTORY_CRASH").unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (db, objects, attempt) = fixture(std::path::Path::new(&root), 0).await;
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        if ["unlinked", "deletion-synced", "retired"].contains(&phase.as_str()) {
            retire(&db, &objects).await.unwrap();
        }
    });
    panic!("checkpoint was not reached");
}

#[test]
fn process_kill_at_publication_commit_and_collection_boundaries() {
    use std::process::{Command, Stdio};
    for phase in [
        "staging",
        "written",
        "file-synced",
        "published",
        "directory-synced",
        "committed",
        "unlinked",
        "deletion-synced",
        "retired",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "adapters::history::tests::object_process_child",
                "--nocapture",
            ])
            .env("SUBMILLI_HISTORY_ROOT", root.path())
            .env("SUBMILLI_HISTORY_CRASH", phase)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !root.path().join("checkpoint").exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("child exited before {phase}: {status}");
            }
            if std::time::Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("timed out at {phase}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (db, objects, attempt) = fixture(root.path(), 0).await;
            let history = objects.history();
            if phase == "committed" {
                assert_eq!(
                    history.get(&attempt.run_id, 0).await.unwrap(),
                    Some(entry(4096))
                );
                assert_eq!(history.progress(&attempt).await.unwrap().position, 1);
            } else if !["unlinked", "deletion-synced", "retired"].contains(&phase) {
                assert!(
                    history.get(&attempt.run_id, 0).await.unwrap().is_none(),
                    "{phase}"
                );
                assert_eq!(history.progress(&attempt).await.unwrap().position, 0);
            }
            retire(&db, &objects).await.unwrap();
            assert!(history.get(&attempt.run_id, 0).await.unwrap().is_none());
            assert!(!root.path().join("objects/session").exists(), "{phase}");
        });
    }
}

pub(super) type Hook = Arc<dyn Fn(&str) -> Result<(), StoreError> + Send + Sync>;
pub(super) type FileHook = std::sync::Mutex<Option<Hook>>;

#[tokio::test]
async fn cancelled_writer_is_drained_before_collection_and_reports_fuel_once() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    let (entered, started) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered));
    let (release, released) = std::sync::mpsc::channel();
    let released = std::sync::Mutex::new(released);
    objects.files.set_hook(Arc::new(move |point| {
        if point == "written" {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            released.lock().unwrap().recv().unwrap();
        }
        Ok(())
    }));
    let history = objects.history();
    let live = history.clone();
    let writer = tokio::spawn(async move { live.append(&attempt, Arc::new(entry(4096))).await });
    started.await.unwrap();
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());
    let draining = history.clone();
    let drain = tokio::spawn(async move { draining.drain().await });
    mark_terminal(&db).await;
    let collecting = objects.clone();
    let collector =
        tokio::spawn(async move { collecting.remove_run("session", &"run".into()).await });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!drain.is_finished());
    assert!(!collector.is_finished());
    release.send(()).unwrap();
    drain.await.unwrap().unwrap();
    collector.await.unwrap().unwrap();
    assert!(history.take_storage_fuel() > fuel::HASH.cost(4096));
    assert_eq!(history.take_storage_fuel(), 0);
    assert!(object_ids(root.path()).is_empty());
    assert!(history.get(&"run".into(), 0).await.unwrap().is_none());
}

#[tokio::test]
async fn reader_protects_object_while_its_run_is_retired() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    objects
        .history()
        .append(&attempt, Arc::new(entry(4096)))
        .await
        .unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered));
    let (release, released) = std::sync::mpsc::channel();
    let released = std::sync::Mutex::new(released);
    objects.files.set_hook(Arc::new(move |point| {
        if point == "reading" {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            released.lock().unwrap().recv().unwrap();
        }
        Ok(())
    }));
    let history = objects.history();
    let reader = tokio::spawn(async move { history.get(&attempt.run_id, 0).await });
    started.await.unwrap();
    mark_terminal(&db).await;
    let collecting = objects.clone();
    let collector =
        tokio::spawn(async move { collecting.remove_run("session", &"run".into()).await });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!collector.is_finished());
    release.send(()).unwrap();
    assert_eq!(reader.await.unwrap().unwrap(), Some(entry(4096)));
    collector.await.unwrap().unwrap();
    assert!(object_ids(root.path()).is_empty());
}

#[tokio::test]
async fn failed_write_or_sync_leaves_no_outcome_and_a_healthy_retry_succeeds() {
    for (phase, kind) in [
        ("staging", std::io::ErrorKind::PermissionDenied),
        ("written", std::io::ErrorKind::StorageFull),
        ("published", std::io::ErrorKind::Other),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (db, objects, attempt) = fixture(root.path(), 0).await;
        objects.files.set_hook(Arc::new(move |point| {
            if point == phase {
                return Err(invalid(std::io::Error::new(
                    kind,
                    "injected storage failure",
                )));
            }
            Ok(())
        }));
        let history = objects.history();
        assert!(
            history
                .append(&attempt, Arc::new(entry(4096)))
                .await
                .is_err()
        );
        assert!(history.get(&attempt.run_id, 0).await.unwrap().is_none());
        assert!(!object_ids(root.path()).is_empty());
        objects.files.set_hook(Arc::new(|_| Ok(())));
        history
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(entry(4096))
        );
        retire(&db, &objects).await.unwrap();
        assert!(object_ids(root.path()).is_empty());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn object_symlinks_and_writable_aliases_are_rejected() {
    use std::os::unix::fs::symlink;
    for alias in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (_db, objects, attempt) = fixture(root.path(), 0).await;
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        let id = object_ids(root.path()).pop().unwrap();
        let path = root
            .path()
            .join("objects/session/run")
            .join(format!("{id}.object"));
        let outside = root.path().join("outside");
        if alias {
            std::fs::hard_link(&path, &outside).unwrap();
        } else {
            std::fs::rename(&path, &outside).unwrap();
            symlink(&outside, &path).unwrap();
        }
        assert!(objects.history().get(&attempt.run_id, 0).await.is_err());
    }
}

#[cfg(unix)]
#[test]
fn store_open_needs_no_directory_read_access_above_the_volume_root() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let ancestor = temporary.path().join("traverse-only");
    let directory = ancestor.join("volumes/durable-objects");
    std::fs::create_dir_all(&directory).unwrap();
    let original = std::fs::metadata(&ancestor).unwrap().permissions();
    std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o111)).unwrap();
    let opened = files::ObjectFiles::open(&directory);
    std::fs::set_permissions(&ancestor, original).unwrap();
    assert!(opened.is_ok(), "{}", opened.err().unwrap());
}

#[test]
fn directory_ids_are_single_unchanged_components() {
    for id in [
        "",
        ".",
        "..",
        "../other",
        "/tmp/other",
        "a/b",
        "a/",
        "a//b",
        "a\\b",
        "a\0b",
    ] {
        assert!(
            files::RunDirectory::new(id, "run").is_err(),
            "session: {id:?}"
        );
        assert!(
            files::RunDirectory::new("session", id).is_err(),
            "run: {id:?}"
        );
    }
    assert!(files::RunDirectory::new(&"a".repeat(256), "run").is_ok());
    assert!(files::RunDirectory::new("session", &"a".repeat(256)).is_ok());
    for id in [
        "run-123",
        "session_123",
        "custom.session",
        "שם",
        "my session",
    ] {
        assert!(files::RunDirectory::new(id, id).is_ok(), "{id}");
    }
}

#[tokio::test]
async fn unsafe_persisted_ids_cannot_write_or_remove_outside_the_root() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    db.transaction(|c| {
        Box::pin(async move {
            sqlx::query("UPDATE runs SET session_id='../outside' WHERE run_id='run'")
                .execute(c)
                .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert!(
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .is_err()
    );
    assert!(
        objects
            .history()
            .get(&attempt.run_id, 0)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        objects
            .remove_run("../outside", &attempt.run_id)
            .await
            .is_err()
    );
    assert!(
        objects
            .remove_run("session", &"../outside".into())
            .await
            .is_err()
    );
    assert!(!root.path().join("outside").exists());
    assert!(!root.path().join("objects/session").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn session_and_run_symlinks_cannot_redirect_reads_writes_or_cleanup() {
    for component in ["session", "session/run"] {
        let root = tempfile::tempdir().unwrap();
        let (_db, objects, attempt) = fixture(root.path(), 0).await;
        let history = objects.history();
        history
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        let path = root.path().join("objects").join(component);
        let outside = root.path().join("outside");
        std::fs::rename(&path, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        let mut next = entry(4096);
        next.sequence = 1;
        assert!(history.append(&attempt, Arc::new(next)).await.is_err());
        assert!(history.get(&attempt.run_id, 0).await.is_err());
        assert!(
            objects
                .remove_run("session", &attempt.run_id)
                .await
                .is_err()
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&outside, &path).unwrap();
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(entry(4096))
        );
    }
}

#[tokio::test]
async fn run_retirement_leaves_sibling_runs_and_sessions_intact() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    objects
        .history()
        .append(&attempt, Arc::new(entry(4096)))
        .await
        .unwrap();
    let sibling_run = root.path().join("objects/session/other-run");
    let sibling_session = root.path().join("objects/other-session/run");
    for path in [&sibling_run, &sibling_session] {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("sentinel"), b"keep").unwrap();
    }
    retire(&db, &objects).await.unwrap();
    assert!(!root.path().join("objects/session/run").exists());
    for path in [&sibling_run, &sibling_session] {
        assert_eq!(std::fs::read(path.join("sentinel")).unwrap(), b"keep");
    }
    objects
        .remove_run("session", &"other-run".into())
        .await
        .unwrap();
    assert!(!root.path().join("objects/session").exists());
    assert!(sibling_session.exists());
}

#[tokio::test]
async fn session_names_cannot_collide_with_process_lock_files() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    db.transaction(|c| {
        Box::pin(async move {
            sqlx::query("UPDATE runs SET session_id='.lock' WHERE run_id='run'")
                .execute(c)
                .await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    let history = objects.history();
    history
        .append(&attempt, Arc::new(entry(4096)))
        .await
        .unwrap();
    assert_eq!(
        history.get(&attempt.run_id, 0).await.unwrap(),
        Some(entry(4096))
    );
    mark_terminal(&db).await;
    objects.remove_run(".lock", &attempt.run_id).await.unwrap();
    assert!(!root.path().join("objects/.lock").exists());
    assert!(root.path().join(".durable-objects.lock").is_file());
}

#[tokio::test]
async fn long_session_ids_and_digest_like_ids_have_separate_safe_directories() {
    let root = tempfile::tempdir().unwrap();
    let (db, objects, attempt) = fixture(root.path(), 0).await;
    let long_id = "a".repeat(256);
    let digest_like = format!("~{:x}", Sha256::digest(long_id.as_bytes()));
    for session in [long_id, digest_like.clone()] {
        let stored = session.clone();
        db.transaction(move |c| Box::pin(async move {
            sqlx::query("UPDATE runs SET session_id=?,history_position=0,history_bytes=0 WHERE run_id='run'").bind(stored).execute(&mut *c).await?;
            sqlx::query("DELETE FROM durable_outcomes WHERE run_id='run'").execute(c).await?;
            Ok(())
        })).await.unwrap();
        let history = objects.history();
        history
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(entry(4096))
        );
    }
    let names: Vec<_> = std::fs::read_dir(root.path().join("objects"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    objects
        .remove_run(&"a".repeat(256), &attempt.run_id)
        .await
        .unwrap();
    assert_eq!(
        objects.history().get(&attempt.run_id, 0).await.unwrap(),
        Some(entry(4096))
    );
    objects
        .remove_run(&digest_like, &attempt.run_id)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("objects"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn case_and_unicode_aliases_cannot_share_a_run_directory() {
    for (first, second) in [("Run", "run"), ("caf\u{e9}", "cafe\u{301}")] {
        let root = tempfile::tempdir().unwrap();
        let (db, objects, _) = fixture(root.path(), 0).await;
        let mut attempts = Vec::new();
        for (index, id) in [first, second].into_iter().enumerate() {
            let id = id.to_owned();
            let inserted = id.clone();
            db.transaction(move |connection| Box::pin(async move {
                sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,attempt,admission_order,created_at_unix_ms,state,recovery_mode) VALUES(?,?,'session','fp','owner',1,?,0,'running','recorded')")
                    .bind(&inserted).bind(&inserted).bind(index as i64 + 2).execute(connection).await?;
                Ok(())
            })).await.unwrap();
            let attempt = ExecutionAttempt {
                run_id: id.into(),
                owner_generation: "owner".into(),
                attempt: 1,
            };
            objects
                .history()
                .append(&attempt, Arc::new(entry(4096)))
                .await
                .unwrap();
            attempts.push(attempt);
        }
        assert_eq!(
            std::fs::read_dir(root.path().join("objects/session"))
                .unwrap()
                .count(),
            2
        );
        objects
            .remove_run("session", &attempts[0].run_id)
            .await
            .unwrap();
        assert_eq!(
            objects.history().get(&attempts[1].run_id, 0).await.unwrap(),
            Some(entry(4096))
        );
        objects
            .remove_run("session", &attempts[1].run_id)
            .await
            .unwrap();
        assert!(!root.path().join("objects/session").exists());
    }
}
