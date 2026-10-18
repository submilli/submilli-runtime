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
    let mut unit = fixture.units()().await.unwrap();
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
    let mut unit = fixture.units()().await.unwrap();
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
async fn changed_filesystem_config_fingerprint_rejects_mutation_and_resource_retries() {
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
        fixture
            .filesystem
            .reopen(
                fs.vfs().clone().with_access(Access::ReadOnly),
                fs.run_id(),
                fixture.units()
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
        let changed = fixture
            .filesystem
            .reopen(vfs, fs.run_id(), fixture.units())
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
    let reopened = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
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
        sqlx::query("WITH RECURSIVE padding(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM padding WHERE n<9999) INSERT INTO durable_filesystem_operations(scope_id,operation_id,operation,version,request,state,status) SELECT ?, 'padding-'||n, 'test', 1, X'', X'', 'rejected' FROM padding")
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
    let reopened = fixture
        .filesystem
        .reopen(fs.vfs().clone(), fs.run_id(), fixture.units())
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

#[tokio::test]
async fn reopen_shares_live_quota_reservations_and_rejects_changed_limits() {
    let fixture = Fixture::with_filesystem_limit(Some(8)).await;
    let root = fixture.filesystem.vfs().root();
    let reopened = fixture
        .filesystem
        .reopen(
            Vfs::external(root.to_owned()).unwrap().with_size_limit(8),
            &fixture.run,
            fixture.units(),
        )
        .await
        .unwrap();
    let original_quota = fixture.filesystem.vfs().quota().unwrap();
    let reopened_quota = reopened.vfs().quota().unwrap();
    original_quota.reserve(8).unwrap();
    assert!(reopened_quota.reserve(1).is_err());
    original_quota.release(8);
    reopened_quota.reserve(1).unwrap();
    assert_eq!(original_quota.used(), 1);
    reopened_quota.release(1);
    assert!(
        fixture
            .filesystem
            .reopen(
                Vfs::external(root.to_owned()).unwrap().with_size_limit(16),
                &fixture.run,
                fixture.units(),
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mkdir_and_append_cannot_reuse_each_others_effect_ids() {
    let fixture = Fixture::new().await;
    let filesystem = fixture.filesystem.operation(None);
    let mkdir = id();
    filesystem
        .create_directory(&mkdir, "/directory")
        .await
        .unwrap();
    assert!(
        filesystem
            .append_bytes(&mkdir, "/file", b"bytes".to_vec())
            .await
            .is_err()
    );
    assert!(!fixture.filesystem.vfs().root().join("file").exists());
    let append = id();
    filesystem
        .append_bytes(&append, "/file", b"bytes".to_vec())
        .await
        .unwrap();
    assert!(
        filesystem
            .create_directory(&append, "/other")
            .await
            .is_err()
    );
    assert!(!fixture.filesystem.vfs().root().join("other").exists());
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("file")).unwrap(),
        b"bytes"
    );
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn durable_quota_changes_stay_on_the_affected_mount() {
    use interpreter::runtime::DiskQuota;
    use interpreter::runtime::vfs::{Access, MountSpec};
    let fixture = Fixture::new().await;
    let mounted = fixture.root.path().join("mounted");
    std::fs::create_dir(&mounted).unwrap();
    std::fs::write(mounted.join("file"), b"old!").unwrap();
    let mount_quota = Arc::new(DiskQuota::new(4, 4));
    let vfs = Vfs::external(fixture.filesystem.vfs().root().to_owned())
        .unwrap()
        .with_size_limit(4)
        .with_mount(MountSpec {
            guest_path: "/mounted".into(),
            host: mounted,
            volume: "mounted".into(),
            access: Access::ReadWrite,
            quota: Some(mount_quota.clone()),
        })
        .unwrap();
    let filesystem = fixture
        .filesystem
        .reopen(vfs, &fixture.run, fixture.units())
        .await
        .unwrap();
    let operation = filesystem.operation(None);
    operation
        .write_bytes(&id(), "/root", b"root".to_vec())
        .await
        .unwrap();
    operation
        .write_bytes(&id(), "/mounted/file", b"new!".to_vec())
        .await
        .unwrap();
    assert_eq!(filesystem.vfs().quota().unwrap().used(), 4);
    assert_eq!(mount_quota.used(), 4);
    operation
        .write_bytes(&id(), "/root", b"hi".to_vec())
        .await
        .unwrap();
    assert_eq!(filesystem.vfs().quota().unwrap().used(), 2);
    assert_eq!(mount_quota.used(), 4);
    assert!(matches!(
        operation
            .write_bytes(&id(), "/mounted/file", b"large".to_vec())
            .await,
        Err(FilesystemError::Rejected(_))
    ));
    assert_eq!(filesystem.vfs().quota().unwrap().used(), 2);
    assert_eq!(mount_quota.used(), 4);
}

#[tokio::test]
async fn durable_filesystem_rejects_overlapping_backing_roots_before_quota_setup() {
    use interpreter::runtime::DiskQuota;
    use interpreter::runtime::vfs::{Access, MountSpec};
    for mode in [
        "root-contains-mount",
        "mount-contains-root",
        "nested-mounts",
    ] {
        let fixture = Fixture::new().await;
        let outer = fixture.root.path().join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let root = match mode {
            "root-contains-mount" => outer.clone(),
            "mount-contains-root" => inner.clone(),
            _ => fixture.filesystem.vfs().root().to_owned(),
        };
        let mut vfs = Vfs::external(root.clone()).unwrap().with_size_limit(4);
        let hosts = if mode == "root-contains-mount" {
            vec![inner]
        } else if mode == "mount-contains-root" {
            vec![outer.clone()]
        } else {
            vec![outer.clone(), inner]
        };
        for (index, host) in hosts.into_iter().enumerate() {
            vfs = vfs
                .with_mount(MountSpec {
                    guest_path: format!("/mount-{index}"),
                    host,
                    volume: format!("volume-{index}"),
                    access: Access::ReadWrite,
                    quota: Some(Arc::new(DiskQuota::new(4, 0))),
                })
                .unwrap();
        }
        let result = open(
            vfs,
            &fixture.run,
            fixture.database.clone(),
            fixture.cipher.clone(),
        )
        .await;
        assert!(
            matches!(
                result,
                Err(FilesystemError::Recovery(
                    submilli_durable::RecoveryError::Unsupported("overlapping writable volumes")
                ))
            ),
            "{mode}"
        );
        assert!(!outer.join(".git-submilli-recovery").exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn overlapping_backing_roots_are_detected_through_symlink_spelling() {
    use interpreter::runtime::vfs::{Access, MountSpec};
    let fixture = Fixture::new().await;
    let outer = fixture.root.path().join("outer");
    std::fs::create_dir_all(outer.join("inner")).unwrap();
    let alias = fixture.root.path().join("alias");
    std::os::unix::fs::symlink(&outer, &alias).unwrap();
    let vfs = Vfs::external(outer)
        .unwrap()
        .with_mount(MountSpec {
            guest_path: "/mounted".into(),
            host: alias.join("inner"),
            volume: "alias".into(),
            access: Access::ReadWrite,
            quota: None,
        })
        .unwrap();
    let result = open(
        vfs,
        &fixture.run,
        fixture.database.clone(),
        fixture.cipher.clone(),
    )
    .await;
    assert!(matches!(
        result,
        Err(FilesystemError::Recovery(
            submilli_durable::RecoveryError::Unsupported("overlapping writable volumes")
        ))
    ));
}
