//! Logical execution time restored at outcome boundaries, excluding paused tools.
use std::time::{Duration, Instant};

pub struct ExecutionClock {
    replay_started: Instant,
    resumed_at: Instant,
    elapsed: Duration,
    paused_before: Duration,
    replaying: bool,
}
impl ExecutionClock {
    pub fn start() -> Self {
        let now = Instant::now();
        Self {
            replay_started: now,
            resumed_at: now,
            elapsed: Duration::ZERO,
            paused_before: Duration::ZERO,
            replaying: true,
        }
    }
    /// Replay replaces reconstruction time with the original logical progress.
    pub fn restore(&mut self, elapsed: Duration, paused: Duration) {
        self.elapsed = elapsed;
        self.resumed_at = Instant::now();
        self.paused_before = paused;
    }
    pub fn begin_live(&mut self) {
        self.replaying = false;
    }
    pub fn elapsed(&self, paused: Duration) -> Duration {
        self.elapsed.saturating_add(
            self.resumed_at
                .elapsed()
                .saturating_sub(paused.saturating_sub(self.paused_before)),
        )
    }
    /// Replay has its own bounded reconstruction allowance. On reaching new work,
    /// the original remaining allowance applies, rather than a fresh timeout.
    pub fn expired(&self, budget: Duration, paused: Duration) -> bool {
        if self.replaying {
            self.replay_started.elapsed() >= budget
        } else {
            self.elapsed(paused) >= budget
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_restores_spent_time_without_counting_paused_waits() {
        let mut clock = ExecutionClock::start();
        clock.restore(Duration::from_secs(25), Duration::ZERO);
        clock.resumed_at -= Duration::from_secs(60);
        clock.begin_live();
        let paused = Duration::from_secs(60);
        assert!(!clock.expired(Duration::from_secs(30), paused));
        assert!(clock.expired(Duration::from_secs(20), paused));
        assert!(clock.elapsed(paused) < Duration::from_secs(26));
    }
    #[test]
    fn a_pure_tail_after_the_last_recorded_call_uses_remaining_time() {
        let mut clock = ExecutionClock::start();
        clock.restore(Duration::from_secs(29), Duration::ZERO);
        clock.begin_live();
        clock.resumed_at -= Duration::from_secs(2);
        assert!(clock.expired(Duration::from_secs(30), Duration::ZERO));
    }
    #[test]
    fn reconstruction_is_bounded_separately() {
        let mut clock = ExecutionClock::start();
        clock.restore(Duration::from_secs(29), Duration::ZERO);
        assert!(!clock.expired(Duration::from_secs(30), Duration::ZERO));
        clock.replay_started -= Duration::from_secs(31);
        assert!(clock.expired(Duration::from_secs(30), Duration::ZERO));
    }
}
