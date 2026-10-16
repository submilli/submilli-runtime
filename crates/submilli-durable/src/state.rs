//! Durable workspace and resource state. The journal owns their transitions.
use crate::recovery::{
    Content, EphemeralReservation, Intent, Mutation, Outcome, ReaderPosition, WorkspaceIdentity,
    WorkspaceLayout,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RecordedOutcome {
    Completed(Outcome),
    Rejected(String),
}

#[derive(Clone)]
pub struct WorkspaceState {
    pub id: String,
    pub run: String,
    pub session: String,
    pub owner: String,
    pub identity: Option<WorkspaceIdentity>,
    pub layout: Option<WorkspaceLayout>,
    pub reservation: Option<EphemeralReservation>,
    pub state: String,
}

pub struct RunState {
    pub session: String,
    pub live: bool,
    pub terminal: bool,
}

#[derive(Clone)]
pub struct Effect {
    pub id: String,
    pub run: String,
    pub operation: String,
    pub version: u32,
    pub request: Vec<u8>,
    pub recovery_payload: Option<Vec<u8>>,
    pub outcome: Option<Vec<u8>>,
    pub rejected: bool,
}

pub struct EffectRecord {
    pub request: Mutation,
    pub plan: Option<String>,
    pub intent: Option<Intent>,
    pub outcome: Option<RecordedOutcome>,
}

#[derive(Clone)]
pub struct Plan {
    pub id: String,
    pub steps: Vec<Mutation>,
    pub outcome: Option<RecordedOutcome>,
}
#[derive(Clone)]
pub struct Reader {
    pub id: String,
    pub path: String,
    pub position: Option<ReaderPosition>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Listing {
    pub id: String,
    pub path: String,
    pub recursive: bool,
    pub content: Option<Content>,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct Writer {
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
#[derive(Clone)]
pub struct Producer {
    pub id: String,
    pub request: [u8; 32],
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
    pub plans: u64,
    pub chunks: u64,
}

impl Effect {
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
        if self.run.is_empty()
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
        if self.run != prior.run
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
impl WorkspaceState {
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(
            matches!(
                self.state.as_str(),
                "preparing" | "active" | "retiring" | "gone"
            ) && (self.state != "active" || self.identity.is_some()),
        )?;
        let Some(prior) = prior else {
            return require(self.state == "preparing" && self.identity.is_none());
        };
        require(
            self.id == prior.id
                && self.run == prior.run
                && self.session == prior.session
                && self.owner == prior.owner
                && unchanged(&prior.identity, &self.identity)
                && unchanged(&prior.layout, &self.layout)
                && unchanged(&prior.reservation, &self.reservation)
                && (self.state == prior.state
                    || matches!(
                        (prior.state.as_str(), self.state.as_str()),
                        ("preparing", "active" | "retiring")
                            | ("active", "retiring")
                            | ("retiring", "gone")
                    )),
        )
    }
}
impl Reader {
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(self.position.is_none() || self.error.is_none())?;
        require(
            prior.map_or(self.position.is_none() && self.error.is_none(), |prior| {
                self.id == prior.id
                    && self.path == prior.path
                    && unchanged(&prior.position, &self.position)
                    && unchanged(&prior.error, &self.error)
            }),
        )
    }
}
impl Listing {
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(self.content.is_none() || self.error.is_none())?;
        require(
            prior.map_or(self.content.is_none() && self.error.is_none(), |prior| {
                self.id == prior.id
                    && self.path == prior.path
                    && self.recursive == prior.recursive
                    && unchanged(&prior.content, &self.content)
                    && unchanged(&prior.error, &self.error)
            }),
        )
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
            self.id == prior.id
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
impl Plan {
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(prior.map_or(self.outcome.is_none(), |prior| {
            self.id == prior.id
                && self.steps == prior.steps
                && unchanged(&prior.outcome, &self.outcome)
        }))
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
impl Producer {
    pub fn validate_successor(
        &self,
        prior: Option<&Self>,
    ) -> Result<(), crate::unit_of_work::StoreError> {
        require(prior.is_none_or(|prior| self.id == prior.id && self.request == prior.request))
    }
}
