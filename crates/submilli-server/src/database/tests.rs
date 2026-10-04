use super::*;
use futures::executor::block_on;
use futures::poll;

#[test]
fn identity_survives_restart_without_tokio_and_second_owner_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    block_on(async {
        let first = ServerDatabase::open(&path).await.unwrap();
        assert!(matches!(
            ServerDatabase::open(&path).await,
            Err(DatabaseError::AlreadyOpen(_))
        ));
        let store_id = first.store_id();
        let generation = first.startup_generation();
        let answer = first
            .transaction(|connection| {
                Ok(connection.query_row("SELECT 42", [], |row| row.get::<_, i64>(0))?)
            })
            .await
            .unwrap();
        assert_eq!(answer, 42);
        first.close().await.unwrap();
        let second = ServerDatabase::open(&path).await.unwrap();
        assert_eq!(second.store_id(), store_id);
        assert_ne!(second.startup_generation(), generation);
        second.close().await.unwrap();
    });
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_alias_uses_same_lock_and_dangling_symlink_is_rejected() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let dangling = directory.path().join("dangling.db");
    symlink(directory.path().join("missing.db"), &dangling).unwrap();
    assert!(matches!(
        ServerDatabase::open(&dangling).await,
        Err(DatabaseError::Io { .. })
    ));
    let database = ServerDatabase::open(&path).await.unwrap();
    let alias = directory.path().join("alias.db");
    symlink(&path, &alias).unwrap();
    assert!(matches!(
        ServerDatabase::open(&alias).await,
        Err(DatabaseError::AlreadyOpen(_))
    ));
    database.close().await.unwrap();
}

#[tokio::test]
async fn hard_link_alias_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let database = ServerDatabase::open(&path).await.unwrap();
    let alias = directory.path().join("alias.db");
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(matches!(
        ServerDatabase::open(&alias).await,
        Err(DatabaseError::MultipleLinks(_))
    ));
    std::fs::remove_file(alias).unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn transaction_errors_and_callback_panics_roll_back() {
    struct PanickingPayload;
    impl Drop for PanickingPayload {
        fn drop(&mut self) {
            panic!("panic payload destructor failed");
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("server.db"))
        .await
        .unwrap();
    let result: Result<(), DatabaseError> = database
        .transaction(|connection| {
            connection.execute_batch("CREATE TABLE rolled_back (value INTEGER)")?;
            Err(DatabaseError::InvalidMetadata)
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::InvalidMetadata)));
    let result: Result<(), DatabaseError> = database
        .transaction(|connection| {
            connection.execute_batch("CREATE TABLE panicked (value INTEGER)")?;
            panic!("caller callback failed");
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::CallbackPanicked)));
    let result: Result<(), DatabaseError> = database
        .transaction(|connection| {
            connection.execute_batch("CREATE TABLE payload_panicked (value INTEGER)")?;
            std::panic::panic_any(PanickingPayload);
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::CallbackPanicked)));
    let count: i64 = database
        .transaction(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('rolled_back', 'panicked', 'payload_panicked')",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(count, 0);
    database.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_claims_serialize_before_reading() {
    let directory = tempfile::tempdir().unwrap();
    let database = Arc::new(
        ServerDatabase::open(&directory.path().join("server.db"))
            .await
            .unwrap(),
    );
    database
        .transaction(|connection| {
            connection.execute_batch("CREATE TABLE claims (key TEXT PRIMARY KEY)")?;
            Ok(())
        })
        .await
        .unwrap();
    let mut claims = Vec::new();
    for _ in 0..8 {
        let database = Arc::clone(&database);
        claims.push(tokio::spawn(async move {
            database
                .transaction(|connection| {
                    let existing: Option<String> = connection
                        .query_row("SELECT key FROM claims WHERE key = 'shared'", [], |row| {
                            row.get(0)
                        })
                        .optional()?;
                    if existing.is_some() {
                        return Ok(false);
                    }
                    connection.execute("INSERT INTO claims VALUES ('shared')", [])?;
                    Ok(true)
                })
                .await
                .unwrap()
        }));
    }
    let mut winners = 0;
    for claim in claims {
        winners += usize::from(claim.await.unwrap());
    }
    assert_eq!(winners, 1);
    database.close().await.unwrap();
}

#[test]
fn invalid_schema_refuses_startup_without_changing_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    block_on(async {
        let first = ServerDatabase::open(&path).await.unwrap();
        let store_id = first.store_id();
        first.close().await.unwrap();
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA user_version=999").unwrap();
        assert!(matches!(
            ServerDatabase::open(&path).await,
            Err(DatabaseError::NewerSchema { found: 999 })
        ));
        connection.execute_batch("PRAGMA user_version=-1").unwrap();
        assert!(matches!(
            ServerDatabase::open(&path).await,
            Err(DatabaseError::InvalidSchemaVersion { found: -1 })
        ));
        connection.execute_batch("PRAGMA user_version=1; INSERT INTO schema_migrations (version, name) VALUES (2, 'unknown')").unwrap();
        assert!(matches!(
            ServerDatabase::open(&path).await,
            Err(DatabaseError::InvalidMetadata)
        ));
        connection
            .execute_batch("DELETE FROM schema_migrations WHERE version=2")
            .unwrap();
        drop(connection);
        let reopened = ServerDatabase::open(&path).await.unwrap();
        assert_eq!(reopened.store_id(), store_id);
        reopened.close().await.unwrap();
    });
}

#[tokio::test]
async fn cancellation_does_not_abandon_admitted_transaction_or_close() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let database = Arc::new(ServerDatabase::open(&path).await.unwrap());
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let request_db = Arc::clone(&database);
    let request = tokio::spawn(async move {
        request_db
            .transaction(move |connection| {
                started_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                connection.execute_batch("CREATE TABLE committed_after_cancel (value INTEGER)")?;
                Ok(())
            })
            .await
    });
    started_rx.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let mut close = Box::pin(database.close());
    assert!(poll!(&mut close).is_pending());
    drop(close);
    assert!(matches!(
        ServerDatabase::open(&path).await,
        Err(DatabaseError::AlreadyOpen(_))
    ));
    assert!(matches!(
        database.transaction(|_| Ok(())).await,
        Err(DatabaseError::Closed)
    ));
    finish_tx.send(()).unwrap();
    database.close().await.unwrap();
    let reopened = ServerDatabase::open(&path).await.unwrap();
    let count: i64 = reopened
        .transaction(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='committed_after_cancel'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(count, 1);
    reopened.close().await.unwrap();
}

#[test]
fn destroying_tokio_runtime_does_not_release_worker_lock() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let database = Arc::new(runtime.block_on(ServerDatabase::open(&path)).unwrap());
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let request_db = Arc::clone(&database);
    runtime.spawn(async move {
        request_db
            .transaction(move |connection| {
                started_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                connection.execute_batch("CREATE TABLE survived_runtime (value INTEGER)")?;
                Ok(())
            })
            .await
    });
    runtime.block_on(started_rx).unwrap();
    drop(runtime);
    // Dropping the final handle must also retain ownership until native work ends.
    let mut completion = database.completion.clone();
    drop(database);
    assert!(matches!(
        block_on(ServerDatabase::open(&path)),
        Err(DatabaseError::AlreadyOpen(_))
    ));
    finish_tx.send(()).unwrap();
    block_on(completion.wait_for(Option::is_some)).unwrap();
    block_on(async {
        let reopened = ServerDatabase::open(&path).await.unwrap();
        let count: i64 = reopened
            .transaction(|connection| {
                Ok(connection.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='survived_runtime'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(count, 1);
        reopened.close().await.unwrap();
    });
}

#[tokio::test]
async fn cancelled_startup_retains_lock_until_native_setup_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let initial = ServerDatabase::open(&path).await.unwrap();
    initial.close().await.unwrap();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let probe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.path().join("server.db.lock"))
        .unwrap();

    let mut startup = Box::pin(ServerDatabase::open(&path));
    assert!(poll!(&mut startup).is_pending());
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match probe.try_lock() {
                Err(std::fs::TryLockError::WouldBlock) => break,
                Ok(()) => probe.unlock().unwrap(),
                Err(error) => panic!("could not probe process lock: {error}"),
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    drop(startup);
    assert!(matches!(
        probe.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    assert!(matches!(
        ServerDatabase::open(&path).await,
        Err(DatabaseError::AlreadyOpen(_))
    ));
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    let reopened = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ServerDatabase::open(&path).await {
                Ok(database) => break database,
                Err(DatabaseError::AlreadyOpen(_)) => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Err(error) => panic!("reopening after cancelled startup failed: {error}"),
            }
        }
    })
    .await
    .unwrap();
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn bounded_queue_rejects_excess_work_and_drains_accepted_work() {
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("server.db"))
        .await
        .unwrap();
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let mut active = Box::pin(database.transaction(move |_| {
        started_tx.send(()).unwrap();
        finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(())
    }));
    assert!(poll!(&mut active).is_pending());
    started_rx.await.unwrap();
    let mut waiting = Vec::new();
    for _ in 0..MAX_WAITING {
        let mut request = Box::pin(database.transaction(|_| Ok(())));
        assert!(poll!(&mut request).is_pending());
        waiting.push(request);
    }
    assert!(matches!(
        database.transaction(|_| Ok(())).await,
        Err(DatabaseError::Busy)
    ));
    database.begin_close().unwrap();
    assert!(matches!(
        database.transaction(|_| Ok(())).await,
        Err(DatabaseError::Closed)
    ));
    finish_tx.send(()).unwrap();
    active.await.unwrap();
    for request in waiting {
        request.await.unwrap();
    }
    database.close().await.unwrap();
}

#[tokio::test]
async fn connection_settings_enforce_durability_and_foreign_keys() {
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("server.db"))
        .await
        .unwrap();
    database
        .transaction(|connection| {
            for (pragma, expected) in [
                ("PRAGMA synchronous", 2_i64),
                ("PRAGMA foreign_keys", 1),
                ("PRAGMA busy_timeout", 5000),
            ] {
                let value: i64 = connection.query_row(pragma, [], |row| row.get(0))?;
                assert_eq!(value, expected, "{pragma}");
            }
            let journal: String =
                connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
            assert_eq!(journal, "wal");
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn database_work_keeps_tokio_timer_responsive() {
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("server.db"))
        .await
        .unwrap();
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let mut work = Box::pin(database.transaction(move |connection| {
        connection.execute_batch("CREATE TABLE writes (value BLOB NOT NULL)")?;
        for _ in 0..128 {
            connection.execute("INSERT INTO writes VALUES (randomblob(4096))", [])?;
        }
        started_tx.send(()).unwrap();
        // A timer on the sole Tokio thread releases native work. If the callback
        // ran on that thread, the receive would time out and this test would fail.
        finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(())
    }));
    assert!(poll!(&mut work).is_pending());
    started_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    finish_tx.send(()).unwrap();
    work.await.unwrap();
    database.close().await.unwrap();
}

#[test]
fn committed_write_survives_abrupt_process_exit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("database::tests::abrupt_exit_child")
        .env("SUBMILLI_DATABASE_CRASH_TEST_PATH", &path)
        .status()
        .unwrap();
    assert!(child.success());
    block_on(async {
        let database = ServerDatabase::open(&path).await.unwrap();
        let count: i64 =
            database
                .transaction(|connection| {
                    Ok(connection
                        .query_row("SELECT COUNT(*) FROM crash_test", [], |row| row.get(0))?)
                })
                .await
                .unwrap();
        assert_eq!(count, 1);
        database.close().await.unwrap();
    });
}

#[test]
fn abrupt_exit_child() {
    let Some(path) = std::env::var_os("SUBMILLI_DATABASE_CRASH_TEST_PATH") else {
        return;
    };
    block_on(async {
        let database = ServerDatabase::open(Path::new(&path)).await.unwrap();
        database.transaction(|connection| {
            connection.execute_batch("CREATE TABLE crash_test (value INTEGER NOT NULL); INSERT INTO crash_test VALUES (1)")?;
            Ok(())
        }).await.unwrap();
        // Exit while the database handle still owns the worker, before cleanup.
        std::process::exit(0);
    });
}

#[tokio::test]
async fn cancelled_result_destructor_cannot_terminate_worker() {
    struct PanickingResult;
    impl Drop for PanickingResult {
        fn drop(&mut self) {
            panic!("cancelled result destructor failed");
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("server.db"))
        .await
        .unwrap();
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let mut request = Box::pin(database.transaction(move |connection| {
        connection.execute_batch("CREATE TABLE result_committed (value INTEGER)")?;
        started_tx.send(()).unwrap();
        finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(PanickingResult)
    }));
    assert!(poll!(&mut request).is_pending());
    started_rx.await.unwrap();
    drop(request);
    finish_tx.send(()).unwrap();
    let count: i64 = database
        .transaction(|connection| {
            Ok(connection.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='result_committed'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(count, 1);
    database.close().await.unwrap();
}
