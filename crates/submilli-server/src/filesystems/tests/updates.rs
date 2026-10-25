use super::*;
use interpreter::runtime::filesystem::{
    FileUpdate, FileUpdateOutput, FilesystemError as PortError, PreparedFileUpdate,
};
use interpreter::runtime::fuel::WorkBudget;
use interpreter::runtime::limits::{HostBytes, TenantLimits};

/// Deliberately non-idempotent transformation; retry must never call prepare again.
pub(super) struct Insert {
    path: std::path::PathBuf,
    pub preparations: usize,
    pub forbid_prepare: bool,
    fuel: u64,
    extra_capacity: usize,
    preparation_memory: Option<HostBytes>,
    limits: Arc<TenantLimits>,
}
impl Insert {
    pub(super) fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            preparations: 0,
            forbid_prepare: false,
            fuel: u64::MAX,
            extra_capacity: 0,
            preparation_memory: None,
            limits: Arc::new(TenantLimits::new(32 << 20)),
        }
    }
}
#[async_trait::async_trait]
impl FileUpdate for Insert {
    async fn prepare(&mut self) -> Result<PreparedFileUpdate, PortError> {
        assert!(
            !self.forbid_prepare,
            "recovery must not recompute a captured transformation"
        );
        self.preparations += 1;
        let original = std::fs::read(&self.path)?;
        let mut contents = if self.extra_capacity > 0 {
            self.preparation_memory = Some(self.reserve(self.extra_capacity as u64)?);
            Vec::with_capacity(self.extra_capacity)
        } else {
            Vec::new()
        };
        contents.extend_from_slice(b"inserted\n");
        contents.extend_from_slice(&original);
        Ok(PreparedFileUpdate {
            contents: Some(contents),
            output: FileUpdateOutput {
                bytes: original,
                memory: self.reserve(1024)?,
            },
        })
    }
    fn authorize(&mut self, output: &[u8], bytes: u64) -> Result<(), PortError> {
        if self.fuel == 0 {
            return Err(PortError::Exhausted(interpreter::runtime::fuel::Exhausted));
        }
        assert_eq!(output, b"original\n");
        assert_eq!(bytes, 18);
        Ok(())
    }
}
impl Insert {
    pub(super) async fn execute(
        &mut self,
        filesystem: &dyn Filesystem,
        effect: &EffectId,
        path: &str,
        request: [u8; 32],
    ) -> Result<FileUpdateOutput, PortError> {
        let work = Arc::new(WorkBudget::new(self.fuel));
        let limits = self.limits.clone();
        filesystem
            .update_file(effect, work, path, request, self, &limits)
            .await
    }
    fn reserve(&mut self, bytes: u64) -> Result<HostBytes, PortError> {
        HostBytes::new(&self.limits, bytes).map_err(|e| PortError::Execution(e.into()))
    }
}

#[tokio::test]
async fn update_receipt_replays_exact_output_without_touching_later_files() {
    let fixture = Fixture::new().await;
    let path = fixture.filesystem.vfs().root().join("file");
    std::fs::write(&path, b"original\n").unwrap();
    let effect = uuid::Uuid::new_v4().to_string().into();
    let mut update = Insert::new(path.clone());
    let output = update
        .execute(&fixture.filesystem, &effect, "/file", [1; 32])
        .await
        .unwrap();
    assert_eq!(output.bytes, b"original\n");
    assert_eq!(update.preparations, 1);
    drop(output);
    update.forbid_prepare = true;
    std::fs::remove_file(&path).unwrap();
    let output = update
        .execute(&fixture.filesystem, &effect, "/file", [1; 32])
        .await
        .unwrap();
    assert_eq!(output.bytes, b"original\n");
    assert!(!path.exists());
    drop(output);
    assert_eq!(update.limits.host_attached_bytes(), 0);
    assert!(
        update
            .execute(&fixture.filesystem, &effect, "/file", [2; 32])
            .await
            .is_err()
    );
    assert!(
        update
            .execute(&fixture.filesystem, &effect, "/other", [1; 32])
            .await
            .is_err()
    );
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn update_recovers_publication_without_receipt_and_preserves_permissions() {
    let fixture = Fixture::new().await;
    let path = fixture.filesystem.vfs().root().join("file");
    std::fs::write(&path, b"original\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o751)).unwrap();
    }
    let effect = uuid::Uuid::new_v4().to_string().into();
    let mut update = Insert::new(path.clone());
    fail_operation_updates(&fixture, "NEW.status='done'").await;
    assert!(
        update
            .execute(&fixture.filesystem, &effect, "/file", [1; 32])
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"inserted\noriginal\n");
    clear_operation_failure(&fixture).await;
    update.forbid_prepare = true;
    let output = update
        .execute(&fixture.filesystem, &effect, "/file", [1; 32])
        .await
        .unwrap();
    assert_eq!(output.bytes, b"original\n");
    assert_eq!(std::fs::read(&path).unwrap(), b"inserted\noriginal\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o751
        );
    }
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn update_recovery_refuses_damaged_candidate_and_missing_result() {
    for damage_result in [false, true] {
        let fixture = Fixture::new().await;
        let path = fixture.filesystem.vfs().root().join("file");
        std::fs::write(&path, b"original\n").unwrap();
        let effect = uuid::Uuid::new_v4().to_string().into();
        let mut update = Insert::new(path.clone());
        fail_operation_updates(&fixture, "NEW.status='done'").await;
        assert!(
            update
                .execute(&fixture.filesystem, &effect, "/file", [1; 32])
                .await
                .is_err()
        );
        clear_operation_failure(&fixture).await;
        if damage_result {
            fixture
                .database
                .transaction(|connection| {
                    Box::pin(async move {
                        sqlx::query("DELETE FROM durable_file_update_outputs")
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                })
                .await
                .unwrap();
        } else {
            // Public and retained candidate are the same inode before the receipt.
            std::fs::write(&path, b"damaged").unwrap();
        }
        update.forbid_prepare = true;
        assert!(
            update
                .execute(&fixture.filesystem, &effect, "/file", [1; 32])
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn cancelled_update_owns_memory_and_serialization_until_receipt_finishes() {
    let fixture = Fixture::new().await;
    let path = fixture.filesystem.vfs().root().join("file");
    std::fs::write(&path, b"original\n").unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    // The first transaction captures preparation. The second commits the receipt.
    let units =
        super::resources::pause_transaction(fixture.units(), 1, entered.clone(), release.clone());
    let backend = fixture
        .filesystem
        .reopen(fixture.filesystem.vfs().clone(), &fixture.run, units)
        .await
        .unwrap();
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let mut update = Insert::new(path.clone());
    let limits = update.limits.clone();
    let task_effect = effect.clone();
    let task = tokio::spawn(async move {
        update
            .execute(&backend, &task_effect, "/file", [1; 32])
            .await
            .map(|_| ())
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(limits.host_attached_bytes() > 0);
    let backend = fixture.filesystem.clone();
    let mut retry = Insert::new(path.clone());
    retry.forbid_prepare = true;
    let retry = tokio::spawn(async move {
        retry
            .execute(&backend, &effect, "/file", [1; 32])
            .await
            .unwrap()
            .bytes
    });
    tokio::task::yield_now().await;
    assert!(
        !retry.is_finished(),
        "retry must drain the cancelled worker"
    );
    release.notify_one();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), retry)
            .await
            .unwrap()
            .unwrap(),
        b"original\n"
    );
    assert_eq!(limits.host_attached_bytes(), 0);
    assert_eq!(std::fs::read(path).unwrap(), b"inserted\noriginal\n");
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn update_preflight_limits_refuse_before_publication() {
    let fixture = Fixture::with_filesystem_limit(Some(12)).await;
    fixture
        .filesystem
        .operation(None)
        .write_bytes(
            &uuid::Uuid::new_v4().to_string().into(),
            "/file",
            b"original\n".to_vec(),
        )
        .await
        .unwrap();
    let path = fixture.filesystem.vfs().root().join("file");
    for kind in ["fuel", "memory", "quota"] {
        let mut update = Insert::new(path.clone());
        if kind == "fuel" {
            update.fuel = 0;
        }
        if kind == "memory" {
            update.limits = Arc::new(TenantLimits::new(4096));
        }
        let error = update
            .execute(
                &fixture.filesystem,
                &uuid::Uuid::new_v4().to_string().into(),
                "/file",
                [1; 32],
            )
            .await
            .err()
            .unwrap();
        match kind {
            "fuel" => assert!(matches!(error, PortError::Exhausted(_)), "{error}"),
            "memory" => assert!(matches!(error, PortError::Execution(_)), "{error}"),
            _ => assert!(matches!(error, PortError::Quota(_)), "{error}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"original\n");
        assert_eq!(update.limits.host_attached_bytes(), 0);
        fixture.assert_no_pending_work().await;
    }
}

#[tokio::test]
async fn update_recovery_admits_actual_stored_size_and_validation_scratch() {
    for malformed_result in [false, true] {
        let fixture = Fixture::new().await;
        let path = fixture.filesystem.vfs().root().join("file");
        std::fs::write(&path, b"original\n").unwrap();
        let effect = uuid::Uuid::new_v4().to_string().into();
        let mut update = Insert::new(path.clone());
        fail_operation_updates(&fixture, "NEW.status='done'").await;
        assert!(
            update
                .execute(&fixture.filesystem, &effect, "/file", [1; 32])
                .await
                .is_err()
        );
        clear_operation_failure(&fixture).await;
        if malformed_result {
            fixture.database.transaction(|connection| Box::pin(async move {
                sqlx::query("UPDATE durable_file_update_outputs SET bytes=1,output=zeroblob(1048576)").execute(connection).await?;
                Ok(())
            })).await.unwrap();
        }
        update.forbid_prepare = true;
        update.limits = Arc::new(TenantLimits::new(if malformed_result {
            256 << 10
        } else {
            32 << 10
        }));
        let error = update
            .execute(&fixture.filesystem, &effect, "/file", [1; 32])
            .await
            .err()
            .unwrap();
        assert!(
            matches!(error, PortError::Execution(error) if error.is::<interpreter::runtime::limits::MemoryCapExceeded>())
        );
        assert_eq!(update.limits.host_attached_bytes(), 0);
        assert_eq!(std::fs::read(path).unwrap(), b"inserted\noriginal\n");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn update_replaces_contained_symlink_without_changing_its_target() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    std::fs::write(root.join("target"), b"original\n").unwrap();
    std::os::unix::fs::symlink("target", root.join("file")).unwrap();
    let effect = uuid::Uuid::new_v4().to_string().into();
    let mut update = Insert::new(root.join("file"));
    fail_operation_updates(&fixture, "NEW.status='done'").await;
    assert!(
        update
            .execute(&fixture.filesystem, &effect, "/file", [1; 32])
            .await
            .is_err()
    );
    clear_operation_failure(&fixture).await;
    update.forbid_prepare = true;
    let output = update
        .execute(&fixture.filesystem, &effect, "/file", [1; 32])
        .await
        .unwrap();
    assert_eq!(output.bytes, b"original\n");
    assert!(
        std::fs::symlink_metadata(root.join("file"))
            .unwrap()
            .is_file()
    );
    assert_eq!(std::fs::read(root.join("target")).unwrap(), b"original\n");
    assert_eq!(
        std::fs::read(root.join("file")).unwrap(),
        b"inserted\noriginal\n"
    );
}

#[tokio::test]
async fn update_admits_candidate_capacity_before_detaching_from_the_caller() {
    let fixture = Fixture::new().await;
    let path = fixture.filesystem.vfs().root().join("file");
    std::fs::write(&path, b"original\n").unwrap();
    let mut update = Insert::new(path.clone());
    update.extra_capacity = 512 << 10;
    update.limits = Arc::new(TenantLimits::new(768 << 10));
    let limits = update.limits.clone();
    let error = update
        .execute(
            &fixture.filesystem,
            &uuid::Uuid::new_v4().to_string().into(),
            "/file",
            [1; 32],
        )
        .await
        .err()
        .unwrap();
    assert!(
        matches!(error, PortError::Execution(error) if error.is::<interpreter::runtime::limits::MemoryCapExceeded>())
    );
    assert_eq!(std::fs::read(path).unwrap(), b"original\n");
    drop(update);
    assert_eq!(limits.host_attached_bytes(), 0);
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn pending_update_authorization_preserves_fuel_exhaustion() {
    let fixture = Fixture::new().await;
    let path = fixture.filesystem.vfs().root().join("file");
    std::fs::write(&path, b"original\n").unwrap();
    let effect = uuid::Uuid::new_v4().to_string().into();
    let mut update = Insert::new(path.clone());
    fail_operation_updates(&fixture, "NEW.status='done'").await;
    assert!(
        update
            .execute(&fixture.filesystem, &effect, "/file", [1; 32])
            .await
            .is_err()
    );
    clear_operation_failure(&fixture).await;
    update.forbid_prepare = true;
    update.fuel = 0;
    let error = update
        .execute(&fixture.filesystem, &effect, "/file", [1; 32])
        .await
        .err()
        .unwrap();
    assert!(matches!(error, PortError::Exhausted(_)));
    update.fuel = u64::MAX;
    update
        .execute(&fixture.filesystem, &effect, "/file", [1; 32])
        .await
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"inserted\noriginal\n");
}
