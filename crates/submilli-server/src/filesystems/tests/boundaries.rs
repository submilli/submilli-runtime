use super::*;
use interpreter::runtime::filesystem::FileReader;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[tokio::test]
async fn pending_resources_finish_on_explicit_retry_and_rejected_opens_replay() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    std::fs::write(fs.vfs().root().join("file"), b"data").unwrap();
    let reader = id();
    fs.test_journal()
        .reader(fs.id(), &reader, "/file", &byte_options())
        .await
        .unwrap();
    let mut handle = fs
        .open_reader(&reader, "/file", byte_options())
        .await
        .unwrap();
    let limits = interpreter::runtime::limits::TenantLimits::new(1 << 20);
    assert_eq!(
        read_bytes(handle.next(&resource_invocation(), &limits).await.unwrap()).unwrap(),
        b"dat"
    );
    drop(handle);
    let listing = id();
    fs.test_journal()
        .listing(fs.id(), &listing, "/", false, None)
        .await
        .unwrap();
    assert_eq!(
        fs.open_directory(&listing, "/", false, 0)
            .await
            .unwrap()
            .count(),
        1
    );
    let missing = id();
    assert!(matches!(
        fs.open_reader(&missing, "/missing", byte_options()).await,
        Err(FilesystemError::Rejected(_))
    ));
    std::fs::write(fs.vfs().root().join("missing"), b"later").unwrap();
    assert!(matches!(
        fs.open_reader(&missing, "/missing", byte_options()).await,
        Err(FilesystemError::Rejected(_))
    ));
}

#[tokio::test]
async fn cancelled_blocking_work_holds_admission_until_it_drains() {
    let fixture = Fixture::new().await;
    let gate = Arc::new(fixture.context.test_gate().clone().lock_owned().await);
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = tokio::spawn(blocking(gate, move || {
        started.send(()).unwrap();
        wait.recv().unwrap();
        Ok(())
    }));
    entered.await.unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(fixture.context.test_gate().try_lock().is_err());
    release.send(()).unwrap();
    let drained = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fixture.context.test_gate().lock(),
    )
    .await
    .unwrap();
    drop(drained);
    fixture
        .filesystem
        .execute_replayed(
            &id(),
            Mutation::CreateDirectory {
                path: "/healthy".into(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn changed_filesystem_binding_rejects_mutation_and_resource_retries() {
    use interpreter::runtime::vfs::{Access, MountSpec};
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    std::fs::write(fs.vfs().root().join("file"), b"original").unwrap();
    let operation = id();
    let request = Mutation::CreateDirectory {
        path: "/dir".into(),
    };
    fs.execute(&operation, request.clone()).await.unwrap();
    let reader = id();
    drop(
        fs.open_reader(&reader, "/file", byte_options())
            .await
            .unwrap(),
    );
    let listing = id();
    fs.open_directory(&listing, "/", false, 0).await.unwrap();
    let writer = id();
    let chunk = id();
    let close = id();
    fs.open_writer(&writer, "/output").await.unwrap();
    fs.write_chunk(&writer, &chunk, b"output".to_vec())
        .await
        .unwrap();
    fs.close_writer(&writer, &close).await.unwrap();
    let alternate = fixture.root.path().join("alternate");
    std::fs::create_dir(&alternate).unwrap();
    assert!(
        DurableFilesystem::open(
            fs.vfs().clone().with_access(Access::ReadOnly),
            fs.id(),
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
        let changed = DurableFilesystem::open(
            vfs,
            fs.id(),
            fixture.database.clone(),
            fixture.context.clone(),
        )
        .await
        .unwrap();
        assert!(changed.execute(&operation, request.clone()).await.is_err());
        assert!(
            changed
                .open_reader(&reader, "/file", byte_options())
                .await
                .is_err()
        );
        assert!(
            changed
                .open_directory(&listing, "/", false, 0)
                .await
                .is_err()
        );
        assert!(changed.open_writer(&writer, "/output").await.is_err());
        assert!(
            changed
                .write_chunk(&writer, &chunk, b"output".to_vec())
                .await
                .is_err()
        );
        assert!(changed.close_writer(&writer, &close).await.is_err());
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
            Mutation::Append {
                path: "/prefix".into(),
                content: content.clone(),
            },
        ),
        (
            second,
            Mutation::Append {
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
        .execute_replayed(&requests[0].0, requests[0].1.clone())
        .await
        .unwrap();
    publication_without_receipt(&fixture.filesystem, &requests[1].0, requests[1].1.clone()).await;
    let reopened = DurableFilesystem::open(
        fixture.filesystem.vfs().clone(),
        fixture.filesystem.id(),
        fixture.database.clone(),
        fixture.context.clone(),
    )
    .await
    .unwrap();
    // The caller has no aggregate completion yet. Its code repeats both calls.
    for (operation, request) in &requests {
        reopened
            .execute_replayed(operation, request.clone())
            .await
            .unwrap();
    }
    // Even another lost caller acknowledgement does not append again.
    for (operation, request) in &requests {
        reopened.execute(operation, request.clone()).await.unwrap();
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
    fs.execute(
        &operation,
        Mutation::CreateDirectory {
            path: "/existing".into(),
        },
    )
    .await
    .unwrap();
    let writer = id();
    fs.open_writer(&writer, "/output").await.unwrap();
    fs.write_chunk(&writer, &id(), b"output".to_vec())
        .await
        .unwrap();
    assert!(fs.close_writer(&writer, &operation).await.is_err());
    assert!(!fs.vfs().root().join("output").exists());
    fs.close_writer(&writer, &id()).await.unwrap();
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
    let request = Mutation::CreateDirectory {
        path: "/existing".into(),
    };
    fs.execute(&operation, request.clone()).await.unwrap();
    let writer = id();
    fs.open_writer(&writer, "/output").await.unwrap();
    fs.write_chunk(&writer, &id(), b"output".to_vec())
        .await
        .unwrap();
    let scope = fs.id().to_owned();
    // Count-only boundary injection: the padding records are never decoded.
    fixture.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("WITH RECURSIVE padding(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM padding WHERE n<9999) INSERT INTO durable_filesystem_operations(scope_id,operation_id,operation,version,request,outcome,state) SELECT ?, 'padding-'||n, 'test', 1, X'', X'', 'rejected' FROM padding")
            .bind(scope).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    fs.execute(&operation, request).await.unwrap();
    assert!(
        fs.execute(
            &id(),
            Mutation::CreateDirectory {
                path: "/blocked".into()
            }
        )
        .await
        .is_err()
    );
    assert!(fs.close_writer(&writer, &id()).await.is_err());
    assert!(!fs.vfs().root().join("blocked").exists());
    assert!(!fs.vfs().root().join("output").exists());
    // Admission refusal has not moved this writer to the closing state.
    fs.write_chunk(&writer, &id(), b" more".to_vec())
        .await
        .unwrap();
}

#[tokio::test]
async fn interrupted_writer_close_reserves_its_id_against_other_mutations() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let writer = id();
    let close = id();
    fs.open_writer(&writer, "/output").await.unwrap();
    fs.write_chunk(&writer, &id(), b"output".to_vec())
        .await
        .unwrap();
    fs.test_journal()
        .closing_writer(fs.id(), &writer, &close, i64::MAX as u64)
        .await
        .unwrap();
    let reopened = DurableFilesystem::open(
        fs.vfs().clone(),
        fs.id(),
        fixture.database.clone(),
        fixture.context.clone(),
    )
    .await
    .unwrap();
    assert!(
        reopened
            .execute(
                &close,
                Mutation::CreateDirectory {
                    path: "/unrelated".into()
                }
            )
            .await
            .is_err()
    );
    assert!(!fs.vfs().root().join("unrelated").exists());
    assert!(!fs.vfs().root().join("output").exists());
    reopened.close_writer(&writer, &close).await.unwrap();
    assert_eq!(
        std::fs::read(fs.vfs().root().join("output")).unwrap(),
        b"output"
    );
    fixture.assert_no_pending_work().await;
}
