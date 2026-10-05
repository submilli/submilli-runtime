use super::*;
use futures::executor::block_on;
use futures::poll;

#[test]
fn reopens_without_tokio_and_second_owner_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    block_on(async {
        let first = ServerDatabase::open(&path).await.unwrap();
        assert!(matches!(
            ServerDatabase::open(&path).await,
            Err(DatabaseError::AlreadyOpen(_))
        ));
        let answer: i64 = first
            .transaction(|connection| {
                Box::pin(async move {
                    Ok(sqlx::query_scalar("SELECT 42")
                        .fetch_one(&mut *connection)
                        .await?)
                })
            })
            .await
            .unwrap();
        assert_eq!(answer, 42);
        first.close().await.unwrap();
        let second = ServerDatabase::open(&path).await.unwrap();
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
            Box::pin(async move {
                sqlx::raw_sql("CREATE TABLE rolled_back (value INTEGER)")
                    .execute(&mut *connection)
                    .await?;
                Err(DatabaseError::Import("injected failure".into()))
            })
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::Import(_))));
    let result: Result<(), DatabaseError> = database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::raw_sql("CREATE TABLE panicked (value INTEGER)")
                    .execute(&mut *connection)
                    .await?;
                panic!("caller callback failed");
            })
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::CallbackPanicked)));
    let result: Result<(), DatabaseError> = database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::raw_sql("CREATE TABLE payload_panicked (value INTEGER)")
                    .execute(&mut *connection)
                    .await?;
                std::panic::panic_any(PanickingPayload);
            })
        })
        .await;
    assert!(matches!(result, Err(DatabaseError::CallbackPanicked)));
    let count: i64 = database
        .transaction(|connection| Box::pin(async move {
            Ok(sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('rolled_back', 'panicked', 'payload_panicked')").fetch_one(&mut *connection).await?)
         }))
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
            Box::pin(async move {
                sqlx::raw_sql("CREATE TABLE claims (key TEXT PRIMARY KEY)")
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    let mut claims = Vec::new();
    for _ in 0..8 {
        let database = Arc::clone(&database);
        claims.push(tokio::spawn(async move {
            database
                .transaction(|connection| {
                    Box::pin(async move {
                        let existing: Option<String> =
                            sqlx::query_scalar("SELECT key FROM claims WHERE key = 'shared'")
                                .fetch_optional(&mut *connection)
                                .await?;
                        if existing.is_some() {
                            return Ok(false);
                        }
                        sqlx::query("INSERT INTO claims VALUES ('shared')")
                            .execute(&mut *connection)
                            .await?;
                        Ok(true)
                    })
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

#[tokio::test]
async fn cancellation_rolls_back_an_active_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let database = Arc::new(
        ServerDatabase::open(&directory.path().join("server.db"))
            .await
            .unwrap(),
    );
    let (started_tx, started_rx) = oneshot::channel();
    let request_db = Arc::clone(&database);
    let request = tokio::spawn(async move {
        request_db
            .transaction(move |connection| {
                Box::pin(async move {
                    sqlx::raw_sql("CREATE TABLE cancelled_write (value INTEGER)")
                        .execute(&mut *connection)
                        .await?;
                    started_tx.send(()).unwrap();
                    std::future::pending::<()>().await;
                    Ok(())
                })
            })
            .await
    });
    started_rx.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let count: i64 = tokio::time::timeout(
        Duration::from_secs(5),
        database.read(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='cancelled_write'",
                )
                .fetch_one(connection)
                .await?)
            })
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(count, 0);
    // Closing drains SQLx's queued rollback before releasing native ownership.
    database.close().await.unwrap();
}

#[tokio::test]
async fn cancellation_skips_a_queued_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("server.db"))
        .await
        .unwrap();
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let mut active = Box::pin(database.transaction(move |_| {
        Box::pin(async move {
            started_tx.send(()).unwrap();
            finish_rx.await.unwrap();
            Ok(())
        })
    }));
    assert!(poll!(&mut active).is_pending());
    started_rx.await.unwrap();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = Arc::clone(&called);
    let mut queued = Box::pin(database.transaction(move |_| {
        Box::pin(async move {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
    }));
    assert!(poll!(&mut queued).is_pending());
    drop(queued);
    finish_tx.send(()).unwrap();
    active.await.unwrap();
    database.close().await.unwrap();
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
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
                Box::pin(async move {
                    started_tx.send(()).unwrap();
                    finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    sqlx::raw_sql("CREATE TABLE survived_runtime (value INTEGER)")
                        .execute(&mut *connection)
                        .await?;
                    // Cancellation is observed between polls. Keep commit out
                    // of reach even if SQLx finishes within this blocking poll.
                    std::future::pending::<Result<(), DatabaseError>>().await
                })
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
                Box::pin(async move {
                    Ok(sqlx::query_scalar(
                        "SELECT COUNT(*) FROM sqlite_master WHERE name='survived_runtime'",
                    )
                    .fetch_one(&mut *connection)
                    .await?)
                })
            })
            .await
            .unwrap();
        assert_eq!(count, 0);
        reopened.close().await.unwrap();
    });
}

#[tokio::test]
async fn cancelled_startup_retains_lock_until_native_setup_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal),
    )
    .await
    .unwrap();
    sqlx::raw_sql("BEGIN IMMEDIATE")
        .execute(&mut writer)
        .await
        .unwrap();
    let probe = OpenOptions::new()
        .create(true)
        .truncate(false)
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
    sqlx::raw_sql("ROLLBACK")
        .execute(&mut writer)
        .await
        .unwrap();
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
        Box::pin(async move {
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })
    }));
    assert!(poll!(&mut active).is_pending());
    started_rx.await.unwrap();
    let mut waiting = Vec::new();
    for _ in 0..MAX_WAITING {
        let mut request = Box::pin(database.transaction(|_| Box::pin(async move { Ok(()) })));
        assert!(poll!(&mut request).is_pending());
        waiting.push(request);
    }
    assert!(matches!(
        database
            .transaction(|_| Box::pin(async move { Ok(()) }))
            .await,
        Err(DatabaseError::Busy)
    ));
    database.begin_close().unwrap();
    assert!(matches!(
        database
            .transaction(|_| Box::pin(async move { Ok(()) }))
            .await,
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
            Box::pin(async move {
                for (pragma, expected) in [
                    ("PRAGMA synchronous", 2_i64),
                    ("PRAGMA foreign_keys", 1),
                    ("PRAGMA busy_timeout", 5000),
                ] {
                    let value: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(pragma))
                        .fetch_one(&mut *connection)
                        .await?;
                    assert_eq!(value, expected, "{pragma}");
                }
                let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
                    .fetch_one(&mut *connection)
                    .await?;
                assert_eq!(journal, "wal");
                Ok(())
            })
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
        Box::pin(async move {
            sqlx::raw_sql("CREATE TABLE writes (value BLOB NOT NULL)")
                .execute(&mut *connection)
                .await?;
            for _ in 0..128 {
                sqlx::query("INSERT INTO writes VALUES (randomblob(4096))")
                    .execute(&mut *connection)
                    .await?;
            }
            started_tx.send(()).unwrap();
            // A timer on the sole Tokio thread releases native work. If the callback
            // ran on that thread, the receive would time out and this test would fail.
            finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
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
        let count: i64 = database
            .transaction(|connection| {
                Box::pin(async move {
                    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM crash_test")
                        .fetch_one(&mut *connection)
                        .await?)
                })
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
        database.transaction(|connection| Box::pin(async move {
            sqlx::raw_sql("CREATE TABLE crash_test (value INTEGER NOT NULL); INSERT INTO crash_test VALUES (1)").execute(&mut *connection).await?;
            Ok(())
         })).await.unwrap();
        // Exit while the database handle still owns the worker, before cleanup.
        std::process::exit(0);
    });
}

#[tokio::test]
async fn cancelled_callback_destructor_cannot_terminate_worker() {
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
    let mut request = Box::pin(database.transaction(move |connection| {
        Box::pin(async move {
            sqlx::raw_sql("CREATE TABLE result_committed (value INTEGER)")
                .execute(&mut *connection)
                .await?;
            let _guard = PanickingResult;
            started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
            Ok(())
        })
    }));
    assert!(poll!(&mut request).is_pending());
    started_rx.await.unwrap();
    drop(request);
    let count: i64 = database
        .transaction(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='result_committed'",
                )
                .fetch_one(&mut *connection)
                .await?)
            })
        })
        .await
        .unwrap();
    assert_eq!(count, 0);
    database.close().await.unwrap();
}

#[tokio::test]
async fn migration_checksum_and_unknown_version_are_rejected() {
    for sql in [
        "UPDATE _sqlx_migrations SET checksum=X'00' WHERE version=1",
        "UPDATE _sqlx_migrations SET version=999 WHERE version=1",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let database = ServerDatabase::open(&path).await.unwrap();
        database
            .read(move |connection| {
                Box::pin(async move {
                    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
        database.close().await.unwrap();
        assert!(matches!(
            ServerDatabase::open(&path).await,
            Err(DatabaseError::Migration(_))
        ));
    }
}

#[tokio::test]
async fn failed_sqlx_migration_rolls_back_schema_and_completion() {
    use sqlx::SqlSafeStr;
    let directory = tempfile::tempdir().unwrap();
    let database = ServerDatabase::open(&directory.path().join("db"))
        .await
        .unwrap();
    database
        .read(|connection| {
            Box::pin(async move {
                let mut migrations: Vec<_> = MIGRATOR.iter().cloned().collect();
                migrations.push(sqlx::migrate::Migration::new(
                    2,
                    "failing".into(),
                    sqlx::migrate::MigrationType::Simple,
                    "CREATE TABLE partial (value INTEGER); INSERT INTO missing VALUES (1);"
                        .into_sql_str(),
                    false,
                ));
                let migrator = sqlx::migrate::Migrator::with_migrations(migrations);
                assert!(
                    migrator
                        .run_direct(None, &mut *connection, false)
                        .await
                        .is_err()
                );
                let count: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name='partial'")
                        .fetch_one(&mut *connection)
                        .await?;
                assert_eq!(count, 0);
                let count: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version=2")
                        .fetch_one(connection)
                        .await?;
                assert_eq!(count, 0);
                Ok(())
            })
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn one_builtin_migration_creates_only_blueprint_tables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db");
    for _ in 0..2 {
        let database = ServerDatabase::open(&path).await.unwrap();
        let tables: Vec<String> = database.read(|connection| Box::pin(async move {
            Ok(sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .fetch_all(&mut *connection).await?)
        })).await.unwrap();
        assert_eq!(
            tables,
            ["_sqlx_migrations", "blueprint_revisions", "blueprints"]
        );
        let versions: Vec<i64> = database
            .read(|connection| {
                Box::pin(async move {
                    Ok(
                        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success=1")
                            .fetch_all(connection)
                            .await?,
                    )
                })
            })
            .await
            .unwrap();
        assert_eq!(versions, [1]);
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn busy_checkpoint_still_closes_connection_and_releases_lock() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db");
    let database = ServerDatabase::open(&path).await.unwrap();
    database.transaction(|connection| Box::pin(async move {
        sqlx::raw_sql("CREATE TABLE checkpoint_test (value INTEGER); INSERT INTO checkpoint_test VALUES (1)").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let mut reader = SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    let mut snapshot = reader.begin().await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT value FROM checkpoint_test")
        .fetch_one(&mut *snapshot)
        .await
        .unwrap();
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("INSERT INTO checkpoint_test VALUES (2)")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert!(
        matches!(database.close().await, Err(DatabaseError::Cleanup(error)) if matches!(*error, DatabaseError::CheckpointBusy))
    );
    snapshot.rollback().await.unwrap();
    reader.close().await.unwrap();
    ServerDatabase::open(&path)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn native_close_unlocks_even_when_a_child_descriptor_remains() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let database = OwnedDatabase::open(&path).await.unwrap();
    // A duplicated descriptor shares the lock as a forked child's copy does.
    let inherited = database.lock.0.as_ref().unwrap().try_clone().unwrap();
    database.close().await.unwrap();
    let reopened = ServerDatabase::open(&path).await.unwrap();
    reopened.close().await.unwrap();
    drop(inherited);
}
