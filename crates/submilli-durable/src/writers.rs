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
//! `write_chunk.rs`, and `close_writer.rs`. This file provides shared access
//! to the staging directory and persisted close state.
use crate::filesystem::{DurableFilesystem, FilesystemError, conflict};
use crate::io::{Files, RecoveryError};
use crate::unit_of_work::*;
use cap_std::fs::Dir;
use interpreter::runtime::durable::EffectId;
use std::sync::Arc;

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
