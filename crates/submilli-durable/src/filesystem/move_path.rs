use super::operation::FilesystemOperation;
use super::{
    FilesystemError, blocking, deserialize, preflight_refusal, serialize, test_checkpoint,
    validate_id, validate_journal_configuration,
};
use crate::io::{
    self as paths, Files, RecoveryError, current, existing, identity, sync, validate_guest_path,
};
use crate::unit_of_work::{JournalEntry, Outcome};
use interpreter::runtime::durable::EffectId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum MoveState {
    #[serde(skip)]
    Initial,
    Prepared {},
    Completed(Result<Outcome, String>),
}

impl FilesystemOperation {
    pub async fn move_path(
        &self,
        effect: &EffectId,
        source: &str,
        destination: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(source)?;
        validate_guest_path(destination)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut journal_entry = self
            .create_initial_or_get_last_journal_entry(
                unit.as_mut(),
                effect,
                "move",
                &(source, destination),
            )
            .await?;
        unit.commit().await?;

        let mut state = decode_state(
            &journal_entry,
            self.filesystem.filesystem_config_fingerprint,
        )?;
        if !matches!(state, MoveState::Completed(_)) {
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                MoveState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let source = source.to_owned();
                    let destination = destination.to_owned();
                    let result = blocking(gate.clone(), move || {
                        let source = files.entry(&source, false)?;
                        let destination = files.entry(&destination, false)?;
                        if source.mount != destination.mount {
                            return Err(RecoveryError::Unsupported(
                                "cross-volume move requires caller-orchestrated copy and remove",
                            ));
                        }
                        if source == destination {
                            return Err(RecoveryError::Unsupported(
                                "move source equals destination",
                            ));
                        }
                        let source_parent = files.parent(&source)?;
                        let source_metadata = existing(&source_parent, &source.name)?
                            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                        files.require_regular(&source, &source_metadata, work.as_deref())?;
                        let destination_parent = files.parent(&destination)?;
                        if let Some(metadata) = existing(&destination_parent, &destination.name)? {
                            files.require_regular(&destination, &metadata, work.as_deref())?;
                            if identity(&metadata)? == identity(&source_metadata)? {
                                return Err(RecoveryError::Unsupported("aliased move destination"));
                            }
                        }
                        // Keep the original data durable at its source name until
                        // publication of the destination has been synchronized.
                        source_parent.open(&source.name)?.sync_all()?;
                        sync(&source_parent)?;

                        paths::test_checkpoint("prepared")?;
                        Ok(())
                    })
                    .await;
                    match result {
                        Ok(()) => MoveState::Prepared {},
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            MoveState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                MoveState::Prepared {} => {
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let effect = effect.to_owned();
                    let source = source.to_owned();
                    let destination = destination.to_owned();
                    blocking(gate.clone(), move || {
                        files.move_path(&effect, &source, &destination)
                    })
                    .await?;

                    test_checkpoint("effect-applied");
                    MoveState::Completed(Ok(Outcome { bytes: 0 }))
                }
                MoveState::Completed(result) => return result.map_err(FilesystemError::Rejected),
            };
            encode_state(&mut journal_entry, &state)?;
            self.save_journal_entry(&journal_entry).await?;
            match &state {
                MoveState::Prepared {} => test_checkpoint("effect-intent"),
                MoveState::Completed(Ok(_)) => test_checkpoint("effect-receipt"),
                _ => {}
            }
        }
    }
}

fn decode_state(
    journal_entry: &JournalEntry,
    config: [u8; 32],
) -> Result<MoveState, FilesystemError> {
    validate_journal_configuration(journal_entry, config)?;
    match &journal_entry.state {
        Some(bytes) => deserialize(bytes),
        None => Ok(MoveState::Initial),
    }
}

fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &MoveState,
) -> Result<(), FilesystemError> {
    journal_entry.state = match state {
        MoveState::Initial => None,
        _ => Some(serialize(state)?),
    };
    Ok(())
}

impl Files {
    fn move_path(
        &self,
        effect: &EffectId,
        source: &str,
        destination: &str,
    ) -> Result<(), RecoveryError> {
        let source = self.entry(source, false)?;
        let destination = self.entry(destination, false)?;
        if source.mount != destination.mount || source == destination {
            return Err(RecoveryError::InvalidRecord);
        }
        let source_parent = self.parent(&source)?;
        let destination_parent = self.parent(&destination)?;
        let volume = self.volume(&source.mount)?;
        if let Some(source_identity) = current(&source_parent, &source.name)? {
            let destination_metadata = existing(&destination_parent, &destination.name)?;
            if destination_metadata.as_ref().map(identity).transpose()? != Some(source_identity) {
                let replaced_bytes = destination_metadata.map_or(0, |metadata| metadata.len());
                let artifacts = self.artifacts(&source, effect, true)?;
                if existing(&artifacts, "publish")?.is_some() {
                    artifacts.remove_file("publish")?;
                }
                source_parent.hard_link(&source.name, &artifacts, "publish")?;
                artifacts.rename("publish", &destination_parent, &destination.name)?;
                crate::test_checkpoint("move-published-unaccounted");
                // The source inode was already counted. Its temporary second
                // public name costs nothing; only the replaced inode is freed.
                volume.record_size_change(replaced_bytes, 0);
                paths::test_checkpoint("destination-published")?;
            }
            // These are separate persistence steps. Removing the source before
            // the destination is durable could lose the only copy on power loss.
            sync(&destination_parent)?;
            source_parent.remove_file(&source.name)?;
            paths::test_checkpoint("source-removed")?;
        } else {
            // Only this effect can have removed the prepared source. Its
            // destination was synchronized before that removal was attempted.
            if existing(&destination_parent, &destination.name)?.is_none() {
                return Err(RecoveryError::Conflict);
            }
            sync(&destination_parent)?;
        }
        sync(&source_parent)?;
        paths::test_checkpoint("before-receipt")
    }
}
