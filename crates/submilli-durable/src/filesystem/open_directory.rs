//! A directory open saves an immutable listing, never a live OS iterator.
use super::operation::FilesystemOperation;
use super::{
    FilesystemError, blocking, conflict, preflight_refusal, require_idle, require_resource_limit,
    test_checkpoint, validate_id,
};
use crate::io::{RecoveryError, validate_guest_path};
use crate::readers::{DirectoryEntry, MAX_LISTING_BYTES};
use crate::unit_of_work::{Listing, StoreError};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::fuel;
use std::sync::Arc;

enum OpenDirectoryState {
    Initial,
    Completed(Result<Vec<DirectoryEntry>, String>),
}
impl FilesystemOperation {
    pub(crate) async fn open_directory_reserved(
        &self,
        resource: &EffectId,
        path: &str,
        recursive: bool,
        memory: Option<Arc<interpreter::runtime::limits::HostBytes>>,
    ) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_guest_path(path)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut listing =
            if let Some(listing) = unit.get_listing(&self.filesystem.run_id, resource).await? {
                if listing.filesystem_config_fingerprint
                    != self.filesystem.filesystem_config_fingerprint
                    || listing.path != path
                    || listing.recursive != recursive
                {
                    return conflict();
                }
                listing
            } else {
                require_idle(unit.as_mut(), &self.filesystem.run_id).await?;
                require_resource_limit(unit.as_mut(), &self.filesystem.run_id).await?;
                let listing = Listing {
                    filesystem_config_fingerprint: self.filesystem.filesystem_config_fingerprint,
                    id: resource.clone(),
                    path: path.into(),
                    recursive,
                    progress: None,
                    content: None,
                    error: None,
                };
                unit.save_listing(&self.filesystem.run_id, listing.clone())
                    .await?;
                listing
            };
        unit.commit().await?;
        let mut state = decode_state(&mut listing)?;
        if let OpenDirectoryState::Completed(Ok(entries)) = &state {
            // A saved observation is already an effect; charge decoding by settlement.
            if let Some(work) = &self.work {
                work.before_effect();
            }
            let bytes = listing_data_bytes(entries);
            self.charge(fuel::HASH, bytes)?;
            self.charge(fuel::PARSE, bytes)?;
        }
        loop {
            state = match state {
                OpenDirectoryState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let path = path.to_owned();
                    let worker_memory = memory.clone();
                    let result = blocking(gate.clone(), move || {
                        let _memory = worker_memory;
                        files.directory_listing_metered(&path, recursive, work.as_deref())
                    })
                    .await;
                    match result {
                        Ok(entries) => {
                            let bytes = listing_data_bytes(&entries);
                            self.charge(fuel::PARSE, bytes.saturating_mul(2))?;
                            self.charge(fuel::COPY, bytes)?;
                            let encoded = serde_json::to_vec(&entries)
                                .map_err(|_| RecoveryError::InvalidRecord)?;
                            if encoded.len() > MAX_LISTING_BYTES {
                                OpenDirectoryState::Completed(Err(
                                    "directory snapshot exceeds the size limit".into(),
                                ))
                            } else {
                                self.charge(fuel::HASH, encoded.len() as u64)?;
                                OpenDirectoryState::Completed(Ok(entries))
                            }
                        }
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            OpenDirectoryState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                OpenDirectoryState::Completed(result) => {
                    result.map_err(FilesystemError::Rejected)?;
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    test_checkpoint("listing-open");
                    return Ok(());
                }
            };
            encode_state(&mut listing, &state);
            let mut unit = (self.filesystem.units)().await?;
            unit.save_listing(&self.filesystem.run_id, listing.clone())
                .await?;
            unit.commit().await?;
        }
    }
}
fn decode_state(listing: &mut Listing) -> Result<OpenDirectoryState, FilesystemError> {
    match (listing.content.take(), listing.error.take()) {
        (None, None) => Ok(OpenDirectoryState::Initial),
        (Some(entries), None) => Ok(OpenDirectoryState::Completed(Ok(entries))),
        (None, Some(reason)) => Ok(OpenDirectoryState::Completed(Err(reason))),
        _ => Err(StoreError::Conflict.into()),
    }
}
fn encode_state(listing: &mut Listing, state: &OpenDirectoryState) {
    match state {
        OpenDirectoryState::Initial => {}
        OpenDirectoryState::Completed(Ok(entries)) => listing.content = Some(entries.clone()),
        OpenDirectoryState::Completed(Err(reason)) => listing.error = Some(reason.clone()),
    }
}
// Include path bytes, not just entry count, in the work charge.
fn listing_data_bytes(entries: &[DirectoryEntry]) -> u64 {
    entries.iter().fold(0u64, |bytes, entry| {
        bytes
            .saturating_add(std::mem::size_of_val(entry) as u64)
            .saturating_add(entry.path.len() as u64)
    })
}
