use super::*;
use interpreter::runtime::vfs::{Access, MountSpec, VfsMode};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[tokio::test]
async fn aggregate_quota_refusal_keeps_writer_editable_and_retireable() {
    let fixture = Fixture::with_limits(crate::objects::ObjectLimits {
        maximum_object_bytes: 1536 * 1024,
        maximum_owner_bytes: 2 * 1024 * 1024,
        maximum_store_bytes: 4 * 1024 * 1024,
    })
    .await;
    let fs = &fixture.filesystem;
    let writer = id();
    fs.open_writer(&writer, "/out").await.unwrap();
    for byte in *b"ab" {
        let chunk = fixture.capture(&vec![byte; 600 * 1024]).await;
        fs.write_chunk(&writer, &id(), chunk).await.unwrap();
    }
    let close = id();
    assert!(matches!(
        fs.close_writer(&writer, &close).await,
        Err(FilesystemError::Objects(ObjectStoreError::Limit))
    ));
    assert!(fs.journal.pending_writer(fs.id()).await.unwrap().is_none());
    let chunk = fixture.capture(b"after refusal").await;
    fs.write_chunk(&writer, &id(), chunk).await.unwrap();
    // A changed chunk list gets another capture ID, rather than conflicting
    // with the private producer tracked by the previous refused attempt.
    assert!(matches!(
        fs.close_writer(&writer, &close).await,
        Err(FilesystemError::Objects(ObjectStoreError::Limit))
    ));
    assert_eq!(
        fs.open_directory(&id(), "/", false, 0)
            .await
            .unwrap()
            .count(),
        0
    );
    fs.execute(
        &id(),
        Mutation::CreateDirectory {
            path: "/healthy".into(),
        },
    )
    .await
    .unwrap();
    fixture.terminal().await;
    fs.retire("owner").await.unwrap();
}

#[tokio::test]
async fn cancelled_close_reservation_allows_more_chunks_and_retires_stale_capture() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let writer = id();
    fs.open_writer(&writer, "/out").await.unwrap();
    let first = fixture.capture(b"first").await;
    fs.write_chunk(&writer, &id(), first.clone()).await.unwrap();
    // State at cancellation after successful quota reservation but before
    // close admission: no publication or close effect has started.
    let stale = fs.reserve_writer_capture(&writer, &[first]).await.unwrap();
    let second = fixture.capture(b" second").await;
    fs.write_chunk(&writer, &id(), second).await.unwrap();
    fs.close_writer(&writer, &id()).await.unwrap();
    assert_eq!(
        std::fs::read(fs.vfs().root().join("out")).unwrap(),
        b"first second"
    );
    fixture.terminal().await;
    fs.retire("owner").await.unwrap();
    fixture.objects.recover().await.unwrap();
    assert!(fixture.objects.reference(&stale).await.is_err());
}

#[tokio::test]
async fn directory_capture_reserves_actual_encoded_size_and_records_quota_refusal() {
    for maximum_owner_bytes in [4096, 2] {
        let fixture = Fixture::with_limits(crate::objects::ObjectLimits {
            maximum_object_bytes: maximum_owner_bytes,
            maximum_owner_bytes,
            maximum_store_bytes: maximum_owner_bytes,
        })
        .await;
        let fs = &fixture.filesystem;
        let resource = id();
        let result = fs.open_directory(&resource, "/", false, 0).await;
        if maximum_owner_bytes == 4096 {
            assert_eq!(result.unwrap().count(), 0);
        } else {
            assert!(matches!(result, Err(FilesystemError::Rejected(_))));
            assert!(matches!(
                fs.open_directory(&resource, "/", false, 0).await,
                Err(FilesystemError::Rejected(_))
            ));
        }
        fs.execute(
            &id(),
            Mutation::CreateDirectory {
                path: "/healthy".into(),
            },
        )
        .await
        .unwrap();
        fixture.terminal().await;
        fs.retire("owner").await.unwrap();
    }
}

#[tokio::test]
async fn permanent_resource_limits_do_not_fence_the_workspace() {
    let fixture = Fixture::with_maximum_object_bytes(16).await;
    let fs = &fixture.filesystem;
    let writer = id();
    fs.open_writer(&writer, "/out").await.unwrap();
    let chunk = fixture.capture(b"0123456789").await;
    fs.write_chunk(&writer, &id(), chunk.clone()).await.unwrap();
    fs.write_chunk(&writer, &id(), chunk).await.unwrap();
    assert!(fs.close_writer(&writer, &id()).await.is_err());
    assert!(fs.journal.pending_writer(fs.id()).await.unwrap().is_none());
    assert!(!fs.vfs().root().join("out").exists());
    assert_eq!(
        fs.open_directory(&id(), "/", false, 0)
            .await
            .unwrap()
            .count(),
        0
    );

    std::fs::write(fs.vfs().root().join("entry"), b"a").unwrap();
    let listing = id();
    assert!(matches!(
        fs.open_directory(&listing, "/", false, 0).await,
        Err(FilesystemError::Rejected(_))
    ));
    fs.execute(
        &id(),
        Mutation::Remove {
            path: "/entry".into(),
        },
    )
    .await
    .unwrap();
    // Replaying the oversized snapshot remains a refusal even after it fits.
    assert!(matches!(
        fs.open_directory(&listing, "/", false, 0).await,
        Err(FilesystemError::Rejected(_))
    ));
    assert!(fs.recover().await.unwrap().is_none());
    fixture.terminal().await;
    fs.retire("owner").await.unwrap();
}

#[tokio::test]
async fn compound_admission_reserves_all_effect_slots() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let writer = id();
    fs.open_writer(&writer, "/writer").await.unwrap();
    // Inject completed rows only to exercise the admission count boundary;
    // these synthetic rows are removed before any outcome decoding or cleanup.
    let workspace = fs.id().to_owned();
    fixture.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<9999) INSERT INTO durable_filesystem_effects(workspace_id,effect_id,request,intent,outcome,state) SELECT ?, 'capacity-'||i, X'00', X'00', X'00', 'done' FROM n")
            .bind(workspace).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let steps = vec![
        Mutation::CreateDirectory {
            path: "/one".into(),
        },
        Mutation::CreateDirectory {
            path: "/two".into(),
        },
    ];
    assert!(fs.execute_plan(&id(), steps).await.is_err());
    assert!(!fs.vfs().root().join("one").exists());
    assert!(fs.journal.pending_plan(fs.id()).await.unwrap().is_none());
    fs.execute_plan(
        &id(),
        vec![Mutation::CreateDirectory {
            path: "/last".into(),
        }],
    )
    .await
    .unwrap();
    assert!(fs.close_writer(&writer, &id()).await.is_err());
    assert!(fs.journal.pending_writer(fs.id()).await.unwrap().is_none());
    let workspace = fs.id().to_owned();
    fixture.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("DELETE FROM durable_filesystem_effects WHERE workspace_id=? AND effect_id LIKE 'capacity-%'").bind(workspace).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    fixture.terminal().await;
    fs.retire("owner").await.unwrap();
}

#[tokio::test]
async fn reused_effect_ids_refuse_compound_admission_before_fencing() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    let plan = id();
    let effect = crate::adapters::durable_filesystems::plan_effect_id(fs.id(), &plan, 0);
    fs.execute(
        &effect,
        Mutation::CreateDirectory {
            path: "/existing".into(),
        },
    )
    .await
    .unwrap();
    assert!(
        fs.execute_plan(
            &plan,
            vec![Mutation::CreateDirectory {
                path: "/new".into()
            }]
        )
        .await
        .is_err()
    );
    let writer = id();
    fs.open_writer(&writer, "/out").await.unwrap();
    assert!(fs.close_writer(&writer, &effect).await.is_err());
    assert!(fs.recover().await.unwrap().is_none());
    fs.close_writer(&writer, &id()).await.unwrap();
    fixture.terminal().await;
    fs.retire("owner").await.unwrap();
}

#[tokio::test]
async fn pending_resources_recover_and_rejected_opens_replay() {
    let fixture = Fixture::new().await;
    let fs = &fixture.filesystem;
    std::fs::write(fs.vfs().root().join("file"), b"data").unwrap();
    let reader = id();
    fs.journal.reader(fs.id(), &reader, "/file").await.unwrap();
    assert!(fs.recover().await.unwrap().is_some());
    let mut handle = fs.open_reader(&reader, "/file").await.unwrap();
    let mut bytes = Vec::new();
    handle.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"data");
    drop(handle);
    let listing = id();
    fs.journal
        .listing(fs.id(), &listing, "/", false, None)
        .await
        .unwrap();
    assert!(fs.recover().await.unwrap().is_some());
    assert_eq!(
        fs.open_directory(&listing, "/", false, 0)
            .await
            .unwrap()
            .count(),
        1
    );
    let missing = id();
    assert!(matches!(
        fs.open_reader(&missing, "/missing").await,
        Err(FilesystemError::Rejected(_))
    ));
    std::fs::write(fs.vfs().root().join("missing"), b"later").unwrap();
    assert!(matches!(
        fs.open_reader(&missing, "/missing").await,
        Err(FilesystemError::Rejected(_))
    ));
    fixture.terminal().await;
    fs.retire("owner").await.unwrap();
}

async fn additional_run(fixture: &Fixture, order: i64) -> String {
    let run = id();
    let value = run.clone();
    fixture.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state) VALUES(?,?,'session','request','startup',?,0,'running')")
            .bind(&value).bind(&value).bind(order).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    run
}

async fn finish_run(fixture: &Fixture, run: String) {
    fixture
        .database
        .transaction(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "UPDATE runs SET state='finished',completed_at_unix_ms=1 WHERE run_id=?",
                )
                .bind(run)
                .execute(connection)
                .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn named_and_session_roots_resume_deleted_cwd_and_release_mounted_quota() {
    let fixture = Fixture::new().await;
    for (index, mode) in [VfsMode::Named, VfsMode::PerSession]
        .into_iter()
        .enumerate()
    {
        let root = tempfile::tempdir().unwrap();
        let mount = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("original"), b"old").unwrap();
        let root_quota = Arc::new(interpreter::runtime::DiskQuota::new(1024, 3));
        let mount_quota = Arc::new(interpreter::runtime::DiskQuota::new(1024, 0));
        let vfs = Vfs::external_with_mode(root.path().into(), mode)
            .unwrap()
            .with_shared_quota(root_quota.clone())
            .with_cwd("/cwd")
            .unwrap()
            .with_mount(MountSpec {
                guest_path: "/mnt".into(),
                host: mount.path().into(),
                volume: "mounted".into(),
                access: Access::ReadWrite,
                quota: Some(mount_quota.clone()),
            })
            .unwrap();
        let run = additional_run(&fixture, index as i64 + 2).await;
        let fs = DurableFilesystem::open(
            vfs,
            &run,
            "owner",
            fixture.database.clone(),
            fixture.objects.clone(),
        )
        .await
        .unwrap();
        fs.execute(
            &id(),
            Mutation::Remove {
                path: "/cwd".into(),
            },
        )
        .await
        .unwrap();
        let content = fixture.capture(b"old").await;
        fs.execute_plan(
            &id(),
            vec![
                Mutation::Write {
                    path: "/mnt/copied".into(),
                    content,
                },
                Mutation::Remove {
                    path: "/original".into(),
                },
            ],
        )
        .await
        .unwrap();
        assert_eq!(root_quota.used(), 3, "unlinked original remains pinned");
        assert_eq!(mount_quota.used(), 3);
        let resumed = DurableFilesystem::resume(
            fs.id(),
            "owner",
            fixture.database.clone(),
            fixture.objects.clone(),
        )
        .await
        .unwrap();
        assert_eq!(resumed.vfs().cwd(), "/cwd");
        assert!(!root.path().join("cwd").exists());
        assert!(Arc::ptr_eq(resumed.vfs().quota().unwrap(), &root_quota));
        let content = fixture.capture(b"changed").await;
        resumed
            .execute(
                &id(),
                Mutation::Write {
                    path: "/mnt/copied".into(),
                    content,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            mount_quota.used(),
            10,
            "old and new versions share one counter"
        );
        finish_run(&fixture, run).await;
        // Cleanup through the restart API also refreshes counters held by an
        // older live VFS in this process.
        DurableFilesystem::recover_cleanup(
            fs.id(),
            "owner",
            fixture.database.clone(),
            fixture.objects.clone(),
        )
        .await
        .unwrap();
        assert_eq!(root_quota.used(), 0);
        assert_eq!(mount_quota.used(), 7);
        assert!(root.path().is_dir());
        assert_eq!(
            std::fs::read(mount.path().join("copied")).unwrap(),
            b"changed"
        );
    }
}

#[tokio::test]
async fn cancelled_blocking_work_holds_admission_until_it_drains() {
    let fixture = Fixture::new().await;
    let gate = Arc::new(fixture.objects.filesystem_gate.clone().lock_owned().await);
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
    assert!(fixture.objects.filesystem_gate.try_lock().is_err());
    release.send(()).unwrap();
    let drained = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        fixture.objects.filesystem_gate.lock(),
    )
    .await
    .unwrap();
    drop(drained);
    fixture
        .filesystem
        .execute(
            &id(),
            Mutation::CreateDirectory {
                path: "/healthy".into(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn capture_storage_cannot_be_a_workspace_or_mount() {
    let fixture = Fixture::new().await;
    let root = tempfile::tempdir().unwrap();
    let vfs = Vfs::external(root.path().into())
        .unwrap()
        .with_mount(MountSpec {
            guest_path: "/captures".into(),
            host: fixture.objects.directory().into(),
            volume: "private".into(),
            access: Access::ReadWrite,
            quota: None,
        })
        .unwrap();
    let run = additional_run(&fixture, 2).await;
    assert!(matches!(
        DurableFilesystem::open(
            vfs,
            &run,
            "owner",
            fixture.database.clone(),
            fixture.objects.clone()
        )
        .await,
        Err(FilesystemError::Recovery(RecoveryError::Unsupported(_)))
    ));
}
