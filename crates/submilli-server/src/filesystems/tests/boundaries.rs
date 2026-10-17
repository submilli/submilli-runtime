use super::*;
use interpreter::runtime::vfs::{Access, MountSpec, VfsMode};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
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
        sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<9999) INSERT INTO durable_effects(run_id,effect_id,operation,version,request,recovery_payload,outcome,state) SELECT (SELECT run_id FROM durable_workspaces WHERE workspace_id=?), 'capacity-'||i, 'capacity-test', 1, X'00', X'00', X'00', 'done' FROM n")
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
    assert!(fs.execute_replayed_plan(&id(), steps).await.is_err());
    assert!(!fs.vfs().root().join("one").exists());
    assert!(
        fs.test_journal()
            .pending_plan(fs.id())
            .await
            .unwrap()
            .is_none()
    );
    fs.execute_replayed_plan(
        &id(),
        vec![Mutation::CreateDirectory {
            path: "/last".into(),
        }],
    )
    .await
    .unwrap();
    assert!(fs.close_writer(&writer, &id()).await.is_err());
    assert!(
        fs.test_journal()
            .pending_writer(fs.id())
            .await
            .unwrap()
            .is_none()
    );
    let workspace = fs.id().to_owned();
    fixture.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("DELETE FROM durable_effects WHERE run_id=(SELECT run_id FROM durable_workspaces WHERE workspace_id=?) AND effect_id LIKE 'capacity-%'").bind(workspace).execute(connection).await?;
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
    fs.execute_replayed(
        &effect,
        Mutation::CreateDirectory {
            path: "/existing".into(),
        },
    )
    .await
    .unwrap();
    assert!(
        fs.execute_replayed_plan(
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
    assert!(fs.recover_replayed().await.unwrap().is_none());
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
    fs.test_journal()
        .reader(fs.id(), &reader, "/file")
        .await
        .unwrap();
    assert!(fs.recover_replayed().await.unwrap().is_some());
    let mut handle = fs.open_reader(&reader, "/file").await.unwrap();
    let mut bytes = Vec::new();
    handle.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"data");
    drop(handle);
    let listing = id();
    fs.test_journal()
        .listing(fs.id(), &listing, "/", false, None)
        .await
        .unwrap();
    assert!(fs.recover_replayed().await.unwrap().is_some());
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
            fixture.context.clone(),
        )
        .await
        .unwrap();
        fs.execute_replayed(
            &id(),
            Mutation::Remove {
                path: "/cwd".into(),
            },
        )
        .await
        .unwrap();
        let content = fixture.input(b"old").await;
        fs.execute_replayed_plan(
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
            fixture.context.clone(),
        )
        .await
        .unwrap();
        assert_eq!(resumed.vfs().cwd(), "/cwd");
        assert!(!root.path().join("cwd").exists());
        assert!(Arc::ptr_eq(resumed.vfs().quota().unwrap(), &root_quota));
        let content = fixture.input(b"changed").await;
        resumed
            .execute_replayed(
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
            fixture.context.clone(),
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
