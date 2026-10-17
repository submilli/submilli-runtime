//! Idempotent filesystem operation and resource records.
use crate::recovery::{Content, Intent, Mutation, Outcome, ReaderPosition};
use interpreter::runtime::filesystem::{ReadPosition, ReaderOptions};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RecordedOutcome {
    Completed(Outcome),
    Rejected(String),
}

#[derive(Clone)]
pub struct Operation {
    pub id: String,
    pub scope: String,
    pub operation: String,
    pub version: u32,
    pub request: Vec<u8>,
    pub recovery_payload: Option<Vec<u8>>,
    pub outcome: Option<Vec<u8>>,
    pub rejected: bool,
}

pub struct OperationRecord {
    pub request: Mutation,
    pub intent: Option<Intent>,
    pub outcome: Option<RecordedOutcome>,
}

/// The last dispatched effect only. Earlier completed effects are replayed by
/// execution history, never dispatched back to this resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress<T> {
    pub effect: String,
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

#[derive(Clone)]
pub struct Reader {
    pub binding: [u8; 32],
    pub id: String,
    pub path: String,
    pub options: ReaderOptions,
    pub position: Option<ReaderPosition>,
    pub progress: Option<Progress<ReadPosition>>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Listing {
    pub binding: [u8; 32],
    pub id: String,
    pub path: String,
    pub recursive: bool,
    pub progress: Option<Progress<ListingPosition>>,
    pub content: Option<Vec<crate::recovery::DirectoryEntry>>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Writer {
    pub binding: [u8; 32],
    pub id: String,
    pub path: String,
    pub close_effect: Option<String>,
    pub state: String,
}
#[derive(Clone)]
pub struct Chunk {
    pub writer: String,
    pub id: String,
    pub sequence: u32,
    pub content: Content,
}

pub struct WriterRecord {
    pub path: String,
    pub chunks: Vec<Content>,
}
pub enum PendingResource {
    Reader {
        id: String,
        path: String,
    },
    Listing {
        id: String,
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

impl Operation {
    pub fn state(&self) -> &'static str {
        if self.outcome.is_some() {
            if self.rejected { "rejected" } else { "done" }
        } else if self.recovery_payload.is_some() {
            "pending"
        } else {
            "preparing"
        }
    }
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        use crate::unit_of_work::StoreError;
        if self.scope.is_empty()
            || self.id.is_empty()
            || self.operation.is_empty()
            || self.version == 0
            || self.request.len() > crate::codec::MAX_RECORD
            || self
                .recovery_payload
                .as_ref()
                .is_some_and(|v| v.len() > crate::codec::MAX_RECORD)
            || self
                .outcome
                .as_ref()
                .is_some_and(|v| v.len() > crate::codec::MAX_RECORD)
            || self.rejected && (self.outcome.is_none() || self.recovery_payload.is_some())
            || self.outcome.is_some() && !self.rejected && self.recovery_payload.is_none()
        {
            return Err(StoreError::Conflict);
        }
        let Some(prior) = prior else {
            return if self.state() == "preparing" {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        };
        if self.scope != prior.scope
            || self.id != prior.id
            || self.operation != prior.operation
            || self.version != prior.version
            || self.request != prior.request
            || prior
                .recovery_payload
                .as_ref()
                .is_some_and(|v| self.recovery_payload.as_ref() != Some(v))
            || prior.outcome.as_ref().is_some_and(|v| {
                self.outcome.as_ref() != Some(v) || self.rejected != prior.rejected
            })
            || prior.state() == "preparing" && self.state() == "done"
        {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
}

fn unchanged<T: PartialEq>(prior: &Option<T>, next: &Option<T>) -> bool {
    prior
        .as_ref()
        .is_none_or(|value| next.as_ref() == Some(value))
}
fn require(valid: bool) -> Result<(), crate::unit_of_work::StoreError> {
    if valid {
        Ok(())
    } else {
        Err(crate::unit_of_work::StoreError::Conflict)
    }
}
impl Reader {
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
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
                self.binding == prior.binding
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
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
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
                self.binding == prior.binding
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
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(
            matches!(self.state.as_str(), "open" | "closing" | "closed")
                && (self.state == "open") == self.close_effect.is_none(),
        )?;
        let Some(prior) = prior else {
            return require(self.state == "open");
        };
        require(
            self.binding == prior.binding
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
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(prior.is_none_or(|prior| {
            self.writer == prior.writer
                && self.id == prior.id
                && self.sequence == prior.sequence
                && self.content == prior.content
        }))
    }
}
