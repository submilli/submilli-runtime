//! Output staging, acknowledged chunks, and idempotent publication.
use crate::filesystem::*;
use crate::filesystem::{
    conflict, decode_outcome, invalid, require_capacity, require_idle, require_resource_limit,
    require_unused_operation,
};
use crate::io::{Files, RecoveryError, validate_guest_path};
use crate::unit_of_work::*;
use cap_std::fs::{Dir, File, OpenOptions};
use interpreter::runtime::filesystem::{
    FileWriter, FilesystemError as PortError, Invocation, ResourceDescriptor, ResourceKind,
};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
impl DurableFilesystem {
    /// Open one output-in-progress. Chunk receipts contain fingerprints only.
    pub async fn open_writer(&self, resource: &str, path: &str) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_guest_path(path)?;
        let _gate = self.context.filesystem_gate.lock().await;
        self.stored_open_writer(resource, path).await
    }

    pub async fn write_chunk(
        &self,
        resource: &str,
        chunk: &str,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_id(chunk)?;
        self.charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)?;
        self.charge(interpreter::runtime::fuel::IO, bytes.len() as u64)?;
        let content = fingerprint(chunk, &bytes)?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let (path, prior, offset) = self
            .stored_chunk_position(resource, chunk, &content)
            .await?;
        if prior {
            return Ok(());
        }
        let workspace = self.files.clone();
        let resource_owned = resource.to_owned();
        let fingerprint = content.clone();
        let staged = blocking(gate.clone(), move || {
            workspace.stage_writer_chunk(&resource_owned, &path, offset, &fingerprint, &bytes)
        })
        .await;
        match staged {
            // These refusals precede acknowledging the chunk. The private tail
            // remains retryable; corruption and uncertain storage I/O stay fatal.
            Err(FilesystemError::Recovery(
                error @ (RecoveryError::Quota
                | RecoveryError::Limit
                | RecoveryError::Containment(_)),
            )) => {
                return Err(FilesystemError::Rejected(error.to_string()));
            }
            result => result?,
        }
        // Cancellation before this commit leaves only an unacknowledged tail;
        // the next attempt rewrites that same offset, never appends blindly.
        if let Some(work) = &self.work {
            work.before_effect();
        }
        self.stored_writer_chunk(resource, chunk, &content).await?;
        checkpoint("writer-chunk-receipt");
        Ok(())
    }

    pub async fn close_writer(
        &self,
        resource: &str,
        effect: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(resource)?;
        validate_id(effect)?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let writer = self
            .stored_writer_for_close(resource, effect, i64::MAX as u64)
            .await?;
        // A close receipt remains authoritative even after the destination is
        // replaced. Replaying close must not read or publish the output again.
        if let Some(outcome) = self.stored_writer_outcome(effect).await? {
            self.stored_writer_closed(resource).await?;
            return unpack(outcome);
        }
        let bytes = writer
            .chunks
            .iter()
            .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.bytes))
            .ok_or(RecoveryError::Limit)?;
        self.charge(interpreter::runtime::fuel::ELEM, writer.chunks.len() as u64)?;
        self.charge(interpreter::runtime::fuel::HASH, bytes.saturating_mul(2))?;
        self.charge(interpreter::runtime::fuel::IO, bytes)?;
        let result = if writer.chunks.is_empty() {
            // Empty output needs no staging volume. Let write preflight persist
            // ordinary target refusals, including a read-only mount.
            let content = fingerprint(resource, &[])?;
            self.stored_closing_writer(resource, effect, i64::MAX as u64)
                .await?;
            self.write_held(
                gate.clone(),
                effect,
                &writer.path,
                (content, std::io::Cursor::new([])),
                false,
                Some(resource),
            )
            .await
        } else {
            let workspace = self.files.clone();
            let resource_owned = resource.to_owned();
            let path = writer.path.clone();
            let chunks = writer.chunks.clone();
            let (input, content) = blocking(gate.clone(), move || {
                workspace.writer_file(&resource_owned, &path, &chunks)
            })
            .await?;
            self.stored_closing_writer(resource, effect, i64::MAX as u64)
                .await?;
            self.write_held(
                gate.clone(),
                effect,
                &writer.path,
                (content, input),
                false,
                Some(resource),
            )
            .await
        };
        if result.is_ok() || matches!(&result, Err(FilesystemError::Rejected(_))) {
            self.stored_writer_closed(resource).await?;
            checkpoint("writer-closed");
        }
        result
    }
}
pub(crate) struct DurableWriter {
    pub(crate) filesystem: DurableFilesystem,
    pub(crate) resource: String,
    pub(crate) closed: bool,
}
#[async_trait::async_trait]
impl FileWriter for DurableWriter {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            scope: self.filesystem.id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Writer,
        })
    }
    fn is_closed(&self) -> bool {
        self.closed
    }
    async fn write(&mut self, invocation: &Invocation, bytes: Vec<u8>) -> Result<(), PortError> {
        let id = operation(invocation)?;
        let filesystem = self.filesystem.metered(invocation);
        filesystem
            .write_chunk(&self.resource, id, bytes)
            .await
            .map_err(backend)
    }
    async fn close(&mut self, invocation: &Invocation) -> Result<(), PortError> {
        let id = operation(invocation)?;
        let filesystem = self.filesystem.metered(invocation);
        let result = filesystem.close_writer(&self.resource, id).await;
        // SQL remains authoritative after either a successful close or a saved
        // refusal, and after an acknowledgement is lost.
        self.closed = filesystem
            .stored_writer_is_closed(&self.resource)
            .await
            .map_err(backend)?;
        result.map(|_| ()).map_err(backend)
    }
}

impl Files {
    pub(crate) fn stage_writer_chunk(
        &self,
        resource: &str,
        path: &str,
        offset: u64,
        content: &Content,
        bytes: &[u8],
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
            let mut source = std::io::Cursor::new(bytes);
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
            crate::io::sync(&directory)?;
            crate::checkpoint("writer-staged");
            Ok(())
        })();
        self.reconcile_quotas()?;
        result
    }
    pub(crate) fn writer_file(
        &self,
        resource: &str,
        path: &str,
        chunks: &[Content],
    ) -> Result<(File, Content), RecoveryError> {
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
        crate::io::sync(&directory)?;
        self.reconcile_quotas()?;
        let content = Content {
            id: resource.into(),
            bytes,
            sha256: whole.finalize().into(),
        };
        file.seek(SeekFrom::Start(0))?;
        Ok((file, content))
    }
    fn writer_directory(
        &self,
        resource: &str,
        path: &str,
        create: bool,
    ) -> Result<(Dir, &crate::io::Volume), RecoveryError> {
        crate::io::validate_id(resource)?;
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
        let directory =
            crate::io::private_dir(&volume.recovery, &format!("writer-{resource}"), create)?;
        Ok((directory, volume))
    }
}

impl DurableFilesystem {
    pub(crate) async fn stored_writer_is_closed(
        &self,
        resource: &str,
    ) -> Result<bool, FilesystemError> {
        let scope = self.id.as_str();
        let mut unit = (self.units)().await?;
        let writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != self.binding {
            return conflict();
        }
        Ok(writer.state == "closed")
    }
    pub(crate) async fn stored_open_writer(
        &self,
        resource: &str,
        path: &str,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        if let Some(prior) = unit.get_writer(scope, resource).await? {
            return if prior.binding == binding && prior.path == path {
                Ok(())
            } else {
                conflict()
            };
        }
        require_idle(unit, scope).await?;
        require_resource_limit(unit, scope).await?;
        unit.save_writer(
            scope,
            Writer {
                binding,
                id: resource.into(),
                path: path.into(),
                close_effect: None,
                state: "open".into(),
            },
        )
        .await?;
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_chunk_position(
        &self,
        resource: &str,
        chunk: &str,
        content: &Content,
    ) -> Result<(String, bool, u64), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut unit = (self.units)().await?;
        let writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != binding {
            return conflict();
        }
        if let Some(prior) = unit.get_chunk(scope, resource, chunk).await? {
            if prior.content != *content {
                return conflict();
            }
            return Ok((writer.path, true, 0));
        }
        require_idle(unit.as_mut(), scope).await?;
        if writer.state != "open" || unit.resource_counts(scope).await?.chunks >= MAX_OPERATIONS {
            return conflict();
        }
        let chunks = unit
            .writer_chunks(scope, resource, MAX_OPERATIONS as u32 + 1)
            .await?;
        let offset = chunks
            .iter()
            .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes))
            .ok_or(StoreError::Conflict)?;
        Ok((writer.path, false, offset))
    }
    pub(crate) async fn stored_writer_outcome(
        &self,
        effect: &str,
    ) -> Result<Option<RecordedOutcome>, FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut unit = (self.units)().await?;
        Ok(unit
            .get_operation(scope, effect)
            .await?
            .map(|row| decode_outcome(&row, binding))
            .transpose()?
            .flatten())
    }
    pub(crate) async fn stored_writer_chunk(
        &self,
        resource: &str,
        chunk: &str,
        content: &Content,
    ) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != binding {
            return conflict();
        }
        if let Some(prior) = unit.get_chunk(scope, resource, chunk).await? {
            return if prior.content == *content {
                Ok(())
            } else {
                conflict()
            };
        }
        require_idle(unit, scope).await?;
        if writer.state != "open" || unit.resource_counts(scope).await?.chunks >= MAX_OPERATIONS {
            return conflict();
        }
        let sequence = unit
            .writer_chunks(scope, resource, MAX_OPERATIONS as u32 + 1)
            .await?
            .len();
        unit.save_chunk(
            scope,
            Chunk {
                writer: resource.into(),
                id: chunk.into(),
                sequence: u32::try_from(sequence).map_err(invalid)?,
                content: content.clone(),
            },
        )
        .await?;
        owned.commit().await?;
        Ok(())
    }
    pub(crate) async fn stored_closing_writer(
        &self,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
    ) -> Result<WriterRecord, FilesystemError> {
        self.stored_writer_close_record(resource, effect, maximum_bytes, true)
            .await
    }
    pub(crate) async fn stored_writer_for_close(
        &self,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
    ) -> Result<WriterRecord, FilesystemError> {
        self.stored_writer_close_record(resource, effect, maximum_bytes, false)
            .await
    }
    async fn stored_writer_close_record(
        &self,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
        admit: bool,
    ) -> Result<WriterRecord, FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != binding {
            return conflict();
        }
        if writer
            .close_effect
            .as_ref()
            .is_some_and(|prior| *prior != effect)
        {
            return conflict();
        }
        let chunks = unit
            .writer_chunks(scope, resource, MAX_OPERATIONS as u32 + 1)
            .await?;
        if chunks.len() > MAX_OPERATIONS as usize {
            return conflict();
        }
        let bytes = chunks
            .iter()
            .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes));
        if bytes.is_none_or(|bytes| bytes > maximum_bytes) {
            return conflict();
        }
        if writer.close_effect.is_none() {
            require_idle(unit, scope).await?;
            require_capacity(unit, scope).await?;
            require_unused_operation(unit, scope, effect).await?;
            if admit {
                writer.close_effect = Some(effect.into());
                writer.state = "closing".into();
                unit.save_writer(scope, writer.clone()).await?;
            }
        }
        owned.commit().await?;
        Ok(WriterRecord {
            path: writer.path,
            chunks: chunks.into_iter().map(|chunk| chunk.content).collect(),
        })
    }
    pub(crate) async fn stored_writer_closed(&self, resource: &str) -> Result<(), FilesystemError> {
        let scope = self.id.as_str();
        let binding = self.binding;
        let mut owned = (self.units)().await?;
        let unit = owned.as_mut();
        let mut writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != binding {
            return conflict();
        }
        writer.state = "closed".into();
        unit.save_writer(scope, writer).await?;
        owned.commit().await?;
        Ok(())
    }
}
