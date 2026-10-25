//! Private capability-relative files. Only encrypted history ever reaches this layer.
use super::{ObjectReference, checkpoint, invalid};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use submilli_durable::history::MAX_STORED_ENTRY_BYTES;
use submilli_durable::unit_of_work::StoreError;

pub(super) struct ObjectFiles {
    root: Dir,
    // Workers own Arc<ObjectFiles>, so cancellation cannot release this process lock.
    _lock: std::fs::File,
    #[cfg(all(test, not(windows)))]
    hook: super::tests::FileHook,
}
impl ObjectFiles {
    pub(super) fn open(path: &Path) -> Result<Self, StoreError> {
        if !std::fs::symlink_metadata(path).map_err(invalid)?.is_dir() {
            return Err(invalid(
                "durable object directory must be a plain directory",
            ));
        }
        let root = Dir::open_ambient_dir(path, cap_std::ambient_authority()).map_err(invalid)?;
        // The volume root is pre-existing or was persisted during preparation.
        // Only this new child's entry needs a barrier here, not unrelated ancestors.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        sync_directory(parent).map_err(invalid)?;
        let lock = root
            .open_with(".lock", options().read(true).write(true).create(true))
            .map_err(invalid)?;
        require_private_file(&lock)?;
        let lock = lock.into_std();
        lock.try_lock().map_err(invalid)?;
        Ok(Self {
            root,
            _lock: lock,
            #[cfg(all(test, not(windows)))]
            hook: Default::default(),
        })
    }

    pub(super) fn identify(&self, identity: &str, has_objects: bool) -> Result<(), StoreError> {
        match self.root.open_with(".identity", options().read(true)) {
            Ok(mut file) => {
                require_private_file(&file)?;
                if file.metadata().map_err(invalid)?.len() != identity.len() as u64 {
                    return Err(invalid("durable object store identity mismatch"));
                }
                let mut bytes = [0; 36];
                file.read_exact(&mut bytes).map_err(invalid)?;
                if bytes.as_slice() != identity.as_bytes() {
                    return Err(invalid("durable object store belongs to another database"));
                }
                // Repeat both barriers when recovering an interrupted initialization.
                file.sync_all().map_err(invalid)?;
                self.sync()
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !has_objects => {
                // A replacement DB must not adopt files whose ownership it has lost.
                for entry in self.root.entries().map_err(invalid)? {
                    let name = entry.map_err(invalid)?.file_name();
                    if name != ".lock" && name != ".identity.partial" {
                        return Err(invalid("unrecognized files in durable object store"));
                    }
                }
                self.remove_if_present(".identity.partial")?;
                let mut file = self
                    .root
                    .open_with(".identity.partial", options().write(true).create_new(true))
                    .map_err(invalid)?;
                file.write_all(identity.as_bytes()).map_err(invalid)?;
                file.sync_all().map_err(invalid)?;
                self.root
                    .rename(".identity.partial", &self.root, ".identity")
                    .map_err(invalid)?;
                self.sync()
            }
            Err(error) => Err(invalid(error)),
        }
    }

    pub(super) fn publish(
        &self,
        reference: &ObjectReference,
        sealed: &[u8],
    ) -> Result<(), StoreError> {
        reference.validate()?;
        if sealed.len() as u64 != reference.sealed_bytes {
            return Err(invalid("object publication content mismatch"));
        }
        let partial = format!("{}.partial", reference.id);
        let final_name = format!("{}.object", reference.id);
        let mut file = self
            .root
            .open_with(&partial, options().write(true).create_new(true))
            .map_err(invalid)?;
        self.checkpoint("staging")?;
        file.write_all(sealed).map_err(invalid)?;
        self.checkpoint("written")?;
        file.sync_all().map_err(invalid)?;
        self.checkpoint("file-synced")?;
        // IDs are fresh, never reused, and the exclusive store owner serializes
        // collection against publication. Existing final names are corruption.
        match self.root.symlink_metadata(&final_name) {
            Ok(_) => return Err(invalid("immutable object already exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(invalid(error)),
        }
        self.root
            .rename(&partial, &self.root, &final_name)
            .map_err(invalid)?;
        self.checkpoint("published")?;
        self.sync()?;
        self.checkpoint("directory-synced")?;
        Ok(())
    }

    pub(super) fn read(&self, reference: &ObjectReference) -> Result<Vec<u8>, StoreError> {
        reference.validate()?;
        self.checkpoint("reading")?;
        let mut file = self
            .root
            .open_with(format!("{}.object", reference.id), options().read(true))
            .map_err(invalid)?;
        require_private_file(&file)?;
        if file.metadata().map_err(invalid)?.len() != reference.sealed_bytes {
            return Err(invalid("durable object length mismatch"));
        }
        let size = usize::try_from(reference.sealed_bytes).map_err(invalid)?;
        if size > MAX_STORED_ENTRY_BYTES {
            return Err(invalid("oversized durable object"));
        }
        let mut content = Vec::new();
        content.try_reserve_exact(size).map_err(invalid)?;
        content.resize(size, 0);
        file.read_exact(&mut content).map_err(invalid)?;
        if file.read(&mut [0]).map_err(invalid)? != 0
            || Sha256::digest(&content).as_slice() != reference.sha256
        {
            return Err(invalid("durable object integrity mismatch"));
        }
        Ok(content)
    }

    pub(super) fn remove(&self, id: &str) -> Result<(), StoreError> {
        super::validate_id(id)?;
        self.remove_if_present(&format!("{id}.partial"))?;
        self.remove_if_present(&format!("{id}.object"))?;
        self.checkpoint("unlinked")?;
        self.sync()?;
        self.checkpoint("deletion-synced")?;
        Ok(())
    }

    #[cfg(all(test, not(windows)))]
    pub(super) fn set_hook(&self, hook: super::tests::Hook) {
        *self.hook.lock().unwrap() = Some(hook);
    }
    fn checkpoint(&self, point: &str) -> Result<(), StoreError> {
        #[cfg(all(test, not(windows)))]
        if let Some(hook) = self.hook.lock().unwrap().clone() {
            hook(point)?;
        }
        checkpoint(point);
        Ok(())
    }

    fn remove_if_present(&self, name: &str) -> Result<(), StoreError> {
        match self.root.remove_file(name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(invalid(error)),
        }
    }
    fn sync(&self) -> Result<(), StoreError> {
        // Dir may hold an O_PATH descriptor on Linux, which cannot be fsynced.
        // Open a readable handle relative to the held directory to retain identity.
        self.root
            .open(".")
            .map_err(invalid)?
            .sync_all()
            .map_err(invalid)
    }
}
fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.follow(FollowSymlinks::No);
    options
}
fn require_private_file(file: &File) -> Result<(), StoreError> {
    let metadata = file.metadata().map_err(invalid)?;
    if !metadata.is_file() || interpreter::runtime::fs::link_count(&metadata) != Some(1) {
        return Err(invalid("durable object must be a private regular file"));
    }
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> std::io::Result<()> {
    let directory = Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
    directory.open(".")?.sync_all()
}
