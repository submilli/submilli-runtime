use super::operation::FilesystemOperation;
use super::{
    FilesystemError, blocking, deserialize, preflight_refusal, require_empty, serialize,
    test_checkpoint, validate_id, validate_journal_configuration,
};
use crate::io::{self as paths, Files, RecoveryError, existing, sync, validate_guest_path};
use crate::unit_of_work::{JournalEntry, Outcome};
use interpreter::runtime::durable::EffectId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum RemoveState {
    #[serde(skip)]
    Initial,
    Prepared {},
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
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                RemoveState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let path = path.to_owned();
                    let result = blocking(gate.clone(), move || {
                        let entry = files.entry(&path, false)?;
                        let parent = files.parent(&entry)?;
                        let metadata = existing(&parent, &entry.name)?
                            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                        let directory = metadata.is_dir() && !metadata.file_type().is_symlink();
                        if directory {
                            require_empty(&parent.open_dir(&entry.name)?)?;
                        } else {
                            files.require_regular(&entry, &metadata, work.as_deref())?;
                        }

                        paths::test_checkpoint("prepared")?;
                        Ok(())
                    })
                    .await;
                    match result {
                        Ok(()) => RemoveState::Prepared {},
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            RemoveState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                RemoveState::Prepared {} => {
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let path = path.to_owned();
                    blocking(gate.clone(), move || files.remove_path(&path)).await?;

                    test_checkpoint("effect-applied");
                    RemoveState::Completed(Ok(Outcome { bytes: 0 }))
                }
                RemoveState::Completed(result) => return result.map_err(FilesystemError::Rejected),
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                RemoveState::Prepared {} => test_checkpoint("effect-intent"),
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
    validate_journal_configuration(journal_entry, config)?;
    match &journal_entry.state {
        Some(bytes) => deserialize(bytes),
        None => Ok(RemoveState::Initial),
    }
}

fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &RemoveState,
) -> Result<(), FilesystemError> {
    journal_entry.state = match state {
        RemoveState::Initial => None,
        _ => Some(serialize(state)?),
    };
    Ok(())
}

impl Files {
    fn remove_path(&self, path: &str) -> Result<(), RecoveryError> {
        let entry = self.entry(path, false)?;
        let volume = self.volume(&entry.mount)?;
        let parent = self.parent(&entry)?;
        if let Some(metadata) = existing(&parent, &entry.name)? {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                require_empty(&parent.open_dir(&entry.name)?)?;
                parent.remove_dir(&entry.name)?;
                paths::test_checkpoint("directory-removed")?;
            } else {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(RecoveryError::Conflict);
                }
                parent.remove_file(&entry.name)?;
                crate::test_checkpoint("remove-unlinked-unaccounted");
                volume.record_size_change(metadata.len(), 0);
                paths::test_checkpoint("source-removed")?;
            }
        }
        // Prepared was saved while the path existed. Absence on retry means
        // this same removal already happened; no later mutation is admitted.
        sync(&parent)?;
        paths::test_checkpoint("before-receipt")
    }
}
