//! The token accounting both `submilli:llm` and `submilli:embedding` budget
//! against: a per-execution reservation that also charges a server-wide
//! aggregate, with held reserve for spend a provider never reported.
//!
//! This module owns the arithmetic and the concurrency. It knows nothing about
//! either capability's error type, flag names, or message text: a refusal comes
//! back as a [`LedgerRefusal`] naming which ceiling was hit, and the capability
//! wrapper renders it in its own vocabulary.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// An aggregate token budget several executions reserve against.
#[derive(Debug, Clone)]
pub struct SharedTokenBudget {
    used: Arc<AtomicU64>,
    cap: u64,
}

impl SharedTokenBudget {
    pub fn new(cap: u64) -> Self {
        Self {
            used: Arc::new(AtomicU64::new(0)),
            cap,
        }
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    pub fn cap(&self) -> u64 {
        self.cap
    }

    /// Add `tokens`, or report the total that would have exceeded the cap.
    ///
    /// Compare-and-swap so two executions reserving at once cannot both observe
    /// the same headroom.
    fn try_add(&self, tokens: u64) -> Result<(), u64> {
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = current.saturating_add(tokens);
            if next > self.cap {
                return Err(next);
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }

    /// Charge `tokens` that were already spent, past the cap if need be.
    fn add_unchecked(&self, tokens: u64) {
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = current.saturating_add(tokens);
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    /// Return `tokens`, saturating at zero.
    fn sub(&self, tokens: u64) {
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = current.saturating_sub(tokens);
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

/// The two ceilings one execution's ledger enforces. The server-wide cap lives on
/// the [`SharedTokenBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerLimits {
    /// Tokens one execution may hold in total.
    pub per_execution_tokens: u64,
    /// Tokens one execution may hold as indeterminate reserve before further
    /// reservation is refused.
    pub max_held_tokens: u64,
}

/// Which ceiling refused a reservation, with the numbers the capability's message
/// needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerRefusal {
    /// Held reserve is already past [`LedgerLimits::max_held_tokens`].
    HeldReserve { held: u64, limit: u64 },
    /// This execution's own ceiling.
    PerExecution { requested: u64, limit: u64 },
    /// The shared aggregate's cap.
    AllExecutions { requested: u64, limit: u64 },
}

/// What one execution has reserved, and what of that is indeterminate.
#[derive(Debug, Default)]
pub(super) struct TokenState {
    /// Tokens reserved against both this execution and the aggregate.
    used: u64,
    /// The part of `used` held for elements the provider reported no usage for.
    /// Tracked separately because it is bounded separately, and because it is
    /// the part that must *not* be returned to the aggregate on teardown.
    held: u64,
}

/// One execution's reservations against its own ceiling and the shared
/// aggregate together.
///
/// Both reservations are taken together and both unwound if either fails, so a
/// refused reservation leaves neither counter charged.
///
/// Releasing on drop is what makes the aggregate usable for execution lifetime:
/// an execution ends by having its budget holder dropped, not by reconciling
/// every reservation first, so without this the reservation of every ended
/// execution would be held forever and the aggregate would ratchet to its cap.
///
/// **Held reserve is the deliberate exception.** It releases from the
/// per-execution ledger, which is over, but stays charged against the aggregate.
/// Releasing indeterminate spend from the aggregate would let it escape the
/// server-wide ceiling entirely: a caller who can reliably induce null usage pays
/// real money the aggregate forgets at execution end, so N executions each under
/// the per-execution ceiling could exceed the server-wide one without ever
/// tripping it.
#[derive(Debug)]
pub struct TokenLedger {
    limits: LedgerLimits,
    state: Mutex<TokenState>,
    aggregate: SharedTokenBudget,
}

impl TokenLedger {
    pub fn new(limits: LedgerLimits, aggregate: SharedTokenBudget) -> Self {
        Self {
            limits,
            state: Mutex::new(TokenState::default()),
            aggregate,
        }
    }

    /// Tokens this execution currently holds against its ceiling.
    pub fn used(&self) -> u64 {
        self.locked().used
    }

    /// The part of [`Self::used`] held for unreported usage.
    pub fn held(&self) -> u64 {
        self.locked().held
    }

    fn locked(&self) -> MutexGuard<'_, TokenState> {
        // A poisoned lock still yields the counters, and a ledger that refused
        // to account after an unrelated panic would leak the whole reservation.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(super) fn lock_state(&self) -> std::sync::LockResult<MutexGuard<'_, TokenState>> {
        self.state.lock()
    }

    #[cfg(test)]
    pub(super) fn state_is_poisoned(&self) -> bool {
        self.state.is_poisoned()
    }

    /// Reserve `tokens` against this execution and the aggregate.
    pub fn reserve(&self, tokens: u64) -> Result<(), LedgerRefusal> {
        let mut state = self.locked();

        if state.held > self.limits.max_held_tokens {
            return Err(LedgerRefusal::HeldReserve {
                held: state.held,
                limit: self.limits.max_held_tokens,
            });
        }

        let next_used = state.used.saturating_add(tokens);
        if next_used > self.limits.per_execution_tokens {
            return Err(LedgerRefusal::PerExecution {
                requested: next_used,
                limit: self.limits.per_execution_tokens,
            });
        }

        if let Err(requested) = self.aggregate.try_add(tokens) {
            return Err(LedgerRefusal::AllExecutions {
                requested,
                limit: self.aggregate.cap(),
            });
        }

        state.used = next_used;
        Ok(())
    }

    /// Reconcile a completed dispatch down from its `reserved` estimate to what
    /// the provider actually reported.
    ///
    /// `reported` is the summed usage of the elements the provider gave counts
    /// for; `indeterminate` is the reserve belonging to elements it reported
    /// nothing for. The indeterminate part is **not** released — `None` means
    /// indeterminate, not free, since a throttled call may still have been
    /// billed — and is instead carried as held reserve, bounded by
    /// [`LedgerLimits::max_held_tokens`].
    pub fn reconcile(&self, reserved: u64, reported: u64, indeterminate: u64) {
        let mut state = self.locked();
        let keep = reported.saturating_add(indeterminate);
        let release = reserved.saturating_sub(keep);
        state.used = state.used.saturating_sub(release);
        state.held = state.held.saturating_add(indeterminate);
        if release > 0 {
            self.aggregate.sub(release);
        }
    }
}

impl TokenLedger {
    /// Settle a sub-batch whose `held_estimate` was already moved to held (via
    /// [`Self::reconcile`] with the whole estimate indeterminate) now that its
    /// outcome is known.
    ///
    /// The estimate leaves held; `reported` stays charged as spend, `indeterminate`
    /// goes back to held, and the rest is released to this execution and the
    /// aggregate. A `reported` above the estimate is spend that already happened:
    /// the excess is charged (saturating, never refused) even if that passes a
    /// cap, and the next reservation is then refused.
    pub fn settle_held(&self, held_estimate: u64, reported: u64, indeterminate: u64) {
        let mut state = self.locked();
        state.held = state.held.saturating_sub(held_estimate);
        let keep = reported.saturating_add(indeterminate);
        let release = held_estimate.saturating_sub(keep);
        let excess = keep.saturating_sub(held_estimate);
        state.used = state.used.saturating_sub(release).saturating_add(excess);
        state.held = state.held.saturating_add(indeterminate);
        if release > 0 {
            self.aggregate.sub(release);
        }
        if excess > 0 {
            self.aggregate.add_unchecked(excess);
        }
    }
}

impl Drop for TokenLedger {
    fn drop(&mut self) {
        // `get_mut` on our own `&mut self` cannot contend; a poisoned lock still
        // yields the state, whose counters are what we owe back.
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        let returnable = state.used.saturating_sub(state.held);
        if returnable > 0 {
            self.aggregate.sub(returnable);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(per_execution_tokens: u64, max_held_tokens: u64) -> LedgerLimits {
        LedgerLimits {
            per_execution_tokens,
            max_held_tokens,
        }
    }

    #[test]
    fn aggregate_cap_binds_across_ledgers_and_refunds_on_drop() {
        let aggregate = SharedTokenBudget::new(100);
        let first = TokenLedger::new(limits(u64::MAX, 0), aggregate.clone());
        let second = TokenLedger::new(limits(u64::MAX, 0), aggregate.clone());

        first.reserve(60).expect("first fits");
        assert_eq!(
            second.reserve(50),
            Err(LedgerRefusal::AllExecutions {
                requested: 110,
                limit: 100
            })
        );
        assert_eq!(aggregate.used(), 60, "a refusal charges nothing");
        second.reserve(40).expect("second fills the remainder");
        assert_eq!(aggregate.used(), 100);

        drop(first);
        assert_eq!(aggregate.used(), 40, "drop refunds the first ledger's 60");
        second.reserve(60).expect("the refund is reusable");
    }

    #[test]
    fn held_reserve_stays_charged_to_the_aggregate_after_drop() {
        let aggregate = SharedTokenBudget::new(1_000);
        {
            let ledger = TokenLedger::new(limits(1_000, 1_000), aggregate.clone());
            ledger.reserve(300).expect("fits");
            ledger.reconcile(300, 100, 50);
            assert_eq!(ledger.used(), 150);
            assert_eq!(ledger.held(), 50);
            assert_eq!(aggregate.used(), 150);
        }
        assert_eq!(aggregate.used(), 50);
    }

    #[test]
    fn held_reserve_past_the_ceiling_refuses_further_reservation() {
        let ledger = TokenLedger::new(limits(1_000, 10), SharedTokenBudget::new(1_000));
        ledger.reserve(100).expect("fits");
        ledger.reconcile(100, 0, 100);
        assert_eq!(
            ledger.reserve(1),
            Err(LedgerRefusal::HeldReserve {
                held: 100,
                limit: 10
            })
        );
    }

    #[test]
    fn settle_held_charges_reported_usage_above_the_estimate() {
        let aggregate = SharedTokenBudget::new(5);
        let ledger = TokenLedger::new(limits(5, 100), aggregate.clone());
        ledger.reserve(1).expect("fits");
        ledger.reconcile(1, 0, 1);
        ledger.settle_held(1, 7, 0);
        assert_eq!((ledger.used(), ledger.held(), aggregate.used()), (7, 0, 7));
        assert!(
            ledger.reserve(1).is_err(),
            "spend past the caps refuses the next reservation"
        );
    }

    #[test]
    fn a_reservation_beyond_i64_max_is_refused_not_wrapped() {
        let aggregate = SharedTokenBudget::new(10);
        let ledger = TokenLedger::new(limits(u64::MAX, 0), aggregate.clone());
        assert!(ledger.reserve(u64::MAX).is_err());
        assert_eq!(aggregate.used(), 0);
    }
}
