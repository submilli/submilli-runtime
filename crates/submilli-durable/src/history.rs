//! Ordered native outcomes for one execution attempt.
//!
//! There is no durable pending state: the program supplies the next call again
//! after restart. Only a committed outcome suppresses its native action.
use interpreter::runtime::durable::EffectId;
use interpreter::runtime::durable::RunId;
use interpreter::runtime::effects::{
    CODEC_VERSION, EffectCall, EffectHistory, HistoryError, RecordedOutcome,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::unit_of_work::{StoreError, UnitFactory};

pub const MAX_ENTRY_BYTES: usize = interpreter::runtime::effects::MAX_OUTCOME_BYTES + 64 * 1024;
pub const MAX_STORED_ENTRY_BYTES: usize = MAX_ENTRY_BYTES + 128;
pub const MAX_HISTORY_BYTES: u64 = 1024 * 1024 * 1024;

/// Existing server ownership/attempt identity. This is not part of an effect ID.
#[derive(Clone, Debug)]
pub struct ExecutionAttempt {
    pub run_id: RunId,
    pub owner_generation: String,
    pub attempt: i64,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryProgress {
    pub position: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub sequence: u64,
    pub operation: String,
    pub version: u32,
    pub arguments_fingerprint: [u8; 32],
    pub outcome: RecordedOutcome,
}

pub struct History {
    attempt: ExecutionAttempt,
    store: Arc<dyn HistoryStore>,
    cursor: Mutex<Cursor>,
}
#[derive(Default)]
struct Cursor {
    sequence: u64,
    pending: Option<PendingCall>,
}
struct PendingCall {
    operation: String,
    arguments_fingerprint: [u8; 32],
}
impl History {
    /// A fresh interpreter always starts at the first call, even with saved history.
    /// Compatibility constructor for embedders retaining inline history in a unit of work.
    pub fn new(attempt: ExecutionAttempt, units: UnitFactory) -> Self {
        Self::with_store(attempt, Arc::new(UnitHistoryStore(units)))
    }

    pub fn with_store(attempt: ExecutionAttempt, store: Arc<dyn HistoryStore>) -> Self {
        Self {
            attempt,
            store,
            cursor: Mutex::new(Cursor::default()),
        }
    }

    /// Drain persistence workers even when an execution future was cancelled.
    pub async fn drain(&self) -> Result<(), HistoryError> {
        self.store.drain().await.map_err(history_error)
    }

    pub async fn position(&self) -> u64 {
        self.cursor.lock().await.sequence
    }

    pub fn effect_id(&self, sequence: u64) -> EffectId {
        effect_id(&self.attempt.run_id, sequence)
    }

    async fn save_outcome(&self, entry: Arc<HistoryEntry>) -> Result<(), StoreError> {
        match self.store.append(&self.attempt, Arc::clone(&entry)).await {
            Ok(()) => Ok(()),
            Err(commit_error) => {
                // A lost acknowledgement is resolved by identity and exact contents,
                // never by running the native action a second time here.
                self.store.progress(&self.attempt).await?;
                if self
                    .store
                    .get(&self.attempt.run_id, entry.sequence)
                    .await?
                    .as_ref()
                    == Some(entry.as_ref())
                {
                    Ok(())
                } else {
                    Err(commit_error)
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl EffectHistory for History {
    fn take_storage_fuel(&self) -> u64 {
        self.store.take_storage_fuel()
    }
    async fn drain(&self) -> Result<(), HistoryError> {
        History::drain(self).await
    }

    async fn begin(
        &self,
        operation: &str,
        fingerprint: [u8; 32],
    ) -> Result<EffectCall, HistoryError> {
        let mut cursor = self.cursor.lock().await;
        if cursor.pending.is_some() {
            return Err(HistoryError(
                "the previous native call has no committed outcome".into(),
            ));
        }
        let progress = self
            .store
            .progress(&self.attempt)
            .await
            .map_err(history_error)?;
        if cursor.sequence > progress.position {
            return Err(HistoryError(
                "history cursor is beyond its committed prefix".into(),
            ));
        }
        let id = self.effect_id(cursor.sequence);
        if cursor.sequence < progress.position {
            let entry = self
                .store
                .get(&self.attempt.run_id, cursor.sequence)
                .await
                .map_err(history_error)?
                .ok_or_else(|| {
                    HistoryError(format!("missing outcome at call {}", cursor.sequence))
                })?;
            if entry.sequence != cursor.sequence
                || entry.operation != operation
                || entry.arguments_fingerprint != fingerprint
                || entry.version != CODEC_VERSION
            {
                return Err(HistoryError(format!(
                    "replay diverged at call {} ({operation})",
                    cursor.sequence
                )));
            }
            let call = EffectCall {
                id,
                sequence: cursor.sequence,
                recorded: Some(entry.outcome),
            };
            cursor.sequence = cursor
                .sequence
                .checked_add(1)
                .ok_or_else(|| HistoryError("effect sequence exhausted".into()))?;
            return Ok(call);
        }
        // Leave room for one maximum-sized outcome before allowing native work.
        if progress.bytes > MAX_HISTORY_BYTES.saturating_sub(MAX_STORED_ENTRY_BYTES as u64) {
            return Err(HistoryError("run history reached its storage limit".into()));
        }
        i64::try_from(
            cursor
                .sequence
                .checked_add(1)
                .ok_or_else(|| HistoryError("effect sequence exhausted".into()))?,
        )
        .map_err(|_| HistoryError("effect sequence exhausted".into()))?;
        cursor.pending = Some(PendingCall {
            operation: operation.into(),
            arguments_fingerprint: fingerprint,
        });
        Ok(EffectCall {
            id,
            sequence: cursor.sequence,
            recorded: None,
        })
    }

    async fn complete(
        &self,
        call: &EffectCall,
        outcome: RecordedOutcome,
    ) -> Result<(), HistoryError> {
        let mut cursor = self.cursor.lock().await;
        if call.recorded.is_some()
            || call.sequence != cursor.sequence
            || call.id != self.effect_id(cursor.sequence)
        {
            return Err(HistoryError(
                "completion does not belong to the current native call".into(),
            ));
        }
        let pending = cursor
            .pending
            .as_ref()
            .ok_or_else(|| HistoryError("no native call is awaiting completion".into()))?;
        let entry = HistoryEntry {
            sequence: cursor.sequence,
            operation: pending.operation.clone(),
            version: CODEC_VERSION,
            arguments_fingerprint: pending.arguments_fingerprint,
            outcome,
        };
        self.save_outcome(Arc::new(entry))
            .await
            .map_err(history_error)?;
        cursor.sequence += 1; // begin checked the successor before allowing native work.
        cursor.pending = None;
        Ok(())
    }

    async fn ensure_history_consumed(&self) -> Result<(), HistoryError> {
        let cursor = self.cursor.lock().await;
        let progress = self
            .store
            .progress(&self.attempt)
            .await
            .map_err(history_error)?;
        if cursor.pending.is_some() || cursor.sequence != progress.position {
            return Err(HistoryError(
                "execution ended before completing its recorded history".into(),
            ));
        }
        Ok(())
    }
}

fn effect_id(run_id: &RunId, sequence: u64) -> EffectId {
    let mut hash = Sha256::new();
    hash.update(b"submilli:effect:v1\0");
    hash.update((run_id.len() as u64).to_be_bytes());
    hash.update(run_id.as_bytes());
    hash.update(sequence.to_be_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]); // SHA-256 always supplies 32 bytes.
    uuid::Builder::from_custom_bytes(bytes)
        .into_uuid()
        .to_string()
        .into()
}
fn history_error(error: StoreError) -> HistoryError {
    HistoryError(error.to_string())
}

/// Persistence of complete outcomes. Append fences the execution attempt and commits
/// the entry, its ownership, position and accounting atomically before returning.
/// Physical payload placement is private to the implementation.
#[async_trait::async_trait]
pub trait HistoryStore: Send + Sync {
    fn take_storage_fuel(&self) -> u64 {
        0
    }
    async fn progress(&self, attempt: &ExecutionAttempt) -> Result<HistoryProgress, StoreError>;
    async fn get(&self, run: &RunId, sequence: u64) -> Result<Option<HistoryEntry>, StoreError>;
    async fn append(
        &self,
        attempt: &ExecutionAttempt,
        entry: Arc<HistoryEntry>,
    ) -> Result<(), StoreError>;
    async fn drain(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

struct UnitHistoryStore(UnitFactory);
#[async_trait::async_trait]
impl HistoryStore for UnitHistoryStore {
    async fn progress(&self, attempt: &ExecutionAttempt) -> Result<HistoryProgress, StoreError> {
        (self.0)().await?.history_progress(attempt).await
    }
    async fn get(&self, run: &RunId, sequence: u64) -> Result<Option<HistoryEntry>, StoreError> {
        (self.0)().await?.get_history_entry(run, sequence).await
    }
    async fn append(
        &self,
        attempt: &ExecutionAttempt,
        entry: Arc<HistoryEntry>,
    ) -> Result<(), StoreError> {
        let mut unit = (self.0)().await?;
        unit.append_history(attempt, entry).await?;
        unit.commit().await
    }
}
