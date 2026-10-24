//! Contained disk access and private artifacts used to retry filesystem work.
//!
//! `Files` retains the configured VFS; `Volume` maps each writable mount to its
//! private recovery directory. Transient entries resolve paths relative to those
//! roots and validate the parent before opening it; journal states do not store them.
//!
//! This layer knows nothing about SQL transactions or which effect to run next.
//! Its callers supply retry facts and retain the mutation gate while disk work
//! runs. Sync barriers make artifacts durable before the journal refers to them.
//! The quota registry at the end shares counters across reopened VFS values.
use cap_std::fs::Dir;
use interpreter::runtime::Vfs;
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::durable::RunId;
use interpreter::runtime::fs::{ContainError, FileIdentity};
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

/// Private artifacts are hidden by this backend. Git's stage namespace also
/// prevents checkout/publication from installing this name.
pub(crate) const PRIVATE: &str = ".git-submilli-recovery";
pub(crate) const MAX_ENTRIES: usize = 10_000;
pub(crate) const MAX_DEPTH: usize = 64;

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

/// Capability-relative recovery access. Keep this value (and its VFS) alive
/// until all blocking work has drained, including when a request is cancelled.
#[derive(Clone)]
pub(crate) struct Files {
    pub(crate) vfs: Vfs,
    run_id: RunId,
    volumes: Arc<BTreeMap<String, Volume>>,
}

/// One backing root and its per-run recovery directory, with the same quota
/// counter used by public access to that storage.
pub(crate) struct Volume {
    root: Arc<Dir>,
    pub(crate) recovery: Dir,
    identity: FileIdentity,
    quota: Option<Arc<interpreter::runtime::DiskQuota>>,
}

impl Files {
    /// Open caller-owned roots and create private recovery directories.
    /// Journaled operations bind requests to these roots; byte writes rely on
    /// the execution framework to preserve configuration across retries.
    pub(crate) fn create(vfs: Vfs, run_id: &RunId) -> Result<Self, RecoveryError> {
        validate_id(run_id)?;
        if vfs.dir().is_none() {
            return Err(RecoveryError::Unsupported("filesystem is disabled"));
        }
        if vfs.is_disposable() {
            return Err(RecoveryError::Unsupported(
                "workspace is owned by a TempDir",
            ));
        }
        let volumes = open_volumes(&vfs, run_id, true)?;
        Ok(Self {
            vfs,
            run_id: run_id.clone(),
            volumes: Arc::new(volumes),
        })
    }

    pub fn identity(&self) -> Roots {
        Roots {
            run_id: self.run_id.clone(),
            volumes: self
                .volumes
                .iter()
                .map(|(mount, volume)| RootIdentity {
                    mount: mount.clone(),
                    root: volume.identity,
                })
                .collect(),
        }
    }

    /// Backing storage for trusted host integration. Guest access must go
    /// through the durable backend, which protects its private artifacts.
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    pub(crate) fn volume(&self, mount: &str) -> Result<&Volume, RecoveryError> {
        self.volumes.get(mount).ok_or(RecoveryError::Conflict)
    }
}

use cap_fs_ext::DirExt;
use cap_std::fs::Metadata;
use interpreter::runtime::fs::{self, ContentPath};
use interpreter::runtime::vfs::{Access, Placement};
use interpreter::runtime::{fuel, fuel::WorkBudget};
use std::collections::HashSet;

use std::path::Path;

pub(crate) fn resolve_content(
    vfs: &Vfs,
    cwd: &str,
    guest: &str,
) -> Result<ContentPath, fs::ContainError> {
    check_access(vfs, cwd, guest)?;
    fs::resolve_content(vfs, cwd, guest)
}

pub(crate) fn check_access(vfs: &Vfs, cwd: &str, guest: &str) -> Result<(), fs::ContainError> {
    locate(vfs, cwd, guest)?;
    Ok(())
}

struct Location {
    root: Arc<Dir>,
    relative: std::path::PathBuf,
    placement: Placement,
}

fn locate(vfs: &Vfs, cwd: &str, guest: &str) -> Result<Location, fs::ContainError> {
    vfs.dir().ok_or(fs::ContainError::Disabled)?;
    let normalized = fs::guest_normalize(cwd, guest)?;
    let relative = normalized.trim_start_matches('/');
    let relative = if relative.is_empty() {
        Path::new(".")
    } else {
        Path::new(relative)
    };
    let (root, relative, placement) = vfs.locate(relative).ok_or(fs::ContainError::Disabled)?;
    check_private_access(&root, &relative)?;
    Ok(Location {
        root,
        relative,
        placement,
    })
}

pub(crate) fn private_recovery_path(path: &Path) -> bool {
    path.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .trim_end_matches([' ', '.'])
            .eq_ignore_ascii_case(PRIVATE)
    })
}

fn check_private_access(root: &Dir, path: &Path) -> Result<(), fs::ContainError> {
    let denied = || {
        fs::ContainError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private filesystem recovery state",
        ))
    };
    if private_recovery_path(path) {
        return Err(denied());
    }
    // The run owns the volumes exclusively, so no guest can swap a link between
    // this check and the operation. Missing leaves still need their parent checked.
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match root.canonicalize(ancestor) {
            Ok(real) if private_recovery_path(&real) => return Err(denied()),
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(crate) fn validate_id(id: &str) -> Result<(), RecoveryError> {
    match uuid::Uuid::parse_str(id) {
        Ok(parsed) if parsed.to_string() == id => Ok(()),
        _ => Err(RecoveryError::InvalidRecord),
    }
}

pub(crate) fn validate_guest_path(path: &str) -> Result<(), RecoveryError> {
    if path.len() > 4096 || path.is_empty() || path.contains('\0') {
        return Err(RecoveryError::InvalidRecord);
    }
    fs::guest_normalize("/", path).map_err(interpreter::runtime::fs::ContainError::from)?;
    Ok(())
}

/// Independent counters require physically disjoint writable roots. Check held
/// directory identities, so symlink spellings and renamed host paths cannot
/// bypass this setup requirement. This never runs on the mutation path.
pub(crate) fn require_disjoint_volumes(vfs: &Vfs) -> Result<(), RecoveryError> {
    let mut roots = Vec::new();
    if let Some(root) = vfs.dir().filter(|_| vfs.access() == Access::ReadWrite) {
        roots.push(root);
    }
    roots.extend(
        vfs.mounts()
            .iter()
            .filter(|mount| mount.access() == Access::ReadWrite)
            .map(interpreter::runtime::vfs::Mount::dir),
    );
    if roots.len() < 2 {
        return Ok(());
    }
    let mut identities = HashSet::new();
    for root in &roots {
        if !identities.insert(identity(&root.dir_metadata()?)?) {
            return Err(RecoveryError::Unsupported("overlapping writable volumes"));
        }
    }
    for root in roots {
        let mut directory = root.try_clone()?;
        let mut directory_id = identity(&directory.dir_metadata()?)?;
        let mut depth = 0;
        loop {
            if depth == MAX_DEPTH {
                return Err(RecoveryError::Limit);
            }
            depth += 1;
            // Setup inspects ancestry of trusted configured handles, never a
            // guest path. Ambient authority is limited to parent metadata here.
            let parent = directory.open_parent_dir(cap_std::ambient_authority())?;
            let parent_id = identity(&parent.dir_metadata()?)?;
            if parent_id == directory_id {
                break;
            }
            if identities.contains(&parent_id) {
                return Err(RecoveryError::Unsupported("overlapping writable volumes"));
            }
            directory = parent;
            directory_id = parent_id;
        }
    }
    Ok(())
}

pub(crate) fn open_volumes(
    vfs: &Vfs,
    run_id: &RunId,
    create: bool,
) -> Result<BTreeMap<String, Volume>, RecoveryError> {
    let mut volumes = BTreeMap::new();
    if let Some(root) = vfs.dir().filter(|_| vfs.access() == Access::ReadWrite) {
        volumes.insert(
            "/".into(),
            open_volume(root, run_id, create, vfs.quota().cloned())?,
        );
    }
    for mount in vfs
        .mounts()
        .iter()
        .filter(|m| m.access() == Access::ReadWrite)
    {
        let volume = open_volume(mount.dir(), run_id, create, mount.quota().cloned())?;
        volumes.insert(mount.guest_path().into(), volume);
    }
    if volumes.is_empty() {
        return Err(RecoveryError::Unsupported("no writable volume"));
    }
    Ok(volumes)
}

fn open_volume(
    root: &Arc<Dir>,
    run_id: &RunId,
    create: bool,
    quota: Option<Arc<interpreter::runtime::DiskQuota>>,
) -> Result<Volume, RecoveryError> {
    // Directory fsync is a required storage contract. Platforms/filesystems
    // that reject it fail explicitly instead of silently weakening durability.
    sync(root)?;
    let private = private_dir(root, PRIVATE, create)?;
    let recovery = private_dir(&private, run_id, create)?;
    sync(&private)?;
    sync(root)?;
    Ok(Volume {
        root: root.clone(),
        recovery,
        identity: identity(&root.dir_metadata()?)?,
        quota,
    })
}

pub(crate) fn private_dir(parent: &Dir, name: &str, create: bool) -> Result<Dir, RecoveryError> {
    if create {
        match parent.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let dir = parent.open_dir_nofollow(name)?;
    sync(&dir)?;
    sync(parent)?;
    Ok(dir)
}

pub(crate) fn sync(dir: &Dir) -> Result<(), RecoveryError> {
    test_checkpoint("before-directory-sync")?;
    // A Dir may carry O_PATH on Linux, which cannot be fsynced. Open a
    // readable handle relative to the held directory, preserving its identity
    // even if its original path has been renamed or replaced.
    dir.open(".")?.sync_all()?;
    test_checkpoint("directory-sync")?;
    Ok(())
}

pub(crate) fn identity(metadata: &Metadata) -> Result<FileIdentity, RecoveryError> {
    Ok(FileIdentity::of(metadata)?)
}

pub(crate) fn existing(dir: &Dir, name: &str) -> Result<Option<Metadata>, RecoveryError> {
    match dir.symlink_metadata(name) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn current(dir: &Dir, name: &str) -> Result<Option<FileIdentity>, RecoveryError> {
    existing(dir, name)?.as_ref().map(identity).transpose()
}

impl Files {
    /// Size of a writable regular file, or zero when it is absent. Flush its
    /// current bytes and parent before the caller saves this size as an offset.
    pub(crate) fn size(&self, path: &str, work: Option<&WorkBudget>) -> Result<u64, RecoveryError> {
        let entry = self.entry(path, true)?;
        let parent = self.parent(&entry)?;
        let size = match existing(&parent, &entry.name)? {
            Some(metadata) => {
                self.require_regular(&entry, &metadata, work)?;
                parent.open(&entry.name)?.sync_all()?;
                metadata.len()
            }
            None => 0,
        };
        sync(&parent)?;
        Ok(size)
    }

    pub(crate) fn check_growth(&self, path: &str, bytes: u64) -> Result<(), RecoveryError> {
        let location = locate(&self.vfs, self.vfs.cwd(), path)?;
        self.volume(location.placement.mount_point())?
            .check_growth(bytes)
    }

    pub(crate) fn entry(&self, guest: &str, follow_final: bool) -> Result<Entry, RecoveryError> {
        let location = locate(&self.vfs, self.vfs.cwd(), guest)?;
        let Location {
            root,
            mut relative,
            placement,
        } = location;
        if placement.access() == Access::ReadOnly {
            return Err(fs::ContainError::ReadOnly(Arc::from(placement.mount_point())).into());
        }
        if relative == Path::new(".") {
            return Err(RecoveryError::Unsupported("volume root mutation"));
        }
        fs::check_mount_points(&root, &relative, &placement, true, follow_final)?;
        if follow_final
            && root
                .symlink_metadata(&relative)
                .is_ok_and(|m| m.file_type().is_symlink())
        {
            relative = root.canonicalize(&relative)?;
        }
        if fs::protected_metadata(&relative) {
            return Err(RecoveryError::Unsupported("protected metadata"));
        }
        let parent = relative
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let resolved = root.canonicalize(parent)?;
        if fs::protected_metadata(&resolved) {
            return Err(RecoveryError::Unsupported("protected metadata alias"));
        }
        let parent = if resolved.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &resolved
        };
        let name = relative
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(RecoveryError::Unsupported("non-UTF-8 path"))?;
        let parent_name = parent
            .to_str()
            .ok_or(RecoveryError::Unsupported("non-UTF-8 path"))?;
        let parent_identity = identity(&root.open_dir(parent)?.dir_metadata()?)?;
        // The primitive models assume the starting namespace survives a crash.
        // Establish that before preparing a public mutation.
        for ancestor in parent.ancestors() {
            let ancestor = if ancestor.as_os_str().is_empty() {
                Path::new(".")
            } else {
                ancestor
            };
            sync(&root.open_dir(ancestor)?)?;
        }
        let entry = Entry {
            mount: placement.mount_point().into(),
            parent: parent_name.into(),
            parent_identity,
            name: name.into(),
        };
        self.volume(&entry.mount)?;
        Ok(entry)
    }

    pub(crate) fn parent(&self, entry: &Entry) -> Result<Dir, RecoveryError> {
        let volume = self.volume(&entry.mount)?;
        let parent = volume.root.open_dir(&entry.parent)?;
        if identity(&parent.dir_metadata()?)? != entry.parent_identity {
            return Err(RecoveryError::Conflict);
        }
        Ok(parent)
    }

    pub(crate) fn artifacts(
        &self,
        entry: &Entry,
        effect: &EffectId,
        create: bool,
    ) -> Result<Dir, RecoveryError> {
        private_dir(&self.volume(&entry.mount)?.recovery, effect, create)
    }

    pub(crate) fn require_regular(
        &self,
        entry: &Entry,
        metadata: &Metadata,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<(), RecoveryError> {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(RecoveryError::Unsupported("non-regular file"));
        }
        let links = fs::link_count(metadata)
            .ok_or(RecoveryError::Unsupported("file link count unavailable"))?;
        if links <= 1 {
            return Ok(());
        }
        // Existing fs guards reject arbitrary hard links because they can alias
        // protected Git metadata. Only this run's private retention aliases are
        // allowed; every other alias still causes refusal.
        let id = identity(metadata)?;
        let private = regular_files(&self.volume(&entry.mount)?.recovery, work)?;
        let retained = private.iter().filter(|(file, _)| *file == id).count() as u64;
        if links != retained + 1 {
            return Err(RecoveryError::Unsupported("unowned hard-link alias"));
        }
        Ok(())
    }
}

impl Volume {
    /// Admission uses only public bytes. The caller holds exclusive mutation
    /// ownership, so temporary files need neither reservations nor a rescan.
    pub(crate) fn check_growth(&self, growth: u64) -> Result<(), RecoveryError> {
        let Some(quota) = &self.quota else {
            return Ok(());
        };
        if growth != 0
            && (quota.is_unmeasured()
                || quota
                    .used()
                    .checked_add(growth)
                    .is_none_or(|used| used > quota.limit()))
        {
            return Err(RecoveryError::Quota);
        }
        Ok(())
    }

    /// Apply a successful public mutation before releasing admission or running
    /// another fallible step. A retry measures the current path, not a saved delta.
    pub(crate) fn record_size_change(&self, before: u64, after: u64) {
        if let Some(quota) = &self.quota {
            if after >= before {
                quota.record(after - before);
            } else {
                quota.release(before - after);
            }
        }
    }
}

fn public_bytes(root: &Dir) -> Result<u64, RecoveryError> {
    let files = regular_files(root, None)?;
    let mut seen = HashSet::new();
    files
        .into_iter()
        .filter(|(id, _)| seen.insert(*id))
        .try_fold(0u64, |used, (_, bytes)| {
            used.checked_add(bytes).ok_or(RecoveryError::Quota)
        })
}

fn regular_files(
    root: &Dir,
    work: Option<&WorkBudget>,
) -> Result<Vec<(FileIdentity, u64)>, RecoveryError> {
    let charge = || -> Result<(), RecoveryError> {
        if let Some(work) = work {
            work.charge(fuel::SYSCALL, 2)?;
            work.charge(fuel::ELEM, 1)?;
        }
        Ok(())
    };
    charge()?;
    let mut files = Vec::new();
    let mut count = 0;
    let mut open = vec![root.entries()?];
    while let Some(directory) = open.last_mut() {
        let Some(entry) = directory.next() else {
            open.pop();
            continue;
        };
        charge()?;
        let entry = entry?;
        // Public usage ignores the recovery tree. Ownership checks start inside
        // that tree instead, and still enumerate its candidate files.
        if private_recovery_path(Path::new(&entry.file_name())) {
            continue;
        }
        count += 1;
        if count > MAX_ENTRIES {
            return Err(RecoveryError::Limit);
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if open.len() > MAX_DEPTH {
                return Err(RecoveryError::Limit);
            }
            open.push(entry.open_dir()?.entries()?);
        } else if kind.is_file() {
            let metadata = cap_fs_ext::DirEntryExt::full_metadata(&entry)?;
            files.push((identity(&metadata)?, metadata.len()));
        }
    }
    Ok(files)
}

pub(crate) fn test_checkpoint(point: &str) -> Result<(), RecoveryError> {
    crate::test_checkpoint(point);
    #[cfg(test)]
    crate::filesystem::tests::test_checkpoint(point)?;
    Ok(())
}

use crate::filesystem::FilesystemError;
use crate::unit_of_work::StoreError;
use interpreter::runtime::DiskQuota;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Weak;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Roots {
    #[serde(rename = "id")]
    pub run_id: RunId,
    pub volumes: Vec<RootIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootIdentity {
    pub mount: String,
    pub root: FileIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub mount: String,
    pub parent: String,
    pub parent_identity: FileIdentity,
    pub name: String,
}

// A retry must resolve the same guest paths against the same configured roots.
// Keep only a fingerprint in each record, not a second persisted mount catalog.
// Quotas remain part of replay compatibility: a changed limit can change whether
// an unrecorded write succeeds. Keep the encoding stable for retained journals;
// retirement does not replay operations and must not require this fingerprint.
pub fn filesystem_config_fingerprint(vfs: &Vfs) -> Result<[u8; 32], FilesystemError> {
    let root = vfs
        .dir()
        .ok_or(RecoveryError::Unsupported("filesystem disabled"))?;
    let root_id = FileIdentity::of(&root.dir_metadata().map_err(RecoveryError::from)?)
        .map_err(RecoveryError::from)?;
    let mounts = vfs
        .mounts()
        .iter()
        .map(|mount| {
            let id = FileIdentity::of(&mount.dir().dir_metadata()?)?;
            Ok((
                mount.guest_path(),
                mount.access(),
                id,
                mount.quota().map(|q| q.limit()),
            ))
        })
        .collect::<Result<Vec<_>, RecoveryError>>()?;
    let bytes = serde_json::to_vec(&(
        vfs.cwd(),
        vfs.access(),
        root_id,
        vfs.quota().map(|q| q.limit()),
        mounts,
    ))
    .map_err(|e| StoreError::Io(e.to_string()))?;
    Ok(Sha256::digest(bytes).into())
}

/// Remove only this run's private artifacts. The execution owner must first
/// commit a terminal state and drain every worker and resource handle.
pub fn retire_filesystem(vfs: &Vfs, run_id: &RunId) -> Result<(), RecoveryError> {
    validate_id(run_id)?;
    let roots = vfs
        .dir()
        .filter(|_| vfs.access() == Access::ReadWrite)
        .into_iter()
        .chain(
            vfs.mounts()
                .iter()
                .filter(|mount| mount.access() == Access::ReadWrite)
                .map(interpreter::runtime::vfs::Mount::dir),
        );
    for root in roots {
        let private = match root.open_dir_nofollow(PRIVATE) {
            Ok(private) => private,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        match private.remove_dir_all(run_id.as_str()) {
            Ok(()) => sync(&private)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(crate) type SharedQuotas = HashMap<FileIdentity, Weak<DiskQuota>>;

// Reopening a logical workspace in the same process must keep the counter used
// by its existing VFS owners. Weak entries do not retain completed workspaces.
pub(crate) fn share_quotas(shared: &mut SharedQuotas, mut vfs: Vfs) -> Result<Vfs, RecoveryError> {
    shared.retain(|_, quota| quota.strong_count() != 0);
    if let (Some(root), Some(quota)) = (vfs.dir(), vfs.quota()) {
        let identity = FileIdentity::of(&root.dir_metadata().map_err(RecoveryError::from)?)
            .map_err(RecoveryError::from)?;
        let quota = reuse(shared, identity, root, quota)?;
        vfs = vfs.with_shared_quota(quota);
    }
    let mounts = vfs.mounts().to_vec();
    for mount in mounts {
        if let Some(quota) = mount.quota() {
            let path = interpreter::runtime::fs::resolve_content(&vfs, "/", mount.guest_path())
                .map_err(RecoveryError::from)?;
            let identity = FileIdentity::of(&path.metadata().map_err(RecoveryError::from)?)
                .map_err(RecoveryError::from)?;
            let quota = reuse(shared, identity, mount.dir(), quota)?;
            vfs = vfs.with_mount_quota(mount.guest_path(), quota);
        }
    }
    Ok(vfs)
}

fn reuse(
    shared: &mut SharedQuotas,
    id: FileIdentity,
    root: &Dir,
    quota: &Arc<DiskQuota>,
) -> Result<Arc<DiskQuota>, RecoveryError> {
    if let Some(existing) = shared.get(&id).and_then(Weak::upgrade) {
        if existing.limit() != quota.limit() {
            return Err(RecoveryError::Conflict);
        }
        return Ok(existing);
    }
    // Only a fresh owner measures disk. Reopen reuses the live counter above,
    // preserving reservations and avoiding any per-operation directory walk.
    let bytes = public_bytes(root)?;
    let quota = if quota.is_unmeasured() {
        Arc::new(DiskQuota::new(quota.limit(), bytes))
    } else {
        quota.set_used(bytes);
        quota.clone()
    };
    shared.insert(id, Arc::downgrade(&quota));
    Ok(quota)
}
