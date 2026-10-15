use super::*;
use crate::domain::durable_object::{CollectionState, PublicationState};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

mod cancellation;
mod failures;
pub(crate) mod process;

struct Fixture {
    root: TempDir,
    database: Arc<ServerDatabase>,
    store: DurableObjects,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("objects")).unwrap();
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("state.db"))
                .await
                .unwrap(),
        );
        let store = DurableObjects::open(
            config(root.path().join("objects")),
            database.clone(),
            cipher(),
        )
        .await
        .unwrap();
        Self {
            root,
            database,
            store,
        }
    }
    async fn reserved(&self, maximum: u64) -> ObjectId {
        let id = DurableObjects::new_id().unwrap();
        self.store
            .reserve(id.clone(), "run".into(), [3; 32], maximum)
            .await
            .unwrap();
        id
    }
    fn path(&self, id: &ObjectId, suffix: &str) -> PathBuf {
        self.root
            .path()
            .join("objects")
            .join(format!("{}.{suffix}", id.as_str()))
    }
    async fn row(&self, id: &ObjectId) -> crate::domain::durable_object::DurableObject {
        let units = crate::adapters::unit_of_work::for_sessions(
            self.database.clone(),
            PathBuf::new(),
            Some(cipher()),
        );
        let mut unit = units.begin().await.unwrap();
        unit.get_object(id).await.unwrap().unwrap()
    }
}

fn cipher() -> Arc<SecretCipher> {
    Arc::new(SecretCipher::from_key([7; 32]))
}
fn config(directory: PathBuf) -> DurableObjectConfig {
    DurableObjectConfig {
        directory,
        ..Default::default()
    }
}
fn retention() -> BTreeSet<Retention> {
    BTreeSet::from([
        Retention::Run("run".into()),
        Retention::Response("result".into()),
    ])
}

#[tokio::test]
async fn pending_publication_and_run_retention_prevent_collection() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(1024).await;
    assert!(fixture.store.collect(&id).await.is_err());
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(b"captured".to_vec()))
        .await
        .unwrap();
    assert!(fixture.store.collect(&id).await.is_err());
    let reference = fixture.store.bind(&prepared, retention()).await.unwrap();
    assert_eq!(reference.content.bytes, 8);
    assert_eq!(
        fixture.store.bind(&prepared, retention()).await.unwrap(),
        reference
    );
    fixture
        .store
        .release(&id, &Retention::Response("result".into()))
        .await
        .unwrap();
    assert!(fixture.store.collect(&id).await.is_err());
    fixture
        .store
        .release(&id, &Retention::Run("run".into()))
        .await
        .unwrap();
    fixture.store.collect(&id).await.unwrap();
    fixture.store.collect(&id).await.unwrap();
    assert!(!fixture.path(&id, "object").exists());
    assert_eq!(fixture.row(&id).await.collection(), CollectionState::Gone);
    assert!(
        fixture
            .store
            .reserve(id, "run".into(), [3; 32], 1024)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn abandoned_publication_refuses_a_late_reference_commit() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(1024).await;
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(vec![1; 80]))
        .await
        .unwrap();
    fixture.store.abandon(&id).await.unwrap();
    assert!(fixture.store.bind(&prepared, retention()).await.is_err());
    fixture.store.collect(&id).await.unwrap();
    assert!(fixture.store.bind(&prepared, retention()).await.is_err());
    assert_eq!(
        fixture.row(&id).await.publication(),
        PublicationState::Abandoned
    );
}

#[tokio::test]
async fn bind_rollback_and_lost_ack_reuse_one_reference_and_accounting() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(1024).await;
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(vec![9; 30]))
        .await
        .unwrap();
    let units = crate::adapters::unit_of_work::for_sessions(
        fixture.database.clone(),
        PathBuf::new(),
        Some(cipher()),
    );
    let mut unit = units.begin().await.unwrap();
    fixture
        .store
        .publications
        .bind_in(unit.as_mut(), &prepared, retention())
        .await
        .unwrap();
    assert_eq!(
        unit.get_object(&id).await.unwrap().unwrap().publication(),
        PublicationState::Bound
    );
    drop(unit);
    assert_eq!(
        fixture.row(&id).await.publication(),
        PublicationState::Pending
    );
    let mut unit = units.begin().await.unwrap();
    let first = fixture
        .store
        .publications
        .bind_in(unit.as_mut(), &prepared, retention())
        .await
        .unwrap();
    unit.commit().await.unwrap(); // Deliberately discard the acknowledgement at the consumer.
    let revision = fixture.row(&id).await.revision();
    assert_eq!(
        fixture.store.bind(&prepared, retention()).await.unwrap(),
        first
    );
    assert_eq!(fixture.row(&id).await.revision(), revision);
    assert!(fixture.row(&id).await.reserved_bytes() < 1024);
}

#[tokio::test]
async fn complete_publication_is_rediscovered_without_a_producer() {
    let Fixture {
        root,
        database,
        store,
    } = Fixture::new().await;
    let id = DurableObjects::new_id().unwrap();
    store
        .reserve(id.clone(), "run".into(), [1; 32], 100)
        .await
        .unwrap();
    let original = store
        .publish(&id, Cursor::new(b"original".to_vec()))
        .await
        .unwrap();
    drop(store);
    let store = DurableObjects::open(config(root.path().join("objects")), database, cipher())
        .await
        .unwrap();
    let recovered = store.recover().await.unwrap();
    assert_eq!(recovered.ready, 1);
    let prepared = store.recover_publication(&id).await.unwrap().unwrap();
    assert_eq!(prepared.content(), original.content());
    let reference = store.bind(&prepared, retention()).await.unwrap();
    let ignored = store
        .publish(&id, Cursor::new(b"different".to_vec()))
        .await
        .unwrap();
    assert_eq!(ignored.content(), &reference.content);
    assert_eq!(store.recover().await.unwrap().validated, 1);
}

#[tokio::test]
async fn damaged_full_length_staging_is_rewritten_but_committed_corruption_fails_closed() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(1000).await;
    std::fs::write(fixture.path(&id, "staging"), vec![0; 1000]).unwrap();
    assert_eq!(fixture.store.recover().await.unwrap().incomplete, 1);
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(vec![77; 1000]))
        .await
        .unwrap();
    fixture.store.bind(&prepared, retention()).await.unwrap();
    std::fs::write(fixture.path(&id, "object"), vec![0; 1000]).unwrap();
    assert!(matches!(
        fixture.store.recover().await,
        Err(ObjectStoreError::Integrity)
    ));
    assert!(
        fixture
            .store
            .publish(&id, Cursor::new(vec![77; 1000]))
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.path(&id, "object")).unwrap(),
        vec![0; 1000]
    );
}

#[tokio::test]
async fn missing_committed_object_is_not_recreated_from_a_retry() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(10).await;
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(b"once".to_vec()))
        .await
        .unwrap();
    fixture.store.bind(&prepared, retention()).await.unwrap();
    std::fs::remove_file(fixture.path(&id, "object")).unwrap();
    assert!(matches!(
        fixture
            .store
            .publish(&id, Cursor::new(b"twice".to_vec()))
            .await,
        Err(ObjectStoreError::Integrity)
    ));
    assert!(!fixture.path(&id, "object").exists());
}

#[tokio::test]
async fn deletion_claim_is_resumed_after_restart() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(10).await;
    fixture
        .store
        .publish(&id, Cursor::new(b"orphan".to_vec()))
        .await
        .unwrap();
    fixture.store.abandon(&id).await.unwrap();
    let units = crate::adapters::unit_of_work::for_sessions(
        fixture.database.clone(),
        PathBuf::new(),
        Some(cipher()),
    );
    let mut unit = units.begin().await.unwrap();
    let mut object = unit.get_object(&id).await.unwrap().unwrap();
    object.collect().unwrap();
    unit.save_object(object).await.unwrap();
    unit.commit().await.unwrap();
    assert!(fixture.path(&id, "object").exists());
    assert_eq!(fixture.store.recover().await.unwrap().collected, 1);
    assert!(!fixture.path(&id, "object").exists());
}

#[tokio::test]
async fn threshold_boundary_and_all_downloads_use_the_expected_storage() {
    let fixture = Fixture::new().await;
    for size in [
        0,
        DEFAULT_INLINE_BYTES - 1,
        DEFAULT_INLINE_BYTES,
        DEFAULT_INLINE_BYTES + 1,
    ] {
        for download in [false, true] {
            let id = DurableObjects::new_id().unwrap();
            let prepared = fixture
                .store
                .prepare_payload(
                    id,
                    "owner".into(),
                    [0; 32],
                    Cursor::new(vec![11; size]),
                    size as u64,
                    download,
                )
                .await
                .unwrap();
            let payload = fixture
                .store
                .bind_payload(prepared, retention())
                .await
                .unwrap();
            assert_eq!(payload.is_object(), download || size > DEFAULT_INLINE_BYTES);
            let encoded = payload.encode().unwrap();
            assert!(!encoded.windows(64).any(|window| window == [11; 64]));
            let decoded = StoredPayload::decode(&encoded).unwrap();
            assert_eq!(decoded.bytes(), size as u64);
            let output = fixture.root.path().join("output");
            fixture
                .store
                .copy_payload("owner", &decoded, std::fs::File::create(&output).unwrap())
                .await
                .unwrap();
            assert_eq!(std::fs::read(output).unwrap(), vec![11; size]);
        }
    }
}

#[tokio::test]
async fn workspace_edit_cannot_change_the_capture_and_files_are_sealed() {
    let fixture = Fixture::new().await;
    let bytes = b"PLAINTEXT-CANARY-7d41e908".repeat(9000);
    let id = fixture.reserved(bytes.len() as u64).await;
    let prepared = fixture
        .store
        .publish(&id, ShortReads(Cursor::new(bytes.clone())))
        .await
        .unwrap();
    let reference = fixture.store.bind(&prepared, retention()).await.unwrap();
    assert_eq!(
        reference.content.sha256,
        <[u8; 32]>::from(Sha256::digest(&bytes))
    );
    let workspace = fixture.root.path().join("download");
    fixture
        .store
        .copy_into(&reference, std::fs::File::create(&workspace).unwrap())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&workspace).unwrap(), bytes);
    std::fs::write(&workspace, b"guest edit").unwrap();
    fixture
        .store
        .copy_into(&reference, std::fs::File::create(&workspace).unwrap())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&workspace).unwrap(), bytes);
    for path in [
        fixture.path(&id, "object"),
        fixture.root.path().join("state.db"),
        fixture.root.path().join("state.db-wal"),
    ] {
        if path.exists() {
            let canary = b"PLAINTEXT-CANARY-7d41e908";
            assert!(
                !std::fs::read(path)
                    .unwrap()
                    .windows(canary.len())
                    .any(|value| value == canary)
            );
        }
    }
}

struct ShortReads(Cursor<Vec<u8>>);
impl Read for ShortReads {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        assert!(buffer.len() <= 64 * 1024);
        let length = buffer.len().min(13);
        self.0.read(&mut buffer[..length])
    }
}

#[tokio::test]
async fn reservation_conflicts_limits_and_oversized_streams_leave_no_reference() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(3).await;
    assert!(
        fixture
            .store
            .reserve(id.clone(), "run".into(), [9; 32], 3)
            .await
            .is_err()
    );
    assert!(matches!(
        fixture
            .store
            .publish(&id, Cursor::new(b"four".to_vec()))
            .await,
        Err(ObjectStoreError::Limit)
    ));
    assert!(fixture.store.reference(&id).await.is_err());
    assert!(!fixture.path(&id, "object").exists());
    fixture.store.abandon(&id).await.unwrap();
    fixture.store.collect(&id).await.unwrap();
    assert!(!fixture.path(&id, "staging").exists());
}

#[tokio::test]
async fn object_directory_cannot_have_two_owners() {
    let fixture = Fixture::new().await;
    assert!(
        DurableObjects::open(
            config(fixture.root.path().join("objects")),
            fixture.database.clone(),
            cipher()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn wrong_key_and_inline_owner_are_rejected() {
    let Fixture {
        root,
        database,
        store,
    } = Fixture::new().await;
    let id = DurableObjects::new_id().unwrap();
    let value = store
        .prepare_payload(
            id,
            "alice".into(),
            [0; 32],
            Cursor::new(b"secret".to_vec()),
            6,
            false,
        )
        .await
        .unwrap();
    let payload = store.bind_payload(value, retention()).await.unwrap();
    assert!(
        store
            .copy_payload("bob", &payload, std::io::sink())
            .await
            .is_err()
    );
    let id = DurableObjects::new_id().unwrap();
    store
        .reserve(id.clone(), "run".into(), [0; 32], 5)
        .await
        .unwrap();
    store
        .publish(&id, Cursor::new(b"hello".to_vec()))
        .await
        .unwrap();
    drop(store);
    let wrong = DurableObjects::open(
        config(root.path().join("objects")),
        database,
        Arc::new(SecretCipher::from_key([8; 32])),
    )
    .await
    .unwrap();
    assert!(wrong.recover().await.is_err());
}
