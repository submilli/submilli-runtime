//! Effect-aware readers and immutable directory snapshots.
use crate::filesystem::*;
use crate::filesystem::{conflict, require_idle, require_resource_limit};
use crate::io::{Files, RecoveryError, Roots, identity, validate_guest_path};
use crate::io::{MAX_DEPTH, MAX_ENTRIES};
use crate::unit_of_work::*;
use cap_std::fs::File;
use interpreter::runtime::filesystem::{
    DirectoryReader, FileReader, FilesystemError as PortError, Invocation, ReadOutput,
    ReadPosition, ReaderOptions, ResourceDescriptor, ResourceKind,
};
use interpreter::runtime::fs;
use interpreter::runtime::fs::FileIdentity;
use interpreter::runtime::limits::TenantLimits;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
/// ID-only handles are valid during replay. Open a native file only for a new
/// backend call; dropping this object never retires its durable row.
pub struct FilesystemReader {
    pub(crate) filesystem: DurableFilesystem,
    pub(crate) resource: String,
    pub(crate) options: ReaderOptions,
}
#[async_trait::async_trait]
impl FileReader for FilesystemReader {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            scope: self.filesystem.id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Reader(self.options.clone()),
        })
    }
    async fn next(
        &mut self,
        invocation: &Invocation,
        limits: &TenantLimits,
    ) -> Result<ReadOutput, PortError> {
        let fs = self.filesystem.metered(invocation);
        let effect = operation(invocation)?;
        validate_id(effect).map_err(backend)?;
        if effect == self.resource {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let _gate = fs.context.filesystem_gate.lock().await;
        let row = fs
            .stored_reader_record(&self.resource)
            .await
            .map_err(backend)?;
        if row.options != self.options {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let before = progress_start(&row.progress, effect, false).map_err(backend)?;
        let initial = row
            .position
            .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
        // Revalidate the path on every dispatched call, even in the same process.
        // Native buffering cannot become an alternative progress authority.
        let mut after = before.clone();
        let output = if before.closed {
            interpreter::runtime::default_filesystem::read_item(
                &mut std::io::Cursor::new([]),
                &self.options,
                &mut after,
                invocation,
                limits,
            )?
        } else {
            let mut native = fs
                .files
                .restore_reader(initial)
                .map_err(|e| backend(e.into()))?;
            interpreter::runtime::default_filesystem::read_item(
                &mut native,
                &self.options,
                &mut after,
                invocation,
                limits,
            )?
        };
        checkpoint("reader-read");
        if let Some(work) = &invocation.work {
            work.before_effect();
        }
        fs.stored_reader_progress(
            &self.resource,
            row.progress,
            Progress {
                effect: effect.into(),
                close: false,
                before,
                after,
            },
        )
        .await
        .map_err(backend)?;
        checkpoint("reader-progress");
        Ok(output)
    }
    async fn close(&mut self, invocation: &Invocation) -> Result<(), PortError> {
        let fs = self.filesystem.metered(invocation);
        let effect = operation(invocation)?;
        validate_id(effect).map_err(backend)?;
        if effect == self.resource {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let _gate = fs.context.filesystem_gate.lock().await;
        let row = fs
            .stored_reader_record(&self.resource)
            .await
            .map_err(backend)?;
        if row.options != self.options {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let before = progress_start(&row.progress, effect, true).map_err(backend)?;
        let after = ReadPosition {
            closed: true,
            ..before.clone()
        };
        if let Some(work) = &invocation.work {
            work.before_effect();
        }
        fs.stored_reader_progress(
            &self.resource,
            row.progress,
            Progress {
                effect: effect.into(),
                close: true,
                before,
                after,
            },
        )
        .await
        .map_err(backend)
    }
}

pub(crate) fn progress_start<T: Clone + Default>(
    progress: &Option<Progress<T>>,
    effect: &str,
    close: bool,
) -> Result<T, FilesystemError> {
    match progress {
        Some(prior) if prior.effect == effect => {
            if prior.close != close {
                return Err(RecoveryError::Conflict.into());
            }
            Ok(prior.before.clone())
        }
        Some(prior) => Ok(prior.after.clone()),
        None => Ok(T::default()),
    }
}

impl DurableFilesystem {
    pub async fn open_reader(
        &self,
        resource: &str,
        path: &str,
        options: ReaderOptions,
    ) -> Result<FilesystemReader, FilesystemError> {
        validate_id(resource)?;
        validate_guest_path(path)?;
        if matches!(options, ReaderOptions::Bytes { chunk_size: 0 }) {
            return Err(RecoveryError::InvalidRecord.into());
        }
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let initial = self.stored_reader(resource, path, &options).await?;
        if initial.is_none() {
            let workspace = self.files.clone();
            let (id, path) = (resource.to_owned(), path.to_owned());
            let opened = blocking(gate.clone(), move || {
                workspace.open_reader(&id, &path)?.position()
            })
            .await;
            let position = match opened {
                Ok(position) => position,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    self.stored_reject_reader(resource, error.to_string())
                        .await?;
                    return Err(FilesystemError::Rejected(error.to_string()));
                }
                Err(error) => return Err(error),
            };
            self.stored_reader_opened(resource, &position).await?;
        }
        checkpoint("reader-open");
        Ok(FilesystemReader {
            filesystem: self.clone(),
            resource: resource.into(),
            options,
        })
    }
}
impl DurableFilesystem {
    pub(crate) async fn listing_step(
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
        let bytes = self.stored_listing_bytes(resource).await?;
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
        use crate::unit_of_work::{ListingPosition, Progress};
        let row = self.stored_listing_record(resource).await?;
        let before = progress_start(&row.progress, effect, close)?;
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
        self.stored_listing_progress(
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
    pub(crate) async fn open_directory_reserved(
        &self,
        resource: &str,
        path: &str,
        recursive: bool,
        memory: Option<Arc<interpreter::runtime::limits::HostBytes>>,
    ) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_guest_path(path)?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let entries = if let Some(entries) =
            self.stored_listing(resource, path, recursive, None).await?
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
            let workspace = self.files.clone();
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
                    self.stored_reject_listing(resource, reason.clone()).await?;
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
                self.stored_reject_listing(resource, reason.clone()).await?;
                return Err(FilesystemError::Rejected(reason));
            }
            self.charge(interpreter::runtime::fuel::HASH, encoded.len() as u64)?;
            self.stored_listing(resource, path, recursive, Some(entries.clone()))
                .await?;
            entries
        };
        if let Some(work) = &self.work {
            work.before_effect();
        }
        drop(entries);
        checkpoint("listing-open");
        Ok(())
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
pub(crate) struct DurableDirectory {
    pub(crate) filesystem: DurableFilesystem,
    pub(crate) resource: String,
}
#[async_trait::async_trait]
impl DirectoryReader for DurableDirectory {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            scope: self.filesystem.id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Directory,
        })
    }
    async fn next_entry(
        &mut self,
        invocation: &Invocation,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<Option<interpreter::runtime::filesystem::DirectoryEntry>, PortError> {
        self.filesystem
            .metered(invocation)
            .listing_step(&self.resource, operation(invocation)?, false, limits)
            .await
            .map_err(backend)
    }
    fn retained_bytes(&self) -> u64 {
        0
    }
    async fn close(
        &mut self,
        invocation: &Invocation,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<(), PortError> {
        self.filesystem
            .metered(invocation)
            .listing_step(&self.resource, operation(invocation)?, true, limits)
            .await
            .map(|_| ())
            .map_err(backend)
    }
}

/// Byte position of a logical reader, independent of an OS descriptor number.
/// Buffered adapters save the position of the first unread byte, together with
/// their decoder flags; they must not save a prefetched descriptor position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderPosition {
    workspace: Roots,
    resource: String,
    path: String,
    mount: String,
    file: FileIdentity,
    offset: u64,
}

pub struct RetainedReader {
    file: File,
    position: ReaderPosition,
}

impl Files {
    /// Capture the configured pathname and file identity before returning a handle.
    pub fn open_reader(&self, resource: &str, path: &str) -> Result<RetainedReader, RecoveryError> {
        crate::io::validate_id(resource)?;
        let entry = self.entry(path, true)?;
        let parent = self.parent(&entry)?;
        let mut file = parent.open(&entry.name)?;
        let metadata = file.metadata()?;
        self.require_regular(&entry, &metadata)?;
        let file_identity = identity(&metadata)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(RetainedReader {
            file,
            position: ReaderPosition {
                workspace: self.identity(),
                resource: resource.into(),
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
        self.require_regular(&entry, &metadata)?;
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
    pub fn resource(&self) -> &str {
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
    pub(crate) async fn stored_reader(
        &self,
        resource: &str,
        path: &str,
        options: &ReaderOptions,
    ) -> Result<Option<ReaderPosition>, FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        if let Some(reader) = unit.get_reader(scope, resource).await? {
            if reader.binding != binding || reader.path != path || reader.options != *options {
                return conflict();
            }
            if let Some(error) = reader.error {
                return Err(FilesystemError::Rejected(error));
            }
            return Ok(reader.position);
        }
        require_idle(unit, scope).await?;
        require_resource_limit(unit, scope).await?;
        unit.save_reader(
            scope,
            Reader {
                binding,
                id: resource.into(),
                path: path.into(),
                options: options.clone(),
                progress: None,
                position: None,
                error: None,
            },
        )
        .await?;
        owned.commit().await?;
        Ok(None)
    }
    pub(crate) async fn stored_reject_reader(
        &self,
        resource: &str,
        error: String,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut reader = unit
            .get_reader(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if reader.binding != binding {
            return conflict();
        }
        if reader.position.is_none() {
            reader.error = Some(error);
            unit.save_reader(scope, reader).await?;
        }
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_reader_opened(
        &self,
        resource: &str,
        position: &ReaderPosition,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut reader = unit
            .get_reader(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if reader.binding != binding {
            return conflict();
        }
        if reader.position.is_none() {
            reader.position = Some(position.clone());
            unit.save_reader(scope, reader).await?;
        }
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_reader_record(
        &self,
        resource: &str,
    ) -> Result<Reader, FilesystemError> {
        let scope = self.id.as_str();
        let mut unit = (self.units)().await?;
        unit.get_reader(scope, resource)
            .await?
            .filter(|r| r.binding == self.binding && r.position.is_some())
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub(crate) async fn stored_reader_progress(
        &self,
        resource: &str,
        expected: Option<Progress<ReadPosition>>,
        progress: Progress<ReadPosition>,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut reader = unit
            .get_reader(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if reader.binding != binding || reader.progress != expected {
            return conflict();
        }
        reader.progress = Some(progress);
        unit.save_reader(scope, reader).await?;
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_listing_bytes(
        &self,
        resource: &str,
    ) -> Result<u64, FilesystemError> {
        let scope = self.id.as_str();
        let mut unit = (self.units)().await?;
        Ok(unit.listing_bytes(scope, resource).await?)
    }
    pub(crate) async fn stored_listing_record(
        &self,
        resource: &str,
    ) -> Result<Listing, FilesystemError> {
        let scope = self.id.as_str();
        let mut unit = (self.units)().await?;
        unit.get_listing(scope, resource)
            .await?
            .filter(|r| r.binding == self.binding && r.content.is_some())
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub(crate) async fn stored_listing_progress(
        &self,
        resource: &str,
        expected: Option<Progress<ListingPosition>>,
        progress: Progress<ListingPosition>,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut listing = unit
            .get_listing(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if listing.binding != binding || listing.progress != expected {
            return conflict();
        }
        listing.progress = Some(progress);
        unit.save_listing(scope, listing).await?;
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_listing(
        &self,
        resource: &str,
        path: &str,
        recursive: bool,
        completed: Option<Vec<DirectoryEntry>>,
    ) -> Result<Option<Vec<DirectoryEntry>>, FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        if let Some(mut listing) = unit.get_listing(scope, resource).await? {
            if listing.binding != binding || listing.path != path || listing.recursive != recursive
            {
                return conflict();
            }
            if let Some(error) = listing.error {
                return Err(FilesystemError::Rejected(error));
            }
            if let Some(content) = listing.content {
                if completed.as_ref().is_some_and(|v| *v != content) {
                    return conflict();
                }
                return Ok(Some(content));
            }
            if let Some(content) = completed {
                listing.content = Some(content.clone());
                unit.save_listing(scope, listing).await?;
                owned.commit().await?;
                return Ok(Some(content));
            }
            return Ok(None);
        }
        if completed.is_some() {
            return conflict();
        }
        require_idle(unit, scope).await?;
        require_resource_limit(unit, scope).await?;
        unit.save_listing(
            scope,
            Listing {
                binding,
                id: resource.into(),
                path: path.into(),
                recursive,
                progress: None,
                content: None,
                error: None,
            },
        )
        .await?;
        owned.commit().await?;
        Ok(None)
    }
    pub(crate) async fn stored_reject_listing(
        &self,
        resource: &str,
        reason: String,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut listing = unit
            .get_listing(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if listing.binding != binding {
            return conflict();
        }
        if listing.content.is_none() {
            listing.error = Some(reason);
            unit.save_listing(scope, listing).await?;
        }
        owned.commit().await?;
        Ok(())
    }
}
