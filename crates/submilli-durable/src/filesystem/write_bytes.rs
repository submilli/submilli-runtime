use super::operation::FilesystemOperation;
use super::{FilesystemError, blocking, preflight_refusal};
use crate::io::{
    self as paths, Files, RecoveryError, existing, sync, validate_guest_path, validate_id,
};
use crate::unit_of_work::Outcome;
use cap_std::fs::OpenOptions;
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::fuel::{self, WorkBudget};
use std::io::Write;
use std::sync::Arc;

impl FilesystemOperation {
    /// Repeat an unfinished write with the same path and bytes. The execution
    /// framework owns completion: it must skip completed effects and must not
    /// advance to another effect until this one's completion is durable.
    /// No filesystem journal or input fingerprint is needed for this operation.
    pub async fn write_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Outcome, FilesystemError> {
        validate_guest_path(path)?;
        validate_id(effect)?;
        self.charge(fuel::IO, bytes.len() as u64)?;
        let gate = Arc::new(self.filesystem.filesystem_gate.clone().lock_owned().await);
        let files = self.filesystem.files.clone();
        let work = self.work.clone();
        let effect = effect.clone();
        let path = path.to_owned();
        blocking(gate, move || {
            Ok(files.write_bytes(&effect, &path, &bytes, work.as_deref()))
        })
        .await?
    }
}

impl Files {
    fn write_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        bytes: &[u8],
        work: Option<&WorkBudget>,
    ) -> Result<Outcome, FilesystemError> {
        let entry = self.entry(path, false).map_err(reject_before_write)?;
        let parent = self.parent(&entry).map_err(reject_before_write)?;
        let volume = self.volume(&entry.mount)?;
        let replacement_name = format!("{effect}.replacement");

        // The name is stable across attempts; the inode must be fresh. A crash
        // during rename can leave this name pointing at the public file. Remove
        // our alias before validating public link counts, without a tree scan.
        match volume.recovery.remove_file(&replacement_name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(RecoveryError::from(error).into()),
        }
        let destination = existing(&parent, &entry.name).map_err(reject_before_write)?;
        if let Some(metadata) = &destination {
            self.require_regular(&entry, metadata, work)
                .map_err(reject_before_write)?;
        }
        let previous_bytes = destination.map_or(0, |metadata| metadata.len());
        let written_bytes = bytes.len() as u64;
        volume
            .check_growth(written_bytes.saturating_sub(previous_bytes))
            .map_err(reject_before_write)?;
        let mut replacement_file = volume
            .recovery
            .open_with(
                &replacement_name,
                OpenOptions::new().write(true).create_new(true),
            )
            .map_err(RecoveryError::from)?;
        paths::test_checkpoint("write-temp-created")?;
        replacement_file
            .write_all(bytes)
            .map_err(RecoveryError::from)?;
        paths::test_checkpoint("write-temp-written")?;
        replacement_file.sync_all().map_err(RecoveryError::from)?;
        paths::test_checkpoint("candidate-sync")?;

        if let Some(work) = work {
            work.before_effect();
        }
        volume
            .recovery
            .rename(&replacement_name, &parent, &entry.name)
            .map_err(RecoveryError::from)?;
        crate::test_checkpoint("replacement-published-unaccounted");
        volume.record_size_change(previous_bytes, written_bytes);
        paths::test_checkpoint("published")?;
        sync(&parent)?;
        sync(&volume.recovery)?;
        paths::test_checkpoint("write-synced")?;
        Ok(Outcome {
            bytes: written_bytes,
        })
    }
}

// Only errors known to precede public mutation can be guest-visible refusals.
// Errors during publication or its sync barriers leave completion uncertain.
fn reject_before_write(error: RecoveryError) -> FilesystemError {
    if preflight_refusal(&error) {
        FilesystemError::Rejected(error.to_string())
    } else {
        error.into()
    }
}
