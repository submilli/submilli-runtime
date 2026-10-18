use crate::{adapters::unit_of_work::durable::units, database::ServerDatabase};
use std::sync::Arc;
use submilli_durable::{UnitFactory, codec::MAX_RECORD, unit_of_work::Operation};
use submilli_shared::secret_store::SecretCipher;
fn operation(id: &str) -> Operation {
    Operation {
        scope: "scope".into(),
        id: id.into(),
        operation: "test.operation".into(),
        version: 1,
        request: b"secret request".to_vec(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    }
}
async fn database(path: &std::path::Path) -> (Arc<ServerDatabase>, UnitFactory) {
    let db = Arc::new(ServerDatabase::open(path).await.unwrap());
    let factory = units(db.clone(), Arc::new(SecretCipher::from_key([7; 32])));
    (db, factory)
}
#[tokio::test]
async fn filesystem_records_survive_reopen_without_run_or_workspace_tables() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    let (db, factory) = database(&path).await;
    let mut unit = factory().await.unwrap();
    let mut row = operation("one");
    unit.save_operation(row.clone()).await.unwrap();
    row.recovery_payload = Some(b"recovery data".to_vec());
    unit.save_operation(row.clone()).await.unwrap();
    row.outcome = Some(b"result".to_vec());
    unit.save_operation(row.clone()).await.unwrap();
    unit.commit().await.unwrap();
    drop(factory);
    db.close().await.unwrap();
    drop(db);
    let (_db, factory) = database(&path).await;
    let mut unit = factory().await.unwrap();
    let mut record = unit.get_operation("scope", "one").await.unwrap().unwrap();
    assert_eq!(record.outcome.as_deref(), Some(b"result".as_slice()));
    assert_eq!(
        record.recovery_payload.as_deref(),
        Some(b"recovery data".as_slice())
    );
    unit.save_operation(record.clone()).await.unwrap();
    record.outcome = Some(b"changed".to_vec());
    assert!(unit.save_operation(record).await.is_err());
    assert_eq!(unit.operation_count("scope").await.unwrap(), 1);
}
#[tokio::test]
async fn filesystem_operation_identity_and_pending_exclusion() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let row = operation("one");
    unit.save_operation(row.clone()).await.unwrap();
    for mut changed in [row.clone(), row.clone(), row.clone()]
        .into_iter()
        .enumerate()
    {
        match changed.0 {
            0 => changed.1.operation = "other.operation".into(),
            1 => changed.1.version = 2,
            _ => changed.1.request = b"changed".to_vec(),
        }
        assert!(unit.save_operation(changed.1).await.is_err());
    }
    unit.commit().await.unwrap();
    let mut collision = factory().await.unwrap();
    assert!(collision.save_operation(operation("two")).await.is_err());
    drop(collision);
    let mut unit = factory().await.unwrap();
    let mut early = row.clone();
    early.outcome = Some(b"too soon".to_vec());
    assert!(unit.save_operation(early).await.is_err());
    let mut refused = row;
    refused.outcome = Some(b"refusal".to_vec());
    refused.rejected = true;
    unit.save_operation(refused).await.unwrap();
    let mut row = operation("two");
    unit.save_operation(row.clone()).await.unwrap();
    row.recovery_payload = Some(b"checkpoint".to_vec());
    unit.save_operation(row.clone()).await.unwrap();
    unit.save_operation(row.clone()).await.unwrap();
    let mut changed = row.clone();
    changed.recovery_payload = Some(b"changed".to_vec());
    assert!(unit.save_operation(changed).await.is_err());
    row.outcome = Some(b"uncertain".to_vec());
    row.rejected = true;
    assert!(unit.save_operation(row).await.is_err());
}
#[tokio::test]
async fn dropped_unit_discards_effect_and_pending_slot() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let mut row = operation("one");
    unit.save_operation(row.clone()).await.unwrap();
    row.recovery_payload = Some(b"checkpoint".to_vec());
    unit.save_operation(row).await.unwrap();
    assert!(unit.get_operation("scope", "one").await.unwrap().is_some());
    drop(unit);
    let mut unit = factory().await.unwrap();
    assert!(unit.get_operation("scope", "one").await.unwrap().is_none());
    assert!(unit.pending_operation("scope").await.unwrap().is_none());
    unit.save_operation(operation("two")).await.unwrap();
    unit.commit().await.unwrap();
}
#[tokio::test]
async fn records_are_bounded_and_sealed_to_field_row_operation_and_version() {
    let root = tempfile::tempdir().unwrap();
    let (db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let mut oversized = operation("one");
    oversized.request = vec![0; MAX_RECORD + 1];
    assert!(unit.save_operation(oversized).await.is_err());
    let mut row = operation("one");
    unit.save_operation(row.clone()).await.unwrap();
    let mut oversized = row.clone();
    oversized.recovery_payload = Some(vec![0; MAX_RECORD + 1]);
    assert!(unit.save_operation(oversized).await.is_err());
    row.recovery_payload = Some(b"secret intent".to_vec());
    unit.save_operation(row.clone()).await.unwrap();
    let mut oversized = row.clone();
    oversized.outcome = Some(vec![0; MAX_RECORD + 1]);
    assert!(unit.save_operation(oversized).await.is_err());
    row.outcome = Some(b"secret outcome".to_vec());
    unit.save_operation(row).await.unwrap();
    unit.commit().await.unwrap();
    let original: (Vec<u8>, Vec<u8>) = db
        .read(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_as(
                    "SELECT recovery_payload,outcome FROM durable_filesystem_operations WHERE operation_id='one'",
                )
                .fetch_one(connection)
                .await?)
            })
        })
        .await
        .unwrap();
    for attack in [
        "UPDATE durable_filesystem_operations SET operation='other'",
        "UPDATE durable_filesystem_operations SET version=2",
        "UPDATE durable_filesystem_operations SET operation_id='two'",
        "UPDATE durable_filesystem_operations SET recovery_payload=request",
        "UPDATE durable_filesystem_operations SET outcome=request",
    ] {
        let changed_row = attack_is_row(attack);
        let attack = attack.to_owned();
        db.transaction(move |connection| {
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(attack.as_str()))
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
        let mut unit = factory().await.unwrap();
        assert!(
            unit.get_operation("scope", if changed_row { "two" } else { "one" })
                .await
                .is_err()
        );
        drop(unit);
        let original = original.clone();
        db.transaction(move |connection| Box::pin(async move {
            sqlx::query("UPDATE durable_filesystem_operations SET operation='test.operation',version=1,operation_id='one',recovery_payload=?,outcome=?")
                .bind(original.0).bind(original.1).execute(connection).await?;
            Ok(())
        })).await.unwrap();
    }
}
fn attack_is_row(attack: &str) -> bool {
    attack.contains("operation_id")
}

#[tokio::test]
async fn direct_persistence_refuses_rewriting_committed_operation_identity() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let mut row = operation("one");
    unit.save_operation(row.clone()).await.unwrap();
    row.request = b"changed".to_vec();
    assert!(unit.save_operation(row).await.is_err());
}
