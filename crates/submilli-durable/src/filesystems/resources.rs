//! Effect-aware logical resources. SQL positions, not native cursors, are authoritative.
use super::backend::{backend, operation};
use super::*;
use crate::state::Progress;
use interpreter::runtime::filesystem::{
    FileReader, FilesystemError as PortError, Invocation, ReadOutput, ReadPosition, ReaderOptions,
    ResourceDescriptor, ResourceKind,
};
use interpreter::runtime::limits::TenantLimits;

/// ID-only handles are valid during replay. Open a native file only for a new
/// backend call; dropping this object never retires its durable row.
pub struct FilesystemReader {
    pub(super) filesystem: DurableFilesystem,
    pub(super) resource: String,
    pub(super) options: ReaderOptions,
}
#[async_trait::async_trait]
impl FileReader for FilesystemReader {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            scope: self.filesystem.id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Reader(self.options.clone()),
        })
    }
    async fn next(
        &mut self,
        invocation: &Invocation,
        limits: &TenantLimits,
    ) -> Result<ReadOutput, PortError> {
        let fs = self.filesystem.metered(invocation);
        let effect = operation(invocation)?;
        validate_id(effect).map_err(backend)?;
        if effect == self.resource {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let _gate = fs.context.filesystem_gate.lock().await;
        let row = fs
            .journal
            .reader_record(&fs.id, &self.resource)
            .await
            .map_err(backend)?;
        if row.options != self.options {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let before = progress_start(&row.progress, effect, false).map_err(backend)?;
        let initial = row
            .position
            .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
        // Revalidate the path on every dispatched call, even in the same process.
        // Native buffering cannot become an alternative progress authority.
        let mut after = before.clone();
        let output = if before.closed {
            interpreter::runtime::default_filesystem::read_item(
                &mut std::io::Cursor::new([]),
                &self.options,
                &mut after,
                invocation,
                limits,
            )?
        } else {
            let mut native = fs
                .workspace
                .restore_reader(initial)
                .map_err(|e| backend(e.into()))?;
            interpreter::runtime::default_filesystem::read_item(
                &mut native,
                &self.options,
                &mut after,
                invocation,
                limits,
            )?
        };
        checkpoint("reader-read");
        if let Some(work) = &invocation.work {
            work.before_effect();
        }
        fs.journal
            .reader_progress(
                &fs.id,
                &self.resource,
                row.progress,
                Progress {
                    effect: effect.into(),
                    close: false,
                    before,
                    after,
                },
            )
            .await
            .map_err(backend)?;
        checkpoint("reader-progress");
        Ok(output)
    }
    async fn close(&mut self, invocation: &Invocation) -> Result<(), PortError> {
        let fs = self.filesystem.metered(invocation);
        let effect = operation(invocation)?;
        validate_id(effect).map_err(backend)?;
        if effect == self.resource {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let _gate = fs.context.filesystem_gate.lock().await;
        let row = fs
            .journal
            .reader_record(&fs.id, &self.resource)
            .await
            .map_err(backend)?;
        if row.options != self.options {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let before = progress_start(&row.progress, effect, true).map_err(backend)?;
        let after = ReadPosition {
            closed: true,
            ..before.clone()
        };
        if let Some(work) = &invocation.work {
            work.before_effect();
        }
        fs.journal
            .reader_progress(
                &fs.id,
                &self.resource,
                row.progress,
                Progress {
                    effect: effect.into(),
                    close: true,
                    before,
                    after,
                },
            )
            .await
            .map_err(backend)
    }
}

pub(super) fn progress_start<T: Clone + Default>(
    progress: &Option<Progress<T>>,
    effect: &str,
    close: bool,
) -> Result<T, FilesystemError> {
    match progress {
        Some(prior) if prior.effect == effect => {
            if prior.close != close {
                return Err(RecoveryError::Conflict.into());
            }
            Ok(prior.before.clone())
        }
        Some(prior) => Ok(prior.after.clone()),
        None => Ok(T::default()),
    }
}

impl DurableFilesystem {
    pub async fn open_reader(
        &self,
        resource: &str,
        path: &str,
        options: ReaderOptions,
    ) -> Result<FilesystemReader, FilesystemError> {
        validate_id(resource)?;
        Mutation::Remove { path: path.into() }.validate()?;
        if matches!(options, ReaderOptions::Bytes { chunk_size: 0 }) {
            return Err(RecoveryError::InvalidRecord.into());
        }
        let gate = Arc::new(self.context.filesystem_gate.clone().lock_owned().await);
        let initial = self
            .journal
            .reader(&self.id, resource, path, &options)
            .await?;
        if initial.is_none() {
            let workspace = self.workspace.clone();
            let (id, path) = (resource.to_owned(), path.to_owned());
            let opened = blocking(gate.clone(), move || {
                workspace.open_reader(&id, &path)?.position()
            })
            .await;
            let position = match opened {
                Ok(position) => position,
                Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                    self.journal
                        .reject_reader(&self.id, resource, error.to_string())
                        .await?;
                    return Err(FilesystemError::Rejected(error.to_string()));
                }
                Err(error) => return Err(error),
            };
            self.journal
                .reader_opened(&self.id, resource, &position)
                .await?;
        }
        checkpoint("reader-open");
        Ok(FilesystemReader {
            filesystem: self.clone(),
            resource: resource.into(),
            options,
        })
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
            .await?;
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
        let result = self
            .execute_held(gate, effect, request, Some(input), Some(resource))
            .await;
        if result.is_ok() || matches!(&result, Err(FilesystemError::Rejected(_))) {
            self.journal.writer_closed(&self.id, resource).await?;
            checkpoint("writer-closed");
        }
        result
    }
}
