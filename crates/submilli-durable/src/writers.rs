//! Writer handles, private output staging, and journal updates.
//!
//! A writer's opening effect identifies its private output. Each write has its
//! own chunk effect: bytes are written and synced at the acknowledged prefix,
//! then a fingerprint receipt commits. A crash before that receipt leaves an
//! unacknowledged tail, which the retry can overwrite at the same offset.
//!
//! Explicit close reserves a close effect and publishes only acknowledged chunks
//! through the mutation journal. Dropping a handle never publishes or deletes
//! its row. The state machines live in `filesystem/open_writer.rs`,
//! `write_chunk.rs`, and `close_writer.rs`. This file adapts the handle and
//! provides shared access to its staging directory.
use crate::filesystem::conflict;
use crate::filesystem::*;
use crate::io::{Files, RecoveryError};
use crate::unit_of_work::*;
use cap_std::fs::Dir;
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::filesystem::{
    FileWriter, FilesystemError as PortError, ResourceDescriptor, ResourceKind,
};
use interpreter::runtime::fuel::WorkBudget;
use std::sync::Arc;
pub(crate) struct DurableWriter {
    pub(crate) filesystem: DurableFilesystem,
    pub(crate) resource: EffectId,
    pub(crate) closed: bool,
}
#[async_trait::async_trait]
impl FileWriter for DurableWriter {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            run_id: self.filesystem.run_id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Writer,
        })
    }
    fn is_closed(&self) -> bool {
        self.closed
    }
    async fn write(
        &mut self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        bytes: Vec<u8>,
    ) -> Result<(), PortError> {
        let operation = self.filesystem.operation(work);
        operation
            .write_chunk(&self.resource, effect_id, bytes)
            .await
            .map_err(backend)
    }
    async fn close(
        &mut self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
    ) -> Result<(), PortError> {
        let operation = self.filesystem.operation(work);
        let result = operation.close_writer(&self.resource, effect_id).await;
        // SQL remains authoritative after either a successful close or a saved
        // refusal, and after an acknowledgement is lost.
        self.closed = self
            .filesystem
            .stored_writer_is_closed(&self.resource)
            .await
            .map_err(backend)?;
        result.map(|_| ()).map_err(backend)
    }
}

impl Files {
    /// Both chunk writes and close resolve staging on the writer's volume.
    pub(crate) fn writer_directory(
        &self,
        resource: &EffectId,
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
        resource: &EffectId,
    ) -> Result<bool, FilesystemError> {
        let run_id = &self.run_id;
        let mut unit = (self.units)().await?;
        let writer = unit
            .get_writer(run_id, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.filesystem_config_fingerprint != self.filesystem_config_fingerprint {
            return conflict();
        }
        Ok(writer.state == "closed")
    }
}
