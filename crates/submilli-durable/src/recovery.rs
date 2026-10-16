//! Recoverable filesystem mutations, below the execution-history boundary.
//!
//! The caller persists a request, then the prepared intent, before calling
//! `apply`. It persists the returned outcome before admitting another mutation.
//! Completed effects are answered by that journal, never by calling `apply`
//! again. One logical run exclusively owns every backing volume; this module
//! does not supply cross-process leases or fencing.

mod layout;
mod listing;
mod mutation;
mod paths;
mod plan;
mod resources;

pub use layout::{EphemeralReservation, WorkspaceLayout};
pub use listing::{DirectoryEntry, MAX_LISTING_BYTES};
pub use plan::{Content, Intent, Mutation, Outcome, VolumeIdentity, WorkspaceIdentity};
pub use resources::{ReaderPosition, RetainedReader};

use cap_std::fs::Dir;
use interpreter::runtime::Vfs;
use interpreter::runtime::fs::{ContainError, FileIdentity};
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::Arc;

/// Reserved even when no durable run is currently using a volume. Git's stage
/// namespace already prevents checkout/publication from installing this name.
pub(crate) const PRIVATE: &str = ".git-submilli-recovery";
const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Fuel(#[from] interpreter::runtime::fuel::Exhausted),
    #[error("filesystem recovery I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("filesystem containment refused recovery: {0}")]
    Containment(#[from] ContainError),
    #[error("filesystem recovery identity or content does not match the intent")]
    Conflict,
    #[error("unsupported durable filesystem operation: {0}")]
    Unsupported(&'static str),
    #[error("invalid filesystem recovery record")]
    InvalidRecord,
    #[error("durable filesystem size limit exceeded")]
    Quota,
    #[error("filesystem recovery traversal limit exceeded")]
    Limit,
}

/// Supplies immutable captured bytes. Implementations must authenticate the
/// complete object before exposing it and must hold its retention while used.
pub trait CapturedContent {
    fn open(&mut self, content: &Content) -> Result<Box<dyn Read + Send>, RecoveryError>;
}

/// Capability-relative recovery access. Keep this value (and its VFS) alive
/// until all blocking work has drained, including when a request is cancelled.
#[derive(Clone)]
pub struct Workspace {
    vfs: Vfs,
    id: String,
    volumes: Arc<BTreeMap<String, Volume>>,
    work: Option<Arc<interpreter::runtime::fuel::WorkBudget>>,
}

struct Volume {
    root: Arc<Dir>,
    recovery: Dir,
    identity: FileIdentity,
    maximum_bytes: Option<u64>,
    quota: Option<Arc<interpreter::runtime::DiskQuota>>,
}

impl Workspace {
    /// The run reservation must already be committed. A restart with a stored
    /// identity uses `create=false` so a lost volume is never replaced by an
    /// empty directory.
    pub fn retained_ephemeral(
        parent: &std::path::Path,
        id: &str,
        create: bool,
    ) -> Result<Vfs, RecoveryError> {
        paths::validate_id(id)?;
        let name = format!("{}{id}", interpreter::runtime::vfs::RETAINED_PREFIX);
        let root = Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
        let dir = paths::private_dir(&root, &name, create)?;
        paths::sync(&dir)?;
        Ok(Vfs::external_with_mode(
            parent.join(name),
            interpreter::runtime::vfs::VfsMode::Ephemeral,
        )?)
    }

    /// Open retained roots and create private recovery directories. The caller
    /// must persist `identity()` before any public filesystem mutation. On
    /// restart pass that identity to `reopen`, never adopt a replacement root.
    pub fn create(vfs: Vfs, id: &str) -> Result<Self, RecoveryError> {
        paths::validate_id(id)?;
        if vfs.dir().is_none() {
            return Err(RecoveryError::Unsupported("filesystem is disabled"));
        }
        if vfs.is_disposable() {
            return Err(RecoveryError::Unsupported(
                "workspace is owned by a TempDir",
            ));
        }
        let volumes = paths::open_volumes(&vfs, id, true)?;
        Ok(Self {
            vfs: vfs.protect_recovery(),
            id: id.to_owned(),
            volumes: Arc::new(volumes),
            work: None,
        })
    }

    pub fn reopen(vfs: Vfs, identity: &WorkspaceIdentity) -> Result<Self, RecoveryError> {
        paths::validate_id(&identity.id)?;
        if vfs.is_disposable() {
            return Err(RecoveryError::Unsupported(
                "workspace is owned by a TempDir",
            ));
        }
        let workspace = Self {
            volumes: Arc::new(paths::open_volumes(&vfs, &identity.id, false)?),
            work: None,
            vfs: vfs.protect_recovery(),
            id: identity.id.clone(),
        };
        if workspace.identity() != *identity {
            return Err(RecoveryError::Conflict);
        }
        Ok(workspace)
    }

    pub(crate) fn metered(
        &self,
        work: Option<Arc<interpreter::runtime::fuel::WorkBudget>>,
    ) -> Self {
        let mut workspace = self.clone();
        workspace.work = work;
        workspace
    }

    pub fn identity(&self) -> WorkspaceIdentity {
        WorkspaceIdentity {
            id: self.id.clone(),
            volumes: self
                .volumes
                .iter()
                .map(|(mount, volume)| VolumeIdentity {
                    mount: mount.clone(),
                    root: volume.identity,
                })
                .collect(),
        }
    }

    /// Read-only guest access to retained storage. Mutating host operations
    /// must go through the journal before this VFS is used for durable runs.
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    /// Preparation only writes private artifacts. Its result must be durably
    /// stored before `apply`. The request's stable ID cannot be reused for a
    /// different mutation, even if a prior outcome acknowledgement was lost.
    pub fn prepare(
        &self,
        effect: &str,
        mutation: Mutation,
        content: &mut dyn CapturedContent,
    ) -> Result<Intent, RecoveryError> {
        paths::validate_id(effect)?;
        mutation.validate()?;
        let result = self.prepare_mutation(effect, mutation, content);
        self.reconcile_quotas()?;
        result
    }

    /// Reconcile a pending intent and perform every required persistence
    /// barrier. An error leaves the intent pending; it is not a definitive
    /// guest error or permission to start the next mutation.
    pub fn apply(
        &self,
        intent: &Intent,
        content: &mut dyn CapturedContent,
    ) -> Result<Outcome, RecoveryError> {
        intent.validate()?;
        if intent.workspace != self.identity() {
            return Err(RecoveryError::Conflict);
        }
        let result = self.apply_mutation(intent, content);
        self.reconcile_quotas()?;
        result
    }

    /// Remove private artifacts only after the logical run is terminal and all
    /// of its workers and resources have drained. A failed cleanup
    /// is retryable; journal tombstones must outlive this operation.
    pub fn release_effect(&self, effect: &str) -> Result<(), RecoveryError> {
        paths::validate_id(effect)?;
        for volume in self.volumes.values() {
            paths::remove_private_tree(&volume.recovery, effect)?;
        }
        self.reconcile_quotas()
    }

    /// Terminal cleanup. The owner must first fence admission in SQL and drain
    /// every reader/worker. This removes recovery evidence, never named-volume
    /// or per-session user contents.
    pub fn retire(&self) -> Result<(), RecoveryError> {
        for volume in self.volumes.values() {
            if FileIdentity::of(&volume.root.dir_metadata()?)? != volume.identity {
                return Err(RecoveryError::Conflict);
            }
            let private = paths::private_dir(&volume.root, PRIVATE, false)?;
            paths::remove_private_tree(&private, &self.id)?;
        }
        self.reconcile_quotas()
    }

    fn volume(&self, mount: &str) -> Result<&Volume, RecoveryError> {
        self.volumes.get(mount).ok_or(RecoveryError::Conflict)
    }

    fn reconcile_quotas(&self) -> Result<(), RecoveryError> {
        for volume in self.volumes.values() {
            if let Some(quota) = &volume.quota {
                quota.reconcile_retained(volume.used_bytes(self.work.as_deref())?);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
