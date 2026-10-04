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
    /// The fuel the run had left when the worker started.
    ceiling: u64,
    algorithm: AtomicU64,
    forgiving: AtomicBool,
    /// Set to stop the operation once the counted work passes the ceiling.
    cancel: Option<Arc<AtomicBool>>,
    /// Whether the meter stopped the operation.
    exhausted: AtomicBool,
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
            ceiling,
            algorithm: AtomicU64::new(0),
            forgiving: AtomicBool::new(false),
            cancel,
            exhausted: AtomicBool::new(false),
        }
    }

    pub(super) fn syscalls(&self, n: u64) {
        self.add(&self.syscalls, n);
    }

    pub(super) fn io(&self, bytes: u64) {
        self.add(&self.io, bytes);
    }

    pub(super) fn scan(&self, bytes: u64) {
        self.add(&self.scan, bytes);
    }

    pub(super) fn parse(&self, bytes: u64) {
        self.add(&self.parse, bytes);
    }

    pub(super) fn hash(&self, bytes: u64) {
        self.add(&self.hash, bytes);
    }

    pub(super) fn elements(&self, n: u64) {
        self.add(&self.elements, n);
    }

    fn add(&self, counter: &AtomicU64, n: u64) {
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_add(n))
        });
        if !self.forgiving.load(Ordering::Relaxed) && self.fuel() > self.ceiling {
            self.exhausted.store(true, Ordering::Relaxed);
            if let Some(cancel) = &self.cancel {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn charge_algorithm(&self, fuel: u64) -> wasmtime::Result<()> {
        let forgiving = self.forgiving.load(Ordering::Relaxed);
        let remaining = self.ceiling.saturating_sub(self.fuel());
        self.add(
            &self.algorithm,
            if forgiving { fuel } else { fuel.min(remaining) },
        );
        if !forgiving && fuel > remaining {
            return Err(wasmtime::Trap::OutOfFuel.into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn algorithm_fuel(&self) -> u64 {
        self.algorithm.load(Ordering::Relaxed)
    }

    pub(super) fn before_effect(&self) {
        self.forgiving.store(true, Ordering::Relaxed);
    }

    /// Whether the work passed the ceiling, which stopped the operation.
    pub(super) fn is_exhausted(&self) -> bool {
        self.exhausted.load(Ordering::Relaxed)
    }

    /// The fuel the counted work costs.
    pub(super) fn fuel(&self) -> u64 {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        [
            fuel::SYSCALL.cost(load(&self.syscalls)),
            fuel::IO.cost(load(&self.io)),
            fuel::SCAN.cost(load(&self.scan)),
            fuel::PARSE.cost(load(&self.parse)),
            fuel::HASH.cost(load(&self.hash)),
            fuel::ELEM.cost(load(&self.elements)),
            load(&self.algorithm),
        ]
        .into_iter()
        .fold(0u64, u64::saturating_add)
    }

    /// Charges the counted work; see the module documentation.
    pub(super) fn settle(&self, ctx: impl AsContextMut<Data = StoreData>) -> wasmtime::Result<()> {
        fuel::settle_host_fuel(ctx, self.fuel())
    }
}

/// The fuel a run has left for host work: what the engine holds, less the
/// host charges not yet applied to it.
pub(super) fn remaining_fuel(
    mut ctx: impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<u64> {
    let ctx = ctx.as_context_mut();
    let pending = ctx.data().host_fuel_pending;
    Ok(ctx.get_fuel()?.saturating_sub(pending))
}

#[cfg(test)]
mod tests {
    use super::*;

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
