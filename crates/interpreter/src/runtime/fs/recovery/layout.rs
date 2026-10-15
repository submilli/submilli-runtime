use super::{FileIdentity, RecoveryError, Vfs, WorkspaceIdentity};
use crate::runtime::vfs::{Access, MountSpec, VfsMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Storage adapter metadata, sealed in SQL. This is not an execution manifest:
/// it contains only the retained filesystem roots and their mount layout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLayout {
    root: PathBuf,
    root_identity: FileIdentity,
    mode: VfsMode,
    access: Access,
    volume: Option<String>,
    maximum_bytes: Option<u64>,
    cwd: String,
    mounts: Vec<MountLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MountLayout {
    path: String,
    host: PathBuf,
    identity: FileIdentity,
    volume: String,
    access: Access,
    maximum_bytes: Option<u64>,
}

impl WorkspaceLayout {
    pub fn capture(vfs: &Vfs) -> Result<Self, RecoveryError> {
        let root = vfs
            .dir()
            .ok_or(RecoveryError::Unsupported("filesystem disabled"))?;
        if vfs.is_disposable() {
            return Err(RecoveryError::Unsupported("disposable workspace"));
        }
        let mounts = vfs
            .mounts()
            .iter()
            .map(|mount| {
                Ok(MountLayout {
                    path: mount.guest_path().into(),
                    host: std::fs::canonicalize(mount.host())?,
                    identity: FileIdentity::of(&mount.dir().dir_metadata()?)?,
                    volume: mount.volume().into(),
                    access: mount.access(),
                    maximum_bytes: mount.quota().map(|q| q.limit()),
                })
            })
            .collect::<Result<_, RecoveryError>>()?;
        Ok(Self {
            root: std::fs::canonicalize(vfs.root())?,
            root_identity: FileIdentity::of(&root.dir_metadata()?)?,
            mode: vfs.mode(),
            access: vfs.access(),
            volume: vfs.volume().map(str::to_owned),
            maximum_bytes: vfs.quota().map(|q| q.limit()),
            cwd: vfs.cwd().into(),
            mounts,
        })
    }

    pub fn restore(&self, identity: &WorkspaceIdentity) -> Result<Vfs, RecoveryError> {
        if self.mounts.len() > crate::runtime::vfs::MAX_MOUNTS {
            return Err(RecoveryError::InvalidRecord);
        }
        let mut vfs =
            Vfs::external_with_mode(self.root.clone(), self.mode)?.with_access(self.access);
        let root = vfs.dir().ok_or(RecoveryError::InvalidRecord)?;
        if FileIdentity::of(&root.dir_metadata()?)? != self.root_identity {
            return Err(RecoveryError::Conflict);
        }
        if let Some(volume) = &self.volume {
            vfs = vfs.with_volume_name(volume);
        }
        if let Some(limit) = self.maximum_bytes {
            vfs = vfs.with_size_limit(limit);
        }
        for mount in &self.mounts {
            let root =
                cap_std::fs::Dir::open_ambient_dir(&mount.host, cap_std::ambient_authority())?;
            if FileIdentity::of(&root.dir_metadata()?)? != mount.identity {
                return Err(RecoveryError::Conflict);
            }
            let quota = mount.maximum_bytes.map(|limit| {
                let measured = crate::runtime::vfs::measure_dir(&root);
                Arc::new(match measured {
                    Ok(used) => crate::runtime::DiskQuota::new(limit, used),
                    Err(_) => crate::runtime::DiskQuota::unmeasured(limit),
                })
            });
            vfs = vfs
                .with_mount(MountSpec {
                    guest_path: mount.path.clone(),
                    host: mount.host.clone(),
                    volume: mount.volume.clone(),
                    access: mount.access,
                    quota,
                })
                .map_err(|_| RecoveryError::Conflict)?;
        }
        vfs = vfs.with_retained_cwd(&self.cwd)?.protect_recovery();
        // Verify the recorded writable backing roots before exposing the VFS.
        for recorded in &identity.volumes {
            let path = crate::runtime::fs::resolve_content(&vfs, "/", &recorded.mount)?;
            if FileIdentity::of(&path.metadata()?)? != recorded.root {
                return Err(RecoveryError::Conflict);
            }
        }
        Ok(vfs)
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
    pub fn mode(&self) -> VfsMode {
        self.mode
    }

    /// Refresh shared volume counters after partial or completed cleanup, once
    /// all native access has drained. Surviving roots must retain their identity.
    pub fn reconcile_quotas(
        &self,
        quotas: &std::collections::HashMap<FileIdentity, Arc<crate::runtime::DiskQuota>>,
    ) -> Result<(), RecoveryError> {
        for (path, identity, removed) in std::iter::once((
            &self.root,
            self.root_identity,
            self.mode == VfsMode::Ephemeral,
        ))
        .chain(self.mounts.iter().map(|m| (&m.host, m.identity, false)))
        {
            let Some(quota) = quotas.get(&identity) else {
                continue;
            };
            let root = match cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())
            {
                Ok(root) => root,
                Err(error) if removed && error.kind() == std::io::ErrorKind::NotFound => {
                    quota.reconcile_retained(0);
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if FileIdentity::of(&root.dir_metadata()?)? != identity {
                return Err(RecoveryError::Conflict);
            }
            quota.reconcile_retained(super::paths::used_bytes(&root)?);
        }
        Ok(())
    }

    /// Immutable capture storage must never be guest-visible, even through a
    /// mount or an enclosing directory selected by a direct library caller.
    pub fn exclude(&self, protected: &std::path::Path) -> Result<(), RecoveryError> {
        for root in std::iter::once(&self.root).chain(self.mounts.iter().map(|mount| &mount.host)) {
            if root.starts_with(protected) || protected.starts_with(root) {
                return Err(RecoveryError::Unsupported(
                    "workspace overlaps immutable capture storage",
                ));
            }
        }
        Ok(())
    }

    /// Cleanup uses the recorded backing roots directly: a previous cleanup
    /// may already have removed cwd, mount placeholders or recovery subtrees.
    pub fn retire(&self, workspace: &str) -> Result<(), RecoveryError> {
        super::paths::validate_id(workspace)?;
        for mount in &self.mounts {
            if mount.access == Access::ReadWrite {
                remove_evidence(&mount.host, mount.identity, workspace)?;
            }
        }
        if self.mode == VfsMode::Ephemeral {
            return self.remove_ephemeral(workspace);
        }
        if self.access == Access::ReadWrite {
            remove_evidence(&self.root, self.root_identity, workspace)?;
        }
        Ok(())
    }

    /// Only a run-keyed ephemeral root is owned by this protocol. Named and
    /// per-session data have separate owners and are never removed here.
    pub fn remove_ephemeral(&self, workspace: &str) -> Result<(), RecoveryError> {
        if self.mode != VfsMode::Ephemeral {
            return Ok(());
        }
        let expected = format!("{}{workspace}", crate::runtime::vfs::RETAINED_PREFIX);
        if self.root.file_name().and_then(|name| name.to_str()) != Some(&expected) {
            return Err(RecoveryError::Conflict);
        }
        let parent_path = self.root.parent().ok_or(RecoveryError::InvalidRecord)?;
        let parent = cap_std::fs::Dir::open_ambient_dir(parent_path, cap_std::ambient_authority())?;
        if let Some(metadata) = super::paths::existing(&parent, &expected)? {
            if metadata.file_type().is_symlink()
                || FileIdentity::of(&metadata)? != self.root_identity
            {
                return Err(RecoveryError::Conflict);
            }
            super::paths::remove_private_tree(&parent, &expected)?;
        }
        super::paths::sync(&parent)
    }
}

fn remove_evidence(
    root: &std::path::Path,
    identity: FileIdentity,
    workspace: &str,
) -> Result<(), RecoveryError> {
    let root = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
    if FileIdentity::of(&root.dir_metadata()?)? != identity {
        return Err(RecoveryError::Conflict);
    }
    if super::paths::existing(&root, super::PRIVATE)?.is_some() {
        let private = super::paths::private_dir(&root, super::PRIVATE, false)?;
        super::paths::remove_private_tree(&private, workspace)?;
    }
    super::paths::sync(&root)
}

/// A reservation made before mkdir. Until a full layout is stored, the only
/// permitted contents of this run's root are an empty directory. This closes
/// the mkdir-to-SQL gap without adopting or deleting an unrelated filesystem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EphemeralReservation {
    parent: PathBuf,
    parent_identity: FileIdentity,
    workspace: String,
}

impl EphemeralReservation {
    pub fn new(parent: &std::path::Path, workspace: &str) -> Result<Self, RecoveryError> {
        super::paths::validate_id(workspace)?;
        let parent = std::fs::canonicalize(parent)?;
        let directory = cap_std::fs::Dir::open_ambient_dir(&parent, cap_std::ambient_authority())?;
        Ok(Self {
            parent,
            parent_identity: FileIdentity::of(&directory.dir_metadata()?)?,
            workspace: workspace.into(),
        })
    }

    pub fn open(&self, create: bool) -> Result<Vfs, RecoveryError> {
        self.parent()?;
        super::Workspace::retained_ephemeral(&self.parent, &self.workspace, create)
    }

    pub fn remove_uninitialized(&self) -> Result<(), RecoveryError> {
        let parent = self.parent()?;
        let name = format!("{}{}", crate::runtime::vfs::RETAINED_PREFIX, self.workspace);
        if let Some(metadata) = super::paths::existing(&parent, &name)? {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(RecoveryError::Conflict);
            }
            // remove_dir deliberately refuses anything created after this
            // reservation's bootstrap phase. Never recursively adopt a root.
            parent.remove_dir(&name)?;
        }
        super::paths::sync(&parent)
    }

    fn parent(&self) -> Result<cap_std::fs::Dir, RecoveryError> {
        super::paths::validate_id(&self.workspace)?;
        let parent =
            cap_std::fs::Dir::open_ambient_dir(&self.parent, cap_std::ambient_authority())?;
        if FileIdentity::of(&parent.dir_metadata()?)? != self.parent_identity {
            return Err(RecoveryError::Conflict);
        }
        Ok(parent)
    }
}
