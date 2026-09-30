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

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cap_std::ambient_authority;
use cap_std::fs::Dir;
use tempfile::TempDir;

use crate::runtime::disk_quota::DiskQuota;
use crate::runtime::fs::FileIdentity;

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
    /// The blueprint's `size_limit`, shared by every clone so the git worker and
    /// the program charge one counter.
    quota: Option<Arc<DiskQuota>>,
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
            quota: None,
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
            quota: None,
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
            quota: None,
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

    /// Enforce `limit` bytes on the files this VFS holds, starting from what the
    /// directory holds now; see [`with_measured_limit`](Self::with_measured_limit).
    pub fn with_size_limit(self, limit: u64) -> Self {
        let measured = self.measure_usage().ok();
        self.with_measured_limit(limit, measured)
    }

    /// Enforce `limit` bytes, starting from `used`, what a walk of the directory
    /// found. A directory already over the limit still opens: reads and removals
    /// work, and writes that grow it are refused until it is back under. One that
    /// couldn't be measured (`None`) opens the same way, as if full, so a program
    /// can still remove files and let a later run measure it.
    pub fn with_measured_limit(mut self, limit: u64, used: Option<u64>) -> Self {
        let quota = match used {
            Some(used) => DiskQuota::new(limit, used),
            None => DiskQuota::unmeasured(limit),
        };
        self.quota = Some(Arc::new(quota));
        self
    }

    /// The size limit and its running count, when the blueprint set one.
    pub fn quota(&self) -> Option<&Arc<DiskQuota>> {
        self.quota.as_ref()
    }

    /// The bytes held by regular files under the root; see [`measure_dir`].
    pub fn measure_usage(&self) -> io::Result<u64> {
        match &self.dir {
            Some(root) => measure_dir(root),
            None => Ok(0),
        }
    }
}

/// The bytes held by regular files under `root`; see [`for_each_regular_file`].
pub fn measure_dir(root: &Dir) -> io::Result<u64> {
    let mut total: u64 = 0;
    for_each_regular_file(root, MAX_MEASURED_ENTRIES, |_, bytes| {
        total = total.saturating_add(bytes);
    })?;
    Ok(total)
}

/// Every regular file under `root` and its size, as a removal of the tree frees
/// them; more than `max_entries` entries is an error.
pub fn regular_files(root: &Dir, max_entries: usize) -> io::Result<Vec<(FileIdentity, u64)>> {
    let mut files = Vec::new();
    for_each_regular_file(root, max_entries, |file, bytes| files.push((file, bytes)))?;
    Ok(files)
}

/// The bytes the regular files under `root` hold, and those of them in `held` with
/// their sizes, from one walk: what a change replacing them frees is the total less
/// the held ones, which stay on disk until released.
pub fn measure_with_held(
    root: &Dir,
    held: &HashSet<FileIdentity>,
) -> io::Result<(u64, Vec<(FileIdentity, u64)>)> {
    if held.is_empty() {
        return Ok((measure_dir(root)?, Vec::new()));
    }
    let mut total: u64 = 0;
    let mut held_files = Vec::new();
    for_each_regular_file(root, MAX_MEASURED_ENTRIES, |file, bytes| {
        total = total.saturating_add(bytes);
        if held.contains(&file) {
            held_files.push((file, bytes));
        }
    })?;
    Ok((total, held_files))
}

/// Visit each regular file under `root`. Links are never followed, so a link cannot
/// make a file count twice. The walk goes depth first, holding one open directory
/// per level, so its descriptors and memory grow with depth, not with the tree.
fn for_each_regular_file(
    root: &Dir,
    max_entries: usize,
    mut visit: impl FnMut(FileIdentity, u64),
) -> io::Result<()> {
    let mut entries: usize = 0;
    let mut open: Vec<cap_std::fs::ReadDir> = vec![root.entries()?];
    while let Some(dir) = open.last_mut() {
        let Some(entry) = dir.next() else {
            open.pop();
            continue;
        };
        let entry = entry?;
        entries += 1;
        if entries > max_entries {
            return Err(io::Error::other(format!("more than {max_entries} entries")));
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if open.len() > MAX_MEASURED_DEPTH {
                return Err(io::Error::other(format!(
                    "directories nested more than {MAX_MEASURED_DEPTH} deep"
                )));
            }
            let child = entry.open_dir()?.entries()?;
            open.push(child);
        } else if kind.is_file() {
            // Full metadata, which Windows reads through a handle, carries the
            // file's identity.
            let metadata = cap_fs_ext::DirEntryExt::full_metadata(&entry)?;
            visit(FileIdentity::of(&metadata)?, metadata.len());
        }
    }
    Ok(())
}

/// How many entries [`for_each_regular_file`] walks before it gives up, so a directory
/// crafted to be huge can't stall every run that opens it.
const MAX_MEASURED_ENTRIES: usize = 1_000_000;

/// How many directories deep below the root [`for_each_regular_file`] descends
/// before it gives up, which bounds the directories it holds open at once.
const MAX_MEASURED_DEPTH: usize = 64;

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

    #[test]
    fn size_limit_starts_from_what_the_directory_holds() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        std::fs::write(outer.path().join("a.txt"), vec![0u8; 300]).expect("write");
        std::fs::create_dir(outer.path().join("sub")).expect("mkdir");
        std::fs::write(outer.path().join("sub/b.txt"), vec![0u8; 200]).expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink("a.txt", outer.path().join("link")).expect("symlink");

        let vfs = Vfs::external_with_mode(outer.path().to_path_buf(), VfsMode::PerSession)
            .expect("external")
            .with_size_limit(1_000);
        let quota = vfs.quota().expect("quota attached");
        assert_eq!(quota.used(), 500, "links count as nothing");
        assert_eq!(quota.limit(), 1_000);
        assert!(
            vfs.clone().quota().is_some_and(|q| Arc::ptr_eq(q, quota)),
            "clones share it"
        );
    }

    #[test]
    fn measuring_many_directories_holds_one_open_per_level() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let mut path = outer.path().to_path_buf();
        for depth in 0..3_000 {
            if depth % 60 == 0 {
                path = outer.path().join(format!("branch{depth}"));
            } else {
                path = path.join("d");
            }
            std::fs::create_dir_all(&path).expect("mkdir");
        }
        std::fs::write(path.join("leaf.txt"), b"12345").expect("write");
        let root = Vfs::external(outer.path().to_path_buf()).expect("external");
        assert_eq!(root.measure_usage().expect("measure"), 5);
    }

    #[test]
    fn listing_files_stops_past_its_entry_cap() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        std::fs::write(outer.path().join("a"), b"1").expect("write");
        std::fs::create_dir(outer.path().join("d")).expect("mkdir");
        let root = Vfs::external(outer.path().to_path_buf()).expect("external");
        let dir = root.dir().expect("dir");
        assert_eq!(regular_files(dir, 2).expect("two entries fit").len(), 1);
        std::fs::write(outer.path().join("d/b"), b"2").expect("write");
        assert!(
            regular_files(dir, 2).is_err(),
            "a third entry passes the cap"
        );
    }

    #[test]
    fn a_tree_nested_past_the_measured_depth_opens_as_full() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let mut path = outer.path().to_path_buf();
        for _ in 0..MAX_MEASURED_DEPTH {
            path = path.join("d");
        }
        std::fs::create_dir_all(&path).expect("mkdir");
        let fits = Vfs::external(outer.path().to_path_buf()).expect("external");
        assert_eq!(fits.measure_usage().expect("measure"), 0);

        std::fs::create_dir(path.join("d")).expect("mkdir");
        let vfs = Vfs::external(outer.path().to_path_buf())
            .expect("external")
            .with_size_limit(1_000);
        assert!(vfs.quota().is_some_and(|quota| quota.is_unmeasured()));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_cannot_be_read_opens_as_full() {
        use std::os::unix::fs::PermissionsExt;
        let outer = tempfile::tempdir().expect("outer tempdir");
        let locked = outer.path().join("locked");
        std::fs::create_dir(&locked).expect("mkdir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let vfs = Vfs::external(outer.path().to_path_buf())
            .expect("external")
            .with_size_limit(1_000);
        let unmeasured = vfs.quota().is_some_and(|quota| quota.is_unmeasured());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("chmod back");
        assert!(unmeasured, "a failed walk leaves the VFS treated as full");
    }
}
