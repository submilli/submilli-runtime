use super::*;
use crate::recovery::{DirectoryEntry, MAX_LISTING_BYTES};

pub struct DirectorySnapshot {
    entries: std::vec::IntoIter<DirectoryEntry>,
    offset: usize,
}
impl DirectorySnapshot {
    pub fn position(&self) -> usize {
        self.offset
    }
}
impl Iterator for DirectorySnapshot {
    type Item = DirectoryEntry;
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.entries.next()?;
        self.offset += 1;
        Some(entry)
    }
}

impl DurableFilesystem {
    pub(super) async fn listing_step(
        &self,
        resource: &str,
        effect: &str,
        close: bool,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<Option<interpreter::runtime::filesystem::DirectoryEntry>, FilesystemError> {
        validate_id(resource)?;
        validate_id(effect)?;
        if resource == effect {
            return Err(RecoveryError::Conflict.into());
        }
        let gate = self.context.filesystem_gate.clone().lock_owned().await;
        let bytes = self.journal.listing_bytes(&self.id, resource).await?;
        // Covers encrypted/plain JSON, decoded entries, validation copies and
        // re-encoding during the transaction. JSON includes every path byte and
        // fixed entry fields; Vec spare capacity and String capacity fit this bound.
        let memory = interpreter::runtime::limits::HostBytes::new(
            limits,
            bytes.saturating_mul(8).saturating_add(8192),
        )?;
        self.charge(interpreter::runtime::fuel::HASH, bytes.saturating_mul(4))?;
        self.charge(interpreter::runtime::fuel::PARSE, bytes.saturating_mul(4))?;
        let filesystem = self.clone();
        let resource = resource.to_owned();
        let effect = effect.to_owned();
        finish_owned(async move {
            let _gate = gate;
            let _memory = memory;
            filesystem.advance_listing(&resource, &effect, close).await
        })
        .await
    }
    async fn advance_listing(
        &self,
        resource: &str,
        effect: &str,
        close: bool,
    ) -> Result<Option<interpreter::runtime::filesystem::DirectoryEntry>, FilesystemError> {
        use crate::state::{ListingPosition, Progress};
        let row = self.journal.listing_record(&self.id, resource).await?;
        let before = super::resources::progress_start(&row.progress, effect, close)?;
        let entries = row.content.ok_or(RecoveryError::Conflict)?;
        if before.index > entries.len() {
            return Err(RecoveryError::Conflict.into());
        }
        let entry = if close || before.closed {
            None
        } else {
            entries.get(before.index).cloned()
        };
        let after = ListingPosition {
            index: before.index + usize::from(entry.is_some()),
            closed: before.closed || close,
        };
        checkpoint("listing-next");
        if let Some(work) = &self.work {
            work.before_effect();
        }
        self.journal
            .listing_progress(
                &self.id,
                resource,
                row.progress,
                Progress {
                    effect: effect.into(),
                    close,
                    before,
                    after,
                },
            )
            .await?;
        checkpoint("listing-progress");
        Ok(
            entry.map(|entry| interpreter::runtime::filesystem::DirectoryEntry {
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
            }),
        )
    }
    /// The enumeration is an output. SQLite stores it once; replay never
    /// re-enumerates a completed listing, even if the directory has changed.
    pub async fn open_directory(
        &self,
        resource: &str,
        path: &str,
        recursive: bool,
        offset: usize,
    ) -> Result<DirectorySnapshot, FilesystemError> {
        self.open_directory_reserved(resource, path, recursive, offset, None)
            .await
    }
    pub(super) async fn open_directory_reserved(
        &self,
        resource: &str,
        path: &str,
        recursive: bool,
        offset: usize,
        memory: Option<Arc<interpreter::runtime::limits::HostBytes>>,
    ) -> Result<DirectorySnapshot, FilesystemError> {
        validate_id(resource)?;
        Mutation::Remove { path: path.into() }.validate()?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let entries = if let Some(entries) = self
            .journal
            .listing(&self.id, resource, path, recursive, None)
            .await?
        {
            // This observation is already committed. Charge decoding and
            // authentication by its data size, settling even on exhausted fuel.
            if let Some(work) = &self.work {
                work.before_effect();
            }
            let bytes = listing_data_bytes(&entries);
            self.charge(interpreter::runtime::fuel::HASH, bytes)?;
            self.charge(interpreter::runtime::fuel::PARSE, bytes)?;
            entries
        } else {
            let workspace = self.workspace.clone();
            let path_owned = path.to_owned();
            let work = self.work.clone();
            let worker_memory = memory.clone();
            let result = blocking(gate.clone(), move || {
                // Cancellation cannot refund memory while enumeration still owns it.
                let _memory = worker_memory;
                workspace.directory_listing_metered(&path_owned, recursive, work.as_deref())
            })
            .await;
            let entries = match result {
                Ok(entries) => entries,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    let reason = error.to_string();
                    self.journal
                        .reject_listing(&self.id, resource, reason.clone())
                        .await?;
                    return Err(FilesystemError::Rejected(reason));
                }
                Err(error) => return Err(error),
            };
            let bytes = listing_data_bytes(&entries);
            // Serialize once for the bound and once in the sealed SQL codec.
            self.charge(interpreter::runtime::fuel::PARSE, bytes.saturating_mul(2))?;
            self.charge(interpreter::runtime::fuel::COPY, bytes)?;
            let encoded = serde_json::to_vec(&entries).map_err(|_| RecoveryError::InvalidRecord)?;
            if encoded.len() > MAX_LISTING_BYTES {
                let reason = "directory snapshot exceeds the size limit".to_owned();
                self.journal
                    .reject_listing(&self.id, resource, reason.clone())
                    .await?;
                return Err(FilesystemError::Rejected(reason));
            }
            self.charge(interpreter::runtime::fuel::HASH, encoded.len() as u64)?;
            self.journal
                .listing(&self.id, resource, path, recursive, Some(entries.clone()))
                .await?;
            entries
        };
        if let Some(work) = &self.work {
            work.before_effect();
        }
        if offset > entries.len() {
            return Err(RecoveryError::InvalidRecord.into());
        }
        let mut entries = entries.into_iter();
        for _ in 0..offset {
            entries.next();
        }
        checkpoint("listing-open");
        Ok(DirectorySnapshot { entries, offset })
    }
}

// Meter the data processed, including paths: entry count alone makes long
// names free. Fixed fields use their in-memory size; strings use their byte size.
fn listing_data_bytes(entries: &[DirectoryEntry]) -> u64 {
    entries.iter().fold(0u64, |bytes, entry| {
        bytes
            .saturating_add(std::mem::size_of_val(entry) as u64)
            .saturating_add(entry.path.len() as u64)
    })
}
