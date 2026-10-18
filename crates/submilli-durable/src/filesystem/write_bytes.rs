use super::operation::FilesystemOperation;
use super::{
    CaptureOptions, FilesystemError, WriteCheckpoint, blocking, decode_checkpoint, decode_outcome,
    fingerprint, preflight_refusal, serialize, test_checkpoint,
};
use crate::io::{self as paths, RecoveryError, validate_guest_path};
use crate::unit_of_work::{JournalEntry, Outcome, RecordedOutcome};
use interpreter::runtime::durable::EffectId;
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
enum WriteState {
    Initial,
    Prepared(WriteCheckpoint),
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn write_bytes(
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
                "write",
                &(path, &input_fingerprint),
            )
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, WriteState::Completed(_)) {
            self.charge_preparation(path, input_fingerprint.bytes)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                WriteState::Initial => {
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
                                    CaptureOptions::default(),
                                    work.as_deref(),
                                )?;
                                let artifacts = files.artifacts(&target.source, &effect, true)?;
                                let candidate = Some(files.prepare_candidate(
                                    &target.source,
                                    &artifacts,
                                    &input_fingerprint,
                                    &mut input,
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
                        Ok(checkpoint) => WriteState::Prepared(checkpoint),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            WriteState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                WriteState::Prepared(checkpoint) => {
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
                    let bytes = bytes.clone();
                    blocking(gate.clone(), move || {
                        let mut input = std::io::Cursor::new(bytes.as_slice());
                        let artifacts =
                            files.artifacts(&checkpoint.target.source, &effect, false)?;
                        let result = files.publish_candidate(
                            &checkpoint,
                            &artifacts,
                            &input_fingerprint,
                            &mut input,
                            work.as_deref(),
                        );
                        files.reconcile_quotas(work.as_deref())?;
                        result?;
                        paths::test_checkpoint("before-receipt")?;
                        Ok(())
                    })
                    .await?;
                    test_checkpoint("effect-applied");
                    WriteState::Completed(Ok(Outcome {
                        bytes: written_bytes,
                    }))
                }
                WriteState::Completed(result) => return result.map_err(FilesystemError::Rejected),
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                WriteState::Prepared(_) => test_checkpoint("effect-intent"),
                WriteState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<WriteState, FilesystemError> {
    if let Some(result) = decode_outcome(journal_entry, config)? {
        return Ok(WriteState::Completed(result));
    }
    match &journal_entry.recovery_payload {
        Some(bytes) => Ok(WriteState::Prepared(decode_checkpoint(bytes)?)),
        None => Ok(WriteState::Initial),
    }
}

// The operation owns this mapping; SQL keeps its existing immutable intent and receipt columns.
fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &WriteState,
) -> Result<(), FilesystemError> {
    match state {
        WriteState::Initial => {}
        WriteState::Prepared(checkpoint) => {
            journal_entry.recovery_payload = Some(serialize(checkpoint)?);
        }
        WriteState::Completed(result) => {
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
