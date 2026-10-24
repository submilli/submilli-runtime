//! Persistent program identity and guarded lifecycle transitions. No I/O.
use super::run_result::{RunOutput, RunResult};
use super::run_status::{IndeterminateReason, RunStatus, StuckReason};
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
    ) -> Result<Self, RequestError> {
        if let Some(key) = &key {
            Self::validate_key(key)?;
        }
        if identity.admission_order <= 0 || revision < 0 || attempt < 0 {
            return Err(RequestError::InvalidTransition);
        }
        if durable && matches!(state, RunStatus::Indeterminate(_)) {
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
        output: RunOutput,
        completed_at: SystemTime,
    ) -> Result<RunResult, RequestError> {
        self.require_reservation(reservation_id)?;
        self.advance_revision()?;
        self.state = RunStatus::Completed;
        self.completed_at = Some(completed_at);
        Ok(RunResult::new(self.reservation_id.clone(), output))
    }

    pub fn mark_indeterminate(&mut self, reservation_id: &str) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        // Recorded runs can retry missing outcomes through their effect history.
        if self.durable {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Indeterminate(IndeterminateReason::Interrupted);
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

    pub fn mark_stuck(&mut self, reason: StuckReason) -> Result<(), RequestError> {
        if !self.durable || !matches!(self.state, RunStatus::Queued | RunStatus::Running) {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Stuck(reason);
        Ok(())
    }

    pub fn resume(&mut self, revision: i64) -> Result<(), RequestError> {
        if !self.durable || !matches!(self.state, RunStatus::Stuck(_)) || self.revision != revision
        {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.state = RunStatus::Queued;
        Ok(())
    }

    /// Called only after the old attempt's native work has drained. SQLite's
    /// exclusive local-store owner establishes that across a process restart.
    pub fn reopen(&mut self, startup: &str) -> Result<(), RequestError> {
        if !self.durable
            || !matches!(
                self.state,
                RunStatus::Queued | RunStatus::Running | RunStatus::Stuck(_)
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
        Ok(())
    }

    pub fn abort_preparation(&mut self) -> Result<(), RequestError> {
        if self.state != RunStatus::Queued {
            return Err(RequestError::InvalidTransition);
        }
        self.advance_revision()?;
        self.key = None;
        self.state = RunStatus::Aborted;
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), RequestError> {
        if !matches!(
            self.state,
            RunStatus::Queued | RunStatus::Running | RunStatus::Stuck(_)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_produces_a_result_only_for_the_runs_current_reservation() {
        let now = SystemTime::UNIX_EPOCH;
        let mut run = Run::reserve(
            "session".into(),
            "key".into(),
            "fingerprint".into(),
            "run".into(),
            "owner".into(),
            now,
        )
        .unwrap();
        run.claim("owner").unwrap();
        let output = RunOutput {
            result: Some("value".into()),
            console: vec!["before completion".into()],
            ..Default::default()
        };
        let before = run.clone();
        assert!(run.complete("another-run", output.clone(), now).is_err());
        assert_eq!(run, before);

        let result = run.complete("run", output.clone(), now).unwrap();
        assert_eq!(result.run_id(), run.reservation_id());
        assert_eq!(result.output(), &output);
        assert_eq!(run.status(), RunStatus::Completed);
        assert_eq!(run.completed_at(), Some(now));
        assert_eq!(run.revision(), before.revision() + 1);

        let completed = run.clone();
        assert!(run.complete("run", output, now).is_err());
        assert_eq!(run, completed);
    }

    #[test]
    fn resuming_and_cancelling_replace_the_entire_status() {
        let mut run = Run::prepare(
            "session".into(),
            None,
            "fingerprint".into(),
            "run".into(),
            "owner".into(),
            RunIdentity::default(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        run.enable_durability().unwrap();
        run.mark_stuck(StuckReason::Fuel).unwrap();
        assert_eq!(run.status(), RunStatus::Stuck(StuckReason::Fuel));
        assert_eq!(run.status().reason(), Some("fuel_exhausted"));
        run.resume(run.revision()).unwrap();
        assert_eq!(run.status(), RunStatus::Queued);
        assert_eq!(run.status().reason(), None);
        run.claim("owner").unwrap();
        run.mark_stuck(StuckReason::Timeout).unwrap();
        assert_eq!(run.status(), RunStatus::Stuck(StuckReason::Timeout));
        run.cancel().unwrap();
        assert_eq!(run.status(), RunStatus::Cancelled);
        assert_eq!(run.status().reason(), Some("session_closed"));
    }
}
