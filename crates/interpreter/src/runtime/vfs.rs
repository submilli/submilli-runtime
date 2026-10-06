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
//!
//! A VFS may also carry mounts: other directories (named volumes) grafted at
//! fixed guest paths below the root. Each has its own handle, access mode and
//! size limit, and a guest path is routed to exactly one of them by
//! [`Vfs::locate`] before anything is opened.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use tempfile::TempDir;

use crate::runtime::disk_quota::DiskQuota;
use crate::runtime::fs::{FileIdentity, dir_identity};

/// Which blueprint VFS mode this directory backs. Kept independent of the
/// `submilli-blueprint` enum so the interpreter doesn't depend on that crate;
/// the server maps one onto the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsMode {
    None,
    Ephemeral,
    PerSession,
    Named,
}

/// Whether guest code may change what a volume holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::ReadWrite => "read_write",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Vfs {
    root: PathBuf,
    cwd: Arc<str>,
    ancestors: Arc<[FileIdentity]>,
    mode: VfsMode,
    /// `None` only for [`VfsMode::None`], which backs no directory.
    dir: Option<Arc<Dir>>,
    _owned: Option<Arc<TempDir>>,
    /// The blueprint's `size_limit`, shared by every clone so the git worker and
    /// the program charge one counter.
    quota: Option<Arc<DiskQuota>>,
    access: Access,
    /// The named volume backing the root, under [`VfsMode::Named`].
    volume: Option<Arc<str>>,
    mounts: Arc<[Mount]>,
}

/// A named volume grafted at a guest path below the root.
#[derive(Debug, Clone)]
pub struct Mount {
    /// The guest path, such as `/memory`.
    guest: Arc<str>,
    /// The same path relative to the root, such as `memory`.
    rel: PathBuf,
    volume: Arc<str>,
    dir: Arc<Dir>,
    /// The host directory `dir` was opened on.
    host: Arc<Path>,
    access: Access,
    quota: Option<Arc<DiskQuota>>,
    /// The identity and exact name of each directory on the way to the mount
    /// point in the root, the mount point last. A case-insensitive filesystem
    /// opens these under other spellings too, some not ASCII at all, so the
    /// mount-point guard recognizes them by identity rather than by name.
    placeholders: Arc<[Placeholder]>,
    ancestors: Arc<[FileIdentity]>,
}

impl Mount {
    pub fn guest_path(&self) -> &str {
        &self.guest
    }

    pub fn volume(&self) -> &str {
        &self.volume
    }

    pub fn access(&self) -> Access {
        self.access
    }

    pub fn quota(&self) -> Option<&Arc<DiskQuota>> {
        self.quota.as_ref()
    }

    pub(crate) fn rel(&self) -> &Path {
        &self.rel
    }

    pub(crate) fn dir(&self) -> &Arc<Dir> {
        &self.dir
    }

    pub(crate) fn placeholders(&self) -> &[Placeholder] {
        &self.placeholders
    }
}

/// A directory in the root on the way to a mount point, or the mount point
/// itself: what a spelling that reaches it must be named as.
#[derive(Debug, Clone)]
pub(crate) struct Placeholder {
    pub(crate) identity: FileIdentity,
    /// Its parent, exactly as the mount spells it; `""` for the root.
    pub(crate) parent: PathBuf,
    pub(crate) name: OsString,
}

/// What the server hands [`Vfs::with_mount`]: a volume it has resolved by name.
pub struct MountSpec {
    /// Absolute guest path, already validated by the blueprint parser; checked
    /// again here so a direct embedder cannot build an ambiguous table.
    pub guest_path: String,
    pub host: PathBuf,
    pub volume: String,
    pub access: Access,
    /// Shared by every VFS that mounts the same volume, so one limit spans them.
    pub quota: Option<Arc<DiskQuota>>,
}

/// The volume a resolved guest path lives in: where writes are charged and
/// whether they are allowed at all.
#[derive(Debug, Clone)]
pub struct Placement {
    access: Access,
    quota: Option<Arc<DiskQuota>>,
    /// The mount's guest path, or `None` for the root volume.
    mount: Option<Arc<str>>,
    /// The mounts below this volume; only the root has any. A change to the root
    /// must not remove, replace or write through one of their mount points.
    nested: Arc<[Mount]>,
    /// The host directory the volume's handle was opened on.
    host: Arc<Path>,
    volume: Option<Arc<str>>,
    ancestors: Arc<[FileIdentity]>,
    protected_roots: Arc<[FileIdentity]>,
    protected_ancestors: Arc<[FileIdentity]>,
}

impl Placement {
    pub fn access(&self) -> Access {
        self.access
    }

    pub fn quota(&self) -> Option<&Arc<DiskQuota>> {
        self.quota.as_ref()
    }

    /// The guest path of the volume's mount point: `/` for the root.
    pub fn mount_point(&self) -> &str {
        self.mount.as_deref().unwrap_or("/")
    }

    pub(crate) fn ancestors(&self) -> &[FileIdentity] {
        &self.ancestors
    }

    pub(crate) fn protects(&self, identity: FileIdentity, recursive: bool) -> bool {
        self.protected_roots.contains(&identity)
            || (recursive && self.protected_ancestors.contains(&identity))
    }

    pub(crate) fn protects_descendants(&self, identity: FileIdentity) -> bool {
        self.protected_ancestors.contains(&identity)
    }

    pub(crate) fn nested(&self) -> &[Mount] {
        &self.nested
    }

    /// The host directory the volume's handle was opened on. Only Git uses it,
    /// to let gix open a repository in place after checking that the path
    /// names the directory the handle holds; see `stdlib::git::location`.
    pub(crate) fn host(&self) -> &Path {
        &self.host
    }

    /// Whether two placements are the same volume, so a rename between them
    /// stays within one directory handle and one size limit.
    pub fn same_volume(&self, other: &Self) -> bool {
        match (&self.volume, &other.volume) {
            (Some(left), Some(right)) => left == right,
            (None, None) => self.mount == other.mount,
            _ => false,
        }
    }
}

/// Why a mount could not be added.
#[derive(Debug)]
pub enum MountError {
    /// The VFS has no root directory to mount below (`vfs: none`).
    RootDisabled,
    /// The guest path is not an absolute, normalized path.
    BadPath(String),
    /// `/` is the root itself.
    AtRoot,
    /// The guest path names Git metadata.
    ProtectedPath(String),
    /// One mount would sit inside another.
    Nested {
        outer: String,
        inner: String,
    },
    /// The same volume is already mounted, or backs the root.
    DuplicateVolume {
        volume: String,
        at: String,
    },
    TooMany,
    /// The mount point exists in the root but is not a plain directory, or
    /// does not exist in a read-only root, which it may not create.
    MountPointUnavailable {
        path: String,
        reason: &'static str,
    },
    /// Preparing the mount point in the root failed.
    RootIo(io::Error),
    /// Opening the volume's directory failed. Carries no host path; the
    /// caller logs that.
    Io(io::Error),
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootDisabled => {
                f.write_str("mounts need a filesystem root, and vfs mode `none` has none")
            }
            Self::BadPath(path) => write!(
                f,
                "mount path `{path}` must be absolute and normalized, made of ASCII letters, \
                 digits, `.`, `_` and `-`, such as `/memory`"
            ),
            Self::AtRoot => f.write_str("a volume cannot be mounted at `/`; that is the root"),
            Self::ProtectedPath(path) => {
                write!(f, "mount path `{path}` names Git metadata")
            }
            Self::Nested { outer, inner } => {
                write!(
                    f,
                    "mount `{inner}` is inside mount `{outer}`; mounts may not nest"
                )
            }
            Self::DuplicateVolume { volume, at } => {
                write!(f, "volume `{volume}` is already mounted at `{at}`")
            }
            Self::TooMany => write!(f, "more than {MAX_MOUNTS} mounts"),
            Self::MountPointUnavailable { path, reason } => {
                write!(f, "mount point `{path}` {reason}")
            }
            Self::RootIo(err) => write!(f, "the mount point could not be prepared: {err}"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for MountError {}

/// The most mounts one VFS carries; routing a path scans them all. The blueprint
/// parser caps `vfs.mounts` at the same number (`submilli_blueprint::MAX_MOUNTS`)
/// so a valid blueprint never reaches this; it holds for direct embedders.
pub const MAX_MOUNTS: usize = 16;

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
            access: Access::ReadWrite,
            volume: None,
            mounts: Arc::from([]),
            cwd: Arc::from("/"),
            ancestors: Arc::from([]),
        }
    }

    /// Mount an existing host directory the interpreter does not own. Used for
    /// `named` roots and for `per_session` dirs owned by the server.
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
        let identity = dir_identity(&dir, Path::new("."), &dir.dir_metadata()?)?;
        Ok(Self {
            root,
            mode,
            dir: Some(dir),
            _owned: None,
            quota: None,
            access: Access::ReadWrite,
            volume: None,
            mounts: Arc::from([]),
            cwd: Arc::from("/"),
            ancestors: Arc::from([identity]),
        })
    }

    /// An existing host directory, as `submilli run --vfs` exposes it.
    pub fn external(root: PathBuf) -> io::Result<Self> {
        Self::external_with_mode(root, VfsMode::Named)
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
        let identity = dir_identity(&dir, Path::new("."), &dir.dir_metadata()?)?;
        Ok(Self {
            root,
            mode: VfsMode::Ephemeral,
            dir: Some(dir),
            _owned: Some(Arc::new(td)),
            quota: None,
            access: Access::ReadWrite,
            volume: None,
            mounts: Arc::from([]),
            cwd: Arc::from("/"),
            ancestors: Arc::from([identity]),
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

    /// Charge the root to `quota`, a limit the server shares between every VFS
    /// that opens the same named volume.
    pub fn with_shared_quota(mut self, quota: Arc<DiskQuota>) -> Self {
        self.quota = Some(quota);
        self
    }

    /// The root's size limit and its running count, when one applies.
    pub fn quota(&self) -> Option<&Arc<DiskQuota>> {
        self.quota.as_ref()
    }

    /// Whether guest code may change the root. Mounts carry their own access.
    pub fn with_access(mut self, access: Access) -> Self {
        self.access = access;
        self
    }

    pub fn access(&self) -> Access {
        self.access
    }

    /// Record the named volume backing the root, for `fs.info()`.
    pub fn with_volume_name(mut self, volume: &str) -> Self {
        self.volume = Some(Arc::from(volume));
        self
    }

    pub fn volume(&self) -> Option<&str> {
        self.volume.as_deref()
    }

    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    /// Graft the volume `spec` describes at its guest path.
    ///
    /// In a writable root the mount point is created as an empty directory, so
    /// listing its parent shows it like any other directory. A read-only root is
    /// never written, so there the mount point must already exist.
    pub fn with_mount(self, spec: MountSpec) -> Result<Self, MountError> {
        self.with_mount_subpath(spec, None)
    }

    pub fn with_mount_subpath(
        mut self,
        spec: MountSpec,
        sub_path: Option<&str>,
    ) -> Result<Self, MountError> {
        let root = self.dir.as_ref().ok_or(MountError::RootDisabled)?;
        if self.mounts.len() >= MAX_MOUNTS {
            return Err(MountError::TooMany);
        }
        let rel = mount_rel(&spec.guest_path)?;
        if crate::runtime::fs::protected_metadata(&rel) {
            return Err(MountError::ProtectedPath(spec.guest_path));
        }
        // Case is folded as the blueprint parser folds it, so two spellings of one
        // directory on a case-insensitive filesystem cannot both be mounted.
        for mount in self.mounts.iter() {
            let (outer, inner) = if starts_with_folded(&rel, &mount.rel) {
                (mount.guest.to_string(), spec.guest_path.clone())
            } else if starts_with_folded(&mount.rel, &rel) {
                (spec.guest_path.clone(), mount.guest.to_string())
            } else {
                continue;
            };
            return Err(MountError::Nested { outer, inner });
        }
        let placeholders = prepare_mount_point(root, &rel, self.access, &spec.guest_path)?;
        let (dir, ancestors) = select_directory(
            open_volume(&spec.host).map_err(MountError::Io)?,
            sub_path,
            spec.access,
        )
        .map_err(MountError::Io)?;
        let mount = Mount {
            guest: Arc::from(spec.guest_path.as_str()),
            rel,
            volume: Arc::from(spec.volume.as_str()),
            dir,
            host: Arc::from(
                sub_path.map_or_else(|| spec.host.clone(), |path| spec.host.join(path)),
            ),
            access: spec.access,
            quota: spec.quota,
            placeholders: Arc::from(placeholders),
            ancestors: Arc::from(ancestors),
        };
        let mut mounts = self.mounts.to_vec();
        mounts.push(mount);
        self.mounts = Arc::from(mounts);
        Ok(self)
    }

    /// Charge the volume mounted at `guest_path` to `quota`, the limit the
    /// server shares between every VFS that mounts it. No mount there is a
    /// no-op.
    pub fn with_mount_quota(mut self, guest_path: &str, quota: Arc<DiskQuota>) -> Self {
        let mounts: Vec<Mount> = self
            .mounts
            .iter()
            .cloned()
            .map(|mut mount| {
                if &*mount.guest == guest_path {
                    mount.quota = Some(Arc::clone(&quota));
                }
                mount
            })
            .collect();
        self.mounts = Arc::from(mounts);
        self
    }

    /// Route a root-relative path (as the lexical pass produced it) to the
    /// volume that holds it: the handle to open it through, the path relative
    /// to that handle, and the volume's placement. `None` under
    /// [`VfsMode::None`].
    ///
    /// Matching is by whole components, so `/memoryx` is not under `/memory`.
    /// A mount point itself resolves to its volume's root, `.`.
    pub fn locate(&self, rel: &Path) -> Option<(Arc<Dir>, PathBuf, Placement)> {
        let root = self.dir.as_ref()?;
        let (protected_roots, protected_ancestors) = self.protected_directories();
        for mount in self.mounts.iter() {
            if let Ok(rest) = rel.strip_prefix(&mount.rel) {
                let rest = if rest.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    rest.to_path_buf()
                };
                let placement = Placement {
                    access: mount.access,
                    quota: mount.quota.clone(),
                    mount: Some(Arc::clone(&mount.guest)),
                    nested: Arc::from([]),
                    host: Arc::clone(&mount.host),
                    volume: Some(Arc::clone(&mount.volume)),
                    ancestors: Arc::clone(&mount.ancestors),
                    protected_roots,
                    protected_ancestors,
                };
                return Some((Arc::clone(&mount.dir), rest, placement));
            }
        }
        let placement = Placement {
            access: self.access,
            quota: self.quota.clone(),
            mount: None,
            nested: Arc::clone(&self.mounts),
            host: Arc::from(self.root.as_path()),
            volume: self.volume.clone(),
            ancestors: Arc::clone(&self.ancestors),
            protected_roots,
            protected_ancestors,
        };
        Some((Arc::clone(root), rel.to_path_buf(), placement))
    }

    fn protected_directories(&self) -> (Arc<[FileIdentity]>, Arc<[FileIdentity]>) {
        let mut roots = Vec::new();
        let mut ancestors = self
            .ancestors
            .split_last()
            .map_or(&[][..], |(_, parents)| parents)
            .to_vec();
        if let Some(identity) = self.ancestors.last() {
            roots.push(*identity);
            if !self.mounts.is_empty() {
                ancestors.push(*identity);
            }
        }
        for mount in self.mounts.iter() {
            if let Some(identity) = mount.ancestors.last() {
                roots.push(*identity);
            }
            if let Some((_, parents)) = mount.ancestors.split_last() {
                ancestors.extend_from_slice(parents);
            }
            // Git must also avoid publishing inside hidden placeholders.
            for placeholder in mount.placeholders.iter() {
                ancestors.push(placeholder.identity);
            }
            if let Some(placeholder) = mount.placeholders.last() {
                roots.push(placeholder.identity);
            }
        }
        (Arc::from(roots), Arc::from(ancestors))
    }

    /// Guest working directory shared by I/O, policy, and introspection.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn with_cwd(mut self, cwd: &str) -> Result<Self, crate::runtime::fs::ContainError> {
        use crate::runtime::fs::{ContainError, guest_normalize, resolve_content};
        if !cwd.starts_with('/') || cwd.len() > 4096 || guest_normalize("/", cwd)? != cwd {
            return Err(ContainError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cwd must be an absolute normalized guest path",
            )));
        }
        let target = resolve_content(&self, "/", cwd)?;
        match target.open_dir() {
            Ok(_) => {}
            Err(ContainError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                target.create_dir_all()?;
                target.open_dir()?;
            }
            Err(error) => return Err(error),
        }
        self.cwd = Arc::from(cwd);
        Ok(self)
    }

    /// Select a directory through the volume handle, without ambient reopening.
    pub fn with_subpath(mut self, sub_path: Option<&str>) -> io::Result<Self> {
        let root = self
            .dir
            .clone()
            .ok_or_else(|| io::Error::other("filesystem disabled"))?;
        let (dir, ancestors) = select_directory(root, sub_path, self.access)?;
        self.dir = Some(dir);
        if let Some(path) = sub_path {
            self.root.push(path);
        }
        self.ancestors = Arc::from(ancestors);
        Ok(self)
    }

    /// The bytes held by regular files under the root; see [`measure_dir`].
    pub fn measure_usage(&self) -> io::Result<u64> {
        match &self.dir {
            Some(root) => measure_dir(root),
            None => Ok(0),
        }
    }
}

/// The bytes held by regular files under the host directory `root`, as a size
/// limit starting from it counts them. The error carries no host path; a tree with too
/// many entries or nested too deep is [`io::ErrorKind::InvalidData`].
pub fn measure_host_dir(root: &Path) -> io::Result<u64> {
    measure_dir(&*open_volume(root)?)
}

/// As [`measure_host_dir`], but an entry removed mid-walk is not counted instead of
/// failing the measure: for a tree being copied, never for a size limit's accounting.
pub fn measure_host_dir_skipping_vanished(root: &Path) -> io::Result<u64> {
    let mut total: u64 = 0;
    let root = open_volume(root)?;
    for_each_regular_file(&root, MAX_MEASURED_ENTRIES, true, |_, bytes| {
        total = total.saturating_add(bytes);
    })?;
    Ok(total)
}

/// The bytes held by regular files under `root`; see [`for_each_regular_file`].
pub fn measure_dir(root: &Dir) -> io::Result<u64> {
    let mut total: u64 = 0;
    for_each_regular_file(root, MAX_MEASURED_ENTRIES, false, |_, bytes| {
        total = total.saturating_add(bytes);
    })?;
    Ok(total)
}

/// Every regular file under `root` and its size, as a removal of the tree frees
/// them; more than `max_entries` entries is an [`io::ErrorKind::InvalidData`] error.
pub fn regular_files(root: &Dir, max_entries: usize) -> io::Result<Vec<(FileIdentity, u64)>> {
    let mut files = Vec::new();
    for_each_regular_file(root, max_entries, false, |file, bytes| {
        files.push((file, bytes));
    })?;
    Ok(files)
}

/// Visit each regular file under `root`. Links are never followed, so a link cannot
/// make a file count twice. The walk goes depth first, holding one open directory
/// per level, so its descriptors and memory grow with depth, not with the tree.
///
/// An entry that cannot be read is an error, so a size limit treats the tree as full;
/// with `skip_vanished` one that was removed meanwhile (`NotFound`) is left out instead.
fn for_each_regular_file(
    root: &Dir,
    max_entries: usize,
    skip_vanished: bool,
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
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("more than {max_entries} entries"),
            ));
        }
        let Some(kind) = vanished(entry.file_type(), skip_vanished)? else {
            continue;
        };
        if kind.is_dir() {
            if open.len() > MAX_MEASURED_DEPTH {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("directories nested more than {MAX_MEASURED_DEPTH} deep"),
                ));
            }
            let Some(child) = vanished(
                entry.open_dir().and_then(|dir| dir.entries()),
                skip_vanished,
            )?
            else {
                continue;
            };
            open.push(child);
        } else if kind.is_file() {
            // Full metadata, which Windows reads through a handle, carries the
            // file's identity.
            let Some(metadata) = vanished(
                cap_fs_ext::DirEntryExt::full_metadata(&entry),
                skip_vanished,
            )?
            else {
                continue;
            };
            visit(FileIdentity::of(&metadata)?, metadata.len());
        }
    }
    Ok(())
}

/// `None` when the entry was removed meanwhile and `skip` is set.
fn vanished<T>(result: io::Result<T>, skip: bool) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if skip && error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Why [`copy_host_dir`] stopped.
#[derive(Debug)]
pub enum CopyDirError {
    Io(io::Error),
    /// The copy went past its byte cap.
    OverCap,
    /// More entries or deeper nesting than a walk goes through, as for [`measure_host_dir`].
    TooMany,
}

impl From<io::Error> for CopyDirError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Copies the host directory `from` to `to`, adding the bytes copied to `bytes` and stopping
/// once they pass `cap`.
///
/// Nothing under `from` is followed: every entry is opened relative to its already open
/// parent without following links, so a program swapping an entry for a link mid-copy
/// cannot get a file from outside the tree copied. A link is recreated as a link; an entry
/// removed meanwhile is skipped. Only `from` itself, which the caller vouches for, is
/// resolved by path.
pub fn copy_host_dir(
    from: &Path,
    to: &Path,
    cap: u64,
    bytes: &mut u64,
) -> Result<(), CopyDirError> {
    let source = open_volume(from)?;
    std::fs::create_dir_all(to)?;
    let target = Dir::open_ambient_dir(to, ambient_authority())?;
    let mut copy = TreeCopy {
        cap,
        bytes,
        entries: 0,
        rel: PathBuf::new(),
    };
    copy.dir(&source, &target, 0)
}

#[derive(Clone, Copy)]
enum EntryKind {
    Dir,
    File,
    Link,
    Other,
}

impl EntryKind {
    fn of(file_type: cap_std::fs::FileType) -> Self {
        if file_type.is_dir() {
            Self::Dir
        } else if file_type.is_file() {
            Self::File
        } else if file_type.is_symlink() {
            Self::Link
        } else {
            Self::Other
        }
    }
}

struct TreeCopy<'a> {
    cap: u64,
    bytes: &'a mut u64,
    entries: usize,
    /// The directory being copied, relative to the root, for errors.
    rel: PathBuf,
}

impl TreeCopy<'_> {
    /// An error naming the entry it came from, relative to the root.
    fn at(&self, name: &OsStr, error: io::Error) -> CopyDirError {
        let path = self.rel.join(name);
        let shown = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &path
        };
        io::Error::new(error.kind(), format!("{}: {error}", shown.display())).into()
    }

    fn dir(&mut self, source: &Dir, target: &Dir, depth: usize) -> Result<(), CopyDirError> {
        let entries = source
            .entries()
            .map_err(|error| self.at(OsStr::new(""), error))?;
        for entry in entries {
            let entry = entry.map_err(|error| self.at(OsStr::new(""), error))?;
            self.entries += 1;
            if self.entries > MAX_MEASURED_ENTRIES {
                return Err(CopyDirError::TooMany);
            }
            let Some(file_type) = vanished(entry.file_type(), true)? else {
                continue;
            };
            let kind = EntryKind::of(file_type);
            self.entry(source, target, &entry.file_name(), kind, depth)?;
        }
        Ok(())
    }

    /// Copies the entry `name` of `source`, which was listed as `kind`. An entry that is
    /// another kind by the time it is opened is copied as what it is, whatever it was listed as.
    fn entry(
        &mut self,
        source: &Dir,
        target: &Dir,
        name: &OsStr,
        kind: EntryKind,
        depth: usize,
    ) -> Result<(), CopyDirError> {
        match kind {
            EntryKind::Dir => self.subdir(source, target, name, depth),
            EntryKind::File => self.file(source, target, name, false),
            EntryKind::Link => self.link(source, target, name, false),
            EntryKind::Other => Ok(()),
        }
    }

    fn subdir(
        &mut self,
        source: &Dir,
        target: &Dir,
        name: &OsStr,
        depth: usize,
    ) -> Result<(), CopyDirError> {
        if depth >= MAX_MEASURED_DEPTH {
            return Err(CopyDirError::TooMany);
        }
        let child = match source.open_dir_nofollow(name) {
            Ok(child) => child,
            Err(error) => return self.changed(source, target, name, EntryKind::Dir, error, false),
        };
        match target.create_dir(name) {
            Ok(()) => {}
            // Listed twice.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
            Err(error) => return Err(self.at(name, error)),
        }
        let copied = target
            .open_dir(name)
            .map_err(|error| self.at(name, error))?;
        self.rel.push(name);
        let result = self.dir(&child, &copied, depth + 1);
        self.rel.pop();
        result
    }

    /// `reclassified`: the entry already turned out another kind than it was listed as.
    fn file(
        &mut self,
        source: &Dir,
        target: &Dir,
        name: &OsStr,
        reclassified: bool,
    ) -> Result<(), CopyDirError> {
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        // A device or pipe swapped in must not block the open, nor become our terminal.
        #[cfg(unix)]
        cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK | libc::O_NOCTTY);
        let mut input = match source.open_with(name, &options) {
            Ok(file) => file,
            Err(error) => {
                return self.changed(source, target, name, EntryKind::File, error, reclassified);
            }
        };
        let metadata = input.metadata().map_err(|error| self.at(name, error))?;
        if !metadata.is_file() {
            return Ok(());
        }
        let mut output = match target.open_with(
            name,
            cap_std::fs::OpenOptions::new().write(true).create_new(true),
        ) {
            Ok(file) => file,
            // Listed twice.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
            Err(error) => return Err(self.at(name, error)),
        };
        // One byte past what is left is enough to tell the cap was passed.
        let room = self.cap.saturating_sub(*self.bytes).saturating_add(1);
        let copied = io::copy(&mut input.by_ref().take(room), &mut output)
            .map_err(|error| self.at(name, error))?;
        *self.bytes = self.bytes.saturating_add(copied);
        if *self.bytes > self.cap {
            return Err(CopyDirError::OverCap);
        }
        output
            .set_permissions(metadata.permissions())
            .map_err(|error| self.at(name, error))?;
        Ok(())
    }

    fn link(
        &mut self,
        source: &Dir,
        target: &Dir,
        name: &OsStr,
        reclassified: bool,
    ) -> Result<(), CopyDirError> {
        // The target is kept as written, where `read_link` would refuse one that leaves
        // the directory; it is never resolved here.
        let to = match source.read_link_contents(name) {
            Ok(to) => to,
            Err(error) => {
                return self.changed(source, target, name, EntryKind::Link, error, reclassified);
            }
        };
        #[cfg(not(windows))]
        match target.symlink_contents(to, name) {
            Ok(()) => {}
            // Listed twice.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(self.at(name, error)),
        }
        #[cfg(windows)]
        let _ = to;
        Ok(())
    }

    /// Settles an entry that could not be handled as `listed`: gone is skipped, and one that
    /// is now another kind is copied as that, by its type without following links. An entry
    /// still of the kind listed fails the copy as unreadable, unless the error says it is not
    /// that kind (it flipped back meanwhile), which skips it like a vanished one. An entry is
    /// re-classified once: one that changed kind again meanwhile is skipped the same way, so a
    /// program flipping it cannot keep the copy at it.
    fn changed(
        &mut self,
        source: &Dir,
        target: &Dir,
        name: &OsStr,
        listed: EntryKind,
        error: io::Error,
        reclassified: bool,
    ) -> Result<(), CopyDirError> {
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(());
        }
        if reclassified {
            return if wrong_kind(&error) {
                Ok(())
            } else {
                Err(self.at(name, error))
            };
        }
        let now = match source.symlink_metadata(name) {
            Ok(meta) => EntryKind::of(meta.file_type()),
            Err(gone) if gone.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(self.at(name, error)),
        };
        match (listed, now) {
            (EntryKind::Dir | EntryKind::Link, EntryKind::File) => {
                self.file(source, target, name, true)
            }
            (EntryKind::Dir | EntryKind::File, EntryKind::Link) => {
                self.link(source, target, name, true)
            }
            // Swapped for a directory since it was listed: skipped like a vanished entry.
            (EntryKind::File | EntryKind::Link, EntryKind::Dir) | (_, EntryKind::Other) => Ok(()),
            // Still the kind listed: skipped only if the error says it is not that kind, as
            // when it flipped back since the check.
            _ if wrong_kind(&error) => Ok(()),
            _ => Err(self.at(name, error)),
        }
    }
}

/// Whether `error` is the one an open or `read_link` gives an entry of another kind than
/// asked for: not a directory, a followed link refused, or not a link.
fn wrong_kind(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(
            error.raw_os_error(),
            Some(libc::ENOTDIR | libc::ELOOP | libc::EINVAL)
        )
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

/// How many entries [`for_each_regular_file`] walks before it gives up, so a directory
/// crafted to be huge can't stall every run that opens it.
const MAX_MEASURED_ENTRIES: usize = 1_000_000;

/// How many directories deep below the root [`for_each_regular_file`] descends
/// before it gives up, which bounds the directories it holds open at once.
pub const MAX_MEASURED_DEPTH: usize = 64;

/// The root-relative form of a mount's guest path, refusing anything that is
/// not absolute, normalized, and made of ASCII letters, digits, `.`, `_` and
/// `-`. ASCII alone is what the case-folded mount-point guard compares
/// reliably on hosts whose filesystems fold case. The blueprint parser applies
/// the same rule; this repeats it for direct embedders.
fn mount_rel(guest: &str) -> Result<PathBuf, MountError> {
    let bad = || MountError::BadPath(guest.to_string());
    let Some(rest) = guest.strip_prefix('/') else {
        return Err(bad());
    };
    if rest.is_empty() {
        return Err(MountError::AtRoot);
    }
    let mut rel = PathBuf::new();
    for part in rest.split('/') {
        let plain = part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        // A trailing dot is dropped by Windows, which would make `memory.` and
        // `memory` one directory.
        if part.is_empty() || part.ends_with('.') || !plain {
            return Err(bad());
        }
        rel.push(part);
    }
    Ok(rel)
}

/// Whether `path` is `prefix` or below it, comparing whole components with
/// ASCII case folded.
fn starts_with_folded(path: &Path, prefix: &Path) -> bool {
    let mut parts = path.components();
    prefix.components().all(|want| {
        parts.next().is_some_and(|have| {
            have.as_os_str()
                .as_encoded_bytes()
                .eq_ignore_ascii_case(want.as_os_str().as_encoded_bytes())
        })
    })
}

/// Make sure the mount point is a plain directory in the root, creating it (and
/// its parents) only when the root may be written.
///
/// Returns each directory on the way, the mount point last; see
/// [`placeholders_on`].
fn prepare_mount_point(
    root: &Dir,
    rel: &Path,
    access: Access,
    guest: &str,
) -> Result<Vec<Placeholder>, MountError> {
    let unavailable = |reason| MountError::MountPointUnavailable {
        path: guest.to_string(),
        reason,
    };
    let mut prefix = PathBuf::new();
    for part in rel.components() {
        prefix.push(part);
        match root.symlink_metadata(&prefix) {
            Ok(meta) if meta.is_dir() => {
                match spelling_of(root, &prefix).map_err(MountError::RootIo)? {
                    Spelling::Exact => {}
                    Spelling::Variant => {
                        return Err(unavailable(
                            "exists in the root under a different letter case; rename it to \
                             match",
                        ));
                    }
                    Spelling::Unknown => {
                        return Err(unavailable(
                            "sits beside too many entries to check its spelling; move some \
                             of them into a subdirectory",
                        ));
                    }
                }
            }
            Ok(_) => return Err(unavailable("exists in the root and is not a directory")),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if access == Access::ReadOnly {
                    return Err(unavailable(
                        "does not exist, and a read-only root cannot create it",
                    ));
                }
                match root.create_dir(&prefix) {
                    Ok(()) => {}
                    // Another run created it between the check and here.
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(err) => return Err(MountError::RootIo(err)),
                }
            }
            Err(err) => return Err(MountError::RootIo(err)),
        }
    }
    // Recheck: a racing writer may have swapped a link in after creation.
    match root.symlink_metadata(rel) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(unavailable("exists in the root and is not a directory")),
        Err(err) => return Err(MountError::RootIo(err)),
    }
    placeholders_on(root, rel).map_err(MountError::RootIo)
}

/// The identity and exact name of each directory on the way to `rel`, `rel`
/// last, taken after the recheck so a directory swapped in before it isn't the
/// one recorded. An identity that can't be read fails the mount: without it an
/// alias of the mount point would go unrecognized.
fn placeholders_on(root: &Dir, rel: &Path) -> io::Result<Vec<Placeholder>> {
    let mut placeholders = Vec::new();
    let mut prefix = PathBuf::new();
    for part in rel.components() {
        prefix.push(part);
        let meta = root.symlink_metadata(&prefix)?;
        placeholders.push(Placeholder {
            identity: dir_identity(root, &prefix, &meta)?,
            parent: prefix.parent().map(Path::to_path_buf).unwrap_or_default(),
            name: part.as_os_str().to_os_string(),
        });
    }
    Ok(placeholders)
}

/// How an existing directory on the way to a mount point is spelled on disk.
enum Spelling {
    Exact,
    /// Found only through a case-insensitive lookup.
    Variant,
    /// Too many entries beside it to tell.
    Unknown,
}

/// Whether the directory entry at `path` is spelled exactly so, not merely found
/// through a case-insensitive lookup. Recursive listings descend into a mount by
/// matching the entry's name, so a case variant standing in as the mount point
/// would hide the volume behind the root's own directory.
///
/// Checked on every host, since a Linux filesystem can fold case too (ext4
/// casefold, or a directory shared in from a Mac). Where the other case finds
/// nothing, or a different directory, the filesystem distinguishes case here and
/// the exact lookup that found `path` settles it; only a case-folding directory
/// is scanned for the exact name.
fn spelling_of(root: &Dir, path: &Path) -> io::Result<Spelling> {
    let Some(name) = path.file_name() else {
        return Ok(Spelling::Exact);
    };
    let flipped: String = name
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    if flipped.as_str() == name {
        return Ok(Spelling::Exact);
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let other = match root.symlink_metadata(parent.join(&flipped)) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Spelling::Exact),
        Err(err) => return Err(err),
    };
    let this = root.symlink_metadata(path)?;
    if let (Ok(this), Ok(other)) = (FileIdentity::of(&this), FileIdentity::of(&other))
        && this != other
    {
        return Ok(Spelling::Exact);
    }
    let entries = if parent.as_os_str().is_empty() {
        root.entries()?
    } else {
        root.read_dir(parent)?
    };
    for (index, entry) in entries.enumerate() {
        if index >= MAX_SPELLING_SCAN {
            return Ok(Spelling::Unknown);
        }
        if entry?.file_name() == name {
            return Ok(Spelling::Exact);
        }
    }
    Ok(Spelling::Variant)
}

/// How many entries [`spelling_of`] reads beside a mount point before giving
/// up, so a huge directory can't slow every run that mounts below it.
const MAX_SPELLING_SCAN: usize = 100_000;

fn open_volume(host: &Path) -> io::Result<Arc<Dir>> {
    let meta = std::fs::metadata(host)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "volume is not a directory",
        ));
    }
    Dir::open_ambient_dir(host, ambient_authority()).map(Arc::new)
}

fn select_directory(
    mut dir: Arc<Dir>,
    sub_path: Option<&str>,
    access: Access,
) -> io::Result<(Arc<Dir>, Vec<FileIdentity>)> {
    let mut ancestors = vec![dir_identity(&dir, Path::new("."), &dir.dir_metadata()?)?];
    let Some(path) = sub_path else {
        return Ok((dir, ancestors));
    };
    if path.is_empty()
        || path.len() > 4096
        || path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.contains(['\\', '\0'])
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "subPath must be a normalized relative path",
        ));
    }
    for component in path.split('/') {
        let child = match dir.open_dir_nofollow(component) {
            Ok(child) => child,
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && access == Access::ReadWrite =>
            {
                match dir.create_dir(component) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                dir.open_dir_nofollow(component)?
            }
            Err(error) => return Err(error),
        };
        ancestors.push(dir_identity(
            &child,
            Path::new("."),
            &child.dir_metadata()?,
        )?);
        dir = Arc::new(child);
    }
    Ok((dir, ancestors))
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
    fn subpath_creation_and_readonly_cwd() {
        let volume = tempfile::tempdir().unwrap();
        let vfs = Vfs::external(volume.path().into())
            .unwrap()
            .with_subpath(Some("users/ada"))
            .unwrap();
        assert!(volume.path().join("users/ada").is_dir());
        let vfs = vfs.with_cwd("/notes/drafts").unwrap();
        assert_eq!(vfs.cwd(), "/notes/drafts");
        assert!(volume.path().join("users/ada/notes/drafts").is_dir());
        let readonly = vfs.with_access(Access::ReadOnly);
        assert!(readonly.clone().with_cwd("/notes").is_ok());
        assert!(readonly.with_cwd("/missing").is_err());
        assert!(
            Vfs::external(volume.path().into())
                .unwrap()
                .with_access(Access::ReadOnly)
                .with_subpath(Some("absent"))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn subpath_selection_rejects_symlinks() {
        let volume = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(volume.path().join("users/ada")).unwrap();
        std::os::unix::fs::symlink("ada", volume.path().join("users/alias")).unwrap();
        let vfs = Vfs::external(volume.path().into()).unwrap();
        assert!(vfs.with_subpath(Some("users/alias")).is_err());
    }

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
        let dir = vfs.dir().expect("an external root backs a directory");
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
        let error = regular_files(dir, 2).expect_err("a third entry passes the cap");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
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

    #[test]
    fn only_a_walk_that_skips_vanished_entries_survives_one() {
        let gone = || io::Result::<u8>::Err(io::ErrorKind::NotFound.into());
        // A size limit's measure fails, so the directory opens as full.
        assert!(vanished(gone(), false).is_err());
        assert_eq!(vanished(gone(), true).expect("skipped"), None);
        let denied = io::Result::<u8>::Err(io::ErrorKind::PermissionDenied.into());
        assert!(vanished(denied, true).is_err());
        assert_eq!(vanished(Ok(1u8), false).expect("ok"), Some(1));
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join("a"), "abc").expect("write");
        assert_eq!(measure_host_dir(dir.path()).expect("strict"), 3);
        assert_eq!(
            measure_host_dir_skipping_vanished(dir.path()).expect("lenient"),
            3
        );
    }

    fn copied(from: &Path, cap: u64) -> (tempfile::TempDir, Result<u64, CopyDirError>) {
        let to = tempfile::tempdir().expect("to");
        let mut bytes = 0;
        let result = copy_host_dir(from, &to.path().join("out"), cap, &mut bytes).map(|()| bytes);
        (to, result)
    }

    #[test]
    fn a_copy_has_the_files_and_stops_at_the_cap() {
        let from = tempfile::tempdir().expect("from");
        std::fs::create_dir(from.path().join("sub")).expect("mkdir");
        std::fs::write(from.path().join("sub/a.txt"), "hello").expect("write");
        let (to, result) = copied(from.path(), 100);
        assert_eq!(result.expect("copied"), 5);
        assert_eq!(
            std::fs::read_to_string(to.path().join("out/sub/a.txt")).expect("read"),
            "hello"
        );
        let (_to, result) = copied(from.path(), 4);
        assert!(matches!(result, Err(CopyDirError::OverCap)));
    }

    #[cfg(unix)]
    #[test]
    fn a_copy_recreates_links_and_never_follows_them() {
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret"), "host file").expect("write");
        std::fs::create_dir(outside.path().join("dir")).expect("mkdir");
        std::fs::write(outside.path().join("dir/inner"), "host dir file").expect("write");
        let from = tempfile::tempdir().expect("from");
        std::os::unix::fs::symlink(outside.path().join("secret"), from.path().join("file-link"))
            .expect("symlink");
        std::os::unix::fs::symlink(outside.path().join("dir"), from.path().join("dir-link"))
            .expect("symlink");
        let (to, result) = copied(from.path(), 1_000);
        assert_eq!(result.expect("copied"), 0, "no linked content is copied");
        for name in ["file-link", "dir-link"] {
            let copy = to.path().join("out").join(name);
            assert!(std::fs::symlink_metadata(&copy).expect("kept").is_symlink());
        }
        assert!(!to.path().join("out/dir-link-copy").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_that_is_a_link_when_opened_is_not_followed() {
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret"), "host file").expect("write");
        std::fs::create_dir(outside.path().join("dir")).expect("mkdir");
        let from = tempfile::tempdir().expect("from");
        let to = tempfile::tempdir().expect("to");
        std::os::unix::fs::symlink(outside.path().join("secret"), from.path().join("swapped"))
            .expect("symlink");
        std::os::unix::fs::symlink(outside.path().join("dir"), from.path().join("swapped-dir"))
            .expect("symlink");
        let source = Dir::open_ambient_dir(from.path(), ambient_authority()).expect("source");
        let target = Dir::open_ambient_dir(to.path(), ambient_authority()).expect("target");
        let mut bytes = 0;
        let mut copy = TreeCopy {
            cap: 1_000,
            bytes: &mut bytes,
            entries: 0,
            rel: PathBuf::new(),
        };
        // Listed as a file and a directory, as they were before being swapped.
        copy.entry(&source, &target, OsStr::new("swapped"), EntryKind::File, 0)
            .expect("file");
        copy.entry(
            &source,
            &target,
            OsStr::new("swapped-dir"),
            EntryKind::Dir,
            0,
        )
        .expect("dir");
        assert_eq!(bytes, 0);
        for name in ["swapped", "swapped-dir"] {
            let copy = to.path().join(name);
            assert!(std::fs::symlink_metadata(&copy).expect("kept").is_symlink());
        }
    }

    #[test]
    fn an_entry_removed_since_it_was_listed_is_skipped() {
        let from = tempfile::tempdir().expect("from");
        let to = tempfile::tempdir().expect("to");
        let source = Dir::open_ambient_dir(from.path(), ambient_authority()).expect("source");
        let target = Dir::open_ambient_dir(to.path(), ambient_authority()).expect("target");
        let mut bytes = 0;
        let mut copy = TreeCopy {
            cap: 1_000,
            bytes: &mut bytes,
            entries: 0,
            rel: PathBuf::new(),
        };
        for kind in [EntryKind::Dir, EntryKind::File, EntryKind::Link] {
            copy.entry(&source, &target, OsStr::new("gone"), kind, 0)
                .expect("skipped");
        }
        assert_eq!(std::fs::read_dir(to.path()).expect("list").count(), 0);
    }

    #[test]
    fn an_entry_that_changed_kind_since_it_was_listed_is_copied_as_what_it_is() {
        let from = tempfile::tempdir().expect("from");
        let to = tempfile::tempdir().expect("to");
        std::fs::write(from.path().join("was-dir"), "abc").expect("write");
        std::fs::write(from.path().join("was-link"), "de").expect("write");
        let source = Dir::open_ambient_dir(from.path(), ambient_authority()).expect("source");
        let target = Dir::open_ambient_dir(to.path(), ambient_authority()).expect("target");
        let mut bytes = 0;
        let mut copy = TreeCopy {
            cap: 1_000,
            bytes: &mut bytes,
            entries: 0,
            rel: PathBuf::new(),
        };
        copy.entry(&source, &target, OsStr::new("was-dir"), EntryKind::Dir, 0)
            .expect("dir now a file");
        copy.entry(&source, &target, OsStr::new("was-link"), EntryKind::Link, 0)
            .expect("link now a file");
        // The same name listed again is left as copied.
        copy.entry(&source, &target, OsStr::new("was-dir"), EntryKind::File, 0)
            .expect("duplicate");
        assert_eq!(bytes, 5);
        assert_eq!(
            std::fs::read_to_string(to.path().join("was-link")).expect("read"),
            "de"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_that_changes_kind_a_second_time_is_skipped_not_chased() {
        let outside = tempfile::tempdir().expect("outside");
        let from = tempfile::tempdir().expect("from");
        let to = tempfile::tempdir().expect("to");
        std::os::unix::fs::symlink(outside.path(), from.path().join("now-link")).expect("symlink");
        std::fs::write(from.path().join("now-file"), "abc").expect("write");
        let source = Dir::open_ambient_dir(from.path(), ambient_authority()).expect("source");
        let target = Dir::open_ambient_dir(to.path(), ambient_authority()).expect("target");
        let mut bytes = 0;
        let mut copy = TreeCopy {
            cap: 1_000,
            bytes: &mut bytes,
            entries: 0,
            rel: PathBuf::new(),
        };
        // Listed as a file, it is a link: copied as one, once.
        copy.file(&source, &target, OsStr::new("now-link"), false)
            .expect("re-classified once");
        assert!(
            std::fs::symlink_metadata(to.path().join("now-link"))
                .expect("kept")
                .is_symlink()
        );
        // Already re-classified, one that fails again is skipped, not re-classified again:
        // here a link that reads as a file, as if it had flipped back.
        copy.link(&source, &target, OsStr::new("now-file"), true)
            .expect("skipped");
        copy.file(&source, &target, OsStr::new("now-link"), true)
            .expect("skipped");
        assert!(!to.path().join("now-file").exists());
        assert_eq!(bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_that_flipped_back_is_skipped_but_an_unreadable_one_still_fails() {
        use std::os::unix::fs::PermissionsExt;
        let from = tempfile::tempdir().expect("from");
        std::fs::write(from.path().join("plain"), "abc").expect("write");
        std::fs::create_dir(from.path().join("dir")).expect("mkdir");
        let locked = from.path().join("locked");
        std::fs::write(&locked, "x").expect("write");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let source = Dir::open_ambient_dir(from.path(), ambient_authority()).expect("source");
        let to = tempfile::tempdir().expect("to");
        let target = Dir::open_ambient_dir(to.path(), ambient_authority()).expect("target");
        let mut bytes = 0;
        let mut copy = TreeCopy {
            cap: 1_000,
            bytes: &mut bytes,
            entries: 0,
            rel: PathBuf::new(),
        };
        // Listed as a directory, a file, and a link; each is still the kind that now fails
        // the open the way a flipped-back entry does.
        copy.entry(&source, &target, OsStr::new("plain"), EntryKind::Dir, 0)
            .expect("file listed as dir, re-classified");
        let not_a_dir = io::Error::from_raw_os_error(libc::ENOTDIR);
        copy.changed(
            &source,
            &target,
            OsStr::new("dir"),
            EntryKind::Dir,
            not_a_dir,
            false,
        )
        .expect("flipped back to a directory");
        let not_a_link = io::Error::from_raw_os_error(libc::EINVAL);
        copy.changed(
            &source,
            &target,
            OsStr::new("plain"),
            EntryKind::File,
            not_a_link,
            false,
        )
        .expect("flipped back to a file");
        let loop_error = io::Error::from_raw_os_error(libc::ELOOP);
        copy.changed(
            &source,
            &target,
            OsStr::new("plain"),
            EntryKind::Link,
            loop_error,
            true,
        )
        .expect("reclassified, wrong kind");
        // A privileged user reads it anyway.
        if unsafe { libc::geteuid() } != 0 {
            // Reclassified: only wrong-kind errors skip; an unreadable file fails.
            let result = copy.file(&source, &target, OsStr::new("locked"), true);
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600))
                .expect("chmod");
            let Err(CopyDirError::Io(error)) = result else {
                panic!("expected an I/O error, got {result:?}");
            };
            assert!(error.to_string().starts_with("locked: "), "{error}");
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        // Other errors on an entry still of the kind listed fail too.
        let denied = io::Error::from_raw_os_error(libc::EIO);
        let result = copy.changed(
            &source,
            &target,
            OsStr::new("dir"),
            EntryKind::Dir,
            denied,
            false,
        );
        assert!(matches!(result, Err(CopyDirError::Io(_))));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_entry_fails_the_copy_naming_its_path_in_the_tree() {
        use std::os::unix::fs::PermissionsExt;
        let from = tempfile::tempdir().expect("from");
        std::fs::create_dir(from.path().join("sub")).expect("mkdir");
        let locked = from.path().join("sub/locked");
        std::fs::write(&locked, "x").expect("write");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let (_to, result) = copied(from.path(), 1_000);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        // A privileged user reads it anyway.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let Err(CopyDirError::Io(error)) = result else {
            panic!("expected an I/O error, got {result:?}");
        };
        let message = error.to_string();
        assert!(message.starts_with("sub/locked: "), "{message}");
        assert!(!message.contains(from.path().to_str().expect("utf8")));
    }

    #[test]
    fn a_copy_stops_in_a_tree_nested_too_deep() {
        let from = tempfile::tempdir().expect("from");
        let mut path = from.path().to_path_buf();
        for _ in 0..=MAX_MEASURED_DEPTH + 1 {
            path = path.join("d");
        }
        std::fs::create_dir_all(&path).expect("mkdir");
        let (_to, result) = copied(from.path(), 1_000);
        assert!(matches!(result, Err(CopyDirError::TooMany)));
    }
}
