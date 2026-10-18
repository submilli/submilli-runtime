use super::operation::FilesystemOperation;
use super::{FilesystemError, blocking, fingerprint, test_checkpoint, validate_id};
use super::{conflict, invalid, require_idle};
use crate::io::RecoveryError;
use crate::unit_of_work::*;
use cap_std::fs::OpenOptions;
use interpreter::runtime::durable::EffectId;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

enum WriteChunkState {
    Initial { path: String, offset: u64 },
    Staged,
    Completed,
}
impl FilesystemOperation {
    pub async fn write_chunk(
        &self,
        resource: &EffectId,
        chunk: &EffectId,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_id(chunk)?;
        self.charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)?;
        self.charge(interpreter::runtime::fuel::IO, bytes.len() as u64)?;
        let content = fingerprint(chunk, &bytes)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let bytes = Arc::new(bytes);
        let mut state = decode_state(self, resource, chunk, &content).await?;
        loop {
            state = match state {
                WriteChunkState::Initial { path, offset } => {
                    let workspace = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let resource_owned = resource.to_owned();
                    let fingerprint = content.clone();
                    let bytes = bytes.clone();
                    let staged = blocking(gate.clone(), move || {
                        let resource = &resource_owned;
                        let path = path.as_str();
                        let content = &fingerprint;
                        let bytes = bytes.as_slice();
                        let work = work.as_deref();

                        let (directory, volume) =
                            workspace.writer_directory(resource, path, true)?;
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
                            volume.check_growth(end.saturating_sub(length), work)?;
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
                            crate::test_checkpoint("writer-staged");
                            Ok(())
                        })();
                        workspace.reconcile_quotas(work)?;
                        result
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
                    WriteChunkState::Staged
                }
                WriteChunkState::Staged => {
                    // Once acknowledged, charge remaining work by settlement.
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    encode_state(self, resource, chunk, &content).await?;
                    test_checkpoint("writer-chunk-receipt");
                    WriteChunkState::Completed
                }
                WriteChunkState::Completed => return Ok(()),
            };
        }
    }
}

async fn decode_state(
    operation: &FilesystemOperation,
    resource: &EffectId,
    chunk: &EffectId,
    content: &Content,
) -> Result<WriteChunkState, FilesystemError> {
    let run_id = &operation.filesystem.run_id;
    let filesystem_config_fingerprint = operation.filesystem.filesystem_config_fingerprint;
    let mut unit = (operation.filesystem.units)().await?;
    let writer = unit
        .get_writer(run_id, resource)
        .await?
        .ok_or(StoreError::Conflict)?;
    if writer.filesystem_config_fingerprint != filesystem_config_fingerprint {
        return conflict();
    }
    if let Some(prior) = unit.get_chunk(run_id, resource, chunk).await? {
        if prior.content != *content {
            return conflict();
        }
        return Ok(WriteChunkState::Completed);
    }
    require_idle(unit.as_mut(), run_id).await?;
    if writer.state != "open" || unit.resource_counts(run_id).await?.chunks >= MAX_OPERATIONS {
        return conflict();
    }
    let chunks = unit
        .writer_chunks(run_id, resource, MAX_OPERATIONS as u32 + 1)
        .await?;
    let offset = chunks
        .iter()
        .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes))
        .ok_or(StoreError::Conflict)?;
    Ok(WriteChunkState::Initial {
        path: writer.path,
        offset,
    })
}
async fn encode_state(
    operation: &FilesystemOperation,
    resource: &EffectId,
    chunk: &EffectId,
    content: &Content,
) -> Result<(), FilesystemError> {
    let run_id = &operation.filesystem.run_id;
    let filesystem_config_fingerprint = operation.filesystem.filesystem_config_fingerprint;
    let mut owned = (operation.filesystem.units)().await?;
    let unit = owned.as_mut();
    let writer = unit
        .get_writer(run_id, resource)
        .await?
        .ok_or(StoreError::Conflict)?;
    if writer.filesystem_config_fingerprint != filesystem_config_fingerprint {
        return conflict();
    }
    if let Some(prior) = unit.get_chunk(run_id, resource, chunk).await? {
        return if prior.content == *content {
            Ok(())
        } else {
            conflict()
        };
    }
    require_idle(unit, run_id).await?;
    if writer.state != "open" || unit.resource_counts(run_id).await?.chunks >= MAX_OPERATIONS {
        return conflict();
    }
    let sequence = unit
        .writer_chunks(run_id, resource, MAX_OPERATIONS as u32 + 1)
        .await?
        .len();
    unit.save_chunk(
        run_id,
        Chunk {
            writer: resource.clone(),
            id: chunk.clone(),
            sequence: u32::try_from(sequence).map_err(invalid)?,
            content: content.clone(),
        },
    )
    .await?;
    owned.commit().await?;
    Ok(())
}
