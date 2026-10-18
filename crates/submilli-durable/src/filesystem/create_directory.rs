use super::operation::FilesystemOperation;
use super::require_empty;
use super::{
    CaptureOptions, FilesystemError, Target, blocking, decode_checkpoint, decode_outcome,
    preflight_refusal, serialize, test_checkpoint, validate_id,
};
use crate::io::{self as paths, RecoveryError, validate_guest_path};
use crate::io::{existing, identity, sync};
use crate::unit_of_work::{JournalEntry, Outcome, RecordedOutcome};
use interpreter::runtime::durable::EffectId;
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
enum CreateDirectoryState {
    Initial,
    Prepared(Target),
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn create_directory(
        &self,
        effect: &EffectId,
        path: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(path)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = self
            .create_initial_or_get_last_journal_entry(unit.as_mut(), effect, "mkdir", &path)
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, CreateDirectoryState::Completed(_)) {
            self.charge_preparation(path, 0)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                CreateDirectoryState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let (path, effect) = (path.to_owned(), effect.to_owned());
                    let result = blocking(gate.clone(), move || {
                        let target = files.capture_target(
                            &effect,
                            &path,
                            CaptureOptions {
                                allow_directory: true,
                                ..Default::default()
                            },
                            work.as_deref(),
                        )?;
                        if target.original.is_some() && !target.original_directory {
                            return Err(
                                std::io::Error::from(std::io::ErrorKind::AlreadyExists).into()
                            );
                        }
                        paths::test_checkpoint("prepared")?;
                        Ok(target)
                    })
                    .await;
                    match result {
                        Ok(checkpoint) => CreateDirectoryState::Prepared(checkpoint),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            CreateDirectoryState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                CreateDirectoryState::Prepared(checkpoint) => {
                    checkpoint.validate()?;
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    blocking(gate.clone(), move || {
                        {
                            let target = &checkpoint;
                            let parent = files.parent(&target.source)?;
                            match existing(&parent, &target.source.name)? {
                                None if target.original.is_none() => {
                                    parent.create_dir(&target.source.name)?;
                                    paths::test_checkpoint("directory-created")?;
                                }
                                Some(metadata)
                                    if metadata.is_dir() && !metadata.file_type().is_symlink() =>
                                {
                                    if let Some(original) = target.original {
                                        if identity(&metadata)? != original {
                                            return Err(RecoveryError::Conflict);
                                        }
                                    } else {
                                        // No later mutation is admitted before this receipt. A new
                                        // pending mkdir may only have produced an empty directory.
                                        require_empty(&parent.open_dir(&target.source.name)?)?;
                                    }
                                }
                                _ => return Err(RecoveryError::Conflict),
                            }
                            sync(&parent.open_dir(&target.source.name)?)?;
                            sync(&parent)?;
                        }
                        files.reconcile_quotas(work.as_deref())?;
                        paths::test_checkpoint("before-receipt")
                    })
                    .await?;

                    test_checkpoint("effect-applied");
                    CreateDirectoryState::Completed(Ok(Outcome { bytes: 0 }))
                }
                CreateDirectoryState::Completed(result) => {
                    return result.map_err(FilesystemError::Rejected);
                }
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                CreateDirectoryState::Prepared(_) => test_checkpoint("effect-intent"),
                CreateDirectoryState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<CreateDirectoryState, FilesystemError> {
    if let Some(result) = decode_outcome(journal_entry, config)? {
        return Ok(CreateDirectoryState::Completed(result));
    }
    match &journal_entry.recovery_payload {
        Some(bytes) => Ok(CreateDirectoryState::Prepared(decode_checkpoint(bytes)?)),
        None => Ok(CreateDirectoryState::Initial),
    }
}

// The operation owns this mapping; SQL keeps its existing immutable intent and receipt columns.
fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &CreateDirectoryState,
) -> Result<(), FilesystemError> {
    match state {
        CreateDirectoryState::Initial => {}
        CreateDirectoryState::Prepared(checkpoint) => {
            journal_entry.recovery_payload = Some(serialize(checkpoint)?);
        }
        CreateDirectoryState::Completed(result) => {
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
