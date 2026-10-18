//! Direct, idempotent filesystem operations. Only the caller retries pending work.
pub use crate::io::FilesystemContext;
use crate::io::{self as paths, Entry, Files, RecoveryError, validate_guest_path};
use crate::readers::{DurableDirectory, FilesystemReader, MAX_LISTING_BYTES};
use crate::unit_of_work::*;
use crate::writers::DurableWriter;
use crate::{UnitFactory, UnitOfWork};
use interpreter::runtime::Vfs;
use interpreter::runtime::durable::{EffectId, RunId};
use interpreter::runtime::filesystem::{
    DirectoryReader, FileReader, FileWriter, Filesystem, FilesystemError as PortError,
    FilesystemInfo, Invocation, ReaderOptions, ResourceDescriptor, ResourceKind,
};
use interpreter::runtime::fs::FileIdentity;
use interpreter::runtime::fuel::WorkBudget;
use interpreter::runtime::{fs, vfs::Placement};
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

#[derive(Clone)]
pub struct DurableFilesystem {
    pub(crate) files: Arc<Files>,
    pub(crate) units: UnitFactory,
    pub(crate) binding: [u8; 32],
    pub(crate) context: FilesystemContext,
    pub(crate) run_id: RunId,
}

impl DurableFilesystem {
    /// The caller supplies the configured mounts and a stable UUID run ID for
    /// effect IDs. It must retain these roots and this journal across retries,
    /// exclusively own writable roots, and drain work before replacing access.
    /// `context` shares mutation admission and disk quotas across these instances.
    pub async fn open(
        vfs: Vfs,
        run_id: &RunId,
        units: UnitFactory,
        context: FilesystemContext,
    ) -> Result<Self, FilesystemError> {
        validate_id(run_id)?;
        let gate = Arc::new(context.filesystem_gate.clone().lock_owned().await);
        let vfs = paths::share_quotas(&context, vfs).await?;
        let binding = paths::binding(&vfs)?;
        let run_id = run_id.clone();
        let worker_id = run_id.clone();
        let workspace = blocking(gate, move || Files::create(vfs, &worker_id)).await?;
        Ok(Self {
            files: Arc::new(workspace),
            units,
            binding,
            context,
            run_id,
        })
    }

    #[cfg(feature = "test-support")]
    pub fn test_context(&self) -> &FilesystemContext {
        &self.context
    }
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
    pub fn vfs(&self) -> &Vfs {
        self.files.vfs()
    }
}
pub(crate) async fn blocking<T: Send + 'static>(
    guard: Arc<tokio::sync::OwnedMutexGuard<()>>,
    operation: impl FnOnce() -> Result<T, RecoveryError> + Send + 'static,
) -> Result<T, FilesystemError> {
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        operation()
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

pub(crate) fn unpack(outcome: RecordedOutcome) -> Result<Outcome, FilesystemError> {
    match outcome {
        RecordedOutcome::Completed(outcome) => Ok(outcome),
        RecordedOutcome::Rejected(reason) => Err(FilesystemError::Rejected(reason)),
    }
}

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

pub(crate) fn checkpoint(_point: &str) {
    crate::checkpoint(_point);
}

/// One call's accounting, separate from the shared filesystem and durable handles.
#[derive(Clone)]
pub struct FilesystemOperation {
    pub(crate) filesystem: DurableFilesystem,
    pub(crate) work: Option<Arc<WorkBudget>>,
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

impl FilesystemOperation {
    pub(crate) fn charge(
        &self,
        rate: interpreter::runtime::fuel::Rate,
        count: u64,
    ) -> Result<(), FilesystemError> {
        if let Some(work) = &self.work {
            work.charge(rate, count).map_err(RecoveryError::from)?;
        }
        Ok(())
    }
    fn charge_preparation(&self, path: &str, payload: u64) -> Result<(), FilesystemError> {
        use interpreter::runtime::fuel;
        if self.work.is_none() {
            return Ok(());
        }
        let original =
            paths::resolve_content(self.filesystem.vfs(), self.filesystem.vfs().cwd(), path)
                .and_then(|p| p.metadata())
                .ok()
                .filter(cap_std::fs::Metadata::is_file)
                .map_or(0, |m| m.len());
        let bytes = original
            .saturating_mul(2)
            .saturating_add(payload.saturating_mul(3));
        self.charge(fuel::HASH, bytes)?;
        self.charge(fuel::IO, bytes)
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
        _invocation: &Invocation,
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
        _invocation: &Invocation,
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
        invocation: &Invocation,
        path: &str,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<(), PortError> {
        let effect_id = require_effect_id(invocation)?;
        let operation = self.operation(invocation.work.clone());
        operation
            .charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)
            .map_err(backend)?;
        operation
            .write_bytes(effect_id, path, bytes, append)
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn mkdir(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<(), PortError> {
        let effect_id = require_effect_id(invocation)?;
        let operation = self.operation(invocation.work.clone());
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
        invocation: &Invocation,
        path: &str,
        recursive: bool,
        budget: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        let effect_id = require_effect_id(invocation)?;
        let operation = self.operation(invocation.work.clone());
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
        invocation: &Invocation,
        from: &str,
        to: &str,
        budget: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        let effect_id = require_effect_id(invocation)?;
        let operation = self.operation(invocation.work.clone());
        operation
            .move_path(effect_id, from, to)
            .await
            .map_err(backend)?;
        budget.effect();
        Ok(())
    }
    async fn copy(
        &self,
        _: &Invocation,
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
        invocation: &Invocation,
        path: &str,
        options: ReaderOptions,
    ) -> Result<Box<dyn FileReader>, PortError> {
        let operation = self.operation(invocation.work.clone());
        Ok(Box::new(
            operation
                .open_reader(require_effect_id(invocation)?, path, options)
                .await
                .map_err(backend)?,
        ))
    }
    async fn open_directory(
        &self,
        invocation: &Invocation,
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
        let effect_id = require_effect_id(invocation)?;
        let operation = self.operation(invocation.work.clone());
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
        invocation: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, PortError> {
        let effect_id = require_effect_id(invocation)?;
        self.operation(invocation.work.clone())
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
pub(crate) fn require_effect_id(invocation: &Invocation) -> Result<&EffectId, PortError> {
    invocation.operation.as_ref().ok_or_else(|| {
        PortError::Backend(Box::new(FilesystemError::Configuration(
            "durable filesystem operation requires a stable operation identity",
        )))
    })
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
    if unit.pending_operation(run_id).await?.is_some() {
        return conflict();
    }
    Ok(())
}
pub(crate) async fn require_capacity(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
) -> Result<(), FilesystemError> {
    if unit.operation_count(run_id).await? >= MAX_OPERATIONS {
        return conflict();
    }
    Ok(())
}
pub(crate) async fn require_unused_operation(
    unit: &mut dyn UnitOfWork,
    run_id: &RunId,
    id: &EffectId,
) -> Result<(), FilesystemError> {
    if unit.get_operation(run_id, id).await?.is_some()
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

// These records contain only the facts a retry cannot rediscover safely.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    source: Entry,
    original: Option<FileIdentity>,
    original_bytes: u64,
    original_sha256: Option<[u8; 32]>,
    original_directory: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteCheckpoint {
    target: Target,
    candidate: Option<FileIdentity>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveCheckpoint {
    target: Target,
    destination: Entry,
    destination_original: Option<FileIdentity>,
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

impl FilesystemOperation {
    pub async fn write_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<Outcome, FilesystemError> {
        let payload = fingerprint(effect, &bytes)?;
        let gate = Arc::new(
            self.filesystem
                .context
                .filesystem_gate
                .clone()
                .lock_owned()
                .await,
        );
        self.write_held(
            gate,
            effect,
            path,
            (payload, std::io::Cursor::new(bytes)),
            append,
            None,
        )
        .await
    }

    pub(crate) async fn write_held<R: Read + Seek + Send + 'static>(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        effect: &EffectId,
        path: &str,
        (payload, mut input): (Content, R),
        append: bool,
        writer: Option<&EffectId>,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(path)?;
        let mut unit = (self.filesystem.units)().await?;
        let row = self
            .attach(
                unit.as_mut(),
                effect,
                if append { "append" } else { "write" },
                &(path, &payload),
                writer,
            )
            .await?;
        unit.commit().await?;
        if let Some(outcome) = decode_outcome(&row, self.filesystem.binding)? {
            return unpack(outcome);
        }
        self.charge_preparation(path, payload.bytes)?;
        checkpoint("effect-request");
        let state: WriteCheckpoint = if let Some(bytes) = &row.recovery_payload {
            decode_checkpoint(bytes)?
        } else {
            let files = self.filesystem.files.clone();
            let work = self.work.clone();
            let (path, effect, payload) = (path.to_owned(), effect.to_owned(), payload.clone());
            let result = blocking(gate.clone(), move || {
                let prepared = (|| {
                    let target = files.capture_target(
                        &effect,
                        &path,
                        CaptureOptions {
                            follow_final: append,
                            ..Default::default()
                        },
                        work.as_deref(),
                    )?;
                    let artifacts = files.artifacts(&target.source, &effect, true)?;
                    let candidate = if append && target.original.is_some() {
                        target
                            .original_bytes
                            .checked_add(payload.bytes)
                            .filter(|n| *n <= i64::MAX as u64)
                            .ok_or(RecoveryError::Limit)?;
                        files
                            .volume(&target.source.mount)?
                            .check_growth(payload.bytes, work.as_deref())?;
                        None
                    } else {
                        Some(files.prepare_candidate(
                            &target.source,
                            &artifacts,
                            &payload,
                            &mut input,
                            work.as_deref(),
                        )?)
                    };
                    paths::sync(&artifacts)?;
                    paths::checkpoint("prepared")?;
                    Ok(WriteCheckpoint { target, candidate })
                })();
                files.reconcile_quotas(work.as_deref())?;
                prepared.map(|state| (state, input))
            })
            .await;
            let (state, returned_input) = match result {
                Ok(value) => value,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    return self.refuse(&row, error).await;
                }
                Err(error) => return Err(error),
            };
            input = returned_input;
            self.save_checkpoint(&row, &state).await?;
            state
        };
        state.target.validate()?;
        if state.target.original_directory
            || (!append && state.candidate.is_none())
            || (state.target.original.is_none() && state.candidate.is_none())
        {
            return Err(RecoveryError::InvalidRecord.into());
        }
        if let Some(work) = &self.work {
            work.before_effect();
        }
        let files = self.filesystem.files.clone();
        let work = self.work.clone();
        let effect = effect.to_owned();
        let bytes = payload.bytes;
        blocking(gate.clone(), move || {
            input.seek(SeekFrom::Start(0))?;
            let artifacts = files.artifacts(&state.target.source, &effect, false)?;
            let result = if append && state.target.original.is_some() {
                files.append_range(
                    &state.target,
                    &artifacts,
                    &payload,
                    &mut input,
                    work.as_deref(),
                )
            } else {
                files.publish_candidate(&state, &artifacts, &payload, &mut input, work.as_deref())
            };
            files.reconcile_quotas(work.as_deref())?;
            result?;
            paths::checkpoint("before-receipt")
        })
        .await?;
        self.record_success(&row, bytes).await
    }

    pub async fn move_path(
        &self,
        effect: &EffectId,
        source: &str,
        destination: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(source)?;
        validate_guest_path(destination)?;
        let gate = Arc::new(
            self.filesystem
                .context
                .filesystem_gate
                .clone()
                .lock_owned()
                .await,
        );
        let mut unit = (self.filesystem.units)().await?;
        let row = self
            .attach(unit.as_mut(), effect, "move", &(source, destination), None)
            .await?;
        unit.commit().await?;
        if let Some(outcome) = decode_outcome(&row, self.filesystem.binding)? {
            return unpack(outcome);
        }
        self.charge_preparation(source, 0)?;
        checkpoint("effect-request");
        let state: MoveCheckpoint = if let Some(bytes) = &row.recovery_payload {
            decode_checkpoint(bytes)?
        } else {
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
                let (destination, destination_original) = files.capture_destination(
                    &target,
                    &artifacts,
                    &destination,
                    work.as_deref(),
                )?;
                paths::sync(&artifacts)?;
                paths::checkpoint("prepared")?;
                Ok(MoveCheckpoint {
                    target,
                    destination,
                    destination_original,
                })
            })
            .await;
            let state = match result {
                Ok(state) => state,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    return self.refuse(&row, error).await;
                }
                Err(error) => return Err(error),
            };
            self.save_checkpoint(&row, &state).await?;
            state
        };
        state.target.validate()?;
        state.destination.validate()?;
        if state.target.original.is_none()
            || state.target.original_directory
            || state.destination.mount != state.target.source.mount
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
            let artifacts = files.artifacts(&state.target.source, &effect, false)?;
            files.move_entry(&state, &artifacts)?;
            files.reconcile_quotas(work.as_deref())?;
            paths::checkpoint("before-receipt")
        })
        .await?;
        self.record_success(&row, 0).await
    }

    pub async fn remove_path(
        &self,
        effect: &EffectId,
        path: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(path)?;
        let gate = Arc::new(
            self.filesystem
                .context
                .filesystem_gate
                .clone()
                .lock_owned()
                .await,
        );
        let mut unit = (self.filesystem.units)().await?;
        let row = self
            .attach(unit.as_mut(), effect, "remove", &path, None)
            .await?;
        unit.commit().await?;
        if let Some(outcome) = decode_outcome(&row, self.filesystem.binding)? {
            return unpack(outcome);
        }
        self.charge_preparation(path, 0)?;
        checkpoint("effect-request");
        let target: Target = if let Some(bytes) = &row.recovery_payload {
            decode_checkpoint(bytes)?
        } else {
            let files = self.filesystem.files.clone();
            let work = self.work.clone();
            let (path, effect) = (path.to_owned(), effect.to_owned());
            let result = blocking(gate.clone(), move || {
                let target = files.capture_target(
                    &effect,
                    &path,
                    CaptureOptions {
                        allow_directory: true,
                        require_empty: true,
                        ..Default::default()
                    },
                    work.as_deref(),
                )?;
                if target.original.is_none() {
                    return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
                }
                paths::checkpoint("prepared")?;
                Ok(target)
            })
            .await;
            let target = match result {
                Ok(target) => target,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    return self.refuse(&row, error).await;
                }
                Err(error) => return Err(error),
            };
            self.save_checkpoint(&row, &target).await?;
            target
        };
        target.validate()?;
        if let Some(work) = &self.work {
            work.before_effect();
        }
        let files = self.filesystem.files.clone();
        let work = self.work.clone();
        let effect = effect.to_owned();
        blocking(gate.clone(), move || {
            if target.original_directory {
                files.remove_directory(&target)?;
            } else {
                let artifacts = files.artifacts(&target.source, &effect, false)?;
                files.remove_entry(&target, &artifacts)?;
            }
            files.reconcile_quotas(work.as_deref())?;
            paths::checkpoint("before-receipt")
        })
        .await?;
        self.record_success(&row, 0).await
    }

    pub async fn create_directory(
        &self,
        effect: &EffectId,
        path: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        validate_guest_path(path)?;
        let gate = Arc::new(
            self.filesystem
                .context
                .filesystem_gate
                .clone()
                .lock_owned()
                .await,
        );
        let mut unit = (self.filesystem.units)().await?;
        let row = self
            .attach(unit.as_mut(), effect, "mkdir", &path, None)
            .await?;
        unit.commit().await?;
        if let Some(outcome) = decode_outcome(&row, self.filesystem.binding)? {
            return unpack(outcome);
        }
        self.charge_preparation(path, 0)?;
        checkpoint("effect-request");
        let target: Target = if let Some(bytes) = &row.recovery_payload {
            decode_checkpoint(bytes)?
        } else {
            let files = self.filesystem.files.clone();
            let work = self.work.clone();
            let (path, effect) = (path.to_owned(), effect.to_owned());
            let result = blocking(gate.clone(), move || {
                let target = files.capture_target(
                    &effect,
                    &path,
                    CaptureOptions {
                        allow_directory: true,
                        ..Default::default()
                    },
                    work.as_deref(),
                )?;
                if target.original.is_some() && !target.original_directory {
                    return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
                }
                paths::checkpoint("prepared")?;
                Ok(target)
            })
            .await;
            let target = match result {
                Ok(target) => target,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    return self.refuse(&row, error).await;
                }
                Err(error) => return Err(error),
            };
            self.save_checkpoint(&row, &target).await?;
            target
        };
        target.validate()?;
        if let Some(work) = &self.work {
            work.before_effect();
        }
        let files = self.filesystem.files.clone();
        let work = self.work.clone();
        blocking(gate.clone(), move || {
            files.create_directory(&target)?;
            files.reconcile_quotas(work.as_deref())?;
            paths::checkpoint("before-receipt")
        })
        .await?;
        self.record_success(&row, 0).await
    }

    // Admission helpers operate on a caller-owned transaction; no IO is hidden here.
    async fn attach<T: Serialize>(
        &self,
        unit: &mut dyn UnitOfWork,
        effect: &EffectId,
        kind: &str,
        request: &T,
        writer: Option<&EffectId>,
    ) -> Result<Operation, FilesystemError> {
        let mut fingerprint = self.filesystem.binding.to_vec();
        fingerprint.extend_from_slice(&Sha256::digest(serialize(request)?));
        if let Some(row) = unit.get_operation(&self.filesystem.run_id, effect).await? {
            if row.operation != kind || row.version != 2 || row.request != fingerprint {
                return conflict();
            }
            return Ok(row);
        }
        require_ready(
            unit,
            &self.filesystem.run_id,
            writer.map(|writer| (writer, effect)),
        )
        .await?;
        require_capacity(unit, &self.filesystem.run_id).await?;
        if unit
            .pending_operation(&self.filesystem.run_id)
            .await?
            .is_some()
        {
            return conflict();
        }
        if let Some(owner) = unit
            .writer_for_effect(&self.filesystem.run_id, effect)
            .await?
            && writer != Some(&owner.id)
        {
            return conflict();
        }
        let row = Operation {
            id: effect.clone(),
            run_id: self.filesystem.run_id.clone(),
            operation: kind.into(),
            version: 2,
            request: fingerprint,
            recovery_payload: None,
            outcome: None,
            rejected: false,
        };
        unit.save_operation(row.clone()).await?;
        Ok(row)
    }
    async fn save_checkpoint<T: Serialize>(
        &self,
        admitted: &Operation,
        state: &T,
    ) -> Result<(), FilesystemError> {
        let mut unit = (self.filesystem.units)().await?;
        let mut row = unit
            .get_operation(&self.filesystem.run_id, &admitted.id)
            .await?
            .ok_or(StoreError::Conflict)?;
        if row.request != admitted.request
            || row.operation != admitted.operation
            || row.outcome.is_some()
        {
            return conflict();
        }
        row.recovery_payload = Some(serialize(state)?);
        unit.save_operation(row).await?;
        unit.commit().await?;
        checkpoint("effect-intent");
        Ok(())
    }
    async fn record_success(
        &self,
        admitted: &Operation,
        bytes: u64,
    ) -> Result<Outcome, FilesystemError> {
        checkpoint("effect-applied");
        let outcome = Outcome { bytes };
        self.save_outcome(admitted, RecordedOutcome::Completed(outcome.clone()))
            .await?;
        checkpoint("effect-receipt");
        Ok(outcome)
    }
    async fn refuse(
        &self,
        admitted: &Operation,
        error: RecoveryError,
    ) -> Result<Outcome, FilesystemError> {
        let outcome = RecordedOutcome::Rejected(error.to_string());
        self.save_outcome(admitted, outcome.clone()).await?;
        unpack(outcome)
    }
    async fn save_outcome(
        &self,
        admitted: &Operation,
        outcome: RecordedOutcome,
    ) -> Result<(), FilesystemError> {
        let mut unit = (self.filesystem.units)().await?;
        let mut row = unit
            .get_operation(&self.filesystem.run_id, &admitted.id)
            .await?
            .ok_or(StoreError::Conflict)?;
        if row.request != admitted.request || row.operation != admitted.operation {
            return conflict();
        }
        row.rejected = matches!(outcome, RecordedOutcome::Rejected(_));
        row.outcome = Some(serialize(&outcome)?);
        unit.save_operation(row).await?;
        unit.commit().await?;
        Ok(())
    }
}
pub(crate) fn decode_outcome(
    row: &Operation,
    binding: [u8; 32],
) -> Result<Option<RecordedOutcome>, FilesystemError> {
    if row.version != 2
        || row.request.len() != 64
        || row.request.get(..32) != Some(binding.as_slice())
    {
        return conflict();
    }
    let outcome: Option<RecordedOutcome> =
        row.outcome.as_deref().map(decode_checkpoint).transpose()?;
    if outcome
        .as_ref()
        .is_some_and(|o| matches!(o, RecordedOutcome::Rejected(_)) != row.rejected)
    {
        return conflict();
    }
    Ok(outcome)
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
        paths::checkpoint("candidate-sync")?;
        sync(artifacts)?;
        identity(&file.metadata()?)
    }

    fn capture_destination(
        &self,
        target: &Target,
        artifacts: &Dir,
        guest: &str,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<(Entry, Option<FileIdentity>), RecoveryError> {
        if target.original.is_none() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        let destination = self.entry(guest, false)?;
        if destination.mount != target.source.mount {
            return Err(RecoveryError::Unsupported(
                "cross-volume move requires caller-orchestrated copy and remove",
            ));
        }
        if destination == target.source {
            return Err(RecoveryError::Unsupported("move source equals destination"));
        }
        let parent = self.parent(&destination)?;
        let mut destination_original = None;
        if let Some(metadata) = existing(&parent, &destination.name)? {
            self.require_regular(&destination, &metadata, work)?;
            let original = identity(&metadata)?;
            if Some(original) == target.original {
                return Err(RecoveryError::Unsupported("aliased move destination"));
            }
            parent.open(&destination.name)?.sync_all()?;
            sync(&parent)?;
            pin(
                &parent,
                &destination.name,
                artifacts,
                "destination",
                original,
            )?;
            destination_original = Some(original);
        }
        Ok((destination, destination_original))
    }

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
        paths::checkpoint("candidate-sync")?;
        if present != Some(candidate) {
            pin(artifacts, "candidate", artifacts, "publish", candidate)?;
            artifacts.rename("publish", &parent, &target.source.name)?;
            paths::checkpoint("published")?;
        }
        sync(&parent)?;
        sync(artifacts)?;
        Ok(())
    }

    fn append_range(
        &self,
        target: &Target,
        artifacts: &Dir,
        payload: &Content,
        content: &mut dyn Read,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<(), RecoveryError> {
        let original = target.original.ok_or(RecoveryError::InvalidRecord)?;
        require_pin(artifacts, "original", original)?;
        let parent = self.parent(&target.source)?;
        if current(&parent, &target.source.name)? != Some(original) {
            return Err(RecoveryError::Conflict);
        }
        let final_length = target
            .original_bytes
            .checked_add(payload.bytes)
            .filter(|bytes| *bytes <= i64::MAX as u64)
            .ok_or(RecoveryError::Limit)?;
        // Deliberately omit O_APPEND: every retry repairs the original interval.
        let mut file =
            artifacts.open_with("original", OpenOptions::new().read(true).write(true))?;
        let length = file.metadata()?.len();
        if length < target.original_bytes
            || length > final_length
            || Some(hash(&mut file, target.original_bytes)?) != target.original_sha256
        {
            return Err(RecoveryError::Conflict);
        }
        self.volume(&target.source.mount)?
            .check_growth(final_length - length, work)?;
        file.seek(SeekFrom::Start(target.original_bytes))?;
        copy_exact(content, &mut file, payload)?;
        file.set_len(final_length)?;
        paths::checkpoint("append-written")?;
        file.sync_all()?;
        paths::checkpoint("append-sync")?;
        sync(&parent)?;
        Ok(())
    }

    fn move_entry(&self, state: &MoveCheckpoint, artifacts: &Dir) -> Result<(), RecoveryError> {
        let target = &state.target;
        let original = target.original.ok_or(RecoveryError::InvalidRecord)?;
        require_pin(artifacts, "original", original)?;
        let destination = &state.destination;
        let source_parent = self.parent(&target.source)?;
        let destination_parent = self.parent(destination)?;
        let source = current(&source_parent, &target.source.name)?;
        let present = current(&destination_parent, &destination.name)?;
        if ![None, Some(original)].contains(&source)
            || ![state.destination_original, Some(original)].contains(&present)
        {
            return Err(RecoveryError::Conflict);
        }
        if present != Some(original) {
            pin(artifacts, "original", artifacts, "publish", original)?;
            artifacts.rename("publish", &destination_parent, &destination.name)?;
            paths::checkpoint("destination-published")?;
        }
        // A destination found on retry may be visible but not yet durable.
        // Persist it before unlinking the source on every attempt.
        sync(&destination_parent)?;
        if source.is_some() {
            source_parent.remove_file(&target.source.name)?;
            paths::checkpoint("source-removed")?;
        }
        sync(&source_parent)?;
        sync(&destination_parent)?;
        sync(artifacts)?;
        Ok(())
    }

    fn remove_entry(&self, target: &Target, artifacts: &Dir) -> Result<(), RecoveryError> {
        let original = target.original.ok_or(RecoveryError::InvalidRecord)?;
        require_pin(artifacts, "original", original)?;
        let parent = self.parent(&target.source)?;
        match current(&parent, &target.source.name)? {
            None => {}
            Some(current) if current == original => {
                parent.remove_file(&target.source.name)?;
                paths::checkpoint("source-removed")?;
            }
            Some(_) => return Err(RecoveryError::Conflict),
        }
        sync(&parent)
    }

    fn create_directory(&self, target: &Target) -> Result<(), RecoveryError> {
        let parent = self.parent(&target.source)?;
        match existing(&parent, &target.source.name)? {
            None if target.original.is_none() => {
                parent.create_dir(&target.source.name)?;
                paths::checkpoint("directory-created")?;
            }
            Some(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                if let Some(original) = target.original {
                    if identity(&metadata)? != original {
                        return Err(RecoveryError::Conflict);
                    }
                } else {
                    // No later mutation is admitted before this receipt. A new
                    // pending mkdir may only have produced an empty directory.
                    require_empty(&parent.open_dir(&target.source.name)?)?;
                }
            }
            _ => return Err(RecoveryError::Conflict),
        }
        sync(&parent.open_dir(&target.source.name)?)?;
        sync(&parent)
    }

    fn remove_directory(&self, target: &Target) -> Result<(), RecoveryError> {
        let parent = self.parent(&target.source)?;
        if let Some(metadata) = existing(&parent, &target.source.name)? {
            if Some(identity(&metadata)?) != target.original
                || !metadata.is_dir()
                || metadata.file_type().is_symlink()
            {
                return Err(RecoveryError::Conflict);
            }
            require_empty(&parent.open_dir(&target.source.name)?)?;
            parent.remove_dir(&target.source.name)?;
            paths::checkpoint("directory-removed")?;
        }
        sync(&parent)
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
    paths::checkpoint("pin-created")?;
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
        paths::checkpoint("write-chunk")?;
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
