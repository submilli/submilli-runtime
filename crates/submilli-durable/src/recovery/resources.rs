use super::mutation::pin;
use super::paths::{self, current, identity, sync};
use super::{FileIdentity, RecoveryError, Workspace, WorkspaceIdentity};
use cap_std::fs::File;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};

/// Byte position of a logical reader, independent of an OS descriptor number.
/// Buffered adapters save the position of the first unread byte, together with
/// their decoder flags; they must not save a prefetched descriptor position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderPosition {
    workspace: WorkspaceIdentity,
    resource: String,
    mount: String,
    file: FileIdentity,
    offset: u64,
}

pub struct RetainedReader {
    file: File,
    position: ReaderPosition,
}

impl Workspace {
    /// The caller journals this logical resource before returning a guest
    /// handle. A crash before that commit leaves a private, collectable pin.
    pub fn open_reader(&self, resource: &str, path: &str) -> Result<RetainedReader, RecoveryError> {
        paths::validate_id(resource)?;
        let entry = self.entry(path, true)?;
        let parent = self.parent(&entry)?;
        let mut file = parent.open(&entry.name)?;
        let metadata = file.metadata()?;
        self.require_regular(&entry, &metadata)?;
        file.sync_all()?;
        sync(&parent)?;
        let file_identity = identity(&metadata)?;
        let readers = paths::private_dir(&self.volume(&entry.mount)?.recovery, "readers", true)?;
        pin(&parent, &entry.name, &readers, resource, file_identity)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(RetainedReader {
            file,
            position: ReaderPosition {
                workspace: self.identity(),
                resource: resource.into(),
                mount: entry.mount,
                file: file_identity,
                offset: 0,
            },
        })
    }

    /// Reopen the retained inode, never whichever file now occupies the guest
    /// pathname. Appends to that inode remain visible; replacements do not.
    pub fn restore_reader(
        &self,
        position: ReaderPosition,
    ) -> Result<RetainedReader, RecoveryError> {
        paths::validate_id(&position.resource)?;
        if position.workspace != self.identity() || position.offset > i64::MAX as u64 {
            return Err(RecoveryError::Conflict);
        }
        let readers =
            paths::private_dir(&self.volume(&position.mount)?.recovery, "readers", false)?;
        if current(&readers, &position.resource)? != Some(position.file) {
            return Err(RecoveryError::Conflict);
        }
        let mut file = readers.open(&position.resource)?;
        file.seek(SeekFrom::Start(position.offset))?;
        Ok(RetainedReader { file, position })
    }
}

impl RetainedReader {
    pub fn position(&mut self) -> Result<ReaderPosition, RecoveryError> {
        self.position.offset = self.file.stream_position()?;
        Ok(self.position.clone())
    }
}

impl ReaderPosition {
    pub fn resource(&self) -> &str {
        &self.resource
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn same_resource(&self, other: &Self) -> bool {
        self.workspace == other.workspace
            && self.resource == other.resource
            && self.mount == other.mount
            && self.file == other.file
    }
}

impl Read for RetainedReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

impl Seek for RetainedReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}
