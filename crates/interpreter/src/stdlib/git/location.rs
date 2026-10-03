//! The host path gix opens a repository at, checked to be the directory the
//! VFS handle holds.
//!
//! gix needs ordinary paths, so Git is the one place a VFS path becomes a host
//! path. What keeps that path naming the repository the program chose is that
//! no program can change it while Git works: the worker has already refused a
//! symlink on the way to the repository, and the file system functions refuse
//! to write into `.git`, to write through an alias of it, or to rename or
//! remove a directory holding one (`check_metadata_mutation` in
//! `runtime/fs.rs`). Only the host can.
use crate::runtime::fs::FileIdentity;
use crate::runtime::vfs::Placement;
use cap_std::fs::Dir;
use std::path::{Path, PathBuf};
use wasmtime::{Result, bail};

pub(super) struct Location {
    /// The repository's directory, by handle.
    pub dir: std::sync::Arc<Dir>,
    /// Its identity, which the per-repository lock is keyed by.
    pub identity: FileIdentity,
    /// Its host path.
    pub host: PathBuf,
}

impl Location {
    /// The repository at `relative` within the volume of `placement`, whose
    /// handle is `dir`.
    pub(super) fn new(
        dir: std::sync::Arc<Dir>,
        placement: &Placement,
        relative: &Path,
    ) -> Result<Self> {
        let host = if relative == Path::new(".") {
            placement.host().to_path_buf()
        } else {
            placement.host().join(relative)
        };
        let identity = FileIdentity::of(&dir.dir_metadata()?)?;
        let opened = Dir::open_ambient_dir(&host, cap_std::ambient_authority())?;
        if FileIdentity::of(&opened.dir_metadata()?)? != identity {
            bail!("git: the repository's host path no longer names its directory");
        }
        Ok(Self {
            dir,
            identity,
            host,
        })
    }

    /// The host path of the repository's `.git`, checked to be the directory
    /// `.git` names through the handle.
    pub(super) fn git_dir(&self) -> Result<PathBuf> {
        let by_handle = FileIdentity::of(&self.dir.open_dir(".git")?.dir_metadata()?)?;
        let path = self.host.join(".git");
        let by_path = Dir::open_ambient_dir(&path, cap_std::ambient_authority())?;
        if FileIdentity::of(&by_path.dir_metadata()?)? != by_handle {
            bail!("git: the repository's .git no longer names its metadata");
        }
        Ok(path)
    }
}

#[cfg(test)]
impl Location {
    /// The directory at the host path `host`.
    pub(super) fn at(host: &Path) -> Self {
        let dir = Dir::open_ambient_dir(host, cap_std::ambient_authority()).unwrap();
        Self {
            identity: FileIdentity::of(&dir.dir_metadata().unwrap()).unwrap(),
            dir: std::sync::Arc::new(dir),
            host: host.to_path_buf(),
        }
    }

    /// The root of `vfs`.
    pub(super) fn of_vfs(vfs: &crate::runtime::Vfs) -> Self {
        Self::at(vfs.root())
    }
}
