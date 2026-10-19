//! Replacement-file work for staged writer close, including empty output.
//! The old destination is never retained. The prepared replacement stays under
//! its private name until retirement, so retry can finish publication without
//! allocating a second copy after a lost acknowledgement.
use crate::io::{self as paths, Files, RecoveryError, current, existing, identity, sync};
use crate::unit_of_work::Content;
use cap_std::fs::{File, OpenOptions};
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::fuel::WorkBudget;
use std::io::{Cursor, Read, Seek, SeekFrom};

use super::{copy_exact, hash};

impl Files {
    pub(super) fn prepare_replacement_from_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        content: &Content,
        bytes: &[u8],
        work: Option<&WorkBudget>,
    ) -> Result<(), RecoveryError> {
        self.prepare_replacement(effect, path, content, &mut Cursor::new(bytes), work)
    }

    pub(super) fn prepare_replacement_from_file(
        &self,
        effect: &EffectId,
        path: &str,
        content: &Content,
        file: &File,
        work: Option<&WorkBudget>,
    ) -> Result<(), RecoveryError> {
        let mut input = file.try_clone()?;
        input.seek(SeekFrom::Start(0))?;
        self.prepare_replacement(effect, path, content, &mut input, work)
    }

    fn prepare_replacement(
        &self,
        effect: &EffectId,
        path: &str,
        content: &Content,
        input: &mut dyn Read,
        work: Option<&WorkBudget>,
    ) -> Result<(), RecoveryError> {
        let entry = self.entry(path, false)?;
        let parent = self.parent(&entry)?;
        let destination = existing(&parent, &entry.name)?;
        if let Some(metadata) = &destination {
            self.require_regular(&entry, metadata, work)?;
        }
        let previous_bytes = destination.map_or(0, |metadata| metadata.len());
        self.volume(&entry.mount)?
            .check_growth(content.bytes.saturating_sub(previous_bytes))?;
        let artifacts = self.artifacts(&entry, effect, true)?;
        let present = existing(&artifacts, "candidate")?;
        if present.as_ref().is_some_and(|metadata| {
            !metadata.is_file() || interpreter::runtime::fs::link_count(metadata) != Some(1)
        }) {
            return Err(RecoveryError::Conflict);
        }
        let mut file = artifacts.open_with(
            "candidate",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true),
        )?;
        copy_exact(input, &mut file, content)?;
        file.sync_all()?;
        paths::test_checkpoint("candidate-sync")?;
        sync(&artifacts)?;
        paths::test_checkpoint("prepared")?;
        Ok(())
    }

    pub(super) fn publish_replacement_from_bytes(
        &self,
        effect: &EffectId,
        path: &str,
        content: &Content,
        bytes: &[u8],
    ) -> Result<(), RecoveryError> {
        self.publish_replacement(effect, path, content, &mut Cursor::new(bytes))
    }

    pub(super) fn publish_replacement_from_file(
        &self,
        effect: &EffectId,
        path: &str,
        content: &Content,
        file: &File,
    ) -> Result<(), RecoveryError> {
        let mut input = file.try_clone()?;
        input.seek(SeekFrom::Start(0))?;
        self.publish_replacement(effect, path, content, &mut input)
    }

    fn publish_replacement(
        &self,
        effect: &EffectId,
        path: &str,
        content: &Content,
        input: &mut dyn Read,
    ) -> Result<(), RecoveryError> {
        let entry = self.entry(path, false)?;
        let volume = self.volume(&entry.mount)?;
        let parent = self.parent(&entry)?;
        let artifacts = self.artifacts(&entry, effect, false)?;
        let candidate = current(&artifacts, "candidate")?.ok_or(RecoveryError::Conflict)?;
        let destination = existing(&parent, &entry.name)?;
        let published = destination.as_ref().map(identity).transpose()? == Some(candidate);
        let previous_bytes = destination.map_or(0, |metadata| metadata.len());
        volume.check_growth(content.bytes.saturating_sub(previous_bytes))?;
        let mut file =
            artifacts.open_with("candidate", OpenOptions::new().read(true).write(true))?;
        let length = file.metadata()?.len();
        if length != content.bytes || hash(&mut file, length)? != content.sha256 {
            // A published candidate had its full size synced before the intent.
            // Repairing same-length damaged bytes preserves its accounted size;
            // an external size change violates exclusive ownership.
            if published && length != content.bytes {
                return Err(RecoveryError::Conflict);
            }
            file.seek(SeekFrom::Start(0))?;
            copy_exact(input, &mut file, content)?;
            file.set_len(content.bytes)?;
        }
        file.sync_all()?;
        paths::test_checkpoint("candidate-sync")?;
        if !published {
            if existing(&artifacts, "publish")?.is_some() {
                artifacts.remove_file("publish")?;
            }
            artifacts.hard_link("candidate", &artifacts, "publish")?;
            artifacts.rename("publish", &parent, &entry.name)?;
            crate::test_checkpoint("replacement-published-unaccounted");
            volume.record_size_change(previous_bytes, content.bytes);
            paths::test_checkpoint("published")?;
        }
        sync(&parent)?;
        sync(&artifacts)?;
        paths::test_checkpoint("before-receipt")
    }
}
