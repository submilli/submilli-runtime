use super::{Files, RecoveryError, locate, sync, sync_ancestors, test_checkpoint};
use cap_fs_ext::DirExt;
use interpreter::runtime::fs;
use std::io;
use std::path::Path;

impl Files {
    /// The count binds replay to the original missing suffix even if an earlier
    /// attempt created some directories without syncing their parent entries.
    pub(crate) fn prepare_directory_tree(
        &self,
        guest: &str,
        work: Option<&super::WorkBudget>,
    ) -> Result<usize, RecoveryError> {
        let location = locate(&self.vfs, self.vfs.cwd(), guest)?;
        let target = fs::resolve_content(&self.vfs, self.vfs.cwd(), guest)?;
        match target.metadata() {
            Ok(meta) if meta.is_dir() => {
                if location.placement.access() == super::Access::ReadWrite {
                    let resolved = location.root.canonicalize(&location.relative)?;
                    sync_ancestors(&location.root, &resolved, work)?;
                }
                return Ok(0);
            }
            Ok(_) => return Err(io::Error::from(io::ErrorKind::AlreadyExists).into()),
            Err(fs::ContainError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        target.check_directory_creation()?;

        let mut prefix = std::path::PathBuf::new();
        let mut parent = location.root.try_clone()?;
        let mut components = location.relative.iter();
        while let Some(name) = components.next() {
            prefix.push(name);
            match parent.symlink_metadata(name) {
                Ok(meta) => {
                    parent = if meta.file_type().is_symlink() {
                        // Link targets are relative to the volume's capability root.
                        location.root.open_dir(&prefix)
                    } else {
                        parent.open_dir_nofollow(name)
                    }
                    .map_err(fs::ContainError::from)?;
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(fs::ContainError::from(error).into()),
            }
            prefix.pop();
            let resolved = if prefix.as_os_str().is_empty() {
                prefix
            } else {
                location.root.canonicalize(&prefix)?
            };
            sync_ancestors(&location.root, &resolved, work)?;
            // Validate every remaining name on this volume while a refusal is
            // still safe, including names too long for a native directory entry.
            for component in components.clone() {
                match parent.symlink_metadata(component) {
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(fs::ContainError::from(error).into()),
                }
            }
            return Ok(1 + components.count());
        }
        Err(RecoveryError::Conflict)
    }

    pub(crate) fn create_directory_tree(
        &self,
        guest: &str,
        missing: usize,
    ) -> Result<(), RecoveryError> {
        if missing == 0 {
            return Ok(());
        }
        let location = locate(&self.vfs, self.vfs.cwd(), guest)?;
        let base = location
            .relative
            .ancestors()
            .nth(missing)
            .ok_or(RecoveryError::InvalidRecord)?;
        let suffix = location
            .relative
            .strip_prefix(base)
            .map_err(|_| RecoveryError::InvalidRecord)?;
        let mut parent = location.root.open_dir(if base.as_os_str().is_empty() {
            Path::new(".")
        } else {
            base
        })?;
        for name in suffix {
            match parent.create_dir(name) {
                Ok(()) => test_checkpoint("directory-created")?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            let child = parent.open_dir_nofollow(name)?;
            sync(&child)?;
            sync(&parent)?;
            parent = child;
        }
        test_checkpoint("before-receipt")
    }
}
