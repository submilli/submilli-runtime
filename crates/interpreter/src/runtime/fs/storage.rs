//! Privileged capability-relative storage operations for filesystem backends.
//! Guest host functions use `Filesystem`; these operations are for backend
//! implementations which must establish persistence barriers and inode identity.
use super::{ContainError, ContentPath, FileIdentity};
use crate::runtime::{DiskQuota, Vfs};
use cap_std::fs::Dir;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Containment(#[from] ContainError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unsupported native storage operation: {0}")]
    Unsupported(&'static str),
}

/// A checked location and held ancestor handles. Names are journal metadata;
/// all actual I/O is relative to the returned capabilities.
pub struct MutationLocation {
    pub path: ContentPath,
    pub mount: String,
    pub parent: String,
    pub name: String,
    pub parent_identity: FileIdentity,
    pub ancestors: Vec<Dir>,
}

pub trait NativeStorage {
    fn mutation_location(
        &self,
        guest: &str,
        follow_final: bool,
        git: bool,
    ) -> Result<MutationLocation, StorageError>;
    fn protect_recovery(self) -> Self;
    fn with_retained_cwd(self, cwd: &str) -> Result<Self, ContainError>
    where
        Self: Sized;
}

impl NativeStorage for Vfs {
    fn mutation_location(
        &self,
        guest: &str,
        follow_final: bool,
        git: bool,
    ) -> Result<MutationLocation, StorageError> {
        let mut path = super::resolve_content(self, self.cwd(), guest)?;
        path.writable()?;
        if path.is_root() {
            return Err(StorageError::Unsupported("volume root mutation"));
        }
        super::check_mount_points(&path.dir, &path.rel, &path.placement, true, follow_final)?;
        if follow_final
            && path
                .dir
                .symlink_metadata(&path.rel)
                .is_ok_and(|m| m.file_type().is_symlink())
        {
            path.rel = path.dir.canonicalize(&path.rel)?;
        }
        if !git && super::protected_metadata(&path.rel) {
            return Err(StorageError::Unsupported("protected metadata"));
        }
        let parent = path
            .rel
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let resolved = path.dir.canonicalize(parent)?;
        if git
            && resolved != parent
            && !(resolved.as_os_str().is_empty() && parent == Path::new("."))
        {
            return Err(StorageError::Unsupported("Git metadata aliases"));
        }
        if !git && super::protected_metadata(&resolved) {
            return Err(StorageError::Unsupported("protected metadata alias"));
        }
        let parent = if resolved.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &resolved
        };
        let name = path
            .rel
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(StorageError::Unsupported("non-UTF-8 path"))?
            .to_owned();
        let parent_name = parent
            .to_str()
            .ok_or(StorageError::Unsupported("non-UTF-8 path"))?
            .to_owned();
        let parent_identity = FileIdentity::of(&path.dir.open_dir(parent)?.dir_metadata()?)?;
        let ancestors = parent
            .ancestors()
            .map(|ancestor| {
                path.dir.open_dir(if ancestor.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    ancestor
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MutationLocation {
            mount: path.placement.mount_point().into(),
            path,
            parent: parent_name,
            name,
            parent_identity,
            ancestors,
        })
    }
    fn protect_recovery(self) -> Self {
        Vfs::protect_recovery(self)
    }
    fn with_retained_cwd(self, cwd: &str) -> Result<Self, ContainError> {
        Vfs::with_retained_cwd(self, cwd)
    }
}

/// Native volume metadata is available only through this backend interface.
pub trait NativeMount {
    fn directory(&self) -> &Arc<Dir>;
    fn host_path(&self) -> &Path;
}
impl NativeMount for crate::runtime::vfs::Mount {
    fn directory(&self) -> &Arc<Dir> {
        self.dir()
    }
    fn host_path(&self) -> &Path {
        self.host()
    }
}

pub trait RetainedQuota {
    /// Call only after all native work has drained and retained inodes have
    /// been measured, including private journal artifacts.
    fn reconcile_retained(&self, bytes: u64);
}
impl RetainedQuota for DiskQuota {
    fn reconcile_retained(&self, bytes: u64) {
        DiskQuota::reconcile_retained(self, bytes);
    }
}

pub trait NativeDirectory {
    fn open_directory(&self) -> Result<Dir, ContainError>;
}
impl NativeDirectory for ContentPath {
    fn open_directory(&self) -> Result<Dir, ContainError> {
        Ok(self.dir.open_dir(&self.rel)?)
    }
}

/// Sync the held directory itself, including when cap-std uses O_PATH on Linux.
pub fn sync_directory(dir: &Dir) -> std::io::Result<()> {
    dir.open(".")?.sync_all()
}

pub fn protected_metadata(path: &Path) -> bool {
    super::protected_metadata(path)
}
pub fn private_recovery_path(path: &Path) -> bool {
    super::private_recovery_path(path)
}
