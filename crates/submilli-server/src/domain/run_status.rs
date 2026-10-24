//! Run lifecycle independent of the retained program: a damaged manifest must
//! not prevent observation, suspension, or cancellation.
use super::run::RequestError;
use super::session::SessionId;
use interpreter::runtime::durable::RunId;
use std::time::SystemTime;

/// Readable even when the retained program cannot be decrypted or decoded.
#[derive(Clone, Debug)]
pub(crate) struct RunSummary {
    pub id: RunId,
    pub session_id: SessionId,
    pub fingerprint: String,
    pub status: RunStatus,
    pub reason: Option<String>,
    pub recorded: bool,
    pub revision: i64,
    pub attempt: i64,
    pub owner: String,
    pub admission_order: i64,
    pub created_at: SystemTime,
    pub completed_at: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunStatus {
    Queued,
    Running,
    Stuck,
    Cancelled,
    Aborted,
    Completed,
    /// An interrupted unrecorded run whose outcome cannot be recovered.
    Indeterminate,
    Expired,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Stuck => "stuck",
            Self::Cancelled => "cancelled",
            Self::Aborted => "aborted",
            Self::Completed => "completed",
            Self::Indeterminate => "indeterminate",
            Self::Expired => "expired",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StuckReason {
    Credentials,
    Storage,
    Compatibility,
    History,
    Fuel,
    Timeout,
    Interrupted,
    Disabled,
}

impl StuckReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Credentials => "credentials_unavailable",
            Self::Storage => "storage_unavailable",
            Self::Compatibility => "incompatible_environment",
            Self::History => "invalid_history",
            Self::Fuel => "fuel_exhausted",
            Self::Timeout => "execution_timeout",
            Self::Interrupted => "execution_interrupted",
            Self::Disabled => "durability_disabled",
        }
    }
}

impl RunSummary {
    pub fn mark_stuck(&mut self, reason: StuckReason) -> Result<(), RequestError> {
        if !self.recorded || !matches!(self.status, RunStatus::Queued | RunStatus::Running) {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.status = RunStatus::Stuck;
        self.reason = Some(reason.as_str().into());
        Ok(())
    }

    pub fn resume(&mut self, revision: i64) -> Result<(), RequestError> {
        if !self.recorded || self.status != RunStatus::Stuck || self.revision != revision {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.status = RunStatus::Queued;
        self.reason = None;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), RequestError> {
        if !matches!(
            self.status,
            RunStatus::Queued | RunStatus::Running | RunStatus::Stuck
        ) {
            return Err(RequestError::InvalidTransition);
        }
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        self.advance_revision()?;
        self.attempt = attempt;
        self.status = RunStatus::Cancelled;
        self.reason = Some("session_closed".into());
        Ok(())
    }

    fn advance_revision(&mut self) -> Result<(), RequestError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        Ok(())
    }
}
