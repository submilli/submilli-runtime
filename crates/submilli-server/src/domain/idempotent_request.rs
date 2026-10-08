//! A request identity prevents an uncertain execution from being repeated.
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedOutcome {
    pub status: u16,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestState {
    Reserved,
    Completed(RecordedOutcome),
    Indeterminate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotentRequest {
    session_id: String,
    key: String,
    fingerprint: String,
    reservation_id: String,
    owner_generation: String,
    created_at: SystemTime,
    state: RequestState,
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("Idempotency-Key must contain between 1 and 120 bytes")]
    InvalidKey,
    #[error("this idempotency key was already used with different code")]
    Conflict,
    #[error("the request is no longer owned by this reservation")]
    ReservationMismatch,
}

impl IdempotentRequest {
    pub fn reserve(
        session_id: String,
        key: String,
        fingerprint: String,
        reservation_id: String,
        owner_generation: String,
        created_at: SystemTime,
    ) -> Result<Self, RequestError> {
        Self::validate_key(&key)?;
        Ok(Self {
            session_id,
            key,
            fingerprint,
            reservation_id,
            owner_generation,
            created_at,
            state: RequestState::Reserved,
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
    ) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        self.state = RequestState::Completed(outcome);
        Ok(())
    }

    pub fn mark_indeterminate(&mut self, reservation_id: &str) -> Result<(), RequestError> {
        self.require_reservation(reservation_id)?;
        self.state = RequestState::Indeterminate;
        Ok(())
    }

    pub fn require_reservation(&self, reservation_id: &str) -> Result<(), RequestError> {
        if self.reservation_id != reservation_id || self.state != RequestState::Reserved {
            return Err(RequestError::ReservationMismatch);
        }
        Ok(())
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn key(&self) -> &str {
        &self.key
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
    pub fn state(&self) -> &RequestState {
        &self.state
    }
}
