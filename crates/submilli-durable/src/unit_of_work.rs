//! Journal records and the application's transaction contract.
//!
//! `JournalEntry` records namespace/content mutations. `Reader` and `Listing` record
//! resource openings and the last cursor step. `Writer` and `Chunk` track an
//! unpublished output until close hands it to a normal mutation operation.
//!
//! The `validate_successor` methods define allowed updates: identity and completed
//! facts cannot be rewritten. Persistence adapters call them before saving. These
//! checks complement the database's uniqueness and pending-operation constraints.
use crate::readers::{DirectoryEntry, ReaderPosition};
use futures::future::BoxFuture;
use interpreter::runtime::durable::{EffectId, RunId};
use interpreter::runtime::filesystem::{ReadPosition, ReaderOptions};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("durable persistence failed: {0}")]
    Database(Box<dyn std::error::Error + Send + Sync>),
    #[error("durable persistence I/O failed: {0}")]
    Io(String),
    #[error("{0}")]
    Credentials(String),
    #[error("the durable record changed since it was read")]
    Conflict,
    #[error("the durable store is busy")]
    Busy,
}

/// Creates a fresh transaction. No server repository or connection escapes it.
pub type UnitFactory =
    Arc<dyn Fn() -> BoxFuture<'static, Result<Box<dyn UnitOfWork>, StoreError>> + Send + Sync>;

/// Reads see pending writes. Commit consumes the unit; dropping it discards
/// uncommitted changes. An uncertain commit must be resolved by rereading the
/// durable identity in a fresh unit, never by repeating an external mutation.
#[async_trait::async_trait]
pub trait UnitOfWork: Send {
    async fn history_progress(
        &mut self,
        attempt: &crate::history::ExecutionAttempt,
    ) -> Result<crate::history::HistoryProgress, StoreError>;
    async fn get_history_entry(
        &mut self,
        run_id: &RunId,
        sequence: u64,
        memory: interpreter::runtime::limits::HostMemory,
    ) -> Result<Option<crate::history::LoadedHistoryEntry>, StoreError>;
    async fn append_history(
        &mut self,
        attempt: &crate::history::ExecutionAttempt,
        entry: Arc<crate::history::HistoryEntry>,
        memory: interpreter::runtime::limits::HostMemory,
    ) -> Result<(), StoreError>;

    async fn get_journal_entry(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<JournalEntry>, StoreError>;
    async fn save_journal_entry(&mut self, journal_entry: JournalEntry) -> Result<(), StoreError>;
    async fn pending_journal_entry(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<EffectId>, StoreError>;
    async fn journal_entry_count(&mut self, run_id: &RunId) -> Result<u64, StoreError>;

    async fn get_reader(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Reader>, StoreError>;
    async fn save_reader(&mut self, run_id: &RunId, reader: Reader) -> Result<(), StoreError>;
    /// Encoded snapshot size, without loading or decoding its payload.
    async fn listing_bytes(&mut self, run_id: &RunId, id: &EffectId) -> Result<u64, StoreError>;
    async fn get_listing(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Listing>, StoreError>;
    async fn save_listing(&mut self, run_id: &RunId, listing: Listing) -> Result<(), StoreError>;
    async fn pending_resource(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<PendingResource>, StoreError>;
    async fn get_writer(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Writer>, StoreError>;
    async fn save_writer(&mut self, run_id: &RunId, writer: Writer) -> Result<(), StoreError>;
    async fn closing_writer(&mut self, run_id: &RunId) -> Result<Option<Writer>, StoreError>;
    async fn writer_for_effect(
        &mut self,
        run_id: &RunId,
        effect: &EffectId,
    ) -> Result<Option<Writer>, StoreError>;
    async fn get_chunk(
        &mut self,
        run_id: &RunId,
        writer: &EffectId,
        id: &EffectId,
    ) -> Result<Option<Chunk>, StoreError>;
    async fn save_chunk(&mut self, run_id: &RunId, chunk: Chunk) -> Result<(), StoreError>;
    async fn writer_chunks(
        &mut self,
        run_id: &RunId,
        writer: &EffectId,
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError>;

    async fn resource_counts(&mut self, run_id: &RunId) -> Result<ResourceCounts, StoreError>;
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

#[cfg(test)]
pub(crate) mod tests;

/// Input fingerprint and logical identity. No payload bytes or storage location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub id: EffectId,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

/// The mutation receipt returned again for a completed effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub bytes: u64,
}

/// One effect's journal. Each operation serializes its own state enum into
/// `state`; absence means Initial. A prepared state can coexist with an already
/// applied public mutation when a crash interrupts completion recording.
#[derive(Clone)]
pub struct JournalEntry {
    pub id: EffectId,
    pub run_id: RunId,
    /// Mutation kind, such as append, move or writer close; part of the effect identity.
    pub operation: String,
    pub version: u32,
    /// Filesystem configuration fingerprint plus the hash of the arguments, not their payload bytes.
    pub request: Vec<u8>,
    /// Encoded operation state, including its result when Completed.
    pub state: Option<Vec<u8>>,
}

/// The last dispatched effect only. Earlier completed effects are replayed by
/// execution history, never dispatched back to this resource.
///
/// A retry of `effect` starts at `before`; the next distinct effect starts at
/// `after`. Both positions are necessary because filesystem progress may commit
/// before the caller records the returned item in execution history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress<T> {
    pub effect: EffectId,
    pub close: bool,
    pub before: T,
    pub after: T,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListingPosition {
    pub index: usize,
    pub closed: bool,
}
impl<T: PartialEq + Default> Progress<T> {
    fn follows(&self, prior: Option<&Self>) -> bool {
        !self.effect.is_empty()
            && match prior {
                Some(prior) if self.effect == prior.effect => self == prior,
                Some(prior) => self.before == prior.after,
                None => self.before == T::default(),
            }
    }
}
fn progress_follows<T: PartialEq + Default>(
    next: &Option<Progress<T>>,
    prior: &Option<Progress<T>>,
) -> bool {
    match next {
        Some(next) => next.follows(prior.as_ref()),
        None => prior.is_none(),
    }
}

/// An opening identity plus cursor progress. `position` fixes the opened file;
/// `progress` holds the evolving byte offset and decoder/close flags.
#[derive(Clone)]
pub struct Reader {
    pub filesystem_config_fingerprint: [u8; 32],
    pub id: EffectId,
    pub path: String,
    pub options: ReaderOptions,
    pub position: Option<ReaderPosition>,
    pub progress: Option<Progress<ReadPosition>>,
    pub error: Option<String>,
}
/// One frozen directory snapshot and progress through it. Retries do not re-enumerate.
#[derive(Clone)]
pub struct Listing {
    pub filesystem_config_fingerprint: [u8; 32],
    pub id: EffectId,
    pub path: String,
    pub recursive: bool,
    pub progress: Option<Progress<ListingPosition>>,
    pub content: Option<Vec<DirectoryEntry>>,
    pub error: Option<String>,
}
/// An unpublished output. close_effect reserves the only effect allowed to publish it.
#[derive(Clone)]
pub struct Writer {
    pub filesystem_config_fingerprint: [u8; 32],
    pub id: EffectId,
    pub path: String,
    pub close_effect: Option<EffectId>,
    pub state: String,
}
/// An acknowledged piece of a writer. Sequence and prior lengths fix its disk offset;
/// content fingerprints verify retry bytes without storing them in the database.
#[derive(Clone)]
pub struct Chunk {
    pub writer: EffectId,
    pub id: EffectId,
    pub sequence: u32,
    pub content: Content,
}

pub enum PendingResource {
    Reader {
        id: EffectId,
        path: String,
    },
    Listing {
        id: EffectId,
        path: String,
        recursive: bool,
    },
}
#[derive(Default)]
pub struct ResourceCounts {
    pub readers: u64,
    pub listings: u64,
    pub writers: u64,
    pub chunks: u64,
}

impl JournalEntry {
    /// Derived index value, never an independent source of execution state.
    /// Mutation enums share Prepared/Completed tags; each operation decodes its
    /// own prepared data. Unknown or malformed states must never become Initial.
    pub fn status(&self) -> Result<&'static str, StoreError> {
        let Some(bytes) = &self.state else {
            return Ok("preparing");
        };
        if bytes.len() > crate::codec::MAX_RECORD {
            return Err(StoreError::Conflict);
        }
        let state: StoredMutationState =
            serde_json::from_slice(bytes).map_err(|_| StoreError::Conflict)?;
        Ok(match state {
            StoredMutationState::Prepared(_) => "pending",
            StoredMutationState::Completed(Ok(_)) => "done",
            StoredMutationState::Completed(Err(_)) => "rejected",
        })
    }

    pub fn validate_successor(&self, prior: Option<&Self>) -> Result<(), StoreError> {
        if self.run_id.is_empty()
            || self.id.is_empty()
            || self.operation.is_empty()
            || self.version == 0
            || self.request.len() > crate::codec::MAX_RECORD
        {
            return Err(StoreError::Conflict);
        }
        let status = self.status()?;
        let Some(prior) = prior else {
            return if status == "preparing" {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        };
        if self.run_id != prior.run_id
            || self.id != prior.id
            || self.operation != prior.operation
            || self.version != prior.version
            || self.request != prior.request
        {
            return Err(StoreError::Conflict);
        }
        // Re-saving identical state is safe after a lost acknowledgement.
        // Prepared facts and completed results cannot be rewritten in place.
        if self.state == prior.state {
            return Ok(());
        }
        match (prior.status()?, status) {
            ("preparing", "pending" | "rejected") | ("pending", "done") => Ok(()),
            _ => Err(StoreError::Conflict),
        }
    }
}

/// Read only the common state shape for indexing and transition validation.
/// Native operations deserialize the same bytes into their specific enum.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum StoredMutationState {
    Prepared(serde::de::IgnoredAny),
    Completed(Result<Outcome, String>),
}

fn unchanged<T: PartialEq>(prior: &Option<T>, next: &Option<T>) -> bool {
    prior
        .as_ref()
        .is_none_or(|value| next.as_ref() == Some(value))
}
fn require(valid: bool) -> Result<(), StoreError> {
    if valid {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}
impl Reader {
    pub fn validate_successor(&self, prior: Option<&Self>) -> Result<(), StoreError> {
        require(self.position.is_none() || self.error.is_none())?;
        require(self.progress.is_none() || self.position.is_some())?;
        if let Some(progress) = &self.progress {
            let consumed = progress.after.offset.checked_sub(progress.before.offset);
            let maximum = match self.options {
                ReaderOptions::Bytes { chunk_size } => chunk_size as u64,
                ReaderOptions::Lines { maximum_bytes } => maximum_bytes,
            };
            require(
                progress.after.offset <= i64::MAX as u64
                    && consumed.is_some_and(|bytes| bytes <= maximum)
                    && (!progress.before.bom_handled || progress.after.bom_handled),
            )?;
            if progress.close {
                require(
                    progress.after
                        == ReadPosition {
                            closed: true,
                            ..progress.before.clone()
                        },
                )?;
            } else {
                require(
                    progress.after.closed == progress.before.closed
                        && (!progress.before.closed || progress.after == progress.before),
                )?;
            }
        }

        require(prior.map_or(
            self.position.is_none() && self.error.is_none() && self.progress.is_none(),
            |prior| {
                self.filesystem_config_fingerprint == prior.filesystem_config_fingerprint
                    && self.id == prior.id
                    && self.path == prior.path
                    && self.options == prior.options
                    && progress_follows(&self.progress, &prior.progress)
                    && unchanged(&prior.position, &self.position)
                    && unchanged(&prior.error, &self.error)
            },
        ))
    }
}
impl Listing {
    pub fn validate_successor(&self, prior: Option<&Self>) -> Result<(), StoreError> {
        require(self.content.is_none() || self.error.is_none())?;
        require(self.progress.is_none() || self.content.is_some())?;
        if let Some(progress) = &self.progress {
            let count = self.content.as_ref().map_or(0, Vec::len);
            require(progress.before.index <= count && progress.after.index <= count)?;
            if progress.close {
                require(
                    progress.after
                        == ListingPosition {
                            index: progress.before.index,
                            closed: true,
                        },
                )?;
            } else {
                let advance = usize::from(!progress.before.closed && progress.before.index < count);
                require(
                    progress.after.index == progress.before.index + advance
                        && progress.after.closed == progress.before.closed,
                )?;
            }
        }

        require(prior.map_or(
            self.content.is_none() && self.error.is_none() && self.progress.is_none(),
            |prior| {
                self.filesystem_config_fingerprint == prior.filesystem_config_fingerprint
                    && self.id == prior.id
                    && self.path == prior.path
                    && self.recursive == prior.recursive
                    && progress_follows(&self.progress, &prior.progress)
                    && unchanged(&prior.content, &self.content)
                    && unchanged(&prior.error, &self.error)
            },
        ))
    }
}
impl Writer {
    pub fn validate_successor(&self, prior: Option<&Self>) -> Result<(), StoreError> {
        require(
            matches!(self.state.as_str(), "open" | "closing" | "closed")
                && (self.state == "open") == self.close_effect.is_none(),
        )?;
        let Some(prior) = prior else {
            return require(self.state == "open");
        };
        require(
            self.filesystem_config_fingerprint == prior.filesystem_config_fingerprint
                && self.id == prior.id
                && self.path == prior.path
                && unchanged(&prior.close_effect, &self.close_effect)
                && (self.state == prior.state
                    || matches!(
                        (prior.state.as_str(), self.state.as_str()),
                        ("open", "closing") | ("closing", "closed")
                    )),
        )
    }
}
impl Chunk {
    pub fn validate_successor(&self, prior: Option<&Self>) -> Result<(), StoreError> {
        require(prior.is_none_or(|prior| {
            self.writer == prior.writer
                && self.id == prior.id
                && self.sequence == prior.sequence
                && self.content == prior.content
        }))
    }
}

pub const MAX_OPERATIONS: u64 = 10_000;
