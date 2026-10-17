use super::*;
use crate::recovery::{DirectoryEntry, MAX_LISTING_BYTES};

pub struct DirectorySnapshot {
    entries: std::vec::IntoIter<DirectoryEntry>,
    offset: usize,
}
impl DirectorySnapshot {
    pub(crate) fn remaining_entries(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn depth_first(mut self) -> Self {
        self.entries
            .as_mut_slice()
            .sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
        self
    }

    pub(crate) fn retained_bytes(&self) -> u64 {
        self.entries.as_slice().iter().fold(0u64, |bytes, entry| {
            bytes
                .saturating_add(std::mem::size_of_val(entry) as u64)
                .saturating_add(entry.path.capacity() as u64)
        })
    }

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
    /// The enumeration is an output. SQLite stores it once; replay never
    /// re-enumerates a completed listing, even if the directory has changed.
    pub async fn open_directory(
        &self,
        resource: &str,
        path: &str,
        recursive: bool,
        offset: usize,
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
            let result = blocking(gate, move || {
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
