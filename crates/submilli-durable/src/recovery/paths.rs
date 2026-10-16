use super::plan::Entry;
use super::{FileIdentity, MAX_DEPTH, MAX_ENTRIES, PRIVATE, RecoveryError, Vfs, Volume, Workspace};
use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};
use interpreter::runtime::fs::{self, ContentPath};
use interpreter::runtime::vfs::Access;
use interpreter::runtime::{fuel, fuel::WorkBudget};
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Component, Path};
use std::sync::Arc;

pub(super) fn validate_id(id: &str) -> Result<(), RecoveryError> {
    match uuid::Uuid::parse_str(id) {
        Ok(parsed) if parsed.to_string() == id => Ok(()),
        _ => Err(RecoveryError::InvalidRecord),
    }
}

pub(super) fn validate_guest_path(path: &str) -> Result<(), RecoveryError> {
    if path.len() > 4096 || path.is_empty() || path.contains('\0') {
        return Err(RecoveryError::InvalidRecord);
    }
    fs::guest_normalize("/", path).map_err(interpreter::runtime::fs::ContainError::from)?;
    Ok(())
}

pub(super) fn validate_relative(path: &str, parent: bool) -> Result<(), RecoveryError> {
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

pub(super) fn open_volumes(
    vfs: &Vfs,
    id: &str,
    create: bool,
) -> Result<BTreeMap<String, Volume>, RecoveryError> {
    let mut volumes = BTreeMap::new();
    if let Some(root) = vfs.dir().filter(|_| vfs.access() == Access::ReadWrite) {
        volumes.insert(
            "/".into(),
            open_volume(root, id, create, vfs.quota().cloned())?,
        );
    }
    for mount in vfs
        .mounts()
        .iter()
        .filter(|m| m.access() == Access::ReadWrite)
    {
        let volume = open_volume(mount.dir(), id, create, mount.quota().cloned())?;
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
    id: &str,
    create: bool,
    quota: Option<Arc<interpreter::runtime::DiskQuota>>,
) -> Result<Volume, RecoveryError> {
    // Directory fsync is a required storage contract. Platforms/filesystems
    // that reject it fail explicitly instead of silently weakening durability.
    sync(root)?;
    let private = private_dir(root, PRIVATE, create)?;
    let recovery = private_dir(&private, id, create)?;
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

pub(super) fn private_dir(parent: &Dir, name: &str, create: bool) -> Result<Dir, RecoveryError> {
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

pub(super) fn sync(dir: &Dir) -> Result<(), RecoveryError> {
    checkpoint("before-directory-sync")?;
    // A Dir may carry O_PATH on Linux, which cannot be fsynced. Open a
    // readable handle relative to the held directory, preserving its identity
    // even if its original path has been renamed or replaced.
    dir.open(".")?.sync_all()?;
    checkpoint("directory-sync")?;
    Ok(())
}

pub(super) fn identity(metadata: &Metadata) -> Result<FileIdentity, RecoveryError> {
    Ok(FileIdentity::of(metadata)?)
}

pub(super) fn existing(dir: &Dir, name: &str) -> Result<Option<Metadata>, RecoveryError> {
    match dir.symlink_metadata(name) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn current(dir: &Dir, name: &str) -> Result<Option<FileIdentity>, RecoveryError> {
    existing(dir, name)?.as_ref().map(identity).transpose()
}

impl Workspace {
    pub(super) fn entry(
        &self,
        guest: &str,
        follow_final: bool,
    ) -> Result<(Entry, ContentPath), RecoveryError> {
        self.entry_access(guest, follow_final, false)
    }

    pub(super) fn entry_access(
        &self,
        guest: &str,
        follow_final: bool,
        git: bool,
    ) -> Result<(Entry, ContentPath), RecoveryError> {
        let location = self
            .vfs
            .mutation_location(guest, follow_final, git)
            .map_err(|error| match error {
                fs::StorageError::Containment(error) => RecoveryError::Containment(error),
                fs::StorageError::Io(error) => RecoveryError::Io(error),
                fs::StorageError::Unsupported(message) => RecoveryError::Unsupported(message),
            })?;
        for ancestor in &location.ancestors {
            sync(ancestor)?;
        }
        let entry = Entry {
            mount: location.mount,
            parent: location.parent,
            parent_identity: location.parent_identity,
            name: location.name,
        };
        self.volume(&entry.mount)?;
        Ok((entry, location.path))
    }

    pub(super) fn parent(&self, entry: &Entry) -> Result<Dir, RecoveryError> {
        let volume = self.volume(&entry.mount)?;
        let parent = volume.root.open_dir(&entry.parent)?;
        if identity(&parent.dir_metadata()?)? != entry.parent_identity {
            return Err(RecoveryError::Conflict);
        }
        Ok(parent)
    }

    pub(super) fn artifacts(
        &self,
        entry: &Entry,
        effect: &str,
        create: bool,
    ) -> Result<Dir, RecoveryError> {
        private_dir(&self.volume(&entry.mount)?.recovery, effect, create)
    }

    pub(super) fn require_regular(
        &self,
        entry: &Entry,
        metadata: &Metadata,
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
        let private = regular_files(&self.volume(&entry.mount)?.recovery, self.work.as_deref())?;
        let retained = private.iter().filter(|(file, _)| *file == id).count() as u64;
        if links != retained + 1 {
            return Err(RecoveryError::Unsupported("unowned hard-link alias"));
        }
        Ok(())
    }
}

impl Volume {
    pub(super) fn check_growth(
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

    pub(super) fn used_bytes(&self, work: Option<&WorkBudget>) -> Result<u64, RecoveryError> {
        measured_bytes(&self.root, work)
    }
}

pub(super) fn used_bytes(root: &Dir) -> Result<u64, RecoveryError> {
    measured_bytes(root, None)
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
    interpreter::runtime::vfs::regular_files_with_budget(root, MAX_ENTRIES, || {
        if let Some(work) = work {
            work.charge(fuel::SYSCALL, 2)?;
            work.charge(fuel::ELEM, 1)?;
        }
        Ok(())
    })
}

pub(super) fn remove_private_tree(parent: &Dir, name: &str) -> Result<(), RecoveryError> {
    if let Some(metadata) = existing(parent, name)? {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(RecoveryError::Conflict);
        }
        let dir = parent.open_dir_nofollow(name)?;
        remove_contents(&dir, &mut 0, 0)?;
        parent.remove_dir(name)?;
    }
    sync(parent)
}

fn remove_contents(dir: &Dir, count: &mut usize, depth: usize) -> Result<(), RecoveryError> {
    if depth > MAX_DEPTH {
        return Err(RecoveryError::Limit);
    }
    for entry in dir.entries()? {
        *count += 1;
        if *count > MAX_ENTRIES {
            return Err(RecoveryError::Limit);
        }
        let entry = entry?;
        let name = entry.file_name();
        let metadata = dir.symlink_metadata(&name)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            remove_contents(&dir.open_dir_nofollow(&name)?, count, depth + 1)?;
            dir.remove_dir(&name)?;
        } else {
            dir.remove_file(&name)?;
        }
    }
    sync(dir)
}

pub(super) fn checkpoint(_point: &str) -> Result<(), RecoveryError> {
    #[cfg(test)]
    super::tests::checkpoint(_point)?;
    Ok(())
}
