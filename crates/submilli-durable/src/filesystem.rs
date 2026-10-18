//! Shared filesystem state and the interpreter-facing filesystem implementation.
//!
//! The trait methods create a per-call `FilesystemOperation`. The lower half of
//! this file contains shared evidence helpers: they compare recorded file
//! identities with disk state to distinguish an unfinished mutation from one
//! that already happened. They never select or schedule another effect.
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
mod write_bytes;
mod write_chunk;
use crate::io::{self as paths, Entry, Files, RecoveryError};
use crate::readers::{DurableDirectory, FilesystemReader, MAX_LISTING_BYTES};
use crate::unit_of_work::*;
use crate::writers::DurableWriter;
use crate::{UnitFactory, UnitOfWork};
use interpreter::runtime::Vfs;
use interpreter::runtime::durable::{EffectId, RunId};
use interpreter::runtime::filesystem::{
    DirectoryReader, FileReader, FileWriter, Filesystem, FilesystemError as PortError,
    FilesystemInfo, ReaderOptions, ResourceDescriptor, ResourceKind,
};
use interpreter::runtime::fs::FileIdentity;
use interpreter::runtime::fuel::WorkBudget;
use interpreter::runtime::{fs, vfs::Placement};
pub use operation::FilesystemOperation;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
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
        let vfs = paths::share_quotas(&filesystem_quotas, vfs).await?;
        let filesystem_config_fingerprint = paths::filesystem_config_fingerprint(&vfs)?;
        let run_id = run_id.clone();
        let worker_id = run_id.clone();
        let workspace = blocking(gate, move || Files::create(vfs, &worker_id)).await?;
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
    fn placement(&self, path: &str) -> Result<Placement, PortError> {
        Ok(paths::resolve_content(self.files.vfs(), self.cwd(), path)?
            .placement()
            .clone())
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
                paths::resolve_content(&vfs, vfs.cwd(), &path)?.metadata()
            } else {
                paths::check_access(&vfs, vfs.cwd(), &path)?;
                fs::resolve_link(&vfs, vfs.cwd(), &path)?.symlink_metadata()
            }
        })
        .await
        .map_err(|error| PortError::Backend(Box::new(error)))?
        .map_err(Into::into)
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
        append: bool,
    ) -> Result<(), PortError> {
        let operation = self.operation(work);
        operation
            .charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)
            .map_err(backend)?;
        if append {
            operation.append_bytes(effect_id, path, bytes).await
        } else {
            operation.write_bytes(effect_id, path, bytes).await
        }
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
    ) -> Result<Box<dyn FileReader>, PortError> {
        let operation = self.operation(work);
        Ok(Box::new(
            operation
                .open_reader(effect_id, path, options)
                .await
                .map_err(backend)?,
        ))
    }
    async fn open_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<Box<dyn DirectoryReader>, PortError> {
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
        Ok(Box::new(DurableDirectory {
            filesystem: self.clone(),
            resource: effect_id.clone(),
        }))
    }
    fn restore_reader(
        &self,
        descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn FileReader>, PortError> {
        check_descriptor(self, &descriptor)?;
        let ResourceKind::Reader(options) = descriptor.kind else {
            return Err(backend(RecoveryError::Conflict.into()));
        };
        Ok(Box::new(FilesystemReader {
            filesystem: self.clone(),
            resource: descriptor.id,
            options,
        }))
    }
    fn restore_directory(
        &self,
        descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn DirectoryReader>, PortError> {
        check_descriptor(self, &descriptor)?;
        if descriptor.kind != ResourceKind::Directory {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        Ok(Box::new(DurableDirectory {
            filesystem: self.clone(),
            resource: descriptor.id,
        }))
    }
    fn restore_writer(
        &self,
        descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn FileWriter>, PortError> {
        check_descriptor(self, &descriptor)?;
        if descriptor.kind != ResourceKind::Writer {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        Ok(Box::new(DurableWriter {
            filesystem: self.clone(),
            resource: descriptor.id,
            closed: false,
        }))
    }
    async fn open_writer(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, PortError> {
        self.operation(work)
            .open_writer(effect_id, path)
            .await
            .map_err(backend)?;
        Ok(Box::new(DurableWriter {
            filesystem: self.clone(),
            resource: effect_id.clone(),
            closed: false,
        }))
    }
}
pub(crate) fn backend(error: FilesystemError) -> PortError {
    match error {
        FilesystemError::Rejected(reason) => PortError::Rejected(reason),
        FilesystemError::Memory(error) => PortError::Execution(error.into()),
        FilesystemError::Recovery(RecoveryError::Fuel(error)) => PortError::Exhausted(error),
        other => PortError::Backend(Box::new(other)),
    }
}

fn check_descriptor(
    filesystem: &DurableFilesystem,
    descriptor: &ResourceDescriptor,
) -> Result<(), PortError> {
    validate_id(&descriptor.id).map_err(backend)?;
    if descriptor.run_id != filesystem.run_id {
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

// These records describe the original disk state; they are not executable plans.
// The operation code decides what to do from these facts on every attempt.
/// The original entry and append baseline, captured before changing its name or bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    source: Entry,
    original: Option<FileIdentity>,
    original_bytes: u64,
    original_sha256: Option<[u8; 32]>,
    original_directory: bool,
}
/// Replacement writes retain a candidate inode; existing-file appends use the baseline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteCheckpoint {
    target: Target,
    candidate: Option<FileIdentity>,
}
impl Target {
    fn validate(&self) -> Result<(), RecoveryError> {
        self.source.validate()?;
        if self.original_bytes > i64::MAX as u64
            || (self.original_directory
                && (self.original.is_none() || self.original_sha256.is_some()))
            || (self.original.is_some()
                && !self.original_directory
                && self.original_sha256.is_none())
        {
            return Err(RecoveryError::InvalidRecord);
        }
        Ok(())
    }
}

/// Return a saved call result, keeping invalid records in the outer error.
/// A saved rejection is still a completed call, including for writer cleanup.
pub(crate) fn decode_outcome(
    journal_entry: &JournalEntry,
    filesystem_config_fingerprint: [u8; 32],
) -> Result<Option<Result<Outcome, String>>, FilesystemError> {
    if journal_entry.version != 2
        || journal_entry.request.len() != 64
        || journal_entry.request.get(..32) != Some(filesystem_config_fingerprint.as_slice())
    {
        return conflict();
    }
    let outcome: Option<RecordedOutcome> = journal_entry
        .outcome
        .as_deref()
        .map(decode_checkpoint)
        .transpose()?;
    if outcome
        .as_ref()
        .is_some_and(|o| matches!(o, RecordedOutcome::Rejected(_)) != journal_entry.rejected)
    {
        return conflict();
    }
    Ok(outcome.map(|outcome| match outcome {
        RecordedOutcome::Completed(outcome) => Ok(outcome),
        RecordedOutcome::Rejected(reason) => Err(reason),
    }))
}
fn decode_checkpoint<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, FilesystemError> {
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

use crate::io::{current, existing, identity, sync};
use cap_std::fs::{Dir, File, OpenOptions};
use std::io::Write;
#[derive(Default)]
struct CaptureOptions {
    follow_final: bool,
    allow_directory: bool,
    require_empty: bool,
}

impl Files {
    fn capture_target(
        &self,
        effect: &EffectId,
        path: &str,
        options: CaptureOptions,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<Target, RecoveryError> {
        let source = self.entry(path, options.follow_final)?;
        let parent = self.parent(&source)?;
        let metadata = existing(&parent, &source.name)?;
        let original_directory = metadata
            .as_ref()
            .is_some_and(|m| m.is_dir() && !m.file_type().is_symlink());
        if let Some(metadata) = &metadata {
            if original_directory && options.allow_directory {
                if options.require_empty {
                    require_empty(&parent.open_dir(&source.name)?)?;
                }
            } else {
                self.require_regular(&source, metadata, work)?;
            }
        }
        let artifacts = self.artifacts(&source, effect, true)?;
        let mut target = Target {
            original: metadata.as_ref().map(identity).transpose()?,
            original_bytes: metadata
                .as_ref()
                .filter(|m| m.is_file())
                .map_or(0, cap_std::fs::Metadata::len),
            original_sha256: None,
            original_directory,
            source,
        };
        if original_directory {
            sync(&parent.open_dir(&target.source.name)?)?;
            sync(&parent)?;
        } else if let Some(original) = target.original {
            let mut file = parent.open(&target.source.name)?;
            file.sync_all()?;
            sync(&parent)?;
            target.original_sha256 = Some(hash(&mut file, target.original_bytes)?);
            pin(
                &parent,
                &target.source.name,
                &artifacts,
                "original",
                original,
            )?;
        }
        sync(&artifacts)?;
        self.reconcile_quotas(work)?;
        Ok(target)
    }
    fn prepare_candidate(
        &self,
        entry: &Entry,
        artifacts: &Dir,
        payload: &Content,
        content: &mut dyn Read,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<FileIdentity, RecoveryError> {
        let present = existing(artifacts, "candidate")?;
        if present
            .as_ref()
            .is_some_and(|m| !m.is_file() || interpreter::runtime::fs::link_count(m) != Some(1))
        {
            return Err(RecoveryError::Conflict);
        }
        self.volume(&entry.mount)?.check_growth(
            payload.bytes.saturating_sub(present.map_or(0, |m| m.len())),
            work,
        )?;
        let mut file = artifacts.open_with(
            "candidate",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true),
        )?;
        copy_exact(content, &mut file, payload)?;
        file.sync_all()?;
        paths::test_checkpoint("candidate-sync")?;
        sync(artifacts)?;
        identity(&file.metadata()?)
    }

    /// Publish the prepared inode, or recognize it at the destination on retry.
    /// A third inode is a conflict: retry must not overwrite someone else's file.
    fn publish_candidate(
        &self,
        state: &WriteCheckpoint,
        artifacts: &Dir,
        payload: &Content,
        content: &mut dyn Read,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<(), RecoveryError> {
        let target = &state.target;
        let candidate = state.candidate.ok_or(RecoveryError::InvalidRecord)?;
        if current(artifacts, "candidate")? != Some(candidate) {
            return Err(RecoveryError::Conflict);
        }
        let parent = self.parent(&target.source)?;
        let present = current(&parent, &target.source.name)?;
        if present != target.original && present != Some(candidate) {
            return Err(RecoveryError::Conflict);
        }
        if let Some(original) = target.original {
            require_pin(artifacts, "original", original)?;
        }
        // An uncommitted candidate may have suffered dirty writeback. Repair the
        // same inode, including when the rename reached disk before the receipt.
        let mut file =
            artifacts.open_with("candidate", OpenOptions::new().read(true).write(true))?;
        let length = file.metadata()?.len();
        if length != payload.bytes || hash(&mut file, length)? != payload.sha256 {
            self.volume(&target.source.mount)?
                .check_growth(payload.bytes.saturating_sub(length), work)?;
            file.seek(SeekFrom::Start(0))?;
            copy_exact(content, &mut file, payload)?;
            file.set_len(payload.bytes)?;
        }
        file.sync_all()?;
        paths::test_checkpoint("candidate-sync")?;
        if present != Some(candidate) {
            pin(artifacts, "candidate", artifacts, "publish", candidate)?;
            artifacts.rename("publish", &parent, &target.source.name)?;
            paths::test_checkpoint("published")?;
        }
        sync(&parent)?;
        sync(artifacts)?;
        Ok(())
    }
}

fn require_empty(dir: &Dir) -> Result<(), RecoveryError> {
    if let Some(entry) = dir.entries()?.next() {
        entry?;
        return Err(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty).into());
    }
    Ok(())
}

fn pin(
    source: &Dir,
    name: &str,
    artifacts: &Dir,
    retained: &str,
    expected: FileIdentity,
) -> Result<(), RecoveryError> {
    if current(source, name)? != Some(expected) {
        return Err(RecoveryError::Conflict);
    }
    match current(artifacts, retained)? {
        Some(actual) if actual != expected => return Err(RecoveryError::Conflict),
        Some(_) => {}
        None => source.hard_link(name, artifacts, retained)?,
    }
    paths::test_checkpoint("pin-created")?;
    sync(artifacts)?;
    Ok(())
}

fn require_pin(artifacts: &Dir, name: &str, expected: FileIdentity) -> Result<(), RecoveryError> {
    if current(artifacts, name)? != Some(expected) {
        return Err(RecoveryError::Conflict);
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
