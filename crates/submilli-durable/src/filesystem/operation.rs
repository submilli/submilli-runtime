//! Per-call accounting and shared journal admission. Operation state machines live in sibling modules.
use super::{
    DurableFilesystem, FilesystemError, conflict, require_capacity, require_ready, serialize,
};
use crate::io::{self as paths, RecoveryError};
use crate::unit_of_work::*;
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::fuel::WorkBudget;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// One call's accounting, separate from the shared filesystem and durable handles.
#[derive(Clone)]
pub struct FilesystemOperation {
    pub(crate) filesystem: DurableFilesystem,
    pub(crate) work: Option<Arc<WorkBudget>>,
}
impl FilesystemOperation {
    pub(crate) fn charge(
        &self,
        rate: interpreter::runtime::fuel::Rate,
        count: u64,
    ) -> Result<(), FilesystemError> {
        if let Some(work) = &self.work {
            work.charge(rate, count).map_err(RecoveryError::from)?;
        }
        Ok(())
    }
    pub(super) fn charge_preparation(
        &self,
        path: &str,
        payload: u64,
    ) -> Result<(), FilesystemError> {
        use interpreter::runtime::fuel;
        if self.work.is_none() {
            return Ok(());
        }
        let original =
            paths::resolve_content(self.filesystem.vfs(), self.filesystem.vfs().cwd(), path)
                .and_then(|p| p.metadata())
                .ok()
                .filter(cap_std::fs::Metadata::is_file)
                .map_or(0, |m| m.len());
        let bytes = original
            .saturating_mul(2)
            .saturating_add(payload.saturating_mul(3));
        self.charge(fuel::HASH, bytes)?;
        self.charge(fuel::IO, bytes)
    }
    pub(super) async fn create_initial_or_get_last_journal_entry<T: Serialize>(
        &self,
        unit: &mut dyn UnitOfWork,
        effect: &EffectId,
        kind: &str,
        request: &T,
    ) -> Result<JournalEntry, FilesystemError> {
        let mut fingerprint = self.filesystem.filesystem_config_fingerprint.to_vec();
        fingerprint.extend_from_slice(&Sha256::digest(serialize(request)?));
        if let Some(journal_entry) = unit
            .get_journal_entry(&self.filesystem.run_id, effect)
            .await?
        {
            if journal_entry.operation != kind
                || journal_entry.version != 2
                || journal_entry.request != fingerprint
            {
                return conflict();
            }
            return Ok(journal_entry);
        }
        require_ready(unit, &self.filesystem.run_id, None).await?;
        require_capacity(unit, &self.filesystem.run_id).await?;
        if unit
            .pending_journal_entry(&self.filesystem.run_id)
            .await?
            .is_some()
        {
            return conflict();
        }
        if unit
            .writer_for_effect(&self.filesystem.run_id, effect)
            .await?
            .is_some()
        {
            return conflict();
        }
        let journal_entry = JournalEntry {
            id: effect.clone(),
            run_id: self.filesystem.run_id.clone(),
            operation: kind.into(),
            version: 2,
            request: fingerprint,
            recovery_payload: None,
            outcome: None,
            rejected: false,
        };
        unit.save_journal_entry(journal_entry.clone()).await?;
        Ok(journal_entry)
    }
    /// Commit one transition. The unit validates immutable intent and completed receipts.
    pub(super) async fn save_journal_entry(
        &self,
        journal_entry: &JournalEntry,
    ) -> Result<(), FilesystemError> {
        let mut unit = (self.filesystem.units)().await?;
        unit.save_journal_entry(journal_entry.clone()).await?;
        unit.commit().await?;
        Ok(())
    }
}
