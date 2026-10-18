//! Reader opening saves identity once; individual reads own their cursor progress.
use super::operation::FilesystemOperation;
use super::{
    FilesystemError, blocking, conflict, preflight_refusal, require_idle, require_resource_limit,
    test_checkpoint, validate_id,
};
use crate::io::{RecoveryError, validate_guest_path};
use crate::readers::{FilesystemReader, ReaderPosition};
use crate::unit_of_work::{Reader, StoreError};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::filesystem::ReaderOptions;
use std::sync::Arc;

enum OpenReaderState {
    Initial,
    Completed(Result<ReaderPosition, String>),
}
impl FilesystemOperation {
    pub async fn open_reader(
        &self,
        resource: &EffectId,
        path: &str,
        options: ReaderOptions,
    ) -> Result<FilesystemReader, FilesystemError> {
        validate_id(resource)?;
        validate_guest_path(path)?;
        if matches!(options, ReaderOptions::Bytes { chunk_size: 0 }) {
            return Err(RecoveryError::InvalidRecord.into());
        }
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let mut unit = (self.filesystem.units)().await?;
        let mut reader = if let Some(reader) =
            unit.get_reader(&self.filesystem.run_id, resource).await?
        {
            if reader.filesystem_config_fingerprint != self.filesystem.filesystem_config_fingerprint
                || reader.path != path
                || reader.options != options
            {
                return conflict();
            }
            reader
        } else {
            require_idle(unit.as_mut(), &self.filesystem.run_id).await?;
            require_resource_limit(unit.as_mut(), &self.filesystem.run_id).await?;
            let reader = Reader {
                filesystem_config_fingerprint: self.filesystem.filesystem_config_fingerprint,
                id: resource.clone(),
                path: path.into(),
                options: options.clone(),
                position: None,
                progress: None,
                error: None,
            };
            unit.save_reader(&self.filesystem.run_id, reader.clone())
                .await?;
            reader
        };
        unit.commit().await?;
        let mut state = decode_state(&reader)?;
        loop {
            state = match state {
                OpenReaderState::Initial => {
                    let files = self.filesystem.files.clone();
                    let work = self.work.clone();
                    let (id, path) = (resource.clone(), path.to_owned());
                    let result = blocking(gate.clone(), move || {
                        files.open_reader(&id, &path, work.as_deref())?.position()
                    })
                    .await;
                    match result {
                        Ok(position) => OpenReaderState::Completed(Ok(position)),
                        Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                            OpenReaderState::Completed(Err(error.to_string()))
                        }
                        Err(error) => return Err(error),
                    }
                }
                OpenReaderState::Completed(result) => {
                    result.map_err(FilesystemError::Rejected)?;
                    test_checkpoint("reader-open");
                    return Ok(FilesystemReader {
                        filesystem: self.filesystem.clone(),
                        resource: resource.clone(),
                        options,
                    });
                }
            };
            encode_state(&mut reader, &state);
            let mut unit = (self.filesystem.units)().await?;
            unit.save_reader(&self.filesystem.run_id, reader.clone())
                .await?;
            unit.commit().await?;
        }
    }
}
fn decode_state(reader: &Reader) -> Result<OpenReaderState, FilesystemError> {
    match (&reader.position, &reader.error) {
        (None, None) => Ok(OpenReaderState::Initial),
        (Some(position), None) => Ok(OpenReaderState::Completed(Ok(position.clone()))),
        (None, Some(reason)) => Ok(OpenReaderState::Completed(Err(reason.clone()))),
        _ => Err(StoreError::Conflict.into()),
    }
}
fn encode_state(reader: &mut Reader, state: &OpenReaderState) {
    match state {
        OpenReaderState::Initial => {}
        OpenReaderState::Completed(Ok(position)) => reader.position = Some(position.clone()),
        OpenReaderState::Completed(Err(reason)) => reader.error = Some(reason.clone()),
    }
}
