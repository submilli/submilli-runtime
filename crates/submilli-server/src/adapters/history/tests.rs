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
async fn object_ids(db: &ServerDatabase) -> Vec<String> {
    db.read(|c| {
        Box::pin(async move {
            Ok(
                sqlx::query_scalar("SELECT object_id FROM history_objects ORDER BY object_id")
                    .fetch_all(c)
                    .await?,
            )
        })
    })
    .await
    .unwrap()
}
async fn retire(db: &ServerDatabase) {
    db.transaction(|c| {
        Box::pin(async move {
            sqlx::query(
                "UPDATE runs SET state='completed',completed_at_unix_ms=1 WHERE run_id='run'",
            )
            .execute(&mut *c)
            .await?;
            crate::adapters::run_recovery::retire(c, "run").await
        })
    })
    .await
    .unwrap();
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
        objects.collect().await.unwrap();
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(sample.clone())
        );
        retire(&db).await;
        objects.collect().await.unwrap();
        assert!(object_ids(&db).await.is_empty());
        assert_eq!(
            std::fs::read_dir(root.path().join("objects"))
                .unwrap()
                .count(),
            2
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
    let (db, objects, attempt) = fixture(root.path(), MAX_ENTRY_BYTES).await;
    let history = History::with_store(attempt, objects.history());
    let first = history.begin("value", [3; 32]).await.unwrap();
    assert_eq!(first.recorded, Some(old.outcome));
    let second = history.begin("value", [3; 32]).await.unwrap();
    assert_eq!(second.recorded, Some(next.outcome));
    history.ensure_history_consumed().await.unwrap();
    assert_eq!(object_ids(&db).await.len(), 1);
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
    for directory in [root.path().to_path_buf(), root.path().join("objects")] {
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
        let id = object_ids(&db).await.pop().unwrap();
        let path = root.path().join("objects").join(format!("{id}.object"));
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
        assert_eq!(object_ids(&db).await.len(), 1);
    }
}

#[tokio::test]
async fn wrong_key_and_store_pairing_are_rejected() {
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
    let other_db = Arc::new(
        ServerDatabase::open(&root.path().join("other-db"))
            .await
            .unwrap(),
    );
    assert!(
        HistoryObjects::open(
            other_db,
            directory,
            Arc::new(SecretCipher::from_key([9; 32])),
            DurableObjectConfig::default()
        )
        .await
        .is_err()
    );
    std::fs::remove_file(root.path().join("objects/.identity")).unwrap();
    assert!(
        HistoryObjects::open(
            db,
            root.path().join("objects"),
            Arc::new(SecretCipher::from_key([9; 32])),
            DurableObjectConfig::default()
        )
        .await
        .is_err()
    );
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
    objects.collect().await.unwrap();
    assert_eq!(
        objects.history().get(&attempt.run_id, 0).await.unwrap(),
        Some(sample)
    );
    let id = object_ids(&db).await.pop().unwrap();
    retire(&db).await;
    let path = root.path().join("objects").join(format!("{id}.object"));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    objects.collect().await.unwrap();
    assert_eq!(object_ids(&db).await, [id]);
    std::fs::remove_dir(&path).unwrap();
    objects.collect().await.unwrap();
    assert!(object_ids(&db).await.is_empty());
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
    assert!(object_ids(&db).await.is_empty());
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
    objects.collect().await.unwrap();
    assert!(object_ids(&db).await.is_empty());
    assert_eq!(
        objects.history().progress(&attempt).await.unwrap().position,
        0
    );
}

#[test]
fn object_process_child() {
    let Ok(root) = std::env::var("SUBMILLI_HISTORY_ROOT") else {
        return;
    };
    let phase = std::env::var("SUBMILLI_HISTORY_CRASH").unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        if phase == "root-created" {
            let path = std::path::Path::new(&root);
            let db = ServerDatabase::open(&path.join("db")).await.unwrap();
            let volume_root = crate::config::VolumeRoot::default_root(path.join("server/volumes"));
            HistoryObjects::prepare_root(&db, &volume_root)
                .await
                .unwrap();
        }
        let (db, objects, attempt) = fixture(std::path::Path::new(&root), 0).await;
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        if [
            "collection-claimed",
            "unlinked",
            "deletion-synced",
            "collected",
        ]
        .contains(&phase.as_str())
        {
            retire(&db).await;
            objects.collect().await.unwrap();
        }
    });
    panic!("checkpoint was not reached");
}

#[test]
fn process_kill_at_publication_commit_and_collection_boundaries() {
    use std::process::{Command, Stdio};
    for phase in [
        "root-created",
        "reserved",
        "staging",
        "written",
        "file-synced",
        "published",
        "directory-synced",
        "committed",
        "collection-claimed",
        "unlinked",
        "deletion-synced",
        "collected",
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
            if phase == "root-created" {
                let db = ServerDatabase::open(&root.path().join("db")).await.unwrap();
                let pending: String = db
                    .read(|c| {
                        Box::pin(async move {
                            Ok(sqlx::query_scalar(
                                "SELECT pending_directory_sync FROM history_object_store",
                            )
                            .fetch_one(c)
                            .await?)
                        })
                    })
                    .await
                    .unwrap();
                assert_ne!(pending, "[]");
                let volume_root =
                    crate::config::VolumeRoot::default_root(root.path().join("server/volumes"));
                HistoryObjects::prepare_root(&db, &volume_root)
                    .await
                    .unwrap();
                let pending: String = db
                    .read(|c| {
                        Box::pin(async move {
                            Ok(sqlx::query_scalar(
                                "SELECT pending_directory_sync FROM history_object_store",
                            )
                            .fetch_one(c)
                            .await?)
                        })
                    })
                    .await
                    .unwrap();
                assert_eq!(pending, "[]");
                return;
            }
            let (db, objects, attempt) = fixture(root.path(), 0).await;
            objects.collect().await.unwrap();
            objects.collect().await.unwrap();
            let history = objects.history();
            if phase == "committed" {
                assert_eq!(
                    history.get(&attempt.run_id, 0).await.unwrap(),
                    Some(entry(4096))
                );
                assert_eq!(history.progress(&attempt).await.unwrap().position, 1);
            } else {
                assert!(
                    history.get(&attempt.run_id, 0).await.unwrap().is_none(),
                    "{phase}"
                );
                assert!(object_ids(&db).await.is_empty(), "{phase}");
            }
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
    let collecting = objects.clone();
    let collector = tokio::spawn(async move { collecting.collect().await });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!drain.is_finished());
    assert!(!collector.is_finished());
    release.send(()).unwrap();
    drain.await.unwrap().unwrap();
    collector.await.unwrap().unwrap();
    assert!(history.take_storage_fuel() > fuel::HASH.cost(4096));
    assert_eq!(history.take_storage_fuel(), 0);
    assert!(object_ids(&db).await.is_empty());
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
    retire(&db).await;
    let collecting = objects.clone();
    let collector = tokio::spawn(async move { collecting.collect().await });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!collector.is_finished());
    release.send(()).unwrap();
    assert_eq!(reader.await.unwrap().unwrap(), Some(entry(4096)));
    collector.await.unwrap().unwrap();
    assert!(object_ids(&db).await.is_empty());
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
        objects.collect().await.unwrap();
        assert!(object_ids(&db).await.is_empty());
        objects.files.set_hook(Arc::new(|_| Ok(())));
        history
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        assert_eq!(
            history.get(&attempt.run_id, 0).await.unwrap(),
            Some(entry(4096))
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn object_symlinks_and_writable_aliases_are_rejected() {
    use std::os::unix::fs::symlink;
    for alias in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (db, objects, attempt) = fixture(root.path(), 0).await;
        objects
            .history()
            .append(&attempt, Arc::new(entry(4096)))
            .await
            .unwrap();
        let id = object_ids(&db).await.pop().unwrap();
        let path = root.path().join("objects").join(format!("{id}.object"));
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

#[cfg(unix)]
#[tokio::test]
async fn failed_root_initialization_retries_barriers_until_they_succeed() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&temporary.path().join("db"))
        .await
        .unwrap();
    let ancestor = temporary.path().join("write-and-traverse");
    std::fs::create_dir(&ancestor).unwrap();
    let original = std::fs::metadata(&ancestor).unwrap().permissions();
    std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o300)).unwrap();
    let root = crate::config::VolumeRoot::default_root(ancestor.join("server/volumes"));
    let first = HistoryObjects::prepare_root(&database, &root).await;
    let second = HistoryObjects::prepare_root(&database, &root).await;
    std::fs::set_permissions(&ancestor, original).unwrap();
    assert!(
        first.is_err(),
        "a failed parent sync must block initialization"
    );
    assert!(
        second.is_err(),
        "existing directories must not bypass a failed barrier"
    );
    assert!(root.path().is_dir());
    HistoryObjects::prepare_root(&database, &root)
        .await
        .unwrap();
    let pending: String = database
        .read(|c| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT pending_directory_sync FROM history_object_store")
                        .fetch_one(c)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(pending, "[]");
}
