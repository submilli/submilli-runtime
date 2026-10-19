//! The same effect rereads from before; a new effect continues from after.
//! Returned bytes belong to execution history, so only cursor progress is encoded here.
use super::operation::FilesystemOperation;
use super::{FilesystemError, backend, test_checkpoint, validate_id};
use crate::io::RecoveryError;
use crate::readers::progress_start;
use crate::unit_of_work::{Progress, Reader};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::filesystem::{
    FilesystemError as PortError, ReadOutput, ReadPosition, ReaderOptions,
};
use interpreter::runtime::limits::TenantLimits;

enum NextReaderState {
    ReadAt(ReadPosition),
    SaveProgress {
        before: ReadPosition,
        after: ReadPosition,
        output: ReadOutput,
    },
    Completed(ReadOutput),
}
impl FilesystemOperation {
    pub(crate) async fn next_reader(
        &self,
        resource: &EffectId,
        effect: &EffectId,
        options: &ReaderOptions,
        limits: &TenantLimits,
    ) -> Result<ReadOutput, PortError> {
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
                NextReaderState::ReadAt(before) => {
                    let initial = reader
                        .position
                        .clone()
                        .ok_or_else(|| backend(RecoveryError::Conflict.into()))?;
                    let mut after = before.clone();
                    // Reopen and validate only for a dispatched read, never on handle restoration.
                    let output = if before.closed {
                        interpreter::runtime::default_filesystem::read_item(
                            &mut std::io::Cursor::new([]),
                            options,
                            &mut after,
                            self.work.as_deref(),
                            limits,
                        )?
                    } else {
                        let mut native = fs
                            .files
                            .restore_reader(initial, self.work.as_deref())
                            .map_err(|e| backend(e.into()))?;
                        interpreter::runtime::default_filesystem::read_item(
                            &mut native,
                            options,
                            &mut after,
                            self.work.as_deref(),
                            limits,
                        )?
                    };
                    test_checkpoint("reader-read");
                    NextReaderState::SaveProgress {
                        before,
                        after,
                        output,
                    }
                }
                NextReaderState::SaveProgress {
                    before,
                    after,
                    output,
                } => {
                    if let Some(work) = &self.work {
                        work.before_effect();
                    }
                    let progress = encode_state(effect, before, after);
                    fs.stored_reader_progress(resource, reader.progress.clone(), progress)
                        .await
                        .map_err(backend)?;
                    test_checkpoint("reader-progress");
                    NextReaderState::Completed(output)
                }
                NextReaderState::Completed(output) => return Ok(output),
            };
        }
    }
}
fn decode_state(reader: &Reader, effect: &EffectId) -> Result<NextReaderState, FilesystemError> {
    Ok(NextReaderState::ReadAt(progress_start(
        &reader.progress,
        effect,
        false,
    )?))
}
fn encode_state(
    effect: &EffectId,
    before: ReadPosition,
    after: ReadPosition,
) -> Progress<ReadPosition> {
    Progress {
        effect: effect.clone(),
        close: false,
        before,
        after,
    }
}
