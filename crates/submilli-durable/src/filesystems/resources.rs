//! Logical file resources. Execution history owns read observations and the
//! cursor checkpoint consumed by replay; this module owns the retained inode.
use super::*;
use crate::recovery::{ReaderPosition, RetainedReader};
use std::io::{self, Seek, SeekFrom};

/// A native reader with a logical, serializable position. Dropping it releases
/// native access, not the durable inode pin. Cleanup waits for all such access.
pub struct FilesystemReader {
    reader: RetainedReader,
    _access: tokio::sync::OwnedRwLockReadGuard<()>,
}

impl FilesystemReader {
    /// Save only at a host boundary, atomically with its recorded observation.
    /// Buffered text adapters must first account for their unread buffer and
    /// decoder flags. A bare descriptor position is not a text checkpoint.
    pub fn position(&mut self) -> Result<ReaderPosition, FilesystemError> {
        Ok(self.reader.position()?)
    }
}
impl Read for FilesystemReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reader.read(bytes)
    }
}
impl Seek for FilesystemReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.reader.seek(position)
    }
}

impl DurableFilesystem {
    pub async fn open_reader(
        &self,
        resource: &str,
        path: &str,
    ) -> Result<FilesystemReader, FilesystemError> {
        validate_id(resource)?;
        Mutation::Remove { path: path.into() }.validate()?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let initial = self.journal.reader(&self.id, resource, path).await?;
        let opening = initial.is_none();
        let access = self.context.filesystem_resources.clone().read_owned().await;
        let workspace = self.workspace.clone();
        let (id, path) = (resource.to_owned(), path.to_owned());
        let opened = blocking(gate.clone(), move || {
            let reader = match initial {
                Some(position) => workspace.restore_reader(position)?,
                None => workspace.open_reader(&id, &path)?,
            };
            Ok(FilesystemReader {
                reader,
                _access: access,
            })
        })
        .await;
        let mut reader = match opened {
            Ok(reader) => reader,
            Err(FilesystemError::Recovery(error)) if opening && preflight_refusal(&error) => {
                self.journal
                    .reject_reader(&self.id, resource, error.to_string())
                    .await?;
                return Err(FilesystemError::Rejected(error.to_string()));
            }
            Err(error) => return Err(error),
        };
        self.journal
            .reader_opened(&self.id, resource, &reader.reader.position()?)
            .await?;
        Ok(reader)
    }

    pub async fn restore_reader(
        &self,
        position: ReaderPosition,
    ) -> Result<FilesystemReader, FilesystemError> {
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let initial = self
            .journal
            .reader_position(&self.id, position.resource())
            .await?;
        if !initial.same_resource(&position) {
            return Err(RecoveryError::Conflict.into());
        }
        let access = self.context.filesystem_resources.clone().read_owned().await;
        let workspace = self.workspace.clone();
        blocking(gate, move || {
            Ok(FilesystemReader {
                reader: workspace.restore_reader(position)?,
                _access: access,
            })
        })
        .await
    }

    /// Open one output-in-progress. Chunk receipts contain fingerprints only.
    pub async fn open_writer(&self, resource: &str, path: &str) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        Mutation::Remove { path: path.into() }.validate()?;
        let _gate = self.context.filesystem_gate.lock().await;
        self.journal.open_writer(&self.id, resource, path).await
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
        let input = ReplayInput::bytes(chunk.into(), bytes)?;
        let content = input.content();
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let (path, prior, offset) = self
            .journal
            .chunk_position(&self.id, resource, chunk, &content)
            .await?;
        if prior {
            return Ok(());
        }
        let workspace = self.workspace.clone();
        let resource_owned = resource.to_owned();
        let fingerprint = content.clone();
        let staged = blocking(gate.clone(), move || {
            workspace.stage_writer_chunk(&resource_owned, &path, offset, &fingerprint, Some(input))
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
        self.journal
            .writer_chunk(&self.id, resource, chunk, &content)
            .await
    }

    pub async fn close_writer(
        &self,
        resource: &str,
        effect: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(resource)?;
        validate_id(effect)?;
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        self.close_writer_held(gate, resource, effect).await
    }

    pub(super) async fn close_writer_held(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        resource: &str,
        effect: &str,
    ) -> Result<Outcome, FilesystemError> {
        let writer = self
            .journal
            .writer_for_close(&self.id, resource, effect, i64::MAX as u64)
            .await?;
        // A close receipt remains authoritative even after the destination is
        // replaced. Replaying close must not read or publish the output again.
        if let Some(outcome) = self.journal.writer_outcome(&self.id, effect).await? {
            self.journal.writer_closed(&self.id, resource).await?;
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
        let workspace = self.workspace.clone();
        let resource_owned = resource.to_owned();
        let path = writer.path.clone();
        let chunks = writer.chunks.clone();
        let input = blocking(gate.clone(), move || {
            workspace.writer_input(&resource_owned, &path, &chunks)
        })
        .await?;
        self.journal
            .closing_writer(&self.id, resource, effect, i64::MAX as u64)
            .await?;
        let request = Mutation::Write {
            path: writer.path,
            content: input.content(),
        };
        let result = self.execute_held(gate, effect, request, Some(input)).await;
        if result.is_ok() || matches!(&result, Err(FilesystemError::Rejected(_))) {
            self.journal.writer_closed(&self.id, resource).await?;
        }
        result
    }
}
