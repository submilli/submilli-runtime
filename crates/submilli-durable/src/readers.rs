//! Reader and directory handles, native reads, and their journal updates.
//!
//! Handles carry IDs rather than trusted native cursors. Each dispatched read
//! loads its journal row, reopens and validates the file, reads from the selected
//! logical position, then commits before/after progress before returning the item.
//! The caller's execution history owns earlier returned items; this journal only
//! needs the last step to bridge a crash before history completion.
//! Retrying that step rereads the file, so the caller must keep its contents
//! unchanged while the read is pending; file identity alone cannot ensure that.
//!
//! Directory handles use an immutable saved listing instead of a live OS iterator.
//! Dropping either kind releases local resources but keeps the journal row, since
//! dropping may mean a run paused and will restore the handle later.
use crate::filesystem::conflict;
use crate::filesystem::*;
use crate::io::{Files, RecoveryError, Roots, identity};
use crate::io::{MAX_DEPTH, MAX_ENTRIES};
use crate::unit_of_work::*;
use cap_std::fs::File;
use interpreter::runtime::durable::EffectId;

use interpreter::runtime::filesystem::ReadPosition;
use interpreter::runtime::fs;
use interpreter::runtime::fs::FileIdentity;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
/// Same effect: repeat its item from before. New effect: continue from after.
/// The caller must replay older completed effects from its own history.
pub(crate) fn progress_start<T: Clone + Default>(
    progress: &Option<Progress<T>>,
    effect: &EffectId,
    close: bool,
) -> Result<T, FilesystemError> {
    match progress {
        Some(prior) if &prior.effect == effect => {
            if prior.close != close {
                return Err(RecoveryError::Conflict.into());
            }
            Ok(prior.before.clone())
        }
        Some(prior) => Ok(prior.after.clone()),
        None => Ok(T::default()),
    }
}

/// Saved opening snapshot: configured roots, path, file identity and initial offset.
/// This is not the current cursor. `Reader.progress` stores logical before/after
/// offsets and decoder flags so read-ahead never advances durable progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderPosition {
    workspace: Roots,
    resource: EffectId,
    path: String,
    mount: String,
    file: FileIdentity,
    offset: u64,
}

/// An OS handle for the current dispatched read only; it never enters the journal.
pub struct RetainedReader {
    file: File,
    position: ReaderPosition,
}

impl Files {
    /// Capture the configured pathname and file identity before returning a handle.
    pub fn open_reader(
        &self,
        resource: &EffectId,
        path: &str,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<RetainedReader, RecoveryError> {
        crate::io::validate_id(resource)?;
        let entry = self.entry(path, true)?;
        let parent = self.parent(&entry)?;
        let mut file = parent.open(&entry.name)?;
        let metadata = file.metadata()?;
        self.require_regular(&entry, &metadata, work)?;
        let file_identity = identity(&metadata)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(RetainedReader {
            file,
            position: ReaderPosition {
                workspace: self.identity(),
                resource: resource.clone(),
                path: path.into(),
                mount: entry.mount,
                file: file_identity,
                offset: 0,
            },
        })
    }

    /// Reopen the configured path and refuse missing/replaced resources.
    pub fn restore_reader(
        &self,
        position: ReaderPosition,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<RetainedReader, RecoveryError> {
        crate::io::validate_id(&position.resource)?;
        if position.workspace != self.identity() || position.offset > i64::MAX as u64 {
            return Err(RecoveryError::Conflict);
        }
        let entry = self.entry(&position.path, true)?;
        if entry.mount != position.mount {
            return Err(RecoveryError::Conflict);
        }
        let parent = self.parent(&entry)?;
        let mut file = parent.open(&entry.name)?;
        let metadata = file.metadata()?;
        self.require_regular(&entry, &metadata, work)?;
        if identity(&metadata)? != position.file {
            return Err(RecoveryError::Conflict);
        }
        file.seek(SeekFrom::Start(position.offset))?;
        Ok(RetainedReader { file, position })
    }
}

impl RetainedReader {
    pub fn position(&mut self) -> Result<ReaderPosition, RecoveryError> {
        self.position.offset = self.file.stream_position()?;
        Ok(self.position.clone())
    }
}

impl ReaderPosition {
    pub fn resource(&self) -> &EffectId {
        &self.resource
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn same_resource(&self, other: &Self) -> bool {
        self.workspace == other.workspace
            && self.resource == other.resource
            && self.path == other.path
            && self.mount == other.mount
            && self.file == other.file
    }
}

impl Read for RetainedReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

impl Seek for RetainedReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

pub const MAX_LISTING_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryEntry {
    pub path: String,
    pub directory: bool,
    pub symlink: bool,
    pub bytes: u64,
}

impl Files {
    /// A bounded frozen enumeration. Replaying its iterator uses these entries
    /// in this order, never a new readdir against mutable storage.
    pub(crate) fn directory_listing_metered(
        &self,
        path: &str,
        recursive: bool,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<Vec<DirectoryEntry>, RecoveryError> {
        use interpreter::runtime::fuel;
        crate::io::validate_guest_path(path)?;
        let guest = fs::guest_normalize(self.vfs.cwd(), path).map_err(fs::ContainError::from)?;
        let mut stack = vec![(guest, 0)];
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        while let Some((guest, depth)) = stack.pop() {
            if depth > MAX_DEPTH {
                return Err(RecoveryError::Limit);
            }
            if let Some(work) = work {
                work.charge(fuel::SYSCALL, 1)?;
            }
            let resolved = crate::io::resolve_content(&self.vfs, "/", &guest)?;
            let directory = resolved.open_dir()?;
            for entry in directory.entries()? {
                if let Some(work) = work {
                    work.charge(fuel::SYSCALL, 2)?;
                }
                let entry = entry?;
                let name = entry.file_name();
                if crate::io::private_recovery_path(std::path::Path::new(&name)) {
                    continue;
                }
                let name = name
                    .to_str()
                    .ok_or(RecoveryError::Unsupported("non-UTF-8 directory entry"))?;
                let child = format!("{}/{name}", guest.trim_end_matches('/'));
                crate::io::validate_guest_path(&child)?;
                let metadata = directory.symlink_metadata(name)?;
                if !metadata.is_file() && !metadata.is_dir() && !metadata.file_type().is_symlink() {
                    return Err(RecoveryError::Unsupported(
                        "special file in directory snapshot",
                    ));
                }
                let item = DirectoryEntry {
                    path: child.clone(),
                    directory: metadata.is_dir(),
                    symlink: metadata.file_type().is_symlink(),
                    bytes: metadata.len(),
                };
                bytes = bytes
                    .checked_add(child.len().saturating_mul(6).saturating_add(128))
                    .ok_or(RecoveryError::Limit)?;
                if bytes > MAX_LISTING_BYTES || entries.len() >= MAX_ENTRIES {
                    return Err(RecoveryError::Limit);
                }
                if let Some(work) = work {
                    work.charge(fuel::PARSE, child.len() as u64)?;
                    work.charge(fuel::ELEM, 1)?;
                }
                if recursive && item.directory && !item.symlink {
                    stack.push((child, depth + 1));
                }
                entries.push(item);
            }
        }
        if let Some(work) = work {
            work.charge_fuel(fuel::sort_cost(entries.len() as u64))?;
        }
        entries.sort_by(|left, right| left.path.split('/').cmp(right.path.split('/')));
        Ok(entries)
    }
}

impl DurableFilesystem {
    pub(crate) async fn stored_reader_record(
        &self,
        resource: &EffectId,
    ) -> Result<Reader, FilesystemError> {
        let run_id = &self.run_id;
        let mut unit = (self.units)().await?;
        unit.get_reader(run_id, resource)
            .await?
            .filter(|r| {
                r.filesystem_config_fingerprint == self.filesystem_config_fingerprint
                    && r.position.is_some()
            })
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub(crate) async fn stored_reader_progress(
        &self,
        resource: &EffectId,
        expected: Option<Progress<ReadPosition>>,
        progress: Progress<ReadPosition>,
    ) -> Result<(), FilesystemError> {
        let run_id = &self.run_id;
        let filesystem_config_fingerprint = self.filesystem_config_fingerprint;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut reader = unit
            .get_reader(run_id, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if reader.filesystem_config_fingerprint != filesystem_config_fingerprint
            || reader.progress != expected
        {
            return conflict();
        }
        reader.progress = Some(progress);
        unit.save_reader(run_id, reader).await?;
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_listing_bytes(
        &self,
        resource: &EffectId,
    ) -> Result<u64, FilesystemError> {
        let run_id = &self.run_id;
        let mut unit = (self.units)().await?;
        Ok(unit.listing_bytes(run_id, resource).await?)
    }
    pub(crate) async fn stored_listing_record(
        &self,
        resource: &EffectId,
    ) -> Result<Listing, FilesystemError> {
        let run_id = &self.run_id;
        let mut unit = (self.units)().await?;
        unit.get_listing(run_id, resource)
            .await?
            .filter(|r| {
                r.filesystem_config_fingerprint == self.filesystem_config_fingerprint
                    && r.content.is_some()
            })
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub(crate) async fn stored_listing_progress(
        &self,
        resource: &EffectId,
        expected: Option<Progress<ListingPosition>>,
        progress: Progress<ListingPosition>,
    ) -> Result<(), FilesystemError> {
        let run_id = &self.run_id;
        let filesystem_config_fingerprint = self.filesystem_config_fingerprint;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut listing = unit
            .get_listing(run_id, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if listing.filesystem_config_fingerprint != filesystem_config_fingerprint
            || listing.progress != expected
        {
            return conflict();
        }
        listing.progress = Some(progress);
        unit.save_listing(run_id, listing).await?;
        owned.commit().await?;
        Ok(())
    }
}
