//! Per-call accounting shared with blocking filesystem workers.
use crate::runtime::fuel::Rate;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, thiserror::Error)]
#[error("filesystem operation exhausted its fuel budget")]
pub struct Exhausted;

pub struct WorkBudget {
    limit: u64,
    spent: AtomicU64,
    after_effect: AtomicBool,
}
impl WorkBudget {
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            spent: AtomicU64::new(0),
            after_effect: AtomicBool::new(false),
        }
    }
    pub fn charge(&self, rate: Rate, count: u64) -> Result<(), Exhausted> {
        self.charge_fuel(rate.cost(count))
    }
    pub fn charge_fuel(&self, amount: u64) -> Result<(), Exhausted> {
        let settled = self.after_effect.load(Ordering::Acquire);
        let mut refused = false;
        let _ = self
            .spent
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |spent| {
                let total = spent.saturating_add(amount);
                refused = !settled && total > self.limit;
                Some(if refused { self.limit } else { total })
            });
        if refused { Err(Exhausted) } else { Ok(()) }
    }
    /// Once publication begins, count remaining work without refusing it.
    pub fn before_effect(&self) {
        self.after_effect.store(true, Ordering::Release);
    }
    pub fn spent(&self) -> u64 {
        self.spent.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::fuel;
    #[test]
    fn refuses_before_effect_and_settles_after_publication_starts() {
        let budget = WorkBudget::new(fuel::HASH.cost(4));
        budget.charge(fuel::HASH, 3).unwrap();
        assert!(budget.charge(fuel::HASH, 2).is_err());
        assert_eq!(budget.spent(), fuel::HASH.cost(4));
        budget.before_effect();
        budget.charge(fuel::HASH, 2).unwrap();
        assert_eq!(budget.spent(), fuel::HASH.cost(6));
    }
}
