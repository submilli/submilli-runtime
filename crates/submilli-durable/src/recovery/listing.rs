use super::{MAX_DEPTH, MAX_ENTRIES, RecoveryError, Workspace, paths};
use interpreter::runtime::fs;
use serde::{Deserialize, Serialize};

pub const MAX_LISTING_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryEntry {
    pub path: String,
    pub directory: bool,
    pub symlink: bool,
    pub bytes: u64,
}

impl Workspace {
    /// A bounded frozen enumeration. Replaying its iterator uses these entries
    /// in this order, never a new readdir against mutable storage.
    pub fn directory_listing(
        &self,
        path: &str,
        recursive: bool,
    ) -> Result<Vec<DirectoryEntry>, RecoveryError> {
        self.directory_listing_metered(path, recursive, None)
    }
    pub(crate) fn directory_listing_metered(
        &self,
        path: &str,
        recursive: bool,
        work: Option<&interpreter::runtime::fuel::WorkBudget>,
    ) -> Result<Vec<DirectoryEntry>, RecoveryError> {
        use interpreter::runtime::fuel;
        paths::validate_guest_path(path)?;
        let guest = fs::guest_normalize(self.vfs.cwd(), path).map_err(fs::ContainError::from)?;
        let mut stack = vec![(guest, 0)];
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        while let Some((guest, depth)) = stack.pop() {
            if depth > MAX_DEPTH {
                return Err(RecoveryError::Limit);
            }
            if let Some(work) = work {
                work.charge(fuel::SYSCALL, 1)?;
            }
            let resolved = fs::resolve_content(&self.vfs, "/", &guest)?;
            let directory = resolved.open_dir()?;
            for entry in directory.entries()? {
                if let Some(work) = work {
                    work.charge(fuel::SYSCALL, 2)?;
                }
                let entry = entry?;
                let name = entry.file_name();
                if fs::private_recovery_path(std::path::Path::new(&name)) {
                    continue;
                }
                let name = name
                    .to_str()
                    .ok_or(RecoveryError::Unsupported("non-UTF-8 directory entry"))?;
                let child = format!("{}/{name}", guest.trim_end_matches('/'));
                paths::validate_guest_path(&child)?;
                let metadata = directory.symlink_metadata(name)?;
                if !metadata.is_file() && !metadata.is_dir() && !metadata.file_type().is_symlink() {
                    return Err(RecoveryError::Unsupported(
                        "special file in directory snapshot",
                    ));
                }
                let item = DirectoryEntry {
                    path: child.clone(),
                    directory: metadata.is_dir(),
                    symlink: metadata.file_type().is_symlink(),
                    bytes: metadata.len(),
                };
                bytes = bytes
                    .checked_add(child.len().saturating_mul(6).saturating_add(128))
                    .ok_or(RecoveryError::Limit)?;
                if bytes > MAX_LISTING_BYTES || entries.len() >= MAX_ENTRIES {
                    return Err(RecoveryError::Limit);
                }
                if let Some(work) = work {
                    work.charge(fuel::PARSE, child.len() as u64)?;
                    work.charge(fuel::ELEM, 1)?;
                }
                if recursive && item.directory && !item.symlink {
                    stack.push((child, depth + 1));
                }
                entries.push(item);
            }
        }
        if let Some(work) = work {
            work.charge_fuel(fuel::sort_cost(entries.len() as u64))?;
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }
}
