use super::operation::FilesystemOperation;
use super::{
    CaptureOptions, FilesystemError, WriteCheckpoint, blocking, conflict, decode_checkpoint,
    decode_outcome, fingerprint, preflight_refusal, require_capacity, require_ready, serialize,
    test_checkpoint, validate_id,
};
use super::{require_idle, require_unused_operation};
use crate::io::{self as paths, RecoveryError, validate_guest_path};
use crate::unit_of_work::*;
use cap_std::fs::OpenOptions;
use interpreter::runtime::durable::EffectId;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

enum CloseWriterState {
    Initial,
    Prepared(WriteCheckpoint),
    Completed(Result<Outcome, String>),
}
impl FilesystemOperation {
    /// Publish the acknowledged writer output. Completed retries never reopen staged bytes.
    pub async fn close_writer(
        &self,
        resource: &EffectId,
        effect: &EffectId,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(resource)?;
        validate_id(effect)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut writer = unit
            .get_writer(&self.filesystem.run_id, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.filesystem_config_fingerprint != self.filesystem.filesystem_config_fingerprint
            || writer
                .close_effect
                .as_ref()
                .is_some_and(|prior| prior != effect)
        {
            return conflict();
        }
        let chunks = unit
            .writer_chunks(&self.filesystem.run_id, resource, MAX_OPERATIONS as u32 + 1)
            .await?;
        if chunks.len() > MAX_OPERATIONS as usize {
            return conflict();
        }
        let bytes = chunks
            .iter()
            .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes))
            .filter(|bytes| *bytes <= i64::MAX as u64)
            .ok_or(StoreError::Conflict)?;
        if writer.close_effect.is_none() {
            require_idle(unit.as_mut(), &self.filesystem.run_id).await?;
            require_capacity(unit.as_mut(), &self.filesystem.run_id).await?;
            require_unused_operation(unit.as_mut(), &self.filesystem.run_id, effect).await?;
        }
        // Publication may have committed before the writer's closed flag.
        let existing = unit
            .get_journal_entry(&self.filesystem.run_id, effect)
            .await?;
        drop(unit);
        if let Some(journal_entry) = existing.as_ref() {
            if journal_entry.operation != "write" {
                return conflict();
            }
            if let CloseWriterState::Completed(result) =
                decode_state(journal_entry, self.filesystem.filesystem_config_fingerprint)?
            {
                mark_writer_closed(self, resource).await?;
                return result.map_err(FilesystemError::Rejected);
            }
        }
        self.charge(interpreter::runtime::fuel::ELEM, chunks.len() as u64)?;
        self.charge(interpreter::runtime::fuel::HASH, bytes.saturating_mul(2))?;
        self.charge(interpreter::runtime::fuel::IO, bytes)?;
        let (input_file, input_fingerprint) = if chunks.is_empty() {
            // Empty output needs no staging volume, even on a read-only target.
            (None, fingerprint(resource, &[])?)
        } else {
            let files = self.filesystem.files.clone();
            let work = self.work.clone();
            let resource = resource.clone();
            let path = writer.path.clone();
            let chunks: Vec<_> = chunks.iter().map(|chunk| chunk.content.clone()).collect();
            let (file, content) = blocking(gate.clone(), move || {
                let resource = &resource;
                let path = path.as_str();
                let chunks = &chunks;
                let work = work.as_deref();

                let (directory, _) = files.writer_directory(resource, path, true)?;
                let mut file = directory.open_with(
                    "output",
                    OpenOptions::new().read(true).write(true).create(true),
                )?;
                let mut whole = Sha256::new();
                let mut bytes = 0u64;
                let mut buffer = [0; 64 * 1024];
                for chunk in chunks {
                    let mut remaining = chunk.bytes;
                    let mut hash = Sha256::new();
                    while remaining != 0 {
                        let count = usize::try_from(remaining.min(buffer.len() as u64))
                            .map_err(|_| RecoveryError::Limit)?;
                        file.read_exact(&mut buffer[..count])?;
                        hash.update(&buffer[..count]);
                        whole.update(&buffer[..count]);
                        remaining -= count as u64;
                    }
                    if <[u8; 32]>::from(hash.finalize()) != chunk.sha256 {
                        return Err(RecoveryError::Conflict);
                    }
                    bytes = bytes.checked_add(chunk.bytes).ok_or(RecoveryError::Limit)?;
                }
                // Discard only the unacknowledged tail from an interrupted chunk.
                file.set_len(bytes)?;
                file.sync_all()?;
                crate::io::sync(&directory)?;
                files.reconcile_quotas(work)?;
                let content = Content {
                    id: resource.clone(),
                    bytes,
                    sha256: whole.finalize().into(),
                };
                file.seek(SeekFrom::Start(0))?;
                Ok((file, content))
            })
            .await?;
            (Some(Arc::new(file)), content)
        };
        // Reserve this close ID before admitting its mutation journal entry.
        // A crash between these commits must still exclude every other writer.
        if writer.close_effect.is_none() {
            writer.close_effect = Some(effect.clone());
            writer.state = "closing".into();
            let mut unit = (self.filesystem.units)().await?;
            unit.save_writer(&self.filesystem.run_id, writer.clone())
                .await?;
            unit.commit().await?;
        }
        validate_guest_path(&writer.path)?;
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = create_initial_or_get_last_journal_entry(
            self,
            unit.as_mut(),
            effect,
            &writer,
            &input_fingerprint,
        )
        .await?;
        unit.commit().await?;
        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, CloseWriterState::Completed(_)) {
            self.charge_preparation(&writer.path, input_fingerprint.bytes)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                CloseWriterState::Initial => {
                    let prepared = {
                        let input_file = input_file.clone();
                        let input_fingerprint = input_fingerprint.clone();
                        let files = self.filesystem.files.clone();
                        let work = self.work.clone();
                        let (effect, path) = (effect.clone(), writer.path.clone());
                        blocking(gate.clone(), move || {
                            let mut file = input_file
                                .as_ref()
                                .map(|file| file.try_clone())
                                .transpose()?;
                            let mut empty = std::io::Cursor::new([]);
                            let input: &mut dyn Read = if let Some(file) = file.as_mut() {
                                file.seek(SeekFrom::Start(0))?;
                                file
                            } else {
                                &mut empty
                            };

                            let prepared = (|| {
                                let target = files.capture_target(
                                    &effect,
                                    &path,
                                    CaptureOptions::default(),
                                    work.as_deref(),
                                )?;
                                let artifacts = files.artifacts(&target.source, &effect, true)?;
                                let candidate = Some(files.prepare_candidate(
                                    &target.source,
                                    &artifacts,
                                    &input_fingerprint,
                                    input,
                                    work.as_deref(),
                                )?);
                                paths::sync(&artifacts)?;
                                paths::test_checkpoint("prepared")?;
                                Ok(WriteCheckpoint { target, candidate })
                            })();
                            files.reconcile_quotas(work.as_deref())?;
                            prepared
                        })
                        .await
                    };
                    match prepared {
                        Ok(checkpoint) => CloseWriterState::Prepared(checkpoint),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            CloseWriterState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                CloseWriterState::Prepared(checkpoint) => {
                    let written_bytes = input_fingerprint.bytes;
                    let input_fingerprint = input_fingerprint.clone();
                    checkpoint.target.validate()?;
                    if checkpoint.target.original_directory || checkpoint.candidate.is_none() {
                        return Err(RecoveryError::InvalidRecord.into());
                    }
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let effect = effect.clone();
                    let input_file = input_file.clone();
                    blocking(gate.clone(), move || {
                        let mut file = input_file
                            .as_ref()
                            .map(|file| file.try_clone())
                            .transpose()?;
                        let mut empty = std::io::Cursor::new([]);
                        let input: &mut dyn Read = if let Some(file) = file.as_mut() {
                            file.seek(SeekFrom::Start(0))?;
                            file
                        } else {
                            &mut empty
                        };

                        let artifacts =
                            files.artifacts(&checkpoint.target.source, &effect, false)?;
                        let result = files.publish_candidate(
                            &checkpoint,
                            &artifacts,
                            &input_fingerprint,
                            input,
                            work.as_deref(),
                        );
                        files.reconcile_quotas(work.as_deref())?;
                        result?;
                        paths::test_checkpoint("before-receipt")?;
                        Ok(())
                    })
                    .await?;
                    test_checkpoint("effect-applied");
                    CloseWriterState::Completed(Ok(Outcome {
                        bytes: written_bytes,
                    }))
                }
                CloseWriterState::Completed(result) => {
                    mark_writer_closed(self, resource).await?;
                    test_checkpoint("writer-closed");
                    return result.map_err(FilesystemError::Rejected);
                }
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                CloseWriterState::Prepared(_) => test_checkpoint("effect-intent"),
                CloseWriterState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<CloseWriterState, FilesystemError> {
    if let Some(result) = decode_outcome(journal_entry, config)? {
        return Ok(CloseWriterState::Completed(result));
    }
    match &journal_entry.recovery_payload {
        Some(bytes) => Ok(CloseWriterState::Prepared(decode_checkpoint(bytes)?)),
        None => Ok(CloseWriterState::Initial),
    }
}

// The operation owns this mapping; SQL keeps its existing immutable intent and receipt columns.
fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &CloseWriterState,
) -> Result<(), FilesystemError> {
    match state {
        CloseWriterState::Initial => {}
        CloseWriterState::Prepared(checkpoint) => {
            journal_entry.recovery_payload = Some(serialize(checkpoint)?);
        }
        CloseWriterState::Completed(result) => {
            let outcome = match result {
                Ok(outcome) => RecordedOutcome::Completed(outcome.clone()),
                Err(reason) => RecordedOutcome::Rejected(reason.clone()),
            };
            journal_entry.rejected = result.is_err();
            journal_entry.outcome = Some(serialize(&outcome)?);
        }
    }
    Ok(())
}

async fn create_initial_or_get_last_journal_entry(
    operation: &FilesystemOperation,
    unit: &mut dyn UnitOfWork,
    effect: &EffectId,
    writer: &Writer,
    input_fingerprint: &Content,
) -> Result<JournalEntry, FilesystemError> {
    let mut fingerprint = operation.filesystem.filesystem_config_fingerprint.to_vec();
    fingerprint.extend_from_slice(&Sha256::digest(serialize(&(
        &writer.path,
        input_fingerprint,
    ))?));
    if let Some(journal_entry) = unit
        .get_journal_entry(&operation.filesystem.run_id, effect)
        .await?
    {
        if journal_entry.operation != "write"
            || journal_entry.version != 2
            || journal_entry.request != fingerprint
        {
            return conflict();
        }
        return Ok(journal_entry);
    }
    require_ready(
        unit,
        &operation.filesystem.run_id,
        Some((&writer.id, effect)),
    )
    .await?;
    require_capacity(unit, &operation.filesystem.run_id).await?;
    if unit
        .pending_journal_entry(&operation.filesystem.run_id)
        .await?
        .is_some()
    {
        return conflict();
    }
    if let Some(owner) = unit
        .writer_for_effect(&operation.filesystem.run_id, effect)
        .await?
        && writer.id != owner.id
    {
        return conflict();
    }
    let journal_entry = JournalEntry {
        id: effect.clone(),
        run_id: operation.filesystem.run_id.clone(),
        operation: "write".into(),
        version: 2,
        request: fingerprint,
        recovery_payload: None,
        outcome: None,
        rejected: false,
    };
    unit.save_journal_entry(journal_entry.clone()).await?;
    Ok(journal_entry)
}

async fn mark_writer_closed(
    operation: &FilesystemOperation,
    resource: &EffectId,
) -> Result<(), FilesystemError> {
    let run_id = &operation.filesystem.run_id;
    let filesystem_config_fingerprint = operation.filesystem.filesystem_config_fingerprint;
    let mut owned = (operation.filesystem.units)().await?;
    let unit = owned.as_mut();
    let mut writer = unit
        .get_writer(run_id, resource)
        .await?
        .ok_or(StoreError::Conflict)?;
    if writer.filesystem_config_fingerprint != filesystem_config_fingerprint {
        return conflict();
    }
    writer.state = "closed".into();
    unit.save_writer(run_id, writer).await?;
    owned.commit().await?;
    Ok(())
}
