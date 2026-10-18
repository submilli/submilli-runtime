use crate::io::Entry;
use interpreter::runtime::fs::FileIdentity;
use serde::{Deserialize, Serialize};
/// Original source and destination identities used to recognize a rename's progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveCheckpoint {
    target: Target,
    destination: Entry,
    destination_original: Option<FileIdentity>,
}

use super::operation::FilesystemOperation;
use super::{
    CaptureOptions, FilesystemError, Target, blocking, decode_checkpoint, decode_outcome,
    preflight_refusal, serialize, test_checkpoint, validate_id,
};
use super::{pin, require_pin};
use crate::io::{self as paths, RecoveryError, validate_guest_path};
use crate::io::{current, existing, identity, sync};
use crate::unit_of_work::{JournalEntry, Outcome, RecordedOutcome};
use interpreter::runtime::durable::EffectId;
use std::sync::Arc;

/// Saved progress for this operation. Every unfinished stage is safe to repeat.
enum MoveState {
    Initial,
    Prepared(Box<MoveCheckpoint>),
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
            self.charge_preparation(source, 0)?;
            test_checkpoint("effect-request");
        }
        loop {
            state = match state {
                MoveState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let (effect, source, destination) =
                        (effect.to_owned(), source.to_owned(), destination.to_owned());
                    let result = blocking(gate.clone(), move || {
                        let target = files.capture_target(
                            &effect,
                            &source,
                            CaptureOptions::default(),
                            work.as_deref(),
                        )?;
                        let artifacts = files.artifacts(&target.source, &effect, true)?;
                        if target.original.is_none() {
                            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
                        }
                        let destination = files.entry(&destination, false)?;
                        if destination.mount != target.source.mount {
                            return Err(RecoveryError::Unsupported(
                                "cross-volume move requires caller-orchestrated copy and remove",
                            ));
                        }
                        if destination == target.source {
                            return Err(RecoveryError::Unsupported(
                                "move source equals destination",
                            ));
                        }
                        let parent = files.parent(&destination)?;
                        let mut destination_original = None;
                        if let Some(metadata) = existing(&parent, &destination.name)? {
                            files.require_regular(&destination, &metadata, work.as_deref())?;
                            let original = identity(&metadata)?;
                            if Some(original) == target.original {
                                return Err(RecoveryError::Unsupported("aliased move destination"));
                            }
                            parent.open(&destination.name)?.sync_all()?;
                            sync(&parent)?;
                            pin(
                                &parent,
                                &destination.name,
                                &artifacts,
                                "destination",
                                original,
                            )?;
                            destination_original = Some(original);
                        }
                        paths::sync(&artifacts)?;
                        paths::test_checkpoint("prepared")?;
                        Ok(MoveCheckpoint {
                            target,
                            destination,
                            destination_original,
                        })
                    })
                    .await;
                    match result {
                        Ok(checkpoint) => MoveState::Prepared(Box::new(checkpoint)),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            MoveState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                MoveState::Prepared(checkpoint) => {
                    checkpoint.target.validate()?;
                    checkpoint.destination.validate()?;
                    if checkpoint.target.original.is_none()
                        || checkpoint.target.original_directory
                        || checkpoint.destination.mount != checkpoint.target.source.mount
                    {
                        return Err(RecoveryError::InvalidRecord.into());
                    }
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let effect = effect.to_owned();
                    blocking(gate.clone(), move || {
                        let artifacts =
                            files.artifacts(&checkpoint.target.source, &effect, false)?;
                        {
                            let state = &checkpoint;
                            let artifacts = &artifacts;

                            let target = &state.target;
                            let original = target.original.ok_or(RecoveryError::InvalidRecord)?;
                            require_pin(artifacts, "original", original)?;
                            let destination = &state.destination;
                            let source_parent = files.parent(&target.source)?;
                            let destination_parent = files.parent(destination)?;
                            let source = current(&source_parent, &target.source.name)?;
                            let present = current(&destination_parent, &destination.name)?;
                            if ![None, Some(original)].contains(&source)
                                || ![state.destination_original, Some(original)].contains(&present)
                            {
                                return Err(RecoveryError::Conflict);
                            }
                            if present != Some(original) {
                                pin(artifacts, "original", artifacts, "publish", original)?;
                                artifacts.rename(
                                    "publish",
                                    &destination_parent,
                                    &destination.name,
                                )?;
                                paths::test_checkpoint("destination-published")?;
                            }
                            // A destination found on retry may be visible but not yet durable.
                            // Persist it before unlinking the source on every attempt.
                            sync(&destination_parent)?;
                            if source.is_some() {
                                source_parent.remove_file(&target.source.name)?;
                                paths::test_checkpoint("source-removed")?;
                            }
                            sync(&source_parent)?;
                            sync(&destination_parent)?;
                            sync(artifacts)?;
                        }
                        files.reconcile_quotas(work.as_deref())?;
                        paths::test_checkpoint("before-receipt")
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
                MoveState::Prepared(_) => test_checkpoint("effect-intent"),
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
    if let Some(result) = decode_outcome(journal_entry, config)? {
        return Ok(MoveState::Completed(result));
    }
    match &journal_entry.recovery_payload {
        Some(bytes) => Ok(MoveState::Prepared(decode_checkpoint(bytes)?)),
        None => Ok(MoveState::Initial),
    }
}

// The operation owns this mapping; SQL keeps its existing immutable intent and receipt columns.
fn encode_state(
    journal_entry: &mut JournalEntry,
    state: &MoveState,
) -> Result<(), FilesystemError> {
    match state {
        MoveState::Initial => {}
        MoveState::Prepared(checkpoint) => {
            journal_entry.recovery_payload = Some(serialize(checkpoint)?);
        }
        MoveState::Completed(result) => {
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
