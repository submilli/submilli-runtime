//! Private capability-relative files. Only encrypted history ever reaches this layer.
use super::{ObjectReference, checkpoint, invalid};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
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
        let parent =
            Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map_err(invalid)?;
        sync(&parent)?;
        // Keep infrastructure names outside the namespace of valid session IDs.
        let lock = parent
            .open_with(
                ".durable-objects.lock",
                options().read(true).write(true).create(true),
            )
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

    pub(super) fn publish(
        &self,
        directory: &RunDirectory,
        reference: &ObjectReference,
        sealed: &[u8],
    ) -> Result<(), StoreError> {
        reference.validate()?;
        if sealed.len() as u64 != reference.sealed_bytes {
            return Err(invalid("object publication content mismatch"));
        }
        let session = prepare_directory(&self.root, &directory.session)?;
        let run = prepare_directory(&session, &directory.run)?;
        let partial = format!("{}.partial", reference.id);
        let final_name = format!("{}.object", reference.id);
        let mut file = run
            .open_with(&partial, options().write(true).create_new(true))
            .map_err(invalid)?;
        self.checkpoint("staging")?;
        file.write_all(sealed).map_err(invalid)?;
        self.checkpoint("written")?;
        file.sync_all().map_err(invalid)?;
        self.checkpoint("file-synced")?;
        // IDs are fresh, never reused, and the exclusive store owner serializes
        // collection against publication. Existing final names are corruption.
        match run.symlink_metadata(&final_name) {
            Ok(_) => return Err(invalid("immutable object already exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(invalid(error)),
        }
        run.rename(&partial, &run, &final_name).map_err(invalid)?;
        self.checkpoint("published")?;
        sync(&run)?;
        self.checkpoint("directory-synced")?;
        Ok(())
    }

    pub(super) fn read(
        &self,
        directory: &RunDirectory,
        reference: &ObjectReference,
    ) -> Result<Vec<u8>, StoreError> {
        reference.validate()?;
        self.checkpoint("reading")?;
        let session = self
            .root
            .open_dir_nofollow(&directory.session)
            .map_err(invalid)?;
        let run = session.open_dir_nofollow(&directory.run).map_err(invalid)?;
        let mut file = run
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

    pub(super) fn remove_run(&self, directory: &RunDirectory) -> Result<(), StoreError> {
        let Some(session) = open_if_present(&self.root, &directory.session)? else {
            return sync(&self.root);
        };
        if let Some(run) = open_if_present(&session, &directory.run)? {
            // This private directory contains only flat immutable objects and
            // staging files. Unlink entries without following their targets.
            for entry in run.entries().map_err(invalid)? {
                run.remove_file(entry.map_err(invalid)?.file_name())
                    .map_err(invalid)?;
            }
            session.remove_dir(&directory.run).map_err(invalid)?;
        }
        self.checkpoint("unlinked")?;
        sync(&session)?;
        match self.root.remove_dir(&directory.session) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => return Err(invalid(error)),
        }
        sync(&self.root)?;
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

/// IDs come from persisted records as well as requests. Validate before any
/// filesystem operation, including cleanup; never normalize an unsafe identity.
pub(super) struct RunDirectory {
    session: String,
    run: String,
}
impl RunDirectory {
    pub(super) fn new(session: &str, run: &str) -> Result<Self, StoreError> {
        Ok(Self {
            session: directory_name(session)?,
            run: directory_name(run)?,
        })
    }
}
fn directory_name(value: &str) -> Result<String, StoreError> {
    crate::domain::session::SessionId::parse(value.to_owned()).map_err(invalid)?;
    // Preserve ordinary UUIDs without case-folding or Unicode-normalization
    // aliases on supported filesystems. '~' cannot occur in literal names, so
    // digest names cannot collide with a literal custom identity either.
    let literal = value.len() <= 255
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        });
    if !literal {
        return Ok(format!("~{:x}", Sha256::digest(value.as_bytes())));
    }
    Ok(value.to_owned())
}
fn prepare_directory(parent: &Dir, name: &str) -> Result<Dir, StoreError> {
    match parent.create_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(invalid(error)),
    }
    let child = parent.open_dir_nofollow(name).map_err(invalid)?;
    // Repeat the barrier for existing names too: an earlier attempt may have
    // stopped after mkdir. The held root is private to the server process.
    sync(parent)?;
    Ok(child)
}
fn open_if_present(parent: &Dir, name: &str) -> Result<Option<Dir>, StoreError> {
    match parent.open_dir_nofollow(name) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(invalid(error)),
    }
}
fn sync(directory: &Dir) -> Result<(), StoreError> {
    // Dir may hold an O_PATH descriptor on Linux, which cannot be fsynced.
    directory
        .open(".")
        .map_err(invalid)?
        .sync_all()
        .map_err(invalid)
}
