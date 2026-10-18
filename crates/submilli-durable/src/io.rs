//! Contained disk access and private artifacts used to retry filesystem work.
//!
//! `Files` retains the configured VFS; `Volume` maps each writable mount to its
//! private recovery directory. Entry records name paths relative to those roots
//! and carry parent identities, so replay cannot silently target a replaced tree.
//!
//! This layer knows nothing about SQL transactions or which effect to run next.
//! Its callers supply retry facts and retain the mutation gate while disk work
//! runs. Sync barriers make artifacts durable before the journal refers to them.
//! The quota registry at the end shares counters across reopened VFS values.
use cap_std::fs::Dir;
use interpreter::runtime::Vfs;
use interpreter::runtime::durable::{EffectId, RunId};
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
    maximum_bytes: Option<u64>,
    quota: Option<Arc<interpreter::runtime::DiskQuota>>,
}

impl Files {
    /// Open caller-owned roots and create private recovery directories.
    /// The journal binds requests to these roots before any public mutation.
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

    pub(crate) fn reconcile_quotas(
        &self,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<(), RecoveryError> {
        for volume in self.volumes.values() {
            if let Some(quota) = &volume.quota {
                quota.set_used(volume.used_bytes(work)?);
            }
        }
        Ok(())
    }
}

use cap_fs_ext::DirExt;
use cap_std::fs::Metadata;
use interpreter::runtime::fs::{self, ContentPath};
use interpreter::runtime::vfs::{Access, Placement};
use interpreter::runtime::{fuel, fuel::WorkBudget};
use std::collections::HashSet;

use std::path::{Component, Path};

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

pub(crate) fn validate_relative(path: &str, parent: bool) -> Result<(), RecoveryError> {
    if parent && path == "." {
        return Ok(());
    }
    if path.is_empty()
        || path.len() > 4096
        || path.contains(['\0', '\\'])
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(RecoveryError::InvalidRecord);
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
        if volumes
            .values()
            .any(|other| other.identity == volume.identity)
        {
            return Err(RecoveryError::Unsupported("aliased writable volumes"));
        }
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
        maximum_bytes: quota.as_ref().map(|quota| quota.limit()),
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
        // Establish that before recording the mutation's intent.
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
    pub(crate) fn check_growth(
        &self,
        growth: u64,
        work: Option<&WorkBudget>,
    ) -> Result<(), RecoveryError> {
        let Some(limit) = self.maximum_bytes else {
            return Ok(());
        };
        if self
            .quota
            .as_ref()
            .is_some_and(|quota| quota.is_unmeasured())
            && growth > 0
        {
            return Err(RecoveryError::Quota);
        }
        let used = self.used_bytes(work)?;
        if used.checked_add(growth).is_none_or(|next| next > limit) && growth != 0 {
            return Err(RecoveryError::Quota);
        }
        Ok(())
    }

    pub(crate) fn used_bytes(&self, work: Option<&WorkBudget>) -> Result<u64, RecoveryError> {
        measured_bytes(&self.root, work)
    }
}

fn measured_bytes(root: &Dir, work: Option<&WorkBudget>) -> Result<u64, RecoveryError> {
    let files = regular_files(root, work)?;
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
use tokio::sync::Mutex;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub mount: String,
    pub parent: String,
    pub parent_identity: FileIdentity,
    pub name: String,
}

impl Entry {
    pub(crate) fn validate(&self) -> Result<(), RecoveryError> {
        validate_relative(&self.parent, true)?;
        validate_relative(&self.name, false)?;
        let parent = std::path::Path::new(&self.parent);
        let name = std::path::Path::new(&self.name);
        if self.name.contains('/')
            || private_recovery_path(parent)
            || private_recovery_path(name)
            || (interpreter::runtime::fs::protected_metadata(parent)
                || interpreter::runtime::fs::protected_metadata(name))
        {
            return Err(RecoveryError::InvalidRecord);
        }
        validate_guest_path(&self.mount)
    }
}

// A retry must resolve the same guest paths against the same configured roots.
// Keep only a fingerprint in each record, not a second persisted mount catalog.
pub(crate) fn filesystem_config_fingerprint(vfs: &Vfs) -> Result<[u8; 32], FilesystemError> {
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

pub(crate) type SharedQuotas = HashMap<FileIdentity, Weak<DiskQuota>>;

// Reopening a logical workspace in the same process must keep the counter used
// by its existing VFS owners. Weak entries do not retain completed workspaces.
pub(crate) async fn share_quotas(
    quotas: &Mutex<SharedQuotas>,
    mut vfs: Vfs,
) -> Result<Vfs, FilesystemError> {
    let mut shared = quotas.lock().await;
    shared.retain(|_, quota| quota.strong_count() != 0);
    if let (Some(root), Some(quota)) = (vfs.dir(), vfs.quota()) {
        let identity = FileIdentity::of(&root.dir_metadata().map_err(RecoveryError::from)?)
            .map_err(RecoveryError::from)?;
        let quota = reuse(&mut shared, identity, quota)?;
        vfs = vfs.with_shared_quota(quota);
    }
    let mounts = vfs.mounts().to_vec();
    for mount in mounts {
        if let Some(quota) = mount.quota() {
            let path = interpreter::runtime::fs::resolve_content(&vfs, "/", mount.guest_path())
                .map_err(RecoveryError::from)?;
            let identity = FileIdentity::of(&path.metadata().map_err(RecoveryError::from)?)
                .map_err(RecoveryError::from)?;
            let quota = reuse(&mut shared, identity, quota)?;
            vfs = vfs.with_mount_quota(mount.guest_path(), quota);
        }
    }
    Ok(vfs)
}

fn reuse(
    shared: &mut SharedQuotas,
    id: FileIdentity,
    quota: &Arc<DiskQuota>,
) -> Result<Arc<DiskQuota>, FilesystemError> {
    if let Some(existing) = shared.get(&id).and_then(Weak::upgrade) {
        if existing.limit() != quota.limit() {
            return Err(RecoveryError::Conflict.into());
        }
        return Ok(existing);
    }
    shared.insert(id, Arc::downgrade(quota));
    Ok(quota.clone())
}
