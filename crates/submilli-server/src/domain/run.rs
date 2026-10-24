//! Persistent program identity and guarded lifecycle transitions. No I/O.
use super::run_status::{RunStatus, StuckReason};
use super::session::SessionId;
use interpreter::runtime::durable::RunId;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    session_id: SessionId,
    key: Option<String>,
    fingerprint: String,
    reservation_id: RunId,
    owner_generation: String,
    created_at: SystemTime,
    /// Set exactly when the request is completed or expired.
    completed_at: Option<SystemTime>,
    state: RunStatus,
    identity: RunIdentity,
    revision: i64,
    attempt: i64,
    history_position: u64,
    durable: bool,
    reason: Option<String>,
}

/// Immutable admission metadata. Imported requests may have no execution ID:
/// their reservation identity is retained without inventing an execution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunIdentity {
    pub execution_id: Option<RunId>,
    pub principal: Option<String>,
    pub entry: String,
    pub admission_order: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("Idempotency-Key must contain between 1 and 120 bytes")]
    InvalidKey,
    #[error("this idempotency key was already used with different code")]
    Conflict,
    #[error("the request is no longer owned by this reservation")]
    ReservationMismatch,
    #[error("the completion time does not match the request's state")]
    CompletionTimeMismatch,
    #[error(transparent)]
    Session(#[from] super::session::SessionRuleError),
    #[error("run counter exhausted")]
    CounterExhausted,
    #[error("run is not in the required lifecycle state")]
    InvalidTransition,
}

impl Run {
    pub fn reserve(
        session_id: String,
        key: String,
        fingerprint: String,
        reservation_id: RunId,
        owner_generation: String,
        created_at: SystemTime,
    ) -> Result<Self, RequestError> {
        Self::prepare(
            session_id,
            Some(key),
            fingerprint,
            reservation_id,
            owner_generation,
            RunIdentity::default(),
            created_at,
        )
    }

    /// Rebuild a stored run, validating its lifecycle and admission counters together.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        session_id: String,
        key: Option<String>,
        fingerprint: String,
        reservation_id: RunId,
        owner_generation: String,
        created_at: SystemTime,
        state: RunStatus,
        completed_at: Option<SystemTime>,
        identity: RunIdentity,
        revision: i64,
        attempt: i64,
        history_position: u64,
        durable: bool,
        reason: Option<String>,
    ) -> Result<Self, RequestError> {
        if let Some(key) = &key {
            Self::validate_key(key)?;
        }
        if identity.admission_order <= 0 || revision < 0 || attempt < 0 {
            return Err(RequestError::InvalidTransition);
        }
        if durable && state == RunStatus::Indeterminate {
            return Err(RequestError::InvalidTransition);
        }
        let has_completed = matches!(state, RunStatus::Completed | RunStatus::Expired);
        if has_completed != completed_at.is_some() {
            return Err(RequestError::CompletionTimeMismatch);
        }
        Ok(Self {
            session_id: SessionId::parse(session_id)?,
            key,
            fingerprint,
            reservation_id,
            owner_generation,
            created_at,
            completed_at,
            state,
            identity,
            revision,
            attempt,
            history_position,
            durable,
            reason,
        })
    }

    pub fn validate_key(key: &str) -> Result<(), RequestError> {
        if key.is_empty() || key.len() > 120 {
            return Err(RequestError::InvalidKey);
        }
        Ok(())
    }

    pub fn verify_fingerprint(&self, fingerprint: &str) -> Result<(), RequestError> {
        if self.fingerprint != fingerprint {
            return Err(RequestError::Conflict);
        }
        Ok(())
    }

    pub fn complete(
        &mut self,
        reservation_id: &str,
        completed_at: SystemTime,
    ) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        self.advance_revision()?;
        self.state = RunStatus::Completed;
        self.reason = None;
        self.completed_at = Some(completed_at);
        Ok(())
    }

    pub fn mark_indeterminate(&mut self, reservation_id: &str) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        // Recorded runs can retry missing outcomes through their effect history.
        if self.durable {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Indeterminate;
        self.reason = Some("interrupted_without_history".into());
        Ok(())
    }

    pub fn require_reservation(&self, reservation_id: &str) -> Result<(), RequestError> {
        if self.reservation_id.as_str() != reservation_id
            || !matches!(self.state, RunStatus::Queued | RunStatus::Running)
        {
            return Err(RequestError::ReservationMismatch);
        }
        Ok(())
    }

    /// Construct preparation for a new program invocation. A missing key never
    /// deduplicates requests, but the generated identity survives commit retries.
    pub fn prepare(
        session_id: String,
        key: Option<String>,
        fingerprint: String,
        run_id: RunId,
        startup: String,
        identity: RunIdentity,
        created_at: SystemTime,
    ) -> Result<Self, RequestError> {
        if let Some(key) = &key {
            Self::validate_key(key)?;
        }
        Ok(Self {
            session_id: SessionId::parse(session_id)?,
            key,
            fingerprint,
            reservation_id: run_id,
            owner_generation: startup,
            created_at,
            completed_at: None,
            state: RunStatus::Queued,
            identity,
            revision: 0,
            attempt: 0,
            history_position: 0,
            durable: false,
            reason: None,
        })
    }

    /// Admission commits the retained program together with this transition.
    pub fn enable_durability(&mut self) -> Result<(), RequestError> {
        if self.state != RunStatus::Queued
            || self.revision != 0
            || self.attempt != 0
            || self.durable
        {
            return Err(RequestError::InvalidTransition);
        }
        self.durable = true;
        Ok(())
    }

    pub fn is_durable(&self) -> bool {
        self.durable
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn mark_stuck(&mut self, reason: StuckReason) -> Result<(), RequestError> {
        if !self.durable || !matches!(self.state, RunStatus::Queued | RunStatus::Running) {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Stuck;
        self.reason = Some(reason.as_str().into());
        Ok(())
    }

    pub fn resume(&mut self, revision: i64) -> Result<(), RequestError> {
        if !self.durable || self.state != RunStatus::Stuck || self.revision != revision {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Queued;
        self.reason = None;
        Ok(())
    }

    /// Called only after the old attempt's native work has drained. SQLite's
    /// exclusive local-store owner establishes that across a process restart.
    pub fn reopen(&mut self, startup: &str) -> Result<(), RequestError> {
        if !self.durable
            || !matches!(
                self.state,
                RunStatus::Queued | RunStatus::Running | RunStatus::Stuck
            )
        {
            return Err(RequestError::InvalidTransition);
        }
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        self.advance_revision()?;
        self.attempt = attempt;
        self.owner_generation = startup.to_owned();
        self.state = RunStatus::Running;
        self.reason = None;
        Ok(())
    }

    pub fn assign_admission(&mut self, order: i64) -> Result<(), RequestError> {
        if self.revision != 0 || order <= 0 {
            return Err(RequestError::InvalidTransition);
        }
        self.identity.admission_order = order;
        Ok(())
    }

    /// Compatibility execution is claimed only by the startup that prepared it.
    /// Restart replay is deliberately unavailable until manifest/history exist.
    pub fn claim(&mut self, startup: &str) -> Result<(), RequestError> {
        if self.state != RunStatus::Queued || self.owner_generation != startup {
            return Err(RequestError::InvalidTransition);
        }
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        self.advance_revision()?;
        self.attempt = attempt;
        self.state = RunStatus::Running;
        self.reason = None;
        Ok(())
    }

    pub fn abort_preparation(&mut self) -> Result<(), RequestError> {
        if self.state != RunStatus::Queued {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.key = None;
        self.state = RunStatus::Aborted;
        self.reason = Some("never_dispatched".into());
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), RequestError> {
        if !matches!(
            self.state,
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
        self.state = RunStatus::Cancelled;
        self.reason = Some("session_closed".into());
        Ok(())
    }

    pub fn finish_unkeyed(&mut self, at: SystemTime) -> Result<(), RequestError> {
        if self.key.is_some() || self.state != RunStatus::Running {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Completed;
        self.completed_at = Some(at);
        Ok(())
    }

    fn advance_revision(&mut self) -> Result<(), RequestError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        Ok(())
    }

    pub fn optional_key(&self) -> Option<&str> {
        self.key.as_deref()
    }
    pub fn identity(&self) -> &RunIdentity {
        &self.identity
    }
    pub fn revision(&self) -> i64 {
        self.revision
    }
    pub fn history_position(&self) -> u64 {
        self.history_position
    }
    pub fn attempt(&self) -> i64 {
        self.attempt
    }

    pub fn session_id(&self) -> &str {
        self.session_id.as_str()
    }
    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or("")
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn reservation_id(&self) -> &RunId {
        &self.reservation_id
    }
    pub fn owner_generation(&self) -> &str {
        &self.owner_generation
    }
    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }
    /// When the request completed; `None` until it does.
    pub fn completed_at(&self) -> Option<SystemTime> {
        self.completed_at
    }
    pub fn status(&self) -> RunStatus {
        self.state
    }
}
