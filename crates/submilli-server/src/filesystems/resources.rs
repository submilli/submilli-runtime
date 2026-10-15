//! Logical file resources. Execution history owns read observations and the
//! cursor checkpoint consumed by replay; this module owns the retained inode.
use super::*;
use crate::domain::durable_object::Retention;
use interpreter::runtime::fs::recovery::{ReaderPosition, RetainedReader};
use std::collections::{BTreeSet, VecDeque};
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
        validate_path(path)?;
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        let initial = self.journal.reader(&self.id, resource, path).await?;
        let opening = initial.is_none();
        let access = self.objects.filesystem_resources.clone().read_owned().await;
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
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        let initial = self
            .journal
            .reader_position(&self.id, position.resource())
            .await?;
        if !initial.same_resource(&position) {
            return Err(RecoveryError::Conflict.into());
        }
        let access = self.objects.filesystem_resources.clone().read_owned().await;
        let workspace = self.workspace.clone();
        blocking(gate, move || {
            Ok(FilesystemReader {
                reader: workspace.restore_reader(position)?,
                _access: access,
            })
        })
        .await
    }

    /// A writer buffers immutable captures, not a disposable native BufWriter.
    /// Each chunk has a stable ID, so replay cannot append a chunk twice.
    pub async fn open_writer(&self, resource: &str, path: &str) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_path(path)?;
        let _gate = self.objects.filesystem_gate.lock().await;
        self.journal.open_writer(&self.id, resource, path).await
    }

    pub async fn write_chunk(
        &self,
        resource: &str,
        chunk: &str,
        content: Content,
    ) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_id(chunk)?;
        Mutation::Write {
            path: "/".into(),
            content: content.clone(),
        }
        .validate()?;
        let _gate = self.objects.filesystem_gate.lock().await;
        self.journal
            .writer_chunk(&self.id, resource, chunk, &content)
            .await
    }

    /// Closing materializes one immutable concatenation through the same write
    /// protocol. A pause before close keeps every chunk; a lost close receipt
    /// is reconciled before another mutation is admitted.
    pub async fn close_writer(
        &self,
        resource: &str,
        effect: &str,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(resource)?;
        validate_id(effect)?;
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
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
            .writer_for_close(&self.id, resource, effect, self.objects.maximum_bytes())
            .await?;
        let id = self
            .reserve_writer_capture(resource, &writer.chunks)
            .await?;
        // The gate keeps the immutable chunk sequence stable across reservation
        // and admission. Cancellation here leaves only a tracked private object;
        // the still-open writer can be retried or retired without a public effect.
        self.journal
            .closing_writer(&self.id, resource, effect, self.objects.maximum_bytes())
            .await?;
        let source = Chunks {
            remaining: writer.chunks.into(),
            current: None,
            objects: self.objects.clone(),
            _guard: gate.clone(),
        };
        let prepared = self.objects.publish(&id, source).await?;
        let reference = self
            .objects
            .bind(
                &prepared,
                BTreeSet::from([Retention::Workspace(self.id.clone())]),
            )
            .await?;
        let request = Mutation::Write {
            path: writer.path,
            content: Content {
                id: reference.id.as_str().into(),
                bytes: reference.content.bytes,
                sha256: reference.content.sha256,
            },
        };
        let result = self.execute_held(gate, effect, request).await;
        if result.is_ok() || matches!(&result, Err(FilesystemError::Rejected(_))) {
            self.journal.writer_closed(&self.id, resource).await?;
        }
        result
    }
    pub(super) async fn reserve_writer_capture(
        &self,
        resource: &str,
        chunks: &[Content],
    ) -> Result<ObjectId, FilesystemError> {
        let bytes = chunks.iter().try_fold(0u64, |sum, chunk| {
            sum.checked_add(chunk.bytes).ok_or(RecoveryError::Limit)
        })?;
        let request: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&chunks).map_err(|_| RecoveryError::InvalidRecord)?)
                .into();
        let fingerprint =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, request);
        let id = ObjectId::parse(workspace_id(
            &format!("writer:{resource}:{fingerprint}"),
            &self.id,
        ))
        .map_err(ObjectStoreError::from)?;
        if chunks.iter().any(|chunk| chunk.id == id.as_str()) {
            return Err(RecoveryError::Conflict.into());
        }
        self.journal
            .track_object(&self.id, id.as_str(), request)
            .await?;
        self.objects
            .reserve(id.clone(), self.owner.clone(), request, bytes)
            .await?;
        Ok(id)
    }
}

struct Chunks {
    remaining: VecDeque<Content>,
    current: Option<Box<dyn Read + Send>>,
    objects: DurableObjects,
    _guard: Arc<tokio::sync::OwnedMutexGuard<()>>,
}
impl Read for Chunks {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(reader) = &mut self.current {
                let read = reader.read(bytes)?;
                if read != 0 {
                    return Ok(read);
                }
                self.current = None;
            }
            let Some(chunk) = self.remaining.pop_front() else {
                return Ok(0);
            };
            let id = ObjectId::parse(chunk.id).map_err(io::Error::other)?;
            self.current = Some(self.objects.stream(ObjectReference {
                id,
                content: ContentIdentity {
                    bytes: chunk.bytes,
                    sha256: chunk.sha256,
                },
            }));
        }
    }
}

fn validate_path(path: &str) -> Result<(), FilesystemError> {
    Mutation::Remove { path: path.into() }.validate()?;
    Ok(())
}
