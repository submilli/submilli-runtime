use super::operation::FilesystemOperation;
use super::{FilesystemError, backend, validate_id};
use crate::io::RecoveryError;
use crate::readers::progress_start;
use crate::unit_of_work::{Progress, Reader};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::filesystem::{FilesystemError as PortError, ReadPosition, ReaderOptions};

enum CloseReaderState {
    CloseAt(ReadPosition),
    Completed,
}
impl FilesystemOperation {
    pub(crate) async fn close_reader(
        &self,
        resource: &EffectId,
        effect: &EffectId,
        options: &ReaderOptions,
    ) -> Result<(), PortError> {
        validate_id(effect).map_err(backend)?;
        if effect == resource {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let fs = &self.filesystem;
        let _gate = fs.filesystem_gate.lock().await;
        let reader = fs.stored_reader_record(resource).await.map_err(backend)?;
        if reader.options != *options {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        let mut state = decode_state(&reader, effect).map_err(backend)?;
        loop {
            state = match state {
                CloseReaderState::CloseAt(before) => {
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    fs.stored_reader_progress(
                        resource,
                        reader.progress.clone(),
                        encode_state(effect, before),
                    )
                    .await
                    .map_err(backend)?;
                    CloseReaderState::Completed
                }
                CloseReaderState::Completed => return Ok(()),
            };
        }
    }
}
fn decode_state(reader: &Reader, effect: &EffectId) -> Result<CloseReaderState, FilesystemError> {
    // Even an already-closed cursor records the new effect, preserving argument validation.
    Ok(CloseReaderState::CloseAt(progress_start(
        &reader.progress,
        effect,
        true,
    )?))
}
fn encode_state(effect: &EffectId, before: ReadPosition) -> Progress<ReadPosition> {
    let after = ReadPosition {
        closed: true,
        ..before.clone()
    };
    Progress {
        effect: effect.clone(),
        close: true,
        before,
        after,
    }
}
