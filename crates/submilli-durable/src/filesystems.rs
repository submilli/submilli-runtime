//! Same-node filesystem recovery, independent of execution replay/supervision.
//!
//! Callers supply stable effect IDs after capability checks and fuel admission.
//! `execute` returns only after the filesystem barriers and SQL receipt commit.
//! It never starts a new run, restores a program or changes run admission.
use crate::journal::{FilesystemJournal, PendingResource, RecordedOutcome};
use crate::object::{ContentIdentity, ObjectId, ObjectReference};
use crate::objects::{DurableObjects, ObjectStoreError};
use crate::recovery::{CapturedContent, Workspace, WorkspaceLayout};
pub use crate::recovery::{Content, Mutation, Outcome, RecoveryError};
use crate::{UnitFactory, unit_of_work::StoreError};
use interpreter::runtime::Vfs;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::Arc;

mod backend;
mod lifecycle;
mod listings;
pub(crate) mod quotas;
pub use listings::DirectorySnapshot;
mod resources;
mod sequences;
pub use resources::FilesystemReader;

#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
    #[error(transparent)]
    Persistence(#[from] StoreError),
    #[error(transparent)]
    Objects(#[from] ObjectStoreError),
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
    objects: DurableObjects,
    id: String,
    owner: String,
}

impl DurableFilesystem {
    /// `vfs` must be retained by run ownership, and every writable backing root
    /// must be exclusive to this run until unfinished effects are reconciled.
    /// Ephemeral roots use `retained_ephemeral` rather than a TempDir.
    pub async fn open(
        vfs: Vfs,
        run_id: &str,
        owner: &str,
        units: UnitFactory,
        objects: DurableObjects,
    ) -> Result<Self, FilesystemError> {
        validate_id(run_id)?;
        validate_owner(owner)?;
        let gate = Arc::new(objects.filesystem_gate.clone().lock_owned().await);
        let journal = FilesystemJournal::open(units, objects.store_identity()).await?;
        let id = workspace_id(run_id, objects.store_identity());
        let vfs = self::quotas::share(&objects, vfs).await?;
        let layout = WorkspaceLayout::capture(&vfs)?;
        layout.exclude(objects.directory())?;
        let identity = journal.register(&id, run_id, owner, Some(layout)).await?;
        checkpoint("workspace-layout");
        let worker_id = id.clone();
        let workspace = blocking(gate.clone(), move || match identity {
            Some(identity) => Workspace::reopen(vfs, &identity),
            None => Workspace::create(vfs, &worker_id),
        })
        .await?;
        journal.identify(&id, &workspace.identity()).await?;
        checkpoint("workspace-active");
        Ok(Self {
            work: None,
            workspace: Arc::new(workspace),
            journal,
            objects,
            id,
            owner: owner.into(),
        })
    }

    #[cfg(feature = "test-support")]
    pub fn test_content(
        &self,
        request: &Mutation,
    ) -> Result<impl CapturedContent + Send + 'static, FilesystemError> {
        self.content(request)
    }
    #[cfg(feature = "test-support")]
    pub async fn test_reserve_writer_capture(
        &self,
        resource: &str,
        chunks: &[Content],
    ) -> Result<ObjectId, FilesystemError> {
        self.reserve_writer_capture(resource, chunks).await
    }
    #[cfg(feature = "test-support")]
    pub fn test_journal(&self) -> &FilesystemJournal {
        &self.journal
    }
    #[cfg(feature = "test-support")]
    pub fn test_workspace(&self) -> &Arc<Workspace> {
        &self.workspace
    }
    #[cfg(feature = "test-support")]
    pub fn test_objects(&self) -> &DurableObjects {
        &self.objects
    }
    #[cfg(feature = "test-support")]
    pub async fn test_execute_step(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        effect: &str,
        request: Mutation,
        plan: Option<&str>,
        finish_publication: bool,
    ) -> Result<Outcome, FilesystemError> {
        self.execute_step(gate, effect, request, plan, finish_publication)
            .await
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
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        self.execute_held(gate, effect, request).await
    }

    async fn execute_held(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        effect: &str,
        request: Mutation,
    ) -> Result<Outcome, FilesystemError> {
        self.execute_step(gate, effect, request, None, false).await
    }

    async fn execute_step(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        effect: &str,
        request: Mutation,
        plan: Option<&str>,
        finish_publication: bool,
    ) -> Result<Outcome, FilesystemError> {
        let record = self
            .journal
            .admit_step(&self.id, effect, &request, plan)
            .await?;
        if let Some(outcome) = record.outcome {
            return unpack(outcome);
        }
        self.charge_preparation(&request)?;
        checkpoint("effect-request");
        let intent = if let Some(intent) = record.intent {
            intent
        } else {
            let workspace = self.workspace.clone();
            let effect_id = effect.to_owned();
            let mut content = self.content(&request)?;
            let prepared = blocking(gate.clone(), move || {
                workspace.prepare(&effect_id, request, &mut content)
            })
            .await;
            match prepared {
                Ok(intent) => {
                    self.journal.prepared(&self.id, &intent).await?;
                    checkpoint("effect-intent");
                    intent
                }
                Err(FilesystemError::Recovery(error))
                    if !finish_publication && preflight_refusal(&error) =>
                {
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
        let mut content = self.content(intent.mutation())?;
        let outcome =
            blocking(gate.clone(), move || workspace.apply(&intent, &mut content)).await?;
        let recorded = RecordedOutcome::Completed(outcome.clone());
        checkpoint("effect-applied");
        self.journal.complete(&self.id, effect, &recorded).await?;
        checkpoint("effect-receipt");
        Ok(outcome)
    }

    /// Recover this filesystem's unfinished mutation. Completed mutations are
    /// not replayed onto retained storage, even if paths have since changed.
    pub async fn recover(&self) -> Result<Option<Outcome>, FilesystemError> {
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        if let Some(resource) = self.journal.pending_resource(&self.id).await? {
            // The pending SQL resource fences new mutations while its public
            // open method reacquires the shared gate. Repeated opens are stable.
            drop(gate);
            match resource {
                PendingResource::Reader { id, path } => {
                    self.open_reader(&id, &path).await?;
                }
                PendingResource::Listing {
                    id,
                    path,
                    recursive,
                } => {
                    self.open_directory(&id, &path, recursive, 0).await?;
                }
            }
            return Ok(Some(Outcome { bytes: 0 }));
        }
        if let Some((id, plan)) = self.journal.pending_plan(&self.id).await? {
            return self
                .execute_plan_held(gate, &id, plan.steps)
                .await
                .map(Some);
        }
        if let Some((resource, effect)) = self.journal.pending_writer(&self.id).await? {
            return self
                .close_writer_held(gate, &resource, &effect)
                .await
                .map(Some);
        }
        let Some((effect, request)) = self.journal.pending(&self.id).await? else {
            return Ok(None);
        };
        self.execute_held(gate, &effect, request).await.map(Some)
    }

    fn content(&self, request: &Mutation) -> Result<Source, FilesystemError> {
        let (Mutation::Write { content, .. }
        | Mutation::Append { content, .. }
        | Mutation::GitWrite { content, .. }) = request
        else {
            return Ok(Source(None));
        };
        let id = ObjectId::parse(content.id.clone()).map_err(ObjectStoreError::from)?;
        let reference = ObjectReference {
            id,
            content: ContentIdentity {
                bytes: content.bytes,
                sha256: content.sha256,
            },
        };
        Ok(Source(Some((
            content.clone(),
            self.objects.stream(reference),
        ))))
    }
}

type CapturedReader = (Content, Box<dyn Read + Send>);
struct Source(Option<CapturedReader>);
impl CapturedContent for Source {
    fn open(&mut self, content: &Content) -> Result<Box<dyn Read + Send>, RecoveryError> {
        match self.0.take() {
            Some((expected, reader)) if expected == *content => Ok(reader),
            _ => Err(RecoveryError::Conflict),
        }
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
        RecoveryError::Conflict | RecoveryError::Fuel(_) => false,
    }
}

fn validate_id(id: &str) -> Result<(), FilesystemError> {
    ObjectId::parse(id.to_owned()).map_err(ObjectStoreError::from)?;
    Ok(())
}

fn validate_owner(owner: &str) -> Result<(), FilesystemError> {
    if owner.is_empty() || owner.len() > 1024 {
        return Err(FilesystemError::Configuration(
            "bounded, nonempty owner required",
        ));
    }
    Ok(())
}

pub fn workspace_id(run: &str, store: &str) -> String {
    let digest = Sha256::digest(format!("submilli-workspace-v1:{store}:{run}").as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string()
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
        use interpreter::runtime::{fs, fuel};
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
        let original = fs::resolve_content(self.vfs(), self.vfs().cwd(), path)
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
