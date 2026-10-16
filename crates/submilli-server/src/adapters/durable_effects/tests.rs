use super::*;
use crate::database::ServerDatabase;
use std::sync::Arc;
use submilli_shared::secret_store::SecretCipher;

fn sealer() -> Sealer {
    Sealer::new(Some(Arc::new(SecretCipher::from_key([7; 32]))))
}

fn key(effect: &str) -> EffectKey<'_> {
    EffectKey {
        run: "run",
        effect,
        operation: "test.operation",
        version: 1,
    }
}

async fn database(path: &std::path::Path) -> ServerDatabase {
    let database = ServerDatabase::open(path).await.unwrap();
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state) VALUES('run','reservation','session','request','startup',1,0,'running')").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    database
}

#[tokio::test]
async fn shared_records_survive_reopen_and_lost_ack_without_a_workspace() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    let db = database(&path).await;
    db.transaction(|connection| {
        Box::pin(async move {
            admit(connection, &sealer(), &key("one"), b"immutable request").await?;
            prepare(connection, &sealer(), &key("one"), b"recovery data").await?;
            complete(connection, &sealer(), &key("one"), b"result", false).await
        })
    })
    .await
    .unwrap();
    db.close().await.unwrap();
    drop(db);
    let db = database(&path).await;
    db.transaction(|connection| {
        Box::pin(async move {
            // The caller did not retain the commit acknowledgement. Re-admission
            // and completion reconcile the same row, without a second effect.
            let record = admit(connection, &sealer(), &key("one"), b"immutable request").await?;
            assert_eq!(record.outcome.as_deref(), Some(b"result".as_slice()));
            assert_eq!(
                record.recovery_payload.as_deref(),
                Some(b"recovery data".as_slice())
            );
            complete(connection, &sealer(), &key("one"), b"result", false).await?;
            assert!(
                complete(connection, &sealer(), &key("one"), b"changed", false)
                    .await
                    .is_err()
            );
            assert!(
                prepare(connection, &sealer(), &key("one"), b"recovery data")
                    .await
                    .is_err()
            );
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_effects")
                .fetch_one(connection)
                .await?;
            assert_eq!(count, 1);
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn identity_and_pending_exclusion_are_shared_across_operation_adapters() {
    let root = tempfile::tempdir().unwrap();
    let db = database(&root.path().join("db")).await;
    db.transaction(|connection| {
        Box::pin(async move {
            admit(connection, &sealer(), &key("one"), b"request").await?;
            let other = EffectKey {
                operation: "other.operation",
                ..key("one")
            };
            assert!(
                admit(connection, &sealer(), &other, b"request")
                    .await
                    .is_err()
            );
            let other_version = EffectKey {
                version: 2,
                ..key("one")
            };
            assert!(load(connection, &sealer(), &other_version).await.is_err());
            assert!(
                admit(connection, &sealer(), &key("one"), b"changed")
                    .await
                    .is_err()
            );
            let other_slot = EffectKey {
                operation: "other.operation",
                ..key("two")
            };
            assert!(
                admit(connection, &sealer(), &other_slot, b"request")
                    .await
                    .is_err()
            );
            assert!(
                complete(
                    connection,
                    &sealer(),
                    &key("one"),
                    b"success too soon",
                    false
                )
                .await
                .is_err()
            );
            complete(connection, &sealer(), &key("one"), b"refusal", true).await?;
            admit(connection, &sealer(), &other_slot, b"request").await?;
            prepare(connection, &sealer(), &other_slot, b"intent").await?;
            prepare(connection, &sealer(), &other_slot, b"intent").await?;
            assert!(
                prepare(connection, &sealer(), &other_slot, b"changed intent")
                    .await
                    .is_err()
            );
            assert!(
                complete(
                    connection,
                    &sealer(),
                    &other_slot,
                    b"uncertain is not refusal",
                    true
                )
                .await
                .is_err()
            );
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn aborted_transaction_leaves_no_effect_or_pending_slot() {
    let root = tempfile::tempdir().unwrap();
    let db = database(&root.path().join("db")).await;
    let result: Result<(), _> = db
        .transaction(|connection| {
            Box::pin(async move {
                admit(connection, &sealer(), &key("one"), b"request").await?;
                prepare(connection, &sealer(), &key("one"), b"intent").await?;
                Err(DatabaseError::SessionConflict)
            })
        })
        .await;
    assert!(result.is_err());
    db.transaction(|connection| {
        Box::pin(async move {
            assert!(load(connection, &sealer(), &key("one")).await?.is_none());
            assert!(pending(connection, "run").await?.is_none());
            admit(connection, &sealer(), &key("two"), b"request").await?;
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn records_are_bounded_and_sealed_to_field_row_operation_and_version() {
    let root = tempfile::tempdir().unwrap();
    let db = database(&root.path().join("db")).await;
    db.transaction(|connection| {
        Box::pin(async move {
            let oversized = vec![0; MAX_RECORD + 1];
            assert!(
                admit(connection, &sealer(), &key("one"), &oversized)
                    .await
                    .is_err()
            );
            admit(connection, &sealer(), &key("one"), &vec![1; MAX_RECORD]).await?;
            assert!(
                prepare(connection, &sealer(), &key("one"), &oversized)
                    .await
                    .is_err()
            );
            prepare(connection, &sealer(), &key("one"), b"recovery canary").await?;
            assert!(
                complete(connection, &sealer(), &key("one"), &oversized, false)
                    .await
                    .is_err()
            );
            complete(connection, &sealer(), &key("one"), b"outcome canary", false).await?;
            let row = sqlx::query("SELECT request,recovery_payload,outcome FROM durable_effects")
                .fetch_one(&mut *connection)
                .await?;
            let recovery: Vec<u8> = row.try_get("recovery_payload")?;
            assert!(!recovery.windows(6).any(|bytes| bytes == b"canary"));
            assert!(open(&sealer(), &key("one"), "outcome", &recovery).is_err());
            assert!(open(&sealer(), &key("two"), "recovery", &recovery).is_err());
            let other_run = EffectKey {
                run: "other",
                ..key("one")
            };
            assert!(open(&sealer(), &other_run, "recovery", &recovery).is_err());
            let wrong_key = Sealer::new(Some(Arc::new(SecretCipher::from_key([8; 32]))));
            assert!(load(connection, &wrong_key, &key("one")).await.is_err());
            assert!(
                load(connection, &Sealer::default(), &key("one"))
                    .await
                    .is_err()
            );
            sqlx::query("UPDATE durable_effects SET version=2")
                .execute(&mut *connection)
                .await?;
            let version_two = EffectKey {
                version: 2,
                ..key("one")
            };
            assert!(load(connection, &sealer(), &version_two).await.is_err());
            sqlx::query("UPDATE durable_effects SET version=1,operation='other'")
                .execute(&mut *connection)
                .await?;
            let other_operation = EffectKey {
                operation: "other",
                ..key("one")
            };
            assert!(load(connection, &sealer(), &other_operation).await.is_err());
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn concurrent_admission_preserves_one_request_and_different_runs_are_independent() {
    let root = tempfile::tempdir().unwrap();
    let db = Arc::new(database(&root.path().join("db")).await);
    let first = db.clone();
    let second = db.clone();
    let (one, two) = tokio::join!(
        first.transaction(|connection| Box::pin(async move {
            admit(connection, &sealer(), &key("one"), b"first")
                .await
                .map(|_| ())
        })),
        second.transaction(|connection| Box::pin(async move {
            admit(connection, &sealer(), &key("one"), b"second")
                .await
                .map(|_| ())
        })),
    );
    assert_ne!(one.is_ok(), two.is_ok());
    db.transaction(|connection| Box::pin(async move {
        sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state) VALUES('other','other-reservation','session','request','startup',2,0,'running')").execute(&mut *connection).await?;
        let other = EffectKey { run: "other", ..key("one") };
        admit(connection, &sealer(), &other, b"independent").await?;
        assert_eq!(pending(connection, "run").await?.as_deref(), Some("one"));
        assert_eq!(pending(connection, "other").await?.as_deref(), Some("one"));
        Ok(())
    })).await.unwrap();
}
