use super::*;
use crate::adapters::unit_of_work::SqliteUnitOfWorkFactory;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::session::{RootVfs, Session, SessionBinding, SessionLifetime};
use std::time::Duration;

const SESSION: &str = "session";
const OTHER: &str = "other";

fn cipher(byte: u8) -> Option<Arc<SecretCipher>> {
    Some(Arc::new(SecretCipher::from_key([byte; 32])))
}

fn units(database: &Arc<ServerDatabase>) -> SqliteUnitOfWorkFactory {
    SqliteUnitOfWorkFactory {
        database: database.clone(),
        session_root: Default::default(),
        cipher: None,
    }
}

/// A database file in `dir` holding two active sessions.
async fn seeded(dir: &Path) -> Arc<ServerDatabase> {
    use crate::blueprint::BlueprintStore;
    let database = Arc::new(ServerDatabase::open(&dir.join("server.db")).await.unwrap());
    crate::blueprint::SqliteBlueprintStore::new(database.clone(), None)
        .upsert(submilli_blueprint::parse("name: test\ndefault: deny\n").unwrap())
        .await
        .unwrap();
    let mut unit = units(&database).begin().await.unwrap();
    for id in [SESSION, OTHER] {
        unit.save_session(
            Session::create(
                SessionId::parse(id.into()).unwrap(),
                SessionBinding::new("test".into(), Default::default()).unwrap(),
                RootVfs::None,
                SessionLifetime::new(Duration::from_secs(3600), SystemTime::now()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }
    unit.commit().await.unwrap();
    database
}

/// Stage `json` for `session` and record it, deleting the file it replaced.
async fn record(last_runs: &LastRuns, session: &str, json: &[u8]) -> StagedLastRun {
    let before = last_runs.spill_file(session).await.unwrap();
    let staged = last_runs.stage(session, json.to_vec()).await.unwrap();
    let mut unit = units(&last_runs.database).begin().await.unwrap();
    unit.put_last_run(session, "run", SystemTime::now(), &staged)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    last_runs
        .release(session, before.into_iter().collect())
        .await;
    staged
}

fn large(fill: &str) -> Vec<u8> {
    format!(
        r#"{{"result":"{}","console":[],"error":null}}"#,
        fill.repeat(INLINE_BYTES)
    )
    .into_bytes()
}

fn files(root: &Path, session: &str) -> Vec<String> {
    match std::fs::read_dir(root.join(session)) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{error}"),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[tokio::test]
async fn a_small_last_run_reads_back_exactly_sealed_or_not() {
    for key in [None, cipher(1)] {
        let dir = tempfile::tempdir().unwrap();
        let last_runs = LastRuns::new(seeded(dir.path()).await, key.clone(), 1 << 20);
        assert_eq!(last_runs.get(SESSION).await.unwrap(), None);
        let json = br#"{"result":"canary","console":["a"],"error":null}"#;
        let staged = record(&last_runs, SESSION, json).await;
        let LastRunStorage::Inline { payload, key_id } = &staged.storage else {
            panic!("a small last run stays in its row");
        };
        assert_eq!(key_id.is_some(), key.is_some());
        assert_eq!(contains(payload, b"canary"), key.is_none());
        assert_eq!(
            last_runs.get(SESSION).await.unwrap(),
            Some(StoredLastRun::Json(json.to_vec()))
        );
        assert_eq!(last_runs.get(OTHER).await.unwrap(), None);
    }
}

#[tokio::test]
async fn a_last_run_sealed_under_another_key_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    record(
        &LastRuns::new(database.clone(), cipher(1), 1 << 20),
        SESSION,
        b"{}",
    )
    .await;
    let error = LastRuns::new(database, cipher(2), 1 << 20)
        .get(SESSION)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("was sealed with key"), "{error}");
}

#[tokio::test]
async fn a_large_last_run_is_a_sealed_file_another_reader_opens() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), cipher(1), 1 << 20);
    let json = large("canary");
    let staged = record(&last_runs, SESSION, &json).await;
    let LastRunStorage::SpillFile { name, .. } = &staged.storage else {
        panic!("a large last run is kept in a file");
    };
    let root = spill_root(database.path());
    assert_eq!(files(&root, SESSION), std::slice::from_ref(name));
    let on_disk = std::fs::read(root.join(SESSION).join(name)).unwrap();
    assert!(!contains(&on_disk, b"canary"), "the file is sealed");
    // A second reader over the same database, as after a restart.
    let reader = LastRuns::new(database, cipher(1), 1 << 20);
    assert_eq!(
        reader.get(SESSION).await.unwrap(),
        Some(StoredLastRun::Json(json))
    );
}

#[tokio::test]
async fn replacing_a_large_last_run_deletes_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), None, 1 << 20);
    record(&last_runs, SESSION, &large("a")).await;
    let second = record(&last_runs, SESSION, &large("b")).await;
    let LastRunStorage::SpillFile { name, .. } = &second.storage else {
        panic!("a large last run is kept in a file");
    };
    let root = spill_root(database.path());
    assert_eq!(files(&root, SESSION), std::slice::from_ref(name));
    record(&last_runs, SESSION, b"{}").await;
    assert!(files(&root, SESSION).is_empty());
}

#[tokio::test]
async fn a_file_moved_to_another_session_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), cipher(1), 1 << 20);
    let theirs = record(&last_runs, SESSION, &large("a")).await;
    let ours = record(&last_runs, OTHER, &large("b")).await;
    let (
        LastRunStorage::SpillFile { name: theirs, .. },
        LastRunStorage::SpillFile { name: ours, .. },
    ) = (&theirs.storage, &ours.storage)
    else {
        panic!("a large last run is kept in a file");
    };
    let root = spill_root(database.path());
    std::fs::copy(root.join(SESSION).join(theirs), root.join(OTHER).join(ours)).unwrap();
    let error = last_runs.get(OTHER).await.unwrap_err();
    assert!(error.to_string().contains("corrupt"), "{error}");
}

#[tokio::test]
async fn a_last_run_over_the_limit_keeps_only_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), None, 10);
    let staged = record(&last_runs, SESSION, b"0123456789!").await;
    assert_eq!(staged.storage, LastRunStorage::TooLarge { limit: 10 });
    assert_eq!(
        last_runs.get(SESSION).await.unwrap(),
        Some(StoredLastRun::TooLarge {
            bytes: 11,
            limit: 10
        })
    );
    assert!(files(&spill_root(database.path()), SESSION).is_empty());
}

#[tokio::test]
async fn a_missing_file_is_an_error_not_an_absent_run() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), None, 1 << 20);
    let staged = record(&last_runs, SESSION, &large("a")).await;
    let LastRunStorage::SpillFile { name, .. } = &staged.storage else {
        panic!("a large last run is kept in a file");
    };
    std::fs::remove_file(spill_root(database.path()).join(SESSION).join(name)).unwrap();
    let error = last_runs.get(SESSION).await.unwrap_err();
    assert!(error.to_string().contains("missing"), "{error}");
}

#[tokio::test]
async fn startup_deletes_the_files_no_row_names() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), None, 1 << 20);
    let kept = record(&last_runs, SESSION, &large("a")).await;
    let LastRunStorage::SpillFile { name: kept, .. } = &kept.storage else {
        panic!("a large last run is kept in a file");
    };
    // Staged but never recorded, as a crash before the commit leaves it.
    last_runs.stage(OTHER, large("b")).await.unwrap();
    let root = spill_root(database.path());
    std::fs::write(root.join(SESSION).join("half-written.tmp"), b"x").unwrap();
    last_runs.sweep().await.unwrap();
    assert_eq!(files(&root, SESSION), std::slice::from_ref(kept));
    assert!(!root.join(OTHER).exists());
    assert_eq!(
        last_runs.get(SESSION).await.unwrap(),
        Some(StoredLastRun::Json(large("a")))
    );
}

#[tokio::test]
async fn closing_a_session_deletes_its_row_and_its_files_go_with_cleanup() {
    use crate::domain::session::ClosedReason;
    for reason in [
        ClosedReason::Deleted,
        ClosedReason::Expired,
        ClosedReason::BlueprintRemoved,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let database = seeded(dir.path()).await;
        let last_runs = LastRuns::new(database.clone(), None, 1 << 20);
        record(&last_runs, SESSION, &large("a")).await;
        let mut unit = units(&database).begin().await.unwrap();
        let mut session = unit.get_session(SESSION).await.unwrap().unwrap();
        assert!(session.close(reason));
        unit.save_session(session).await.unwrap();
        unit.commit().await.unwrap();
        assert_eq!(last_runs.get(SESSION).await.unwrap(), None);
        last_runs.remove_session_files(SESSION).await;
        assert!(!spill_root(database.path()).join(SESSION).exists());
    }
}

#[tokio::test]
async fn an_in_memory_database_keeps_every_last_run_in_its_row() {
    let database = Arc::new(ServerDatabase::open_ephemeral().await.unwrap());
    let last_runs = LastRuns::new(database, None, 1 << 20);
    let staged = last_runs.stage(SESSION, large("a")).await.unwrap();
    assert!(matches!(staged.storage, LastRunStorage::Inline { .. }));
    // Its rows stay well inside SQLite's limit on a value.
    let database = Arc::new(ServerDatabase::open_ephemeral().await.unwrap());
    assert_eq!(
        LastRuns::new(database, None, u64::MAX).max_bytes(),
        MAX_ROW_BYTES
    );
}

#[tokio::test]
async fn the_inline_and_retained_limits_are_inclusive() {
    let dir = tempfile::tempdir().unwrap();
    let last_runs = LastRuns::new(seeded(dir.path()).await, None, INLINE_BYTES as u64 + 1);
    let stage = async |bytes: usize| last_runs.stage(SESSION, vec![b'x'; bytes]).await.unwrap();
    assert!(matches!(
        stage(INLINE_BYTES).await.storage,
        LastRunStorage::Inline { .. }
    ));
    assert!(stage(INLINE_BYTES + 1).await.spill_file().is_some());
    assert!(matches!(
        stage(INLINE_BYTES + 2).await.storage,
        LastRunStorage::TooLarge { .. }
    ));
}

#[tokio::test]
async fn a_last_run_over_the_limit_reports_the_limit_it_was_over() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    record(
        &LastRuns::new(database.clone(), None, 10),
        SESSION,
        b"0123456789!",
    )
    .await;
    // The limit was raised since, as on a restart with a new setting.
    assert_eq!(
        LastRuns::new(database, None, 100)
            .get(SESSION)
            .await
            .unwrap(),
        Some(StoredLastRun::TooLarge {
            bytes: 11,
            limit: 10
        })
    );
}

#[tokio::test]
async fn startup_leaves_what_it_cannot_delete_and_carries_on() {
    let dir = tempfile::tempdir().unwrap();
    let database = seeded(dir.path()).await;
    let last_runs = LastRuns::new(database.clone(), None, 1 << 20);
    let root = spill_root(database.path());
    std::fs::create_dir_all(root.join(SESSION).join("stray")).unwrap();
    std::fs::write(root.join(SESSION).join("orphan"), b"x").unwrap();
    std::fs::write(root.join("not-a-session"), b"x").unwrap();
    last_runs.sweep().await.unwrap();
    assert_eq!(files(&root, SESSION), ["stray"]);
    assert!(root.join("not-a-session").exists());
}
