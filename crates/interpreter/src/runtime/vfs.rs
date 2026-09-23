//! VFS root directory plumbing.
//!
//! The interpreter is agnostic about *how* a VFS came to exist — it only
//! cares about the directory on disk that backs it. [`Vfs`] carries that
//! path and, when the interpreter allocated it, owns cleanup.
//!
//! It also carries the capability handle every guest path is opened through.
//! The root is opened once with ambient authority at construction; from then on
//! every host function resolves against that handle, so a symlink pointing out of
//! the root is refused by the kernel rather than checked for.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cap_std::ambient_authority;
use cap_std::fs::Dir;
use tempfile::TempDir;

/// Which blueprint VFS mode this directory backs. Kept independent of the
/// `submilli-blueprint` enum so the interpreter doesn't depend on that crate;
/// the server maps one onto the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsMode {
    None,
    Ephemeral,
    PerSession,
    Persistent,
}

#[derive(Debug, Clone)]
pub struct Vfs {
    root: PathBuf,
    mode: VfsMode,
    /// `None` only for [`VfsMode::None`], which backs no directory.
    dir: Option<Arc<Dir>>,
    _owned: Option<Arc<TempDir>>,
}

impl Vfs {
    /// A VFS that backs no directory. Every `submilli:fs.*` call traps; only
    /// `fs.info()` works (reporting `mode: none`).
    pub fn none() -> Self {
        Self {
            root: PathBuf::new(),
            mode: VfsMode::None,
            dir: None,
            _owned: None,
        }
    }

    /// Mount an existing host directory the interpreter does not own. Used for
    /// `persistent` mode and for `per_session` dirs owned by the server.
    pub fn external_with_mode(root: PathBuf, mode: VfsMode) -> io::Result<Self> {
        let meta = std::fs::metadata(&root)
            .map_err(|e| io::Error::new(e.kind(), format!("VFS root {}: {e}", root.display())))?;
        if !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("VFS root {} is not a directory", root.display()),
            ));
        }
        let dir = open_root(&root)?;
        Ok(Self {
            root,
            mode,
            dir: Some(dir),
            _owned: None,
        })
    }

    pub fn external(root: PathBuf) -> io::Result<Self> {
        Self::external_with_mode(root, VfsMode::Persistent)
    }

    pub fn tempdir() -> io::Result<Self> {
        let td = tempfile::Builder::new().prefix("submilli-vfs-").tempdir()?;
        Self::from_owned(td)
    }

    /// Like [`tempdir`](Self::tempdir) but allocated under `parent` (created if
    /// absent). Lets the host place ephemeral scratch on a chosen volume.
    pub fn tempdir_in(parent: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(parent)?;
        let td = tempfile::Builder::new()
            .prefix("submilli-vfs-")
            .tempdir_in(parent)?;
        Self::from_owned(td)
    }

    fn from_owned(td: TempDir) -> io::Result<Self> {
        let root = td.path().to_path_buf();
        let dir = open_root(&root)?;
        Ok(Self {
            root,
            mode: VfsMode::Ephemeral,
            dir: Some(dir),
            _owned: Some(Arc::new(td)),
        })
    }

    /// The host path backing this VFS. Server-side logging and test setup only —
    /// it is never the thing a guest path is resolved against. Empty for
    /// [`VfsMode::None`].
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The capability handle guest paths resolve against, or `None` under
    /// [`VfsMode::None`].
    ///
    /// A handle outlives the directory it was opened on: after the root is deleted
    /// the handle still answers `is_dir()`, so it is not a liveness check.
    pub fn dir(&self) -> Option<&Arc<Dir>> {
        self.dir.as_ref()
    }

    pub fn mode(&self) -> VfsMode {
        self.mode
    }
}

fn open_root(root: &Path) -> io::Result<Arc<Dir>> {
    Dir::open_ambient_dir(root, ambient_authority())
        .map(Arc::new)
        .map_err(|e| io::Error::new(e.kind(), format!("VFS root {}: {e}", root.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempdir_is_deleted_on_drop() {
        let vfs = Vfs::tempdir().expect("tempdir");
        let path = vfs.root().to_path_buf();
        assert!(path.is_dir());
        drop(vfs);
        assert!(!path.exists(), "tempdir should be deleted on drop");
    }

    #[test]
    fn external_does_not_delete_directory() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let path = outer.path().to_path_buf();
        let vfs = Vfs::external(path.clone()).expect("external");
        drop(vfs);
        assert!(path.is_dir(), "external path must survive Vfs drop");
    }

    #[test]
    fn external_rejects_missing_path() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let missing = outer.path().join("does-not-exist");
        let err = Vfs::external(missing).expect_err("must reject missing path");
        assert!(
            err.to_string().contains("VFS root"),
            "error should mention VFS root: {err}"
        );
    }

    #[test]
    fn real_directory_yields_a_usable_handle() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        std::fs::write(outer.path().join("a.txt"), b"hi").expect("write");
        let vfs = Vfs::external(outer.path().to_path_buf()).expect("external");
        let dir = vfs.dir().expect("persistent mode backs a directory");
        assert_eq!(dir.read("a.txt").expect("read through handle"), b"hi");
    }

    #[test]
    fn none_backs_no_handle() {
        let vfs = Vfs::none();
        assert!(vfs.dir().is_none());
        assert_eq!(vfs.mode(), VfsMode::None);
    }

    #[test]
    fn tempdir_modes_carry_a_handle() {
        let vfs = Vfs::tempdir().expect("tempdir");
        assert!(
            vfs.dir().is_some(),
            "ephemeral roots resolve through a handle too"
        );
    }

    #[test]
    fn handle_survives_a_rename_of_the_root() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let from = outer.path().join("before");
        let to = outer.path().join("after");
        std::fs::create_dir(&from).expect("mkdir");
        std::fs::write(from.join("a.txt"), b"hi").expect("write");

        let vfs = Vfs::external(from.clone()).expect("external");
        std::fs::rename(&from, &to).expect("rename");

        // The handle tracks the inode, not the name — which is also why it cannot be
        // used as a liveness check for the directory it was opened on.
        let dir = vfs.dir().expect("handle");
        assert_eq!(dir.read("a.txt").expect("read after rename"), b"hi");
    }

    #[test]
    fn external_rejects_file() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let file = outer.path().join("a.txt");
        std::fs::write(&file, b"hi").expect("write");
        let err = Vfs::external(file).expect_err("must reject file");
        assert!(
            err.to_string().contains("not a directory"),
            "error should mention not a directory: {err}"
        );
    }
}
