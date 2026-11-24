use super::operation::FilesystemOperation;
use super::{
    FilesystemError, blocking, deserialize, preflight_refusal, serialize, test_checkpoint,
    validate_id, validate_journal_configuration,
};
use crate::io::{self as paths, Files, RecoveryError, existing, sync, validate_guest_path};
use crate::unit_of_work::{JournalEntry, Outcome};
use interpreter::runtime::durable::EffectId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum CreateDirectoryState {
    #[serde(skip)]
    Initial,
    Prepared {
        // Older single-directory journal entries have no missing-components field.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        missing: Option<usize>,
    },
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn create_directory(
        &self,
        effect: &EffectId,
        path: &str,
    ) -> Result<Outcome, FilesystemError> {
        self.mkdir(effect, path, false).await
    }

    pub(super) async fn mkdir(
        &self,
        effect: &EffectId,
        path: &str,
        recursive: bool,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        if recursive {
            paths::validate_guest_path_at(self.filesystem.files.vfs().cwd(), path)?;
        } else {
            validate_guest_path(path)?;
        }
        let kind = if recursive {
            "mkdir-recursive"
        } else {
            "mkdir"
        };
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = self
            .create_initial_or_get_last_journal_entry(unit.as_mut(), effect, kind, &path)
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        // A resumed intent may already have changed the filesystem. Account for
        // this attempt against its fresh budget without refusing the remaining work.
        if let CreateDirectoryState::Prepared {
            missing: Some(count),
        } = &state
            && let Some(work) = &self.work
        {
            work.before_effect();
            work.record_fuel(directory_tree_cost(*count));
        }
        if !matches!(state, CreateDirectoryState::Completed(_)) {
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                CreateDirectoryState::Initial => {
                    let files = self.filesystem.files.clone();
                    let path = path.to_owned();
                    let work = self.work.clone();
                    let result = blocking(gate.clone(), move || {
                        let missing = if recursive {
                            Some(files.prepare_directory_tree(&path, work.as_deref())?)
                        } else {
                            files.prepare_directory(&path)?;
                            None
                        };
                        paths::test_checkpoint("prepared")?;
                        Ok(missing)
                    })
                    .await;
                    match result {
                        Ok(missing) => {
                            if let Some(count) = missing
                                && let Some(work) = &self.work
                            {
                                work.charge_fuel(directory_tree_cost(count))
                                    .map_err(RecoveryError::from)?;
                            }
                            CreateDirectoryState::Prepared { missing }
                        }
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            CreateDirectoryState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                CreateDirectoryState::Prepared { missing } => {
                    if missing.is_some() != recursive {
                        return Err(RecoveryError::InvalidRecord.into());
                    }
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let path = path.to_owned();
                    blocking(gate.clone(), move || match missing {
                        Some(missing) => files.create_directory_tree(&path, missing),
                        None => files.create_directory(&path),
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
                CreateDirectoryState::Prepared { .. } => test_checkpoint("effect-intent"),
                CreateDirectoryState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn directory_tree_cost(count: usize) -> u64 {
    // Create, open and sync each child, then sync its parent.
    interpreter::runtime::fuel::SYSCALL.cost((count as u64).saturating_mul(4))
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<CreateDirectoryState, FilesystemError> {
    validate_journal_configuration(journal_entry, config)?;
    match &journal_entry.state {
        Some(bytes) => deserialize(bytes),
        None => Ok(CreateDirectoryState::Initial),
    }
}

fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &CreateDirectoryState,
) -> Result<(), FilesystemError> {
    journal_entry.state = match state {
        CreateDirectoryState::Initial => None,
        _ => Some(serialize(state)?),
    };
    Ok(())
}

impl Files {
    fn prepare_directory(&self, path: &str) -> Result<(), RecoveryError> {
        let entry = self.entry(path, false)?;
        let parent = self.parent(&entry)?;
        if let Some(metadata) = existing(&parent, &entry.name)?
            && (!metadata.is_dir() || metadata.file_type().is_symlink())
        {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
        }

        Ok(())
    }

    fn create_directory(&self, path: &str) -> Result<(), RecoveryError> {
        let entry = self.entry(path, false)?;
        let parent = self.parent(&entry)?;
        match existing(&parent, &entry.name)? {
            None => {
                parent.create_dir(&entry.name)?;
                paths::test_checkpoint("directory-created")?;
            }
            Some(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Some(_) => return Err(RecoveryError::Conflict),
        }
        // Under exclusive ownership, an existing directory is the one accepted
        // initially or created by an earlier attempt of this effect.
        sync(&parent.open_dir(&entry.name)?)?;
        sync(&parent)?;
        paths::test_checkpoint("before-receipt")
    }
}
