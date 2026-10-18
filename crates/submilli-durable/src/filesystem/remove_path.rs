use super::operation::FilesystemOperation;
use super::{
    CaptureOptions, FilesystemError, Target, blocking, decode_checkpoint, decode_outcome,
    preflight_refusal, serialize, test_checkpoint, validate_id,
};
use super::{require_empty, require_pin};
use crate::io::{self as paths, RecoveryError, validate_guest_path};
use crate::io::{current, existing, identity, sync};
use crate::unit_of_work::{JournalEntry, Outcome, RecordedOutcome};
use interpreter::runtime::durable::EffectId;
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
enum RemoveState {
    Initial,
    Prepared(Target),
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn remove_path(
        &self,
        effect: &EffectId,
        path: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(path)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = self
            .create_initial_or_get_last_journal_entry(unit.as_mut(), effect, "remove", &path)
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, RemoveState::Completed(_)) {
            self.charge_preparation(path, 0)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                RemoveState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let (path, effect) = (path.to_owned(), effect.to_owned());
                    let result = blocking(gate.clone(), move || {
                        let target = files.capture_target(
                            &effect,
                            &path,
                            CaptureOptions {
                                allow_directory: true,
                                require_empty: true,
                                ..Default::default()
                            },
                            work.as_deref(),
                        )?;
                        if target.original.is_none() {
                            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
                        }
                        paths::test_checkpoint("prepared")?;
                        Ok(target)
                    })
                    .await;
                    match result {
                        Ok(checkpoint) => RemoveState::Prepared(checkpoint),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            RemoveState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                RemoveState::Prepared(checkpoint) => {
                    checkpoint.validate()?;
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let effect = effect.to_owned();
                    blocking(gate.clone(), move || {
                        if checkpoint.original_directory {
                            {
                                let target = &checkpoint;
                                let parent = files.parent(&target.source)?;
                                if let Some(metadata) = existing(&parent, &target.source.name)? {
                                    if Some(identity(&metadata)?) != target.original
                                        || !metadata.is_dir()
                                        || metadata.file_type().is_symlink()
                                    {
                                        return Err(RecoveryError::Conflict);
                                    }
                                    require_empty(&parent.open_dir(&target.source.name)?)?;
                                    parent.remove_dir(&target.source.name)?;
                                    paths::test_checkpoint("directory-removed")?;
                                }
                                sync(&parent)?;
                            }
                        } else {
                            let artifacts = files.artifacts(&checkpoint.source, &effect, false)?;
                            {
                                let target = &checkpoint;
                                let artifacts = &artifacts;
                                let original =
                                    target.original.ok_or(RecoveryError::InvalidRecord)?;
                                require_pin(artifacts, "original", original)?;
                                let parent = files.parent(&target.source)?;
                                match current(&parent, &target.source.name)? {
                                    None => {}
                                    Some(current) if current == original => {
                                        parent.remove_file(&target.source.name)?;
                                        paths::test_checkpoint("source-removed")?;
                                    }
                                    Some(_) => return Err(RecoveryError::Conflict),
                                }
                                sync(&parent)?;
                            }
                        }
                        files.reconcile_quotas(work.as_deref())?;
                        paths::test_checkpoint("before-receipt")
                    })
                    .await?;

                    test_checkpoint("effect-applied");
                    RemoveState::Completed(Ok(Outcome { bytes: 0 }))
                }
                RemoveState::Completed(result) => return result.map_err(FilesystemError::Rejected),
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                RemoveState::Prepared(_) => test_checkpoint("effect-intent"),
                RemoveState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<RemoveState, FilesystemError> {
    if let Some(result) = decode_outcome(journal_entry, config)? {
        return Ok(RemoveState::Completed(result));
    }
    match &journal_entry.recovery_payload {
        Some(bytes) => Ok(RemoveState::Prepared(decode_checkpoint(bytes)?)),
        None => Ok(RemoveState::Initial),
    }
}

// The operation owns this mapping; SQL keeps its existing immutable intent and receipt columns.
fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &RemoveState,
) -> Result<(), FilesystemError> {
    match state {
        RemoveState::Initial => {}
        RemoveState::Prepared(checkpoint) => {
            journal_entry.recovery_payload = Some(serialize(checkpoint)?);
        }
        RemoveState::Completed(result) => {
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
