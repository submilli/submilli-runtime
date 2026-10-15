//! Persistent program identity and guarded lifecycle transitions. No I/O.
use super::idempotent_request::{RecordedOutcome, RequestState};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    session_id: String,
    key: Option<String>,
    fingerprint: String,
    reservation_id: String,
    owner_generation: String,
    created_at: SystemTime,
    /// Set exactly when the request is completed or expired.
    completed_at: Option<SystemTime>,
    state: RequestState,
    identity: RunIdentity,
    revision: i64,
    attempt: i64,
}

/// Immutable admission metadata. Imported requests may have no execution ID:
/// their reservation identity is retained without inventing an execution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunIdentity {
    pub execution_id: Option<String>,
    pub principal: Option<String>,
    pub entry: String,
    pub origin_node: String,
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
        reservation_id: String,
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
        reservation_id: String,
        owner_generation: String,
        created_at: SystemTime,
        state: RequestState,
        completed_at: Option<SystemTime>,
        identity: RunIdentity,
        revision: i64,
        attempt: i64,
    ) -> Result<Self, RequestError> {
        if let Some(key) = &key {
            Self::validate_key(key)?;
        }
        if identity.origin_node.is_empty()
            || identity.admission_order <= 0
            || revision < 0
            || attempt < 0
        {
            return Err(RequestError::InvalidTransition);
        }
        let has_completed = matches!(
            state,
            RequestState::Completed(_) | RequestState::Expired | RequestState::Finished
        );
        if has_completed != completed_at.is_some() {
            return Err(RequestError::CompletionTimeMismatch);
        }
        Ok(Self {
            session_id,
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
        outcome: RecordedOutcome,
        completed_at: SystemTime,
    ) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        self.advance_revision()?;
        self.state = RequestState::Completed(outcome);
        self.completed_at = Some(completed_at);
        Ok(())
    }

    pub fn mark_indeterminate(&mut self, reservation_id: &str) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        self.advance_revision()?;
        self.state = RequestState::Indeterminate;
        Ok(())
    }

    pub fn require_reservation(&self, reservation_id: &str) -> Result<(), RequestError> {
        if self.reservation_id != reservation_id
            || !matches!(self.state, RequestState::Reserved | RequestState::Running)
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
        run_id: String,
        startup: String,
        identity: RunIdentity,
        created_at: SystemTime,
    ) -> Result<Self, RequestError> {
        if let Some(key) = &key {
            Self::validate_key(key)?;
        }
        Ok(Self {
            session_id,
            key,
            fingerprint,
            reservation_id: run_id,
            owner_generation: startup,
            created_at,
            completed_at: None,
            state: RequestState::Reserved,
            identity,
            revision: 0,
            attempt: 0,
        })
    }

    pub fn assign_admission(&mut self, node: String, order: i64) -> Result<(), RequestError> {
        if self.revision != 0 || order <= 0 || node.is_empty() {
            return Err(RequestError::InvalidTransition);
        }
        self.identity.origin_node = node;
        self.identity.admission_order = order;
        Ok(())
    }

    /// Compatibility execution is claimed only by the startup that prepared it.
    /// Restart replay is deliberately unavailable until manifest/history exist.
    pub fn claim(&mut self, startup: &str) -> Result<(), RequestError> {
        if self.state != RequestState::Reserved || self.owner_generation != startup {
            return Err(RequestError::InvalidTransition);
        }
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        self.advance_revision()?;
        self.attempt = attempt;
        self.state = RequestState::Running;
        Ok(())
    }

    pub fn abort_preparation(&mut self) -> Result<(), RequestError> {
        if self.state != RequestState::Reserved {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.key = None;
        self.state = RequestState::Aborted;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), RequestError> {
        if !matches!(
            self.state,
            RequestState::Reserved | RequestState::Running | RequestState::Blocked
        ) {
            return Err(RequestError::InvalidTransition);
        }
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or(RequestError::CounterExhausted)?;
        self.advance_revision()?;
        self.attempt = attempt;
        self.state = RequestState::Cancelled;
        Ok(())
    }

    pub fn block(&mut self) -> Result<(), RequestError> {
        if self.state != RequestState::Running {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RequestState::Blocked;
        Ok(())
    }

    pub fn finish_unkeyed(&mut self, at: SystemTime) -> Result<(), RequestError> {
        if self.key.is_some() || self.state != RequestState::Running {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RequestState::Finished;
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
    pub fn attempt(&self) -> i64 {
        self.attempt
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or("")
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn reservation_id(&self) -> &str {
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
    pub fn state(&self) -> &RequestState {
        &self.state
    }
}
