//! A request identity prevents an uncertain execution from being repeated.
use std::time::{Duration, SystemTime};

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
    /// Completed, but its response is no longer kept: the request must not run
    /// again, and there is nothing left to replay. Storage makes this transition
    /// in bulk (`UnitOfWork::expire_responses`), without loading the bodies it drops.
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotentRequest {
    session_id: String,
    key: String,
    fingerprint: String,
    reservation_id: String,
    owner_generation: String,
    created_at: SystemTime,
    /// Set exactly when the request is completed or expired.
    completed_at: Option<SystemTime>,
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
    #[error("the completion time does not match the request's state")]
    CompletionTimeMismatch,
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
            completed_at: None,
            state: RequestState::Reserved,
        })
    }

    /// Rebuild a request from storage. A request has a completion time exactly
    /// when it is completed or expired.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        session_id: String,
        key: String,
        fingerprint: String,
        reservation_id: String,
        owner_generation: String,
        created_at: SystemTime,
        state: RequestState,
        completed_at: Option<SystemTime>,
    ) -> Result<Self, RequestError> {
        Self::validate_key(&key)?;
        let has_completed = matches!(state, RequestState::Completed(_) | RequestState::Expired);
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
        self.state = RequestState::Completed(outcome);
        self.completed_at = Some(completed_at);
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
    /// When the request completed; `None` until it does.
    pub fn completed_at(&self) -> Option<SystemTime> {
        self.completed_at
    }
    pub fn state(&self) -> &RequestState {
        &self.state
    }
}

/// A completed response, without its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedResponse {
    pub key: String,
    pub completed_at: SystemTime,
}

/// How long a session keeps completed responses: every response for `ttl`
/// after it completed, and the newest `keep_last` whatever their age, so a
/// client retrying its latest calls after a long pause still gets their bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseRetention {
    pub ttl: Duration,
    pub keep_last: usize,
}

impl Default for ResponseRetention {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(24 * 60 * 60),
            keep_last: 3,
        }
    }
}

impl ResponseRetention {
    /// The keys of one session's completed responses that outlived both the
    /// TTL and the newest `keep_last`.
    pub fn expired<'a>(&self, completed: &'a [CompletedResponse], now: SystemTime) -> Vec<&'a str> {
        let mut newest_first: Vec<&CompletedResponse> = completed.iter().collect();
        newest_first.sort_by(|a, b| {
            b.completed_at
                .cmp(&a.completed_at)
                .then_with(|| b.key.cmp(&a.key))
        });
        newest_first
            .into_iter()
            .skip(self.keep_last)
            .filter(|response| self.outlived(response.completed_at, now))
            .map(|response| response.key.as_str())
            .collect()
    }

    /// The latest completion time a response may have and still be past the TTL
    /// at `now`. `None` when no response can be past the TTL yet: `now - ttl`
    /// falls before the Unix epoch, and every stored completion time is after it.
    pub fn cutoff(&self, now: SystemTime) -> Option<SystemTime> {
        now.checked_sub(self.ttl)
            .filter(|cutoff| *cutoff >= std::time::UNIX_EPOCH)
    }

    fn outlived(&self, completed_at: SystemTime, now: SystemTime) -> bool {
        self.cutoff(now)
            .is_some_and(|cutoff| completed_at <= cutoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    fn at(hours: u64) -> SystemTime {
        UNIX_EPOCH + HOUR * hours as u32
    }

    fn response(key: &str, hours: u64) -> CompletedResponse {
        CompletedResponse {
            key: key.into(),
            completed_at: at(hours),
        }
    }

    #[test]
    fn the_newest_responses_are_kept_whatever_their_age() {
        let retention = ResponseRetention::default();
        let completed = [
            response("a", 1),
            response("b", 2),
            response("c", 3),
            response("d", 4),
            response("e", 5),
        ];
        let mut expired = retention.expired(&completed, at(100));
        expired.sort_unstable();
        assert_eq!(expired, ["a", "b"]);
        assert!(retention.expired(&completed[..3], at(100)).is_empty());
    }

    #[test]
    fn a_response_older_than_the_newest_expires_only_past_the_ttl() {
        let retention = ResponseRetention {
            ttl: HOUR * 24,
            keep_last: 1,
        };
        let completed = [response("old", 10), response("new", 20)];
        assert!(retention.expired(&completed, at(33)).is_empty());
        assert_eq!(retention.expired(&completed, at(34)), ["old"]);
        assert!(retention.expired(&completed, at(1)).is_empty());
    }

    #[test]
    fn responses_completed_together_are_ordered_by_key() {
        let retention = ResponseRetention {
            ttl: HOUR,
            keep_last: 1,
        };
        let completed = [response("b", 1), response("a", 1)];
        assert_eq!(retention.expired(&completed, at(5)), ["a"]);
    }

    #[test]
    fn a_ttl_reaching_back_before_the_epoch_expires_nothing() {
        let retention = ResponseRetention {
            ttl: HOUR * 1000,
            keep_last: 0,
        };
        assert_eq!(retention.cutoff(at(999)), None);
        assert!(retention.expired(&[response("a", 1)], at(999)).is_empty());
        assert_eq!(retention.cutoff(at(1000)), Some(at(0)));
    }

    #[test]
    fn a_restored_request_has_a_completion_time_exactly_when_it_completed() {
        let restore = |state, completed_at| {
            IdempotentRequest::restore(
                "s".into(),
                "k".into(),
                "f".into(),
                "r".into(),
                "g".into(),
                at(0),
                state,
                completed_at,
            )
        };
        assert!(restore(RequestState::Expired, Some(at(1))).is_ok());
        assert!(restore(RequestState::Reserved, None).is_ok());
        assert!(matches!(
            restore(RequestState::Expired, None),
            Err(RequestError::CompletionTimeMismatch)
        ));
        assert!(matches!(
            restore(RequestState::Indeterminate, Some(at(1))),
            Err(RequestError::CompletionTimeMismatch)
        ));
    }

    #[test]
    fn keeping_none_expires_everything_past_the_ttl() {
        let retention = ResponseRetention {
            ttl: HOUR,
            keep_last: 0,
        };
        let completed = [response("a", 1), response("b", 9)];
        assert_eq!(retention.expired(&completed, at(9)), ["a"]);
    }
}
