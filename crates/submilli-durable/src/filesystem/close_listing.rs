use super::operation::FilesystemOperation;
use super::{FilesystemError, finish_owned, test_checkpoint, validate_id};
use crate::io::RecoveryError;
use crate::readers::progress_start;
use crate::unit_of_work::{Listing, ListingPosition, Progress};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::{
    fuel,
    limits::{HostBytes, TenantLimits},
};

enum CloseListingState {
    CloseAt(ListingPosition),
    Completed,
}
impl FilesystemOperation {
    pub(crate) async fn close_listing(
        &self,
        resource: &EffectId,
        effect: &EffectId,
        limits: &TenantLimits,
    ) -> Result<(), FilesystemError> {
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
                    CloseListingState::CloseAt(before) => {
                        if let Some(work) = &operation.work {
                            work.before_effect();
                        }
                        operation
                            .filesystem
                            .stored_listing_progress(
                                &resource,
                                listing.progress.clone(),
                                encode_state(&effect, before),
                            )
                            .await?;
                        test_checkpoint("listing-progress");
                        CloseListingState::Completed
                    }
                    CloseListingState::Completed => return Ok(()),
                };
            }
        })
        .await
    }
}
fn decode_state(
    listing: &Listing,
    effect: &EffectId,
) -> Result<CloseListingState, FilesystemError> {
    let before = progress_start(&listing.progress, effect, true)?;
    if before.index
        > listing
            .content
            .as_ref()
            .ok_or(RecoveryError::Conflict)?
            .len()
    {
        return Err(RecoveryError::Conflict.into());
    }
    Ok(CloseListingState::CloseAt(before))
}
fn encode_state(effect: &EffectId, before: ListingPosition) -> Progress<ListingPosition> {
    let after = ListingPosition {
        index: before.index,
        closed: true,
    };
    Progress {
        effect: effect.clone(),
        close: true,
        before,
        after,
    }
}
