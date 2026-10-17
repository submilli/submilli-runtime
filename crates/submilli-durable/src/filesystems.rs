//! Filesystem operations that remain idempotent across process restarts.
//!
//! Callers supply stable effect IDs after capability checks and fuel admission.
//! `execute` returns only after the filesystem barriers and SQL receipt commit.
//! Reconciliation happens only when a caller retries a specific operation.
//! Opening a workspace never discovers or executes pending work.
use crate::journal::{FilesystemJournal, RecordedOutcome};
pub use crate::recovery::{Content, Mutation, Outcome, RecoveryError};
use crate::recovery::{InputBytes, Workspace};
use crate::{UnitFactory, unit_of_work::StoreError};
use interpreter::runtime::Vfs;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::Arc;

mod backend;
mod context;
mod input;
pub use context::FilesystemContext;
pub use input::ReplayInput;
mod listings;
pub(crate) mod quotas;
pub use listings::DirectorySnapshot;
mod resources;
pub use resources::FilesystemReader;

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
    work: Option<Arc<interpreter::runtime::fuel::WorkBudget>>,
    workspace: Arc<Workspace>,
    journal: FilesystemJournal,
    context: FilesystemContext,
    id: String,
}

impl DurableFilesystem {
    /// The caller supplies the configured mounts and a stable UUID scope for
    /// operation IDs. It must retain these roots and this journal across retries,
    /// exclusively own writable roots, and drain work before replacing access.
    pub async fn open(
        vfs: Vfs,
        scope: &str,
        units: UnitFactory,
        context: FilesystemContext,
    ) -> Result<Self, FilesystemError> {
        validate_id(scope)?;
        let gate = Arc::new(context.filesystem_gate.clone().lock_owned().await);
        let vfs = self::quotas::share(&context, vfs).await?;
        let binding = context::binding(&vfs)?;
        let journal = FilesystemJournal::new(units, binding);
        let id = scope.to_owned();
        let worker_id = id.clone();
        let workspace = blocking(gate, move || Workspace::create(vfs, &worker_id)).await?;
        Ok(Self {
            work: None,
            workspace: Arc::new(workspace),
            journal,
            context,
            id,
        })
    }

    #[cfg(feature = "test-support")]
    pub fn test_context(&self) -> &FilesystemContext {
        &self.context
    }
    #[cfg(feature = "test-support")]
    pub fn test_journal(&self) -> &FilesystemJournal {
        &self.journal
    }
    #[cfg(feature = "test-support")]
    pub fn test_workspace(&self) -> &Arc<Workspace> {
        &self.workspace
    }
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn vfs(&self) -> &Vfs {
        self.workspace.vfs()
    }

    pub async fn execute(
        &self,
        effect: &str,
        request: Mutation,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        request.validate()?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        self.execute_held(gate, effect, request, None, None).await
    }

    /// Retry the same effect with bytes reconstructed by execution replay.
    pub async fn execute_with_input(
        &self,
        effect: &str,
        request: Mutation,
        input: ReplayInput,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(effect)?;
        request.validate()?;
        if request.content() != Some(&input.content()) {
            return Err(RecoveryError::Conflict.into());
        }
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        self.execute_held(gate, effect, request, Some(input), None)
            .await
    }

    async fn execute_held(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        effect: &str,
        request: Mutation,
        input: Option<ReplayInput>,
        writer: Option<&str>,
    ) -> Result<Outcome, FilesystemError> {
        let record = match writer {
            Some(writer) => {
                self.journal
                    .admit_writer_close(&self.id, effect, &request, writer)
                    .await?
            }
            None => self.journal.admit(&self.id, effect, &request).await?,
        };
        if let Some(outcome) = record.outcome {
            return unpack(outcome);
        }
        if request.content().is_some() && input.is_none() {
            return Err(RecoveryError::ReplayRequired.into());
        }
        self.charge_preparation(&request)?;
        checkpoint("effect-request");
        let intent = if let Some(intent) = record.intent {
            intent
        } else {
            let workspace = self.workspace.clone();
            let operation_id = effect.to_owned();
            let mut content = input.clone();
            let prepared = blocking(gate.clone(), move || {
                workspace.prepare(&operation_id, request, &mut content)
            })
            .await;
            match prepared {
                Ok(intent) => {
                    self.journal.prepared(&self.id, &intent).await?;
                    checkpoint("effect-intent");
                    intent
                }
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    let outcome = RecordedOutcome::Rejected(error.to_string());
                    self.journal.complete(&self.id, effect, &outcome).await?;
                    return unpack(outcome);
                }
                Err(error) => return Err(error),
            }
        };
        if let Some(work) = &self.work {
            work.before_effect();
        }
        let workspace = self.workspace.clone();
        let mut content = input;
        let outcome =
            blocking(gate.clone(), move || workspace.apply(&intent, &mut content)).await?;
        let recorded = RecordedOutcome::Completed(outcome.clone());
        checkpoint("effect-applied");
        self.journal.complete(&self.id, effect, &recorded).await?;
        checkpoint("effect-receipt");
        Ok(outcome)
    }
}

async fn blocking<T: Send + 'static>(
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
async fn finish_owned<T: Send + 'static>(
    operation: impl std::future::Future<Output = Result<T, FilesystemError>> + Send + 'static,
) -> Result<T, FilesystemError> {
    tokio::spawn(operation)
        .await
        .map_err(|error| FilesystemError::Worker(error.to_string()))?
}

fn unpack(outcome: RecordedOutcome) -> Result<Outcome, FilesystemError> {
    match outcome {
        RecordedOutcome::Completed(outcome) => Ok(outcome),
        RecordedOutcome::Rejected(reason) => Err(FilesystemError::Rejected(reason)),
    }
}

fn preflight_refusal(error: &RecoveryError) -> bool {
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
        RecoveryError::Conflict | RecoveryError::ReplayRequired | RecoveryError::Fuel(_) => false,
    }
}

fn validate_id(id: &str) -> Result<(), FilesystemError> {
    crate::recovery::paths::validate_id(id)?;
    Ok(())
}

fn checkpoint(_point: &str) {
    crate::checkpoint(_point);
}

impl DurableFilesystem {
    fn metered(&self, invocation: &interpreter::runtime::filesystem::Invocation) -> Self {
        let mut filesystem = self.clone();
        filesystem.work = invocation.work.clone();
        filesystem.workspace = Arc::new(self.workspace.metered(invocation.work.clone()));
        filesystem
    }
    fn charge(
        &self,
        rate: interpreter::runtime::fuel::Rate,
        count: u64,
    ) -> Result<(), FilesystemError> {
        if let Some(work) = &self.work {
            work.charge(rate, count).map_err(RecoveryError::from)?;
        }
        Ok(())
    }
    fn charge_preparation(&self, request: &Mutation) -> Result<(), FilesystemError> {
        use interpreter::runtime::fuel;
        if self.work.is_none() {
            return Ok(());
        }
        let path = match request {
            Mutation::Write { path, .. }
            | Mutation::Append { path, .. }
            | Mutation::Remove { path }
            | Mutation::CreateDirectory { path }
            | Mutation::GitWrite { path, .. }
            | Mutation::GitRemove { path, .. }
            | Mutation::GitDirectory { path, .. } => path,
            Mutation::Move { source, .. } => source,
        };
        // The protocol repeats validation before use. An absent/unreadable path
        // is diagnosed there; it contributes no known content-hashing cost here.
        let original = crate::recovery::paths::resolve_content(self.vfs(), self.vfs().cwd(), path)
            .and_then(|p| p.metadata())
            .ok()
            .filter(cap_std::fs::Metadata::is_file)
            .map_or(0, |m| m.len());
        let payload = match request {
            Mutation::Write { content, .. }
            | Mutation::Append { content, .. }
            | Mutation::GitWrite { content, .. } => content.bytes,
            _ => 0,
        };
        self.charge(
            fuel::HASH,
            original
                .saturating_mul(2)
                .saturating_add(payload.saturating_mul(3)),
        )?;
        self.charge(
            fuel::IO,
            original
                .saturating_mul(2)
                .saturating_add(payload.saturating_mul(3)),
        )
    }
}
