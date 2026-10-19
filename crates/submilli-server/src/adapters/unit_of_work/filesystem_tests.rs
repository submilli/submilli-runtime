use crate::{adapters::unit_of_work::durable::units, database::ServerDatabase};
use std::sync::Arc;
use submilli_durable::{UnitFactory, codec::MAX_RECORD, unit_of_work::JournalEntry};
use submilli_shared::secret_store::SecretCipher;
fn journal_entry(id: &str) -> JournalEntry {
    JournalEntry {
        run_id: "scope".into(),
        id: id.into(),
        operation: "test.operation".into(),
        version: 1,
        request: b"secret request".to_vec(),
        state: None,
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
    let mut row = journal_entry("one");
    unit.save_journal_entry(row.clone()).await.unwrap();
    row.state = Some(br#"{"Prepared":"recovery data"}"#.to_vec());
    unit.save_journal_entry(row.clone()).await.unwrap();
    row.state = Some(br#"{"Completed":{"Ok":{"bytes":6}}}"#.to_vec());
    unit.save_journal_entry(row.clone()).await.unwrap();
    unit.commit().await.unwrap();
    drop(factory);
    db.close().await.unwrap();
    drop(db);
    let (_db, factory) = database(&path).await;
    let mut unit = factory().await.unwrap();
    let mut record = unit
        .get_journal_entry(&"scope".into(), &"one".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.state.as_deref(),
        Some(br#"{"Completed":{"Ok":{"bytes":6}}}"#.as_slice())
    );
    assert!(!String::from_utf8_lossy(record.state.as_ref().unwrap()).contains("recovery data"));
    unit.save_journal_entry(record.clone()).await.unwrap();
    record.state = Some(br#"{"Completed":{"Ok":{"bytes":7}}}"#.to_vec());
    assert!(unit.save_journal_entry(record).await.is_err());
    assert_eq!(unit.journal_entry_count(&"scope".into()).await.unwrap(), 1);
}
#[tokio::test]
async fn filesystem_operation_identity_and_pending_exclusion() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let row = journal_entry("one");
    unit.save_journal_entry(row.clone()).await.unwrap();
    for mut changed in [row.clone(), row.clone(), row.clone()]
        .into_iter()
        .enumerate()
    {
        match changed.0 {
            0 => changed.1.operation = "other.operation".into(),
            1 => changed.1.version = 2,
            _ => changed.1.request = b"changed".to_vec(),
        }
        assert!(unit.save_journal_entry(changed.1).await.is_err());
    }
    unit.commit().await.unwrap();
    let mut collision = factory().await.unwrap();
    assert!(
        collision
            .save_journal_entry(journal_entry("two"))
            .await
            .is_err()
    );
    drop(collision);
    let mut unit = factory().await.unwrap();
    let mut early = row.clone();
    early.state = Some(br#"{"Completed":{"Ok":{"bytes":8}}}"#.to_vec());
    assert!(unit.save_journal_entry(early).await.is_err());
    let mut refused = row;
    refused.state = Some(br#"{"Completed":{"Err":"refusal"}}"#.to_vec());
    unit.save_journal_entry(refused).await.unwrap();
    let mut row = journal_entry("two");
    unit.save_journal_entry(row.clone()).await.unwrap();
    row.state = Some(br#"{"Prepared":"checkpoint"}"#.to_vec());
    unit.save_journal_entry(row.clone()).await.unwrap();
    unit.save_journal_entry(row.clone()).await.unwrap();
    let mut changed = row.clone();
    changed.state = Some(br#"{"Prepared":"changed"}"#.to_vec());
    assert!(unit.save_journal_entry(changed).await.is_err());
    row.state = Some(br#"{"Completed":{"Err":"uncertain"}}"#.to_vec());
    assert!(unit.save_journal_entry(row).await.is_err());
}
#[tokio::test]
async fn dropped_unit_discards_effect_and_pending_slot() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let mut row = journal_entry("one");
    unit.save_journal_entry(row.clone()).await.unwrap();
    row.state = Some(br#"{"Prepared":"checkpoint"}"#.to_vec());
    unit.save_journal_entry(row).await.unwrap();
    assert!(
        unit.get_journal_entry(&"scope".into(), &"one".into())
            .await
            .unwrap()
            .is_some()
    );
    drop(unit);
    let mut unit = factory().await.unwrap();
    assert!(
        unit.get_journal_entry(&"scope".into(), &"one".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        unit.pending_journal_entry(&"scope".into())
            .await
            .unwrap()
            .is_none()
    );
    unit.save_journal_entry(journal_entry("two")).await.unwrap();
    unit.commit().await.unwrap();
}
#[tokio::test]
async fn records_are_bounded_and_sealed_to_field_row_operation_and_version() {
    let root = tempfile::tempdir().unwrap();
    let (db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let mut oversized = journal_entry("one");
    oversized.request = vec![0; MAX_RECORD + 1];
    assert!(unit.save_journal_entry(oversized).await.is_err());
    let mut row = journal_entry("one");
    unit.save_journal_entry(row.clone()).await.unwrap();
    let mut oversized = row.clone();
    oversized.state = Some(vec![0; MAX_RECORD + 1]);
    assert!(unit.save_journal_entry(oversized).await.is_err());
    row.state = Some(br#"{"Prepared":"secret intent"}"#.to_vec());
    unit.save_journal_entry(row.clone()).await.unwrap();
    let mut oversized = row.clone();
    oversized.state = Some(vec![0; MAX_RECORD + 1]);
    assert!(unit.save_journal_entry(oversized).await.is_err());
    row.state = Some(br#"{"Completed":{"Ok":{"bytes":14}}}"#.to_vec());
    unit.save_journal_entry(row).await.unwrap();
    unit.commit().await.unwrap();
    let original: Vec<u8> = db
        .read(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT state FROM durable_filesystem_operations WHERE operation_id='one'",
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
        "UPDATE durable_filesystem_operations SET state=request",
        "UPDATE durable_filesystem_operations SET status='pending'",
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
            unit.get_journal_entry(
                &"scope".into(),
                &if changed_row { "two" } else { "one" }.into()
            )
            .await
            .is_err()
        );
        drop(unit);
        let original = original.clone();
        db.transaction(move |connection| Box::pin(async move {
            sqlx::query("UPDATE durable_filesystem_operations SET operation='test.operation',version=1,operation_id='one',state=?,status='done'")
                .bind(original).execute(connection).await?;
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
    let mut row = journal_entry("one");
    unit.save_journal_entry(row.clone()).await.unwrap();
    row.request = b"changed".to_vec();
    assert!(unit.save_journal_entry(row).await.is_err());
}

#[tokio::test]
async fn malformed_state_and_backward_transitions_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let (_db, factory) = database(&root.path().join("db")).await;
    let mut unit = factory().await.unwrap();
    let mut entry = journal_entry("one");
    unit.save_journal_entry(entry.clone()).await.unwrap();
    for encoded in [
        b"not json".as_slice(),
        br#""Unknown""#,
        br#""Initial""#,
        br#"{"Completed":{"Ok":{"bytes":-1}}}"#,
        br#"{"Completed":{"Err":123}}"#,
        br#"{"Prepared":{},"Completed":{"Ok":{"bytes":0}}}"#,
    ] {
        let mut malformed = entry.clone();
        malformed.state = Some(encoded.to_vec());
        assert!(unit.save_journal_entry(malformed).await.is_err());
    }
    entry.state = Some(br#"{"Prepared":{"offset":7}}"#.to_vec());
    unit.save_journal_entry(entry.clone()).await.unwrap();
    let mut reset = entry.clone();
    reset.state = None;
    assert!(unit.save_journal_entry(reset).await.is_err());
    entry.state = Some(br#"{"Completed":{"Ok":{"bytes":3}}}"#.to_vec());
    unit.save_journal_entry(entry.clone()).await.unwrap();
    let mut reset = entry.clone();
    reset.state = None;
    assert!(unit.save_journal_entry(reset).await.is_err());
    entry.state = Some(br#"{"Prepared":{"offset":7}}"#.to_vec());
    assert!(unit.save_journal_entry(entry).await.is_err());
}
