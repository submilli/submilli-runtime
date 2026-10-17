use super::paths::*;
use super::*;
use crate::filesystems::ReplayInput;
use cap_std::fs::OpenOptions;
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom, Write};

impl Workspace {
    pub(crate) fn stage_writer_chunk(
        &self,
        resource: &str,
        path: &str,
        offset: u64,
        content: &Content,
        mut input: Option<ReplayInput>,
    ) -> Result<(), RecoveryError> {
        let (directory, volume) = self.writer_directory(resource, path, true)?;
        let result = (|| {
            let mut file = directory.open_with(
                "output",
                OpenOptions::new().read(true).write(true).create(true),
            )?;
            let length = file.metadata()?.len();
            if length < offset {
                return Err(RecoveryError::Conflict);
            }
            let end = offset
                .checked_add(content.bytes)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or(RecoveryError::Limit)?;
            volume.check_growth(end.saturating_sub(length), self.work.as_deref())?;
            file.seek(SeekFrom::Start(offset))?;
            let mut source = input.open(content)?;
            let mut buffer = [0; 64 * 1024];
            let mut remaining = content.bytes;
            while remaining != 0 {
                let count = usize::try_from(remaining.min(buffer.len() as u64))
                    .map_err(|_| RecoveryError::Limit)?;
                source.read_exact(&mut buffer[..count])?;
                file.write_all(&buffer[..count])?;
                remaining -= count as u64;
            }
            file.set_len(end)?;
            file.sync_all()?;
            paths::sync(&directory)?;
            crate::checkpoint("writer-staged");
            Ok(())
        })();
        self.reconcile_quotas()?;
        result
    }
    pub(crate) fn writer_input(
        &self,
        resource: &str,
        path: &str,
        chunks: &[Content],
    ) -> Result<ReplayInput, RecoveryError> {
        if chunks.is_empty() {
            return ReplayInput::bytes(resource.into(), Vec::new());
        }
        let (directory, _) = self.writer_directory(resource, path, true)?;
        let mut file = directory.open_with(
            "output",
            OpenOptions::new().read(true).write(true).create(true),
        )?;
        let mut whole = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0; 64 * 1024];
        for chunk in chunks {
            let mut remaining = chunk.bytes;
            let mut hash = Sha256::new();
            while remaining != 0 {
                let count = usize::try_from(remaining.min(buffer.len() as u64))
                    .map_err(|_| RecoveryError::Limit)?;
                file.read_exact(&mut buffer[..count])?;
                hash.update(&buffer[..count]);
                whole.update(&buffer[..count]);
                remaining -= count as u64;
            }
            if <[u8; 32]>::from(hash.finalize()) != chunk.sha256 {
                return Err(RecoveryError::Conflict);
            }
            bytes = bytes.checked_add(chunk.bytes).ok_or(RecoveryError::Limit)?;
        }
        // Discard only the unacknowledged tail from an interrupted chunk.
        file.set_len(bytes)?;
        file.sync_all()?;
        paths::sync(&directory)?;
        self.reconcile_quotas()?;
        let content = Content {
            id: resource.into(),
            bytes,
            sha256: whole.finalize().into(),
        };
        let directory = Arc::new(directory);
        Ok(ReplayInput::staged(content, move || {
            Ok(Box::new(directory.open("output")?))
        }))
    }
    fn writer_directory(
        &self,
        resource: &str,
        path: &str,
        create: bool,
    ) -> Result<(Dir, &Volume), RecoveryError> {
        validate_id(resource)?;
        // Staging belongs to the volume, not the destination's current parent.
        // Publication validates the destination and records ordinary refusals.
        // This resolution performs no guest-path I/O; only the private recovery
        // directory is opened below, including when the destination is an alias.
        let target = interpreter::runtime::fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        let placement = target.placement();
        if placement.access() == interpreter::runtime::vfs::Access::ReadOnly {
            return Err(interpreter::runtime::fs::ContainError::ReadOnly(Arc::from(
                placement.mount_point(),
            ))
            .into());
        }
        let volume = self.volume(placement.mount_point())?;
        let directory = private_dir(&volume.recovery, &format!("writer-{resource}"), create)?;
        Ok((directory, volume))
    }
}
