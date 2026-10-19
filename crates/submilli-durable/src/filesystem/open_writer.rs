use super::operation::FilesystemOperation;
use super::{FilesystemError, conflict, require_idle, require_resource_limit, validate_id};
use crate::io::validate_guest_path;
use crate::unit_of_work::Writer;
use interpreter::runtime::durable::EffectId;

enum OpenWriterState {
    Initial,
    Completed,
}
impl FilesystemOperation {
    pub async fn open_writer(
        &self,
        resource: &EffectId,
        path: &str,
    ) -> Result<(), FilesystemError> {
        validate_id(resource)?;
        validate_guest_path(path)?;
        let _gate = self.filesystem.filesystem_gate.lock().await;
        let mut unit = (self.filesystem.units)().await?;
        let existing = unit.get_writer(&self.filesystem.run_id, resource).await?;
        let mut state = decode_state(
            existing.as_ref(),
            self.filesystem.filesystem_config_fingerprint,
            path,
        )?;
        drop(unit);
        loop {
            state = match state {
                OpenWriterState::Initial => {
                    let mut unit = (self.filesystem.units)().await?;
                    require_idle(unit.as_mut(), &self.filesystem.run_id).await?;
                    require_resource_limit(unit.as_mut(), &self.filesystem.run_id).await?;
                    let writer = encode_state(
                        resource,
                        path,
                        self.filesystem.filesystem_config_fingerprint,
                    );
                    unit.save_writer(&self.filesystem.run_id, writer).await?;
                    unit.commit().await?;
                    OpenWriterState::Completed
                }
                OpenWriterState::Completed => return Ok(()),
            };
        }
    }
}
fn decode_state(
    writer: Option<&Writer>,
    config: [u8; 32],
    path: &str,
) -> Result<OpenWriterState, FilesystemError> {
    match writer {
        None => Ok(OpenWriterState::Initial),
        Some(writer) if writer.filesystem_config_fingerprint == config && writer.path == path => {
            Ok(OpenWriterState::Completed)
        }
        _ => conflict(),
    }
}
fn encode_state(resource: &EffectId, path: &str, config: [u8; 32]) -> Writer {
    Writer {
        filesystem_config_fingerprint: config,
        id: resource.clone(),
        path: path.into(),
        close_effect: None,
        state: "open".into(),
    }
}
