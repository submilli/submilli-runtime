use super::*;
use interpreter::runtime::filesystem::FileReader;

fn id() -> EffectId {
    uuid::Uuid::new_v4().to_string().into()
}

#[tokio::test]
async fn pending_resources_finish_on_explicit_retry_and_rejected_opens_replay() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    std::fs::write(fs.vfs().root().join("file"), b"data").unwrap();
    let reader = id();
    // Capture a valid request using the real open, then seed its pre-observation
    // state in a new identity to exercise retry of a pending resource.
    let seed = id();
    fs.operation(None)
        .open_reader(&seed, "/file", byte_options())
        .await
        .unwrap();
    let mut unit = fixture.context.units(fixture.database.clone())()
        .await
        .unwrap();
    let mut row = unit.get_reader(&fixture.run, &seed).await.unwrap().unwrap();
    row.id = reader.clone();
    row.position = None;
    unit.save_reader(&fixture.run, row).await.unwrap();
    unit.commit().await.unwrap();
    let mut handle = fs
        .operation(None)
        .open_reader(&reader, "/file", byte_options())
        .await
        .unwrap();
    let limits = interpreter::runtime::limits::TenantLimits::new(1 << 20);
    assert_eq!(
        read_bytes(
            handle
                .next(&resource_effect_id(), None, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        b"dat"
    );
    drop(handle);
    let listing = id();
    let seed = id();
    fs.listing(&seed, "/", false).await.unwrap();
    let mut unit = fixture.context.units(fixture.database.clone())()
        .await
        .unwrap();
    let mut row = unit
        .get_listing(&fixture.run, &seed)
        .await
        .unwrap()
        .unwrap();
    row.id = listing.clone();
    row.content = None;
    unit.save_listing(&fixture.run, row).await.unwrap();
    unit.commit().await.unwrap();
    let mut listing = fs.listing(&listing, "/", false).await.unwrap();
    let limits = interpreter::runtime::limits::TenantLimits::new(64 << 20);
    assert!(
        listing
            .next_entry(&resource_effect_id(), None, &limits)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        listing
            .next_entry(&resource_effect_id(), None, &limits)
            .await
            .unwrap()
            .is_none()
    );
    let missing = id();
    assert!(matches!(
        fs.operation(None)
            .open_reader(&missing, "/missing", byte_options())
            .await,
        Err(FilesystemError::Rejected(_))
    ));
    std::fs::write(fs.vfs().root().join("missing"), b"later").unwrap();
    assert!(matches!(
        fs.operation(None)
            .open_reader(&missing, "/missing", byte_options())
            .await,
        Err(FilesystemError::Rejected(_))
    ));
}

#[tokio::test]
async fn changed_filesystem_binding_rejects_mutation_and_resource_retries() {
    use interpreter::runtime::vfs::{Access, MountSpec};
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    std::fs::write(fs.vfs().root().join("file"), b"original").unwrap();
    let operation = id();
    let request = TestCall::CreateDirectory {
        path: "/dir".into(),
    };
    fs.call(&operation, request.clone()).await.unwrap();
    let reader = id();
    drop(
        fs.operation(None)
            .open_reader(&reader, "/file", byte_options())
            .await
            .unwrap(),
    );
    let listing = id();
    fs.listing(&listing, "/", false).await.unwrap();
    let writer = id();
    let chunk = id();
    let close = id();
    fs.operation(None)
        .open_writer(&writer, "/output")
        .await
        .unwrap();
    fs.operation(None)
        .write_chunk(&writer, &chunk, b"output".to_vec())
        .await
        .unwrap();
    fs.operation(None)
        .close_writer(&writer, &close)
        .await
        .unwrap();
    let alternate = fixture.root.path().join("alternate");
    std::fs::create_dir(&alternate).unwrap();
    assert!(
        open(
            fs.vfs().clone().with_access(Access::ReadOnly),
            fs.run_id(),
            fixture.database.clone(),
            fixture.context.clone(),
        )
        .await
        .is_err()
    );
    let configurations = [
        Vfs::external(alternate.clone()).unwrap(),
        fs.vfs().clone().with_cwd("/dir").unwrap(),
        fs.vfs()
            .clone()
            .with_mount(MountSpec {
                guest_path: "/mounted".into(),
                host: alternate.clone(),
                volume: "extra".into(),
                access: Access::ReadWrite,
                quota: None,
            })
            .unwrap(),
    ];
    for vfs in configurations {
        let changed = open(
            vfs,
            fs.run_id(),
            fixture.database.clone(),
            fixture.context.clone(),
        )
        .await
        .unwrap();
        assert!(changed.call(&operation, request.clone()).await.is_err());
        assert!(
            changed
                .operation(None)
                .open_reader(&reader, "/file", byte_options())
                .await
                .is_err()
        );
        assert!(changed.listing(&listing, "/", false).await.is_err());
        assert!(
            changed
                .operation(None)
                .open_writer(&writer, "/output")
                .await
                .is_err()
        );
        assert!(
            changed
                .operation(None)
                .write_chunk(&writer, &chunk, b"output".to_vec())
                .await
                .is_err()
        );
        assert!(
            changed
                .operation(None)
                .close_writer(&writer, &close)
                .await
                .is_err()
        );
    }
    assert!(!alternate.join("dir").exists());
    assert!(!alternate.join("output").exists());
    assert_eq!(
        std::fs::read(fs.vfs().root().join("file")).unwrap(),
        b"original"
    );
    assert_eq!(
        std::fs::read(fs.vfs().root().join("output")).unwrap(),
        b"output"
    );
}

#[tokio::test]
async fn caller_code_replays_completed_prefix_and_finishes_pending_suffix() {
    let fixture = Fixture::new().await;
    let first = id();
    let second = id();
    let content = fixture.input(b"once").await;
    let requests = [
        (
            first,
            TestCall::Append {
                path: "/prefix".into(),
                content: content.clone(),
            },
        ),
        (
            second,
            TestCall::Append {
                path: "/suffix".into(),
                content,
            },
        ),
    ];
    for name in ["prefix", "suffix"] {
        std::fs::write(fixture.filesystem.vfs().root().join(name), b"").unwrap();
    }
    fixture
        .filesystem
        .call(&requests[0].0, requests[0].1.clone())
        .await
        .unwrap();
    publication_without_receipt(&fixture, &requests[1].0, requests[1].1.clone()).await;
    let reopened = open(
        fixture.filesystem.vfs().clone(),
        fixture.filesystem.run_id(),
        fixture.database.clone(),
        fixture.context.clone(),
    )
    .await
    .unwrap();
    // The caller has no aggregate completion yet. Its code repeats both calls.
    for (operation, request) in &requests {
        reopened.call(operation, request.clone()).await.unwrap();
    }
    // Even another lost caller acknowledgement does not append again.
    for (operation, request) in &requests {
        reopened.call(operation, request.clone()).await.unwrap();
    }
    for name in ["prefix", "suffix"] {
        assert_eq!(
            std::fs::read(reopened.vfs().root().join(name)).unwrap(),
            b"once"
        );
    }
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn writer_close_cannot_reuse_a_completed_operation_id() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let operation = id();
    fs.call(
        &operation,
        TestCall::CreateDirectory {
            path: "/existing".into(),
        },
    )
    .await
    .unwrap();
    let writer = id();
    fs.operation(None)
        .open_writer(&writer, "/output")
        .await
        .unwrap();
    fs.operation(None)
        .write_chunk(&writer, &id(), b"output".to_vec())
        .await
        .unwrap();
    assert!(
        fs.operation(None)
            .close_writer(&writer, &operation)
            .await
            .is_err()
    );
    assert!(!fs.vfs().root().join("output").exists());
    fs.operation(None)
        .close_writer(&writer, &id())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(fs.vfs().root().join("output")).unwrap(),
        b"output"
    );
}

#[tokio::test]
async fn operation_limit_preserves_replay_and_does_not_fence_a_writer() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let operation = id();
    let request = TestCall::CreateDirectory {
        path: "/existing".into(),
    };
    fs.call(&operation, request.clone()).await.unwrap();
    let writer = id();
    fs.operation(None)
        .open_writer(&writer, "/output")
        .await
        .unwrap();
    fs.operation(None)
        .write_chunk(&writer, &id(), b"output".to_vec())
        .await
        .unwrap();
    let run_id = fs.run_id().to_owned();
    // Count-only boundary injection: the padding records are never decoded.
    fixture.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("WITH RECURSIVE padding(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM padding WHERE n<9999) INSERT INTO durable_filesystem_operations(scope_id,operation_id,operation,version,request,outcome,state) SELECT ?, 'padding-'||n, 'test', 1, X'', X'', 'rejected' FROM padding")
            .bind(run_id.as_str()).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    fs.call(&operation, request).await.unwrap();
    assert!(
        fs.call(
            &id(),
            TestCall::CreateDirectory {
                path: "/blocked".into()
            }
        )
        .await
        .is_err()
    );
    assert!(
        fs.operation(None)
            .close_writer(&writer, &id())
            .await
            .is_err()
    );
    assert!(!fs.vfs().root().join("blocked").exists());
    assert!(!fs.vfs().root().join("output").exists());
    // Admission refusal has not moved this writer to the closing state.
    fs.operation(None)
        .write_chunk(&writer, &id(), b" more".to_vec())
        .await
        .unwrap();
}

#[tokio::test]
async fn interrupted_writer_close_reserves_its_id_against_other_mutations() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let writer = id();
    let close = id();
    fs.operation(None)
        .open_writer(&writer, "/output")
        .await
        .unwrap();
    fs.operation(None)
        .write_chunk(&writer, &id(), b"output".to_vec())
        .await
        .unwrap();
    reserve_close(&fixture, &writer, &close).await;
    let reopened = open(
        fs.vfs().clone(),
        fs.run_id(),
        fixture.database.clone(),
        fixture.context.clone(),
    )
    .await
    .unwrap();
    assert!(
        reopened
            .call(
                &close,
                TestCall::CreateDirectory {
                    path: "/unrelated".into()
                }
            )
            .await
            .is_err()
    );
    assert!(!fs.vfs().root().join("unrelated").exists());
    assert!(!fs.vfs().root().join("output").exists());
    reopened
        .operation(None)
        .close_writer(&writer, &close)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(fs.vfs().root().join("output")).unwrap(),
        b"output"
    );
    fixture.assert_no_pending_work().await;
}
