//! Shared filesystem state and the interpreter-facing filesystem implementation.
//!
//! The trait methods create a per-call `FilesystemOperation`. Its state machines
//! assume exclusive root ownership and no later mutation before a pending call
//! finishes. Shared replacement helpers retain only the new bytes; namespace
//! operations never preserve or hash the old destination. No retry is scheduled
//! here: the caller supplies the same effect ID and input again.
mod append_bytes;
mod close_listing;
mod close_reader;
mod close_writer;
mod create_directory;
mod move_path;
mod next_listing;
mod next_reader;
mod open_directory;
mod open_reader;
mod open_writer;
mod operation;
mod remove_path;
mod replacement;
mod update_file;
mod write_bytes;
mod write_chunk;
use crate::io::{self as paths, Files, RecoveryError};
use crate::readers::MAX_LISTING_BYTES;
use crate::unit_of_work::*;
use crate::{UnitFactory, UnitOfWork};
use interpreter::runtime::Vfs;
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::durable::RunId;
use interpreter::runtime::filesystem::{
    DirectoryEntry, DirectoryReader, FileReader, FileWriter, Filesystem,
    FilesystemError as PortError, FilesystemInfo, ReadOutput, ReaderOptions,
};
use interpreter::runtime::fuel::WorkBudget;
use interpreter::runtime::limits::TenantLimits;
use interpreter::runtime::{fs, vfs::Placement};
pub use operation::FilesystemOperation;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
    #[error(transparent)]
    Quota(interpreter::runtime::QuotaExceeded),
    #[error(transparent)]
    Memory(#[from] interpreter::runtime::limits::MemoryCapExceeded),
    #[error(transparent)]
    Persistence(#[from] StoreError),
    #[error(transparent)]
    Recovery(#[from] RecoveryError),
    #[error("filesystem operation was rejected before mutation: {0}")]
    Rejected(String),
    #[error("filesystem worker failed: {0}")]
    Worker(String),
    #[error("invalid durable filesystem configuration: {0}")]
    Configuration(&'static str),
}

/// A run's configured filesystem and journal, shared by calls and resource handles.
/// Clones share the same mounts, admission gate and quota registry. Per-call fuel
/// belongs to `FilesystemOperation`, so a retained handle cannot retain an old budget.
#[derive(Clone)]
pub struct DurableFilesystem {
    pub(crate) files: Arc<Files>,
    pub(crate) units: UnitFactory,
    /// Fingerprint of mounts, root identities, access modes, cwd and quota limits.
    pub(crate) filesystem_config_fingerprint: [u8; 32],
    /// Serializes native mutations and drains workers that outlive cancellation.
    pub(crate) filesystem_gate: Arc<tokio::sync::Mutex<()>>,
    /// Reopened VFS values must use the counters held by existing handles.
    filesystem_quotas: Arc<tokio::sync::Mutex<paths::SharedQuotas>>,
    pub(crate) run_id: RunId,
}

impl DurableFilesystem {
    /// Open the configured mounts for a run. The caller must exclusively own
    /// writable roots and retain these roots and their journal across retries.
    /// Use `reopen` while another handle or worker may still access these roots.
    pub async fn open(
        vfs: Vfs,
        run_id: &RunId,
        units: UnitFactory,
    ) -> Result<Self, FilesystemError> {
        Self::open_shared(vfs, run_id, units, Default::default(), Default::default()).await
    }

    /// Reopen with shared mutation admission and quota counters. The same gate
    /// drains work still owned by cancelled calls before replacing access.
    /// This only opens access; pending journal entries wait for explicit retries.
    pub async fn reopen(
        &self,
        vfs: Vfs,
        run_id: &RunId,
        units: UnitFactory,
    ) -> Result<Self, FilesystemError> {
        Self::open_shared(
            vfs,
            run_id,
            units,
            self.filesystem_gate.clone(),
            self.filesystem_quotas.clone(),
        )
        .await
    }

    async fn open_shared(
        vfs: Vfs,
        run_id: &RunId,
        units: UnitFactory,
        filesystem_gate: Arc<tokio::sync::Mutex<()>>,
        filesystem_quotas: Arc<tokio::sync::Mutex<paths::SharedQuotas>>,
    ) -> Result<Self, FilesystemError> {
        validate_id(run_id)?;
        let gate = Arc::new(filesystem_gate.clone().lock_owned().await);
        let mut quotas = filesystem_quotas.clone().lock_owned().await;
        let filesystem_config_fingerprint = paths::filesystem_config_fingerprint(&vfs)?;
        let run_id = run_id.clone();
        let worker_id = run_id.clone();
        let workspace = blocking(gate, move || {
            paths::require_disjoint_volumes(&vfs)?;
            let vfs = paths::share_quotas(&mut quotas, vfs)?;
            Files::create(vfs, &worker_id)
        })
        .await?;
        Ok(Self {
            files: Arc::new(workspace),
            units,
            filesystem_config_fingerprint,
            filesystem_gate,
            filesystem_quotas,
            run_id,
        })
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
    pub fn vfs(&self) -> &Vfs {
        self.files.vfs()
    }
}
/// The worker owns admission until it exits, even if its awaiting task is dropped.
pub(crate) async fn blocking<T: Send + 'static>(
    guard: Arc<tokio::sync::OwnedMutexGuard<()>>,
    operation: impl FnOnce() -> Result<T, RecoveryError> + Send + 'static,
) -> Result<T, FilesystemError> {
    #[cfg(test)]
    let failure = tests::take_failure();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        #[cfg(test)]
        tests::set_failure(failure);
        let result = operation();
        #[cfg(test)]
        tests::set_failure(None);
        result
    })
    .await
    .map_err(|error| FilesystemError::Worker(error.to_string()))?
    .map_err(Into::into)
}

/// Keep accounting and serialization alive until independently owned SQL work
/// finishes, even if the host call is cancelled. Subsequent attempts drain by
/// acquiring the same filesystem gate.
pub(crate) async fn finish_owned<T: Send + 'static>(
    operation: impl std::future::Future<Output = Result<T, FilesystemError>> + Send + 'static,
) -> Result<T, FilesystemError> {
    tokio::spawn(operation)
        .await
        .map_err(|error| FilesystemError::Worker(error.to_string()))?
}

/// Only a definite refusal before public mutation can become a replayable error.
/// Uncertain I/O leaves the operation pending so a retry can inspect disk state.
pub(crate) fn preflight_refusal(error: &RecoveryError) -> bool {
    match error {
        RecoveryError::Unsupported(_)
        | RecoveryError::Containment(_)
        | RecoveryError::Quota
        | RecoveryError::QuotaRefused(_)
        | RecoveryError::Limit
        | RecoveryError::InvalidRecord => true,
        RecoveryError::Io(error) => matches!(
            error.kind(),
            std::io::ErrorKind::NotFound
                | std::io::ErrorKind::AlreadyExists
                | std::io::ErrorKind::NotADirectory
                | std::io::ErrorKind::IsADirectory
                | std::io::ErrorKind::DirectoryNotEmpty
                | std::io::ErrorKind::PermissionDenied
        ),
        RecoveryError::Conflict | RecoveryError::Fuel(_) => false,
    }
}

pub(crate) fn validate_id(id: &str) -> Result<(), FilesystemError> {
    crate::io::validate_id(id)?;
    Ok(())
}

pub(crate) fn test_checkpoint(_point: &str) {
    crate::test_checkpoint(_point);
}

impl DurableFilesystem {
    /// Start a call with its own budget; handles retain only the shared filesystem.
    pub fn operation(&self, work: Option<Arc<WorkBudget>>) -> FilesystemOperation {
        FilesystemOperation {
            filesystem: self.clone(),
            work,
        }
    }
}

#[async_trait::async_trait]
impl Filesystem for DurableFilesystem {
    fn supports_update_replay(&self) -> bool {
        true
    }
    fn supports_resource_replay(&self) -> bool {
        true
    }

    fn info(&self) -> FilesystemInfo {
        FilesystemInfo::from_vfs(self.files.vfs())
    }
    fn cwd(&self) -> &str {
        self.files.vfs().cwd()
    }
    fn path_cost(&self, path: &str) -> u64 {
        use interpreter::runtime::fuel;
        fuel::SCAN
            .cost(path.len() as u64)
            .saturating_add(fuel::SYSCALL.cost(path.split('/').count() as u64))
    }
    async fn placement(&self, path: &str) -> Result<Placement, PortError> {
        let vfs = self.files.vfs().clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            Ok(paths::resolve_content(&vfs, vfs.cwd(), &path)?
                .placement()
                .clone())
        })
        .await
        .map_err(|e| PortError::Backend(Box::new(e)))?
    }
    async fn read_regular(
        &self,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        maximum_bytes: u64,
        limits: &TenantLimits,
    ) -> Result<interpreter::runtime::filesystem::FileContents, PortError> {
        let vfs = self.files.vfs().clone();
        let path = path.to_owned();
        let checked_vfs = vfs.clone();
        let checked_path = path.clone();
        tokio::task::spawn_blocking(move || {
            paths::check_access(&checked_vfs, checked_vfs.cwd(), &checked_path)
        })
        .await
        .map_err(|e| PortError::Backend(Box::new(e)))??;
        interpreter::runtime::default_filesystem::read_regular(
            vfs,
            work,
            path,
            maximum_bytes,
            limits,
        )
        .await
    }
    async fn directory_names(
        &self,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        maximum_entries: usize,
        limits: &TenantLimits,
    ) -> Result<interpreter::runtime::filesystem::DirectoryNames, PortError> {
        let vfs = self.files.vfs().clone();
        let path = path.to_owned();
        let checked_vfs = vfs.clone();
        let checked_path = path.clone();
        tokio::task::spawn_blocking(move || {
            paths::check_access(&checked_vfs, checked_vfs.cwd(), &checked_path)
        })
        .await
        .map_err(|e| PortError::Backend(Box::new(e)))??;
        interpreter::runtime::default_filesystem::directory_names(
            vfs,
            work,
            path,
            maximum_entries,
            limits,
        )
        .await
    }
    async fn begin_file_update(
        &self,
        effect: &EffectId,
        path: &str,
        request: [u8; 32],
        limits: &TenantLimits,
    ) -> Result<Box<dyn interpreter::runtime::filesystem::FileUpdate>, PortError> {
        DurableFilesystem::begin_file_update(self, effect, path, request, limits).await
    }
    async fn metadata(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, PortError> {
        let vfs = self.files.vfs().clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            if follow {
                Ok(paths::resolve_content(&vfs, vfs.cwd(), &path)?.metadata()?)
            } else {
                paths::check_access(&vfs, vfs.cwd(), &path)?;
                Ok(fs::resolve_link(&vfs, vfs.cwd(), &path)?.symlink_metadata()?)
            }
        })
        .await
        .map_err(|e| PortError::Backend(Box::new(e)))?
    }
    async fn read(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, PortError> {
        let vfs = self.files.vfs().clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = paths::resolve_content(&vfs, vfs.cwd(), &path)?.open()?;
            interpreter::runtime::filesystem::bounded_read(file, offset, maximum_bytes)
        })
        .await
        .map_err(|error| PortError::Backend(Box::new(error)))?
    }
    async fn write(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), PortError> {
        let operation = self.operation(work);
        operation
            .charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)
            .map_err(backend)?;
        operation
            .write_bytes(effect_id, path, bytes)
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn append(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), PortError> {
        let operation = self.operation(work);
        operation
            .charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)
            .map_err(backend)?;
        operation
            .append_bytes(effect_id, path, bytes)
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn mkdir(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
    ) -> Result<(), PortError> {
        let operation = self.operation(work);
        if recursive {
            return Err(PortError::Unsupported(
                "recursive directory creation requires caller-orchestrated primitive operations",
            ));
        }
        operation
            .create_directory(effect_id, path)
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn remove(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        budget: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        let operation = self.operation(work);
        if recursive {
            return Err(PortError::Unsupported(
                "recursive removal requires caller-orchestrated primitive operations",
            ));
        }
        operation
            .remove_path(effect_id, path)
            .await
            .map_err(backend)?;
        budget.effect();
        Ok(())
    }

    async fn move_entry(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        from: &str,
        to: &str,
        budget: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        let operation = self.operation(work);
        operation
            .move_path(effect_id, from, to)
            .await
            .map_err(backend)?;
        budget.effect();
        Ok(())
    }
    async fn copy(
        &self,
        _effect_id: &EffectId,
        _work: Option<Arc<WorkBudget>>,
        _: &str,
        _: &str,
        _: bool,
        _: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        Err(PortError::Unsupported(
            "copy requires caller-supplied reads and writes",
        ))
    }
    async fn open_reader(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        options: ReaderOptions,
    ) -> Result<FileReader, PortError> {
        let operation = self.operation(work);
        Ok(operation
            .open_reader(effect_id, path, options)
            .await
            .map_err(backend)?)
    }
    async fn open_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<DirectoryReader, PortError> {
        // Enumeration has a fixed 4 MiB encoded ceiling. Admit its worst-case
        // working set before SQL decoding or native enumeration. The guard also
        // covers codec/validation copies until the snapshot has been committed.
        let memory = Arc::new(
            interpreter::runtime::limits::HostBytes::new(
                limits,
                (MAX_LISTING_BYTES as u64) * 8 + 8192,
            )
            .map_err(|error| PortError::Execution(error.into()))?,
        );
        let operation = self.operation(work);
        let resource = effect_id.to_owned();
        let path = path.to_owned();
        finish_owned(async move {
            let _memory = memory.clone();
            operation
                .open_directory_reserved(&resource, &path, recursive, Some(memory))
                .await
                .map(|_| ())
        })
        .await
        .map_err(backend)?;
        Ok(DirectoryReader::new(self.run_id.clone(), effect_id.clone()))
    }
    async fn next_reader(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        reader: &mut FileReader,
        limits: &TenantLimits,
    ) -> Result<ReadOutput, PortError> {
        check_resource(self, &reader.run_id, &reader.id)?;
        self.operation(work)
            .next_reader(&reader.id, effect_id, &reader.options, limits)
            .await
    }
    async fn close_reader(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        reader: &mut FileReader,
    ) -> Result<(), PortError> {
        check_resource(self, &reader.run_id, &reader.id)?;
        self.operation(work)
            .close_reader(&reader.id, effect_id, &reader.options)
            .await
    }
    async fn next_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        directory: &mut DirectoryReader,
        limits: &TenantLimits,
    ) -> Result<Option<DirectoryEntry>, PortError> {
        check_resource(self, &directory.run_id, &directory.id)?;
        self.operation(work)
            .next_listing(&directory.id, effect_id, limits)
            .await
            .map_err(backend)
    }
    async fn close_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        directory: &mut DirectoryReader,
        limits: &TenantLimits,
    ) -> Result<(), PortError> {
        check_resource(self, &directory.run_id, &directory.id)?;
        self.operation(work)
            .close_listing(&directory.id, effect_id, limits)
            .await
            .map(|_| ())
            .map_err(backend)
    }
    async fn write_chunk(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        writer: &mut FileWriter,
        bytes: Vec<u8>,
    ) -> Result<(), PortError> {
        check_resource(self, &writer.run_id, &writer.id)?;
        self.operation(work)
            .write_chunk(&writer.id, effect_id, bytes)
            .await
            .map_err(backend)
    }
    async fn close_writer(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        writer: &mut FileWriter,
    ) -> Result<(), PortError> {
        check_resource(self, &writer.run_id, &writer.id)?;
        let result = self
            .operation(work)
            .close_writer(&writer.id, effect_id)
            .await;
        writer.closed = self
            .stored_writer_is_closed(&writer.id)
            .await
            .map_err(backend)?;
        result.map(|_| ()).map_err(backend)
    }
    async fn open_writer(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
    ) -> Result<FileWriter, PortError> {
        self.operation(work)
            .open_writer(effect_id, path)
            .await
            .map_err(backend)?;
        Ok(FileWriter::new(self.run_id.clone(), effect_id.clone()))
    }
}
pub(crate) fn backend(error: FilesystemError) -> PortError {
    match error {
        FilesystemError::Quota(error) => PortError::Quota(error),
        FilesystemError::Rejected(reason) => PortError::Rejected(reason),
        FilesystemError::Memory(error) => PortError::Execution(error.into()),
        FilesystemError::Recovery(RecoveryError::Fuel(error)) => PortError::Exhausted(error),
        other => PortError::Backend(Box::new(other)),
    }
}

fn check_resource(
    filesystem: &DurableFilesystem,
    run_id: &RunId,
    id: &EffectId,
) -> Result<(), PortError> {
    validate_id(id).map_err(backend)?;
    if run_id != &filesystem.run_id {
        return Err(backend(RecoveryError::Conflict.into()));
    }
    Ok(())
}

/// An unfinished resource open or writer close must finish before unrelated work.
/// The closing writer may enter its own publication effect.
pub(crate) async fn require_ready(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
    writer_close: Option<(&EffectId, &EffectId)>,
) -> Result<(), FilesystemError> {
    if unit.pending_resource(run_id).await?.is_some() {
        return conflict();
    }
    if let Some(writer) = unit.closing_writer(run_id).await?
        && writer_close
            != writer
                .close_effect
                .as_ref()
                .map(|effect| (&writer.id, effect))
    {
        return conflict();
    }
    Ok(())
}
pub(crate) async fn require_idle(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
) -> Result<(), FilesystemError> {
    require_ready(unit, run_id, None).await?;
    if unit.pending_journal_entry(run_id).await?.is_some() {
        return conflict();
    }
    Ok(())
}
pub(crate) async fn require_capacity(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
) -> Result<(), FilesystemError> {
    if unit.journal_entry_count(run_id).await? >= MAX_OPERATIONS {
        return conflict();
    }
    Ok(())
}
pub(crate) async fn require_unused_operation(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
    id: &EffectId,
) -> Result<(), FilesystemError> {
    if unit.get_journal_entry(run_id, id).await?.is_some()
        || unit.writer_for_effect(run_id, id).await?.is_some()
    {
        return conflict();
    }
    Ok(())
}
pub(crate) async fn require_resource_limit(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
) -> Result<(), FilesystemError> {
    let count = unit.resource_counts(run_id).await?;
    if count
        .readers
        .saturating_add(count.listings)
        .saturating_add(count.writers)
        >= MAX_OPERATIONS
    {
        return conflict();
    }
    Ok(())
}
pub(crate) fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, FilesystemError> {
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > crate::codec::MAX_RECORD {
        return Err(invalid("filesystem record exceeds limit"));
    }
    Ok(bytes)
}
pub(crate) fn invalid(error: impl std::fmt::Display) -> FilesystemError {
    StoreError::Io(error.to_string()).into()
}
pub(crate) fn conflict<T>() -> Result<T, FilesystemError> {
    Err(StoreError::Conflict.into())
}

/// Refuse a state recorded for a different format or filesystem configuration.
pub(crate) fn validate_journal_configuration(
    journal_entry: &JournalEntry,
    filesystem_config_fingerprint: [u8; 32],
) -> Result<(), FilesystemError> {
    if journal_entry.version != 4
        || journal_entry.request.len() != 64
        || journal_entry.request.get(..32) != Some(filesystem_config_fingerprint.as_slice())
    {
        return conflict();
    }
    Ok(())
}

fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, FilesystemError> {
    if bytes.len() > crate::codec::MAX_RECORD {
        return Err(RecoveryError::Limit.into());
    }
    serde_json::from_slice(bytes).map_err(invalid)
}
pub(crate) fn fingerprint(id: &EffectId, bytes: &[u8]) -> Result<Content, RecoveryError> {
    paths::validate_id(id)?;
    Ok(Content {
        id: id.clone(),
        bytes: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    })
}

use cap_std::fs::{Dir, File};
use std::io::Write;

fn require_empty(dir: &Dir) -> Result<(), RecoveryError> {
    if let Some(entry) = dir.entries()?.next() {
        entry?;
        return Err(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty).into());
    }
    Ok(())
}

fn buffer() -> Result<Vec<u8>, RecoveryError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(64 * 1024)
        .map_err(|_| RecoveryError::Limit)?;
    buffer.resize(64 * 1024, 0);
    Ok(buffer)
}

fn hash(file: &mut File, bytes: u64) -> Result<[u8; 32], RecoveryError> {
    file.seek(SeekFrom::Start(0))?;
    let mut remaining = bytes;
    let mut digest = Sha256::new();
    let mut buffer = buffer()?;
    while remaining != 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(digest.finalize().into())
}

fn copy_exact(
    source: &mut dyn Read,
    file: &mut File,
    payload: &Content,
) -> Result<(), RecoveryError> {
    let mut remaining = payload.bytes;
    let mut digest = Sha256::new();
    let mut buffer = buffer()?;
    while remaining != 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
        // Fault tests can stop between chunks, exercising partial append.
        file.write_all(&buffer[..count])?;
        remaining -= count as u64;
        paths::test_checkpoint("write-chunk")?;
    }
    let mut trailing = [0];
    if source.read(&mut trailing)? != 0 || <[u8; 32]>::from(digest.finalize()) != payload.sha256 {
        return Err(RecoveryError::Conflict);
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
pub(crate) mod tests;
