use super::*;

#[tokio::test]
async fn publication_acknowledgements_and_transactions_belong_to_one_store() {
    let a = Fixture::new().await;
    let b = Fixture::new().await;
    let id = a.reserved(100).await;
    b.store
        .reserve(id.clone(), "run".into(), [3; 32], 100)
        .await
        .unwrap();
    let prepared = a
        .store
        .publish(&id, Cursor::new(b"original".to_vec()))
        .await
        .unwrap();
    assert!(b.store.bind(&prepared, retention()).await.is_err());
    let mut foreign_unit = b.store.begin().await.unwrap();
    assert!(
        a.store
            .bind_in(foreign_unit.as_mut(), &prepared, retention())
            .await
            .is_err()
    );
    drop(foreign_unit);
    assert!(b.store.reference(&id).await.is_err());
    a.store.bind(&prepared, retention()).await.unwrap();
    assert_eq!(a.store.recover().await.unwrap().validated, 1);

    let another_directory = a.root.path().join("another");
    std::fs::create_dir(&another_directory).unwrap();
    assert!(
        DurableObjects::open(config(another_directory), a.database.clone(), cipher())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn directory_identity_survives_restart_and_rejects_a_replacement_database() {
    let Fixture {
        root,
        database,
        store,
    } = Fixture::new().await;
    let id = DurableObjects::new_id().unwrap();
    store
        .reserve(id.clone(), "run".into(), [0; 32], 1)
        .await
        .unwrap();
    store.publish(&id, Cursor::new(vec![1])).await.unwrap();
    drop(store);
    let replacement = Arc::new(
        ServerDatabase::open(&root.path().join("new.db"))
            .await
            .unwrap(),
    );
    assert!(
        DurableObjects::open(config(root.path().join("objects")), replacement, cipher())
            .await
            .is_err()
    );
    let store = DurableObjects::open(config(root.path().join("objects")), database, cipher())
        .await
        .unwrap();
    assert_eq!(store.recover().await.unwrap().ready, 1);
}

#[tokio::test]
async fn replacing_an_empty_stores_database_invalidates_the_old_database() {
    for with_tombstone in [false, true] {
        let Fixture {
            root,
            database,
            store,
        } = Fixture::new().await;
        if with_tombstone {
            let id = DurableObjects::new_id().unwrap();
            store
                .reserve(id.clone(), "old".into(), [0; 32], 0)
                .await
                .unwrap();
            store.abandon(&id).await.unwrap();
            store.collect(&id).await.unwrap();
        }
        drop(store);
        let replacement = Arc::new(
            ServerDatabase::open(&root.path().join("replacement.db"))
                .await
                .unwrap(),
        );
        let new_store =
            DurableObjects::open(config(root.path().join("objects")), replacement, cipher())
                .await
                .unwrap();
        let id = DurableObjects::new_id().unwrap();
        new_store
            .reserve(id.clone(), "new".into(), [1; 32], 3)
            .await
            .unwrap();
        new_store
            .publish(&id, Cursor::new(vec![1; 3]))
            .await
            .unwrap();
        drop(new_store);
        assert!(
            DurableObjects::open(config(root.path().join("objects")), database, cipher())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn damaged_pending_key_header_is_repaired_after_authenticating_the_reservation() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(100).await;
    fixture
        .store
        .publish(&id, Cursor::new(b"candidate".to_vec()))
        .await
        .unwrap();
    std::fs::rename(fixture.path(&id, "object"), fixture.path(&id, "staging")).unwrap();
    let mut bytes = std::fs::read(fixture.path(&id, "staging")).unwrap();
    bytes[8] ^= 1; // Preserve magic; damage only the key-ID header.
    std::fs::write(fixture.path(&id, "staging"), bytes).unwrap();
    assert_eq!(fixture.store.recover().await.unwrap().incomplete, 1);
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(b"candidate".to_vec()))
        .await
        .unwrap();
    fixture.store.bind(&prepared, retention()).await.unwrap();
    let mut bytes = std::fs::read(fixture.path(&id, "object")).unwrap();
    bytes[8] ^= 1;
    std::fs::write(fixture.path(&id, "object"), bytes).unwrap();
    assert!(matches!(
        fixture.store.recover().await,
        Err(ObjectStoreError::Key)
    ));
}

#[tokio::test]
async fn direct_store_rejects_an_ephemeral_database() {
    let root = tempfile::tempdir().unwrap();
    let database = Arc::new(
        ServerDatabase::open(std::path::Path::new(":memory:"))
            .await
            .unwrap(),
    );
    assert!(matches!(
        DurableObjects::open(config(root.path().to_path_buf()), database, cipher()).await,
        Err(ObjectStoreError::Configuration(_))
    ));
}

#[tokio::test]
async fn boot_validates_committed_objects_before_starting_sessions() {
    use crate::{AppState, ServerConfig};
    let root = tempfile::tempdir().unwrap();
    let objects = root.path().join("objects");
    let volumes = root.path().join("volumes");
    std::fs::create_dir(&objects).unwrap();
    std::fs::create_dir(&volumes).unwrap();
    let state = AppState::new(ServerConfig {
        database_path: Some(root.path().join("state.db")),
        durable_objects: Some(config(objects.clone())),
        session_cipher: Some(cipher()),
        volume_root: Some(volumes),
        ..crate::config::test_config()
    })
    .await
    .unwrap();
    let store = state.durable_objects().unwrap();
    let id = DurableObjects::new_id().unwrap();
    store
        .reserve(id.clone(), "run".into(), [1; 32], 3)
        .await
        .unwrap();
    let prepared = store.publish(&id, Cursor::new(vec![8; 3])).await.unwrap();
    store.bind(&prepared, retention()).await.unwrap();
    std::fs::remove_file(objects.join(format!("{}.object", id.as_str()))).unwrap();
    assert!(matches!(
        state.boot().await,
        Err(crate::session_manager::BootError::Objects(
            ObjectStoreError::Integrity
        ))
    ));
}

#[tokio::test]
async fn reservations_serialize_and_only_durable_deletion_refunds_quota() {
    let Fixture {
        root,
        database,
        store,
    } = Fixture::new().await;
    drop(store);
    let mut settings = config(root.path().join("objects"));
    settings.limits = ObjectLimits {
        maximum_object_bytes: 1000,
        maximum_owner_bytes: 1500,
        maximum_store_bytes: 2000,
    };
    let store = DurableObjects::open(settings, database, cipher())
        .await
        .unwrap();
    let first = DurableObjects::new_id().unwrap();
    let second = DurableObjects::new_id().unwrap();
    let (a, b) = tokio::join!(
        store.reserve(first.clone(), "owner".into(), [1; 32], 1000),
        store.reserve(second.clone(), "owner".into(), [2; 32], 1000)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let (winner, loser, request) = if a.is_ok() {
        (first, second, [2; 32])
    } else {
        (second, first, [1; 32])
    };
    assert!(matches!(
        store
            .reserve(loser.clone(), "another-owner".into(), request, 1000)
            .await,
        Err(ObjectStoreError::Limit)
    ));
    let prepared = store
        .publish(&winner, Cursor::new(vec![8; 3]))
        .await
        .unwrap();
    store.bind(&prepared, retention()).await.unwrap();
    store
        .reserve(loser.clone(), "owner".into(), request, 1000)
        .await
        .unwrap();
    store.abandon(&loser).await.unwrap();
    let third = DurableObjects::new_id().unwrap();
    assert!(matches!(
        store
            .reserve(third.clone(), "owner".into(), [3; 32], 1000)
            .await,
        Err(ObjectStoreError::Limit)
    ));
    // Simulate an operational unlink failure, retaining all accounting.
    std::fs::create_dir(
        root.path()
            .join("objects")
            .join(format!("{}.object", loser.as_str())),
    )
    .unwrap();
    assert!(store.collect(&loser).await.is_err());
    assert!(matches!(
        store
            .reserve(third.clone(), "owner".into(), [3; 32], 1000)
            .await,
        Err(ObjectStoreError::Limit)
    ));
    std::fs::remove_dir(
        root.path()
            .join("objects")
            .join(format!("{}.object", loser.as_str())),
    )
    .unwrap();
    store.collect(&loser).await.unwrap();
    store
        .reserve(third, "owner".into(), [3; 32], 1000)
        .await
        .unwrap();
}

#[tokio::test]
async fn interrupted_source_never_creates_a_reference_and_retry_is_healthy() {
    struct FailedSource(Cursor<Vec<u8>>);
    impl Read for FailedSource {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = self.0.read(buffer)?;
            if count == 0 {
                Err(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
            } else {
                Ok(count)
            }
        }
    }
    let fixture = Fixture::new().await;
    let id = fixture.reserved(150_000).await;
    assert!(matches!(
        fixture
            .store
            .publish(&id, FailedSource(Cursor::new(vec![1; 70_000])))
            .await,
        Err(ObjectStoreError::Io(_))
    ));
    assert!(fixture.store.reference(&id).await.is_err());
    assert!(!fixture.path(&id, "object").exists());
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(vec![1; 150_000]))
        .await
        .unwrap();
    fixture.store.bind(&prepared, retention()).await.unwrap();
    assert_eq!(fixture.store.recover().await.unwrap().validated, 1);
}

#[tokio::test]
async fn payload_transaction_rollback_and_wrong_owner_fail_closed() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(3).await;
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(vec![3; 3]))
        .await
        .unwrap();
    let mut unit = fixture.store.begin().await.unwrap();
    let payload = fixture
        .store
        .bind_payload_in(
            unit.as_mut(),
            PreparedPayload::Object(prepared),
            retention(),
        )
        .await
        .unwrap();
    drop(unit);
    assert!(
        fixture
            .store
            .copy_payload("run", &payload, std::io::sink())
            .await
            .is_err()
    );
    let prepared = fixture
        .store
        .recover_publication(&id)
        .await
        .unwrap()
        .unwrap();
    let payload = fixture
        .store
        .bind_payload(PreparedPayload::Object(prepared), retention())
        .await
        .unwrap();
    assert!(
        fixture
            .store
            .copy_payload("another-run", &payload, std::io::sink())
            .await
            .is_err()
    );
    fixture
        .store
        .copy_payload("run", &payload, std::io::sink())
        .await
        .unwrap();
    assert!(StoredPayload::decode(br#"{"V2":{"Object":{}}}"#).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_and_hardlinks_cannot_replace_private_objects() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.path().join("outside");
    std::fs::write(&outside, b"do not overwrite").unwrap();
    for hardlink in [false, true] {
        let id = fixture.reserved(100).await;
        let staging = fixture.path(&id, "staging");
        if hardlink {
            std::fs::hard_link(&outside, &staging).unwrap();
        } else {
            std::os::unix::fs::symlink(&outside, &staging).unwrap();
        }
        assert!(
            fixture
                .store
                .publish(&id, Cursor::new(b"replace".to_vec()))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&outside).unwrap(), b"do not overwrite");
        std::fs::remove_file(staging).unwrap();
    }
}

#[test]
fn object_directory_is_distinct_from_cache_database_and_volumes() {
    use crate::config::{ServerDirectories, VolumeTable, validate_volumes};
    let root = tempfile::tempdir().unwrap();
    let dirs = ServerDirectories {
        durable_object_dir: Some(root.path().join("objects")),
        package_store_root: Some(root.path().join("packages")),
        database_path: Some(root.path().join("state.db")),
        volume_root: Some(root.path().join("volumes")),
        ..Default::default()
    };
    validate_volumes(&VolumeTable::new(), &dirs).unwrap();
    for path in [
        root.path().to_path_buf(),
        root.path().join("packages"),
        root.path().join("packages/objects"),
        root.path().join("volumes/objects"),
    ] {
        assert!(
            validate_volumes(
                &VolumeTable::new(),
                &ServerDirectories {
                    durable_object_dir: Some(path),
                    ..dirs.clone()
                }
            )
            .is_err()
        );
    }
}
