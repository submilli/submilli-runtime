//! Captured native updates: prepare once, publish the captured bytes, retain the result.
use super::*;
use interpreter::runtime::filesystem::{FileUpdate, FileUpdateOutput, PreparedFileUpdate};
use interpreter::runtime::fuel;
use interpreter::runtime::limits::HostBytes;
use serde::{Deserialize, Serialize};

const MAX_OUTPUT: u64 = 64 * 1024 * 1024;
const MAX_RUN_OUTPUT: u64 = 1024 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
enum FileUpdateState {
    #[serde(skip)]
    Initial,
    Prepared(Content),
    Completed(Result<Outcome, String>),
}

impl DurableFilesystem {
    pub(super) async fn begin_file_update(
        &self,
        effect: &EffectId,
        path: &str,
        request: [u8; 32],
        limits: &TenantLimits,
    ) -> Result<Box<dyn FileUpdate>, PortError> {
        paths::validate_guest_path(path)
            .map_err(FilesystemError::from)
            .map_err(backend)?;
        validate_id(effect).map_err(backend)?;
        let gate = Arc::new(self.filesystem_gate.clone().lock_owned().await);
        let operation = self.operation(None);
        let mut unit = (self.units)()
            .await
            .map_err(FilesystemError::from)
            .map_err(backend)?;
        let journal = operation
            .create_initial_or_get_last_journal_entry(
                &mut *unit,
                effect,
                "file-update",
                &(path, request),
            )
            .await
            .map_err(backend)?;
        let state = decode_state(&journal).map_err(backend)?;
        let mut operation = UpdatePublication {
            filesystem: self.clone(),
            state,
            unit: Some(unit),
            output: None,
            gate,
            journal,
            path: path.to_owned(),
        };
        if !matches!(operation.state, FileUpdateState::Initial) {
            operation.load_saved_output(limits).await?;
        }
        Ok(Box::new(operation))
    }
}

/// Owns serialization through native work, SQL completion, and cancellation.
struct UpdatePublication {
    filesystem: DurableFilesystem,
    state: FileUpdateState,
    unit: Option<Box<dyn UnitOfWork>>,
    output: Option<FileUpdateOutput>,
    gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
    journal: JournalEntry,
    path: String,
}
#[async_trait::async_trait]
impl FileUpdate for UpdatePublication {
    fn saved_output(&self) -> Option<(&[u8], u64)> {
        let bytes = match &self.state {
            FileUpdateState::Prepared(content) => content.bytes,
            FileUpdateState::Completed(Ok(outcome)) => outcome.bytes,
            _ => return None,
        };
        self.output
            .as_ref()
            .map(|output| (output.bytes.as_slice(), bytes))
    }
    async fn publish(
        mut self: Box<Self>,
        work: Arc<WorkBudget>,
        prepared: Option<PreparedFileUpdate>,
        limits: &TenantLimits,
    ) -> Result<FileUpdateOutput, PortError> {
        match &self.state {
            FileUpdateState::Initial => {
                let unit = self
                    .unit
                    .take()
                    .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
                let prepared = prepared.ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
                self.prepare(unit, prepared, work, limits).await
            }
            FileUpdateState::Prepared(content) => {
                let content = content.clone();
                let output = self
                    .output
                    .take()
                    .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
                work.before_effect();
                self.complete(content, output, work).await.map_err(backend)
            }
            FileUpdateState::Completed(Ok(_)) => self
                .output
                .take()
                .ok_or_else(|| backend(RecoveryError::Conflict.into())),
            FileUpdateState::Completed(Err(_)) => Err(backend(RecoveryError::Conflict.into())),
        }
    }
}
impl UpdatePublication {
    async fn load_saved_output(&mut self, limits: &TenantLimits) -> Result<(), PortError> {
        if matches!(self.state, FileUpdateState::Completed(Err(_))) {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let mut unit = self
            .unit
            .take()
            .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
        let size = unit
            .update_output_bytes(&self.filesystem.run_id, &self.journal.id)
            .await
            .map_err(FilesystemError::from)
            .map_err(backend)?
            .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
        if size > MAX_OUTPUT + 4096 {
            return Err(backend(RecoveryError::Limit.into()));
        }
        let memory = HostBytes::new(limits, size.saturating_mul(6).saturating_add(128 * 1024))
            .map_err(|e| PortError::Execution(e.into()))?;
        let run = self.filesystem.run_id.clone();
        let id = self.journal.id.clone();
        let retained_gate = self.gate.clone();
        let output = finish_owned(async move {
            let _gate = retained_gate;
            let bytes = unit
                .get_update_output(&run, &id)
                .await?
                .ok_or(RecoveryError::Conflict)?;
            if bytes.len() as u64 > MAX_OUTPUT || bytes.len() as u64 > size {
                return conflict();
            }
            Ok(FileUpdateOutput { bytes, memory })
        })
        .await
        .map_err(backend)?;
        self.output = Some(output);
        Ok(())
    }
    async fn prepare(
        mut self,
        mut unit: Box<dyn UnitOfWork>,
        prepared: PreparedFileUpdate,
        work: Arc<WorkBudget>,
        limits: &TenantLimits,
    ) -> Result<FileUpdateOutput, PortError> {
        let Some(contents) = prepared.contents else {
            return Ok(prepared.output);
        };
        let size = prepared.output.bytes.len() as u64;
        if size > MAX_OUTPUT
            || unit
                .total_update_output_bytes(&self.filesystem.run_id)
                .await
                .map_err(FilesystemError::from)
                .map_err(backend)?
                .saturating_add(size)
                > MAX_RUN_OUTPUT
        {
            return Err(PortError::Rejected(
                "file update results exceed durable storage limit".into(),
            ));
        }
        let memory = HostBytes::new(
            limits,
            (contents.capacity() as u64)
                .saturating_add(size.saturating_mul(6))
                .saturating_add(128 * 1024),
        )
        .map_err(|e| PortError::Execution(e.into()))?;
        finish_owned(async move {
            let _memory = memory;
            work.charge(fuel::HASH, (contents.len() as u64).saturating_mul(2))
                .map_err(RecoveryError::from)?;
            work.charge(fuel::IO, contents.len() as u64)
                .map_err(RecoveryError::from)?;
            work.charge(fuel::PARSE, size.saturating_mul(2))
                .map_err(RecoveryError::from)?;
            work.charge(fuel::SYSCALL, 12)
                .map_err(RecoveryError::from)?;
            let content = fingerprint(&self.journal.id, &contents)?;
            let files = self.filesystem.files.clone();
            let id = self.journal.id.clone();
            let target = self.path.clone();
            let captured = content.clone();
            let worker_work = work.clone();
            blocking(self.gate.clone(), move || {
                files.prepare_file_update(&id, &target, &captured, &contents, &worker_work)
            })
            .await
            .map_err(|error| match error {
                FilesystemError::Recovery(RecoveryError::QuotaRefused(error)) => {
                    FilesystemError::Quota(error)
                }
                FilesystemError::Recovery(error) if preflight_refusal(&error) => {
                    FilesystemError::Rejected(error.to_string())
                }
                other => other,
            })?;
            self.journal.state = Some(serialize(&FileUpdateState::Prepared(content.clone()))?);
            unit.save_journal_entry(self.journal.clone()).await?;
            let output = Arc::new(prepared.output.bytes);
            unit.save_update_output(&self.journal.run_id, &self.journal.id, output.clone())
                .await?;
            // From this point a lost commit acknowledgement must resume from the
            // saved identity; no caller may recompute the transformation.
            work.before_effect();
            unit.commit().await?;
            paths::test_checkpoint("update-captured")?;
            let output = output.as_ref().clone();
            let output = FileUpdateOutput {
                bytes: output,
                memory: prepared.output.memory,
            };
            self.complete(content, output, work).await
        })
        .await
        .map_err(backend)
    }
    async fn complete(
        mut self,
        content: Content,
        output: FileUpdateOutput,
        work: Arc<WorkBudget>,
    ) -> Result<FileUpdateOutput, FilesystemError> {
        finish_owned(async move {
            work.charge(fuel::HASH, content.bytes)
                .map_err(RecoveryError::from)?;
            work.charge(fuel::IO, content.bytes)
                .map_err(RecoveryError::from)?;
            work.charge(fuel::SYSCALL, 8).map_err(RecoveryError::from)?;
            let files = self.filesystem.files.clone();
            let id = self.journal.id.clone();
            let captured = content.clone();
            let path = self.path.clone();
            blocking(self.gate.clone(), move || {
                files.publish_file_update(&id, &path, &captured, &work)
            })
            .await?;
            self.journal.state = Some(serialize(&FileUpdateState::Completed(Ok(Outcome {
                bytes: content.bytes,
            })))?);
            self.filesystem
                .operation(None)
                .save_journal_entry(&self.journal)
                .await?;
            paths::test_checkpoint("update-receipt")?;
            drop(self.gate);
            Ok(output)
        })
        .await
    }
}

fn decode_state(journal: &JournalEntry) -> Result<FileUpdateState, FilesystemError> {
    match &journal.state {
        Some(bytes) => deserialize(bytes),
        None => Ok(FileUpdateState::Initial),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interpreter::runtime::filesystem::PreparedFileUpdate;
    use interpreter::runtime::limits::{HostBytes, TenantLimits};

    struct PreparedOnce {
        calls: usize,
        limits: TenantLimits,
    }
    impl PreparedOnce {
        async fn prepare(&mut self) -> Result<PreparedFileUpdate, PortError> {
            self.calls += 1;
            assert_eq!(
                self.calls, 1,
                "a captured transformation must never run twice"
            );
            Ok(PreparedFileUpdate {
                contents: Some(b"inserted\noriginal\n".to_vec()),
                output: FileUpdateOutput {
                    bytes: b"exact native output".to_vec(),
                    memory: HostBytes::new(&self.limits, 64)
                        .map_err(|e| PortError::Execution(e.into()))?,
                },
            })
        }
        fn authorize(&mut self, output: &[u8], bytes: u64) -> Result<(), PortError> {
            assert_eq!(output, b"exact native output");
            assert_eq!(bytes, 18);
            Ok(())
        }
    }

    #[tokio::test]
    async fn lost_capture_acknowledgement_replays_prepared_bytes_and_native_output() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file"), b"original\n").unwrap();
        let (units, store) = crate::unit_of_work::tests::filesystem_units();
        let run = uuid::Uuid::new_v4().to_string().into();
        let filesystem =
            DurableFilesystem::open(Vfs::external(root.path().to_owned()).unwrap(), &run, units)
                .await
                .unwrap();
        let effect = uuid::Uuid::new_v4().to_string().into();
        let mut update = PreparedOnce {
            calls: 0,
            limits: TenantLimits::new(8 << 20),
        };
        store.lock().unwrap().stop_after_prepared = true;
        let pending = filesystem
            .begin_file_update(&effect, "/file", [1; 32], &update.limits)
            .await
            .unwrap();
        assert!(pending.saved_output().is_none());
        let prepared = update.prepare().await.unwrap();
        assert!(
            pending
                .publish(
                    Arc::new(WorkBudget::new(u64::MAX)),
                    Some(prepared),
                    &update.limits
                )
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join("file")).unwrap(),
            b"original\n"
        );
        let pending = filesystem
            .begin_file_update(&effect, "/file", [1; 32], &update.limits)
            .await
            .unwrap();
        let (output, bytes) = pending.saved_output().unwrap();
        update.authorize(output, bytes).unwrap();
        let output = pending
            .publish(Arc::new(WorkBudget::new(u64::MAX)), None, &update.limits)
            .await
            .unwrap();
        assert_eq!(output.bytes, b"exact native output");
        assert_eq!(
            std::fs::read(root.path().join("file")).unwrap(),
            b"inserted\noriginal\n"
        );
        drop(output);
        assert_eq!(update.calls, 1);
        assert_eq!(update.limits.host_attached_bytes(), 0);
    }
}
