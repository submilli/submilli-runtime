//! A request identity prevents an uncertain execution from being repeated.
pub use super::run_status::RunStatus;
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedOutcome {
    pub status: u16,
    pub body: String,
}

// Compatibility names for existing HTTP callers; the run is the only aggregate.
pub use super::run::{RequestError, Run as IdempotentRequest};

/// A completed response, without its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedResponse {
    pub run_id: String,
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
                .then_with(|| b.run_id.cmp(&a.run_id))
        });
        newest_first
            .into_iter()
            .skip(self.keep_last)
            .filter(|response| self.outlived(response.completed_at, now))
            .map(|response| response.run_id.as_str())
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
    use crate::domain::run_status::IndeterminateReason;
    use std::time::UNIX_EPOCH;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    fn at(hours: u64) -> SystemTime {
        UNIX_EPOCH + HOUR * hours as u32
    }

    fn response(key: &str, hours: u64) -> CompletedResponse {
        CompletedResponse {
            run_id: key.into(),
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
                Some("k".into()),
                "f".into(),
                "r".into(),
                "g".into(),
                at(0),
                state,
                completed_at,
                super::super::run::RunIdentity {
                    admission_order: 1,
                    ..Default::default()
                },
                0,
                0,
                0,
                false,
            )
        };
        assert!(restore(RunStatus::Expired, Some(at(1))).is_ok());
        assert!(restore(RunStatus::Queued, None).is_ok());
        assert!(matches!(
            restore(RunStatus::Expired, None),
            Err(RequestError::CompletionTimeMismatch)
        ));
        assert!(matches!(
            restore(
                RunStatus::Indeterminate(IndeterminateReason::Interrupted),
                Some(at(1))
            ),
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
