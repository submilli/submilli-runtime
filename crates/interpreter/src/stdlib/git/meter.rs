//! The work a Git worker does, counted for fuel.
//!
//! The worker runs on the blocking pool, away from the store, so it counts
//! its own work here as it goes, and the store thread charges it when the
//! worker returns, whether or not the operation succeeded.
//!
//! Filesystem/object counters and algorithm admission share the fuel ceiling.
//! Before an externally visible effect, exhausting it stops the worker. Before
//! repository creation, a remote send, or publication, the worker switches the
//! meter to forgiveness: later work is counted without refusal or fuel-driven
//! cancellation. The store settles the complete count after the worker drains,
//! preserving its effect or error, then stops at the next guest instruction.
use crate::runtime::StoreData;
use crate::runtime::fuel;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use wasmtime::AsContextMut;

#[derive(Debug)]
pub(super) struct Meter {
    /// Filesystem operations: an entry listed or stat'ed, a file opened, a
    /// rename, a directory created.
    syscalls: AtomicU64,
    /// Bytes read from or written to files, or received.
    io: AtomicU64,
    /// Units scanned without decoding: bytes of text checked for a diff, and
    /// the units of work ignore matching counts (patterns times path bytes).
    scan: AtomicU64,
    /// Bytes decoded: trees, commits, the index, blobs, packs.
    parse: AtomicU64,
    /// Bytes hashed.
    hash: AtomicU64,
    /// Entries built: paths in a file set, index entries, objects indexed,
    /// references read.
    elements: AtomicU64,
    budget: fuel::WorkBudget,
}

impl Default for Meter {
    /// A meter with no ceiling.
    fn default() -> Self {
        Self::new(u64::MAX, None)
    }
}

impl Meter {
    /// A meter that sets `cancel` once the work passes `ceiling` fuel.
    pub(super) fn new(ceiling: u64, cancel: Option<Arc<AtomicBool>>) -> Self {
        Self {
            syscalls: AtomicU64::new(0),
            io: AtomicU64::new(0),
            scan: AtomicU64::new(0),
            parse: AtomicU64::new(0),
            hash: AtomicU64::new(0),
            elements: AtomicU64::new(0),
            budget: match cancel {
                Some(cancel) => fuel::WorkBudget::new(ceiling).with_cancellation(cancel),
                None => fuel::WorkBudget::new(ceiling),
            },
        }
    }

    pub(super) fn syscalls(&self, n: u64) {
        self.add(&self.syscalls, fuel::SYSCALL, n);
    }

    pub(super) fn io(&self, bytes: u64) {
        self.add(&self.io, fuel::IO, bytes);
    }

    pub(super) fn scan(&self, bytes: u64) {
        self.add(&self.scan, fuel::SCAN, bytes);
    }

    pub(super) fn parse(&self, bytes: u64) {
        self.add(&self.parse, fuel::PARSE, bytes);
    }

    pub(super) fn hash(&self, bytes: u64) {
        self.add(&self.hash, fuel::HASH, bytes);
    }

    pub(super) fn elements(&self, n: u64) {
        self.add(&self.elements, fuel::ELEM, n);
    }

    fn add(&self, counter: &AtomicU64, rate: fuel::Rate, units: u64) {
        // Retain cumulative rounding: four one-byte reads cost IO.cost(4),
        // not four separately rounded IO.cost(1) charges.
        let previous = counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(units))
            })
            .unwrap_or_else(|previous| previous);
        let cost = rate
            .cost(previous.saturating_add(units))
            .saturating_sub(rate.cost(previous));
        self.budget.record_fuel(cost);
    }

    pub(super) fn charge_algorithm(&self, fuel: u64) -> wasmtime::Result<()> {
        self.budget
            .charge_fuel(fuel)
            .map_err(|_| wasmtime::Trap::OutOfFuel.into())
    }

    #[cfg(test)]
    pub(super) fn algorithm_fuel(&self) -> u64 {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let counted = [
            fuel::SYSCALL.cost(load(&self.syscalls)),
            fuel::IO.cost(load(&self.io)),
            fuel::SCAN.cost(load(&self.scan)),
            fuel::PARSE.cost(load(&self.parse)),
            fuel::HASH.cost(load(&self.hash)),
            fuel::ELEM.cost(load(&self.elements)),
        ]
        .into_iter()
        .fold(0u64, u64::saturating_add);
        self.budget.spent().saturating_sub(counted)
    }

    pub(super) fn before_effect(&self) {
        self.budget.before_effect();
    }

    pub(super) fn is_exhausted(&self) -> bool {
        self.budget.is_exhausted()
    }

    #[cfg(test)]
    pub(super) fn fuel(&self) -> u64 {
        self.budget.spent()
    }

    /// Charges the counted work; see the module documentation.
    pub(super) fn settle(&self, ctx: impl AsContextMut<Data = StoreData>) -> wasmtime::Result<()> {
        self.budget.settle(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_categories_keep_cumulative_rounding_and_share_algorithm_admission() {
        let meter = Meter::new(100, None);
        for _ in 0..4 {
            meter.io(1);
        }
        assert_eq!(meter.fuel(), fuel::IO.cost(4));
        meter.scan(2);
        meter.parse(3);
        meter.hash(4);
        meter.elements(1);
        let native = fuel::IO.cost(4)
            + fuel::SCAN.cost(2)
            + fuel::PARSE.cost(3)
            + fuel::HASH.cost(4)
            + fuel::ELEM.cost(1);
        assert_eq!(meter.fuel(), native);
        meter.charge_algorithm(100 - native).unwrap();
        assert_eq!(meter.algorithm_fuel(), 100 - native);
        assert!(meter.charge_algorithm(1).is_err());
        assert_eq!(meter.fuel(), 100);
        meter.before_effect();
        meter.syscalls(1);
        assert_eq!(meter.fuel(), 100 + fuel::SYSCALL.cost(1));
        assert!(!meter.is_exhausted());
    }

    #[test]
    fn saturated_native_counters_do_not_charge_again() {
        let meter = Meter::default();
        meter.io(u64::MAX);
        meter.io(1);
        assert_eq!(meter.fuel(), fuel::IO.cost(u64::MAX));
        meter.hash(u64::MAX);
        meter.elements(u64::MAX);
        assert_eq!(meter.fuel(), u64::MAX);
        assert!(!meter.is_exhausted());
    }

    #[test]
    fn passing_the_ceiling_cancels_the_operation() {
        let cancel = Arc::new(AtomicBool::new(false));
        let meter = Meter::new(fuel::SYSCALL.cost(3), Some(Arc::clone(&cancel)));
        meter.syscalls(3);
        assert!(!cancel.load(Ordering::Relaxed));
        meter.syscalls(1);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(meter.is_exhausted());
    }
    #[test]
    fn algorithm_and_native_work_share_a_ceiling_and_forgive_effects() {
        let cancel = Arc::new(AtomicBool::new(false));
        let meter = Arc::new(Meter::new(10, Some(Arc::clone(&cancel))));
        let work = super::super::work::AlgorithmWork::with_meter(Arc::clone(&meter));
        meter.io(12);
        work.charge(7).unwrap();
        assert_eq!(meter.fuel(), 10);
        assert!(work.charge(1).unwrap_err().is::<wasmtime::Trap>());
        assert_eq!(meter.fuel(), 10);
        work.before_effect();
        meter.io(80);
        work.charge(30).unwrap();
        assert_eq!(meter.fuel(), 60);
        assert!(!cancel.load(Ordering::Relaxed));
        assert!(!meter.is_exhausted());
    }
}
