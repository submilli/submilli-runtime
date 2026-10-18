use super::operation::FilesystemOperation;
use super::{FilesystemError, finish_owned, test_checkpoint, validate_id};
use crate::io::RecoveryError;
use crate::readers::{DirectoryEntry, progress_start};
use crate::unit_of_work::{Listing, ListingPosition, Progress};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::{
    fuel,
    limits::{HostBytes, TenantLimits},
};

enum NextListingState {
    ReadAt(ListingPosition),
    SaveProgress {
        before: ListingPosition,
        after: ListingPosition,
        entry: Option<DirectoryEntry>,
    },
    Completed(Option<DirectoryEntry>),
}
impl FilesystemOperation {
    pub(crate) async fn next_listing(
        &self,
        resource: &EffectId,
        effect: &EffectId,
        limits: &TenantLimits,
    ) -> Result<Option<interpreter::runtime::filesystem::DirectoryEntry>, FilesystemError> {
        validate_id(resource)?;
        validate_id(effect)?;
        if resource == effect {
            return Err(RecoveryError::Conflict.into());
        }
        let gate = self.filesystem.filesystem_gate.clone().lock_owned().await;
        let bytes = self.filesystem.stored_listing_bytes(resource).await?;
        // Covers ciphertext, decoded entries, validation copies and transaction encoding.
        let memory = HostBytes::new(limits, bytes.saturating_mul(8).saturating_add(8192))?;
        self.charge(fuel::HASH, bytes.saturating_mul(4))?;
        self.charge(fuel::PARSE, bytes.saturating_mul(4))?;
        let operation = self.clone();
        let resource = resource.clone();
        let effect = effect.clone();
        finish_owned(async move {
            let _gate = gate;
            let _memory = memory;
            let listing = operation
                .filesystem
                .stored_listing_record(&resource)
                .await?;
            let mut state = decode_state(&listing, &effect)?;
            loop {
                state = match state {
                    NextListingState::ReadAt(before) => {
                        let entries = listing.content.as_ref().ok_or(RecoveryError::Conflict)?;
                        if before.index > entries.len() {
                            return Err(RecoveryError::Conflict.into());
                        }
                        let entry = if before.closed {
                            None
                        } else {
                            entries.get(before.index).cloned()
                        };
                        let after = ListingPosition {
                            index: before.index + usize::from(entry.is_some()),
                            closed: before.closed,
                        };
                        test_checkpoint("listing-next");
                        NextListingState::SaveProgress {
                            before,
                            after,
                            entry,
                        }
                    }
                    NextListingState::SaveProgress {
                        before,
                        after,
                        entry,
                    } => {
                        if let Some(work) = &operation.work {
                            work.before_effect();
                        }
                        operation
                            .filesystem
                            .stored_listing_progress(
                                &resource,
                                listing.progress.clone(),
                                encode_state(&effect, before, after),
                            )
                            .await?;
                        test_checkpoint("listing-progress");
                        NextListingState::Completed(entry)
                    }
                    NextListingState::Completed(entry) => {
                        return Ok(entry.map(|entry| {
                            interpreter::runtime::filesystem::DirectoryEntry {
                                name: entry.path.rsplit('/').next().unwrap_or_default().into(),
                                path: entry.path,
                                kind: if entry.directory {
                                    "directory"
                                } else if entry.symlink {
                                    "symlink"
                                } else {
                                    "file"
                                },
                                bytes: if entry.directory || entry.symlink {
                                    0
                                } else {
                                    entry.bytes
                                },
                            }
                        }));
                    }
                };
            }
        })
        .await
    }
}
fn decode_state(listing: &Listing, effect: &EffectId) -> Result<NextListingState, FilesystemError> {
    let before = progress_start(&listing.progress, effect, false)?;
    if before.index
        > listing
            .content
            .as_ref()
            .ok_or(RecoveryError::Conflict)?
            .len()
    {
        return Err(RecoveryError::Conflict.into());
    }
    Ok(NextListingState::ReadAt(before))
}
fn encode_state(
    effect: &EffectId,
    before: ListingPosition,
    after: ListingPosition,
) -> Progress<ListingPosition> {
    Progress {
        effect: effect.clone(),
        close: false,
        before,
        after,
    }
}
