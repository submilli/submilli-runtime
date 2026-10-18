use super::operation::FilesystemOperation;
use super::{
    CaptureOptions, FilesystemError, WriteCheckpoint, blocking, decode_checkpoint, decode_outcome,
    fingerprint, preflight_refusal, serialize, test_checkpoint,
};
use super::{copy_exact, hash, require_pin};
use crate::io::{self as paths, RecoveryError, validate_guest_path};
use crate::io::{current, sync};
use crate::unit_of_work::{JournalEntry, Outcome, RecordedOutcome};
use cap_std::fs::OpenOptions;
use interpreter::runtime::durable::EffectId;
use std::io::{Seek, SeekFrom};
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
enum AppendState {
    Initial,
    Prepared(WriteCheckpoint),
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn append_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Outcome, FilesystemError> {
        validate_guest_path(path)?;
        let input_fingerprint = fingerprint(effect, &bytes)?;
        let bytes = Arc::new(bytes);
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = self
            .create_initial_or_get_last_journal_entry(
                unit.as_mut(),
                effect,
                "append",
                &(path, &input_fingerprint),
            )
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, AppendState::Completed(_)) {
            self.charge_preparation(path, input_fingerprint.bytes)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                AppendState::Initial => {
                    let prepared = {
                        let bytes = bytes.clone();
                        let input_fingerprint = input_fingerprint.clone();
                        let files = self.filesystem.files.clone();
                        let work = self.work.clone();
                        let (effect, path) = (effect.clone(), path.to_owned());
                        blocking(gate.clone(), move || {
                            let mut input = std::io::Cursor::new(bytes.as_slice());
                            let prepared = (|| {
                                let target = files.capture_target(
                                    &effect,
                                    &path,
                                    CaptureOptions {
                                        follow_final: true,
                                        ..Default::default()
                                    },
                                    work.as_deref(),
                                )?;
                                let artifacts = files.artifacts(&target.source, &effect, true)?;
                                let candidate = if target.original.is_some() {
                                    target
                                        .original_bytes
                                        .checked_add(input_fingerprint.bytes)
                                        .filter(|n| *n <= i64::MAX as u64)
                                        .ok_or(RecoveryError::Limit)?;
                                    files
                                        .volume(&target.source.mount)?
                                        .check_growth(input_fingerprint.bytes, work.as_deref())?;
                                    None
                                } else {
                                    Some(files.prepare_candidate(
                                        &target.source,
                                        &artifacts,
                                        &input_fingerprint,
                                        &mut input,
                                        work.as_deref(),
                                    )?)
                                };
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
                        Ok(checkpoint) => AppendState::Prepared(checkpoint),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            AppendState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                AppendState::Prepared(checkpoint) => {
                    let written_bytes = input_fingerprint.bytes;
                    let input_fingerprint = input_fingerprint.clone();
                    checkpoint.target.validate()?;
                    if checkpoint.target.original_directory
                        || (checkpoint.target.original.is_none() && checkpoint.candidate.is_none())
                    {
                        return Err(RecoveryError::InvalidRecord.into());
                    }
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let effect = effect.clone();
                    let bytes = bytes.clone();
                    blocking(gate.clone(), move || {
                        let mut input = std::io::Cursor::new(bytes.as_slice());
                        let artifacts =
                            files.artifacts(&checkpoint.target.source, &effect, false)?;
                        let result = if checkpoint.target.original.is_some() {
                            (|| -> Result<_, RecoveryError> {
                                let target = &checkpoint.target;
                                let artifacts = &artifacts;
                                let payload = &input_fingerprint;
                                let content = &mut input;
                                let work = work.as_deref();
                                let original =
                                    target.original.ok_or(RecoveryError::InvalidRecord)?;
                                require_pin(artifacts, "original", original)?;
                                let parent = files.parent(&target.source)?;
                                if current(&parent, &target.source.name)? != Some(original) {
                                    return Err(RecoveryError::Conflict);
                                }
                                let final_length = target
                                    .original_bytes
                                    .checked_add(payload.bytes)
                                    .filter(|bytes| *bytes <= i64::MAX as u64)
                                    .ok_or(RecoveryError::Limit)?;
                                // Deliberately omit O_APPEND: every retry repairs the original interval.
                                let mut file = artifacts.open_with(
                                    "original",
                                    OpenOptions::new().read(true).write(true),
                                )?;
                                let length = file.metadata()?.len();
                                if length < target.original_bytes
                                    || length > final_length
                                    || Some(hash(&mut file, target.original_bytes)?)
                                        != target.original_sha256
                                {
                                    return Err(RecoveryError::Conflict);
                                }
                                files
                                    .volume(&target.source.mount)?
                                    .check_growth(final_length - length, work)?;
                                file.seek(SeekFrom::Start(target.original_bytes))?;
                                copy_exact(content, &mut file, payload)?;
                                file.set_len(final_length)?;
                                paths::test_checkpoint("append-written")?;
                                file.sync_all()?;
                                paths::test_checkpoint("append-sync")?;
                                sync(&parent)?;
                                Ok(())
                            })()
                        } else {
                            files.publish_candidate(
                                &checkpoint,
                                &artifacts,
                                &input_fingerprint,
                                &mut input,
                                work.as_deref(),
                            )
                        };
                        files.reconcile_quotas(work.as_deref())?;
                        result?;
                        paths::test_checkpoint("before-receipt")?;
                        Ok(())
                    })
                    .await?;
                    test_checkpoint("effect-applied");
                    AppendState::Completed(Ok(Outcome {
                        bytes: written_bytes,
                    }))
                }
                AppendState::Completed(result) => return result.map_err(FilesystemError::Rejected),
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                AppendState::Prepared(_) => test_checkpoint("effect-intent"),
                AppendState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<AppendState, FilesystemError> {
    if let Some(result) = decode_outcome(journal_entry, config)? {
        return Ok(AppendState::Completed(result));
    }
    match &journal_entry.recovery_payload {
        Some(bytes) => Ok(AppendState::Prepared(decode_checkpoint(bytes)?)),
        None => Ok(AppendState::Initial),
    }
}

// The operation owns this mapping; SQL keeps its existing immutable intent and receipt columns.
fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &AppendState,
) -> Result<(), FilesystemError> {
    match state {
        AppendState::Initial => {}
        AppendState::Prepared(checkpoint) => {
            journal_entry.recovery_payload = Some(serialize(checkpoint)?);
        }
        AppendState::Completed(result) => {
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
