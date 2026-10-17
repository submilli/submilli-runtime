use crate::{adapters::unit_of_work::durable::units, database::ServerDatabase};
use std::sync::Arc;
use submilli_durable::{UnitFactory, operations::*, unit_of_work::StoreError};
use submilli_shared::secret_store::SecretCipher;
fn key(effect: &str) -> OperationKey<'_> {
    OperationKey {
        scope: "scope",
        effect,
        operation: "test.operation",
        version: 1,
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
    admit(unit.as_mut(), &key("one"), b"immutable request")
        .await
        .unwrap();
    prepare(unit.as_mut(), &key("one"), b"recovery data")
        .await
        .unwrap();
    complete(unit.as_mut(), &key("one"), b"result", false)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    drop(factory);
    db.close().await.unwrap();
    drop(db);
    let (_db, factory) = database(&path).await;
    let mut unit = factory().await.unwrap();
    let record = admit(unit.as_mut(), &key("one"), b"immutable request")
        .await
        .unwrap();
    assert_eq!(record.outcome.as_deref(), Some(b"result".as_slice()));
    assert_eq!(
        record.recovery_payload.as_deref(),
        Some(b"recovery data".as_slice())
    );
    complete(unit.as_mut(), &key("one"), b"result", false)
        .await
        .unwrap();
    assert!(
        complete(unit.as_mut(), &key("one"), b"changed", false)
            .await
            .is_err()
    );
    assert!(
        prepare(unit.as_mut(), &key("one"), b"recovery data")
            .await
            .is_err()
    );
    assert_eq!(unit.operation_count("scope").await.unwrap(), 1);
}
#[tokio::test]
async fn filesystem_operation_identity_and_pending_exclusion() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    admit(unit.as_mut(), &key("one"), b"request").await.unwrap();
    let other = OperationKey {
        operation: "other.operation",
        ..key("one")
    };
    assert!(admit(unit.as_mut(), &other, b"request").await.is_err());
    assert!(
        load(
            unit.as_mut(),
            &OperationKey {
                version: 2,
                ..key("one")
            }
        )
        .await
        .is_err()
    );
    assert!(admit(unit.as_mut(), &key("one"), b"changed").await.is_err());
    let other_slot = OperationKey {
        operation: "other.operation",
        ..key("two")
    };
    assert!(admit(unit.as_mut(), &other_slot, b"request").await.is_err());
    assert!(
        complete(unit.as_mut(), &key("one"), b"too soon", false)
            .await
            .is_err()
    );
    complete(unit.as_mut(), &key("one"), b"refusal", true)
        .await
        .unwrap();
    admit(unit.as_mut(), &other_slot, b"request").await.unwrap();
    prepare(unit.as_mut(), &other_slot, b"intent")
        .await
        .unwrap();
    prepare(unit.as_mut(), &other_slot, b"intent")
        .await
        .unwrap();
    assert!(
        prepare(unit.as_mut(), &other_slot, b"changed")
            .await
            .is_err()
    );
    assert!(
        complete(unit.as_mut(), &other_slot, b"uncertain", true)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn dropped_unit_discards_effect_and_pending_slot() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    admit(unit.as_mut(), &key("one"), b"request").await.unwrap();
    prepare(unit.as_mut(), &key("one"), b"intent")
        .await
        .unwrap();
    drop(unit);
    let mut unit = factory().await.unwrap();
    assert!(load(unit.as_mut(), &key("one")).await.unwrap().is_none());
    assert!(unit.pending_operation("scope").await.unwrap().is_none());
    admit(unit.as_mut(), &key("two"), b"request").await.unwrap();
    unit.commit().await.unwrap();
}
#[tokio::test]
async fn records_are_bounded_and_sealed_to_field_row_operation_and_version() {
    let root = tempfile::tempdir().unwrap();
    let (db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let oversized = vec![0; MAX_RECORD + 1];
    assert!(admit(unit.as_mut(), &key("one"), &oversized).await.is_err());
    admit(unit.as_mut(), &key("one"), b"secret request")
        .await
        .unwrap();
    assert!(
        prepare(unit.as_mut(), &key("one"), &oversized)
            .await
            .is_err()
    );
    prepare(unit.as_mut(), &key("one"), b"secret intent")
        .await
        .unwrap();
    assert!(
        complete(unit.as_mut(), &key("one"), &oversized, false)
            .await
            .is_err()
    );
    complete(unit.as_mut(), &key("one"), b"secret outcome", false)
        .await
        .unwrap();
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
    let mut record = admit(unit.as_mut(), &key("one"), b"request").await.unwrap();
    record.request = b"changed".to_vec();
    assert!(matches!(
        unit.save_operation(record).await,
        Err(StoreError::Io(_) | StoreError::Database(_) | StoreError::Conflict)
    ));
}
