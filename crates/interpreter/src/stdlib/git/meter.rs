//! The work a Git worker does, counted for fuel.
//!
//! The worker runs on the blocking pool, away from the store, so it counts
//! its own work here as it goes, and the store thread charges it when the
//! worker returns, whether or not the operation succeeded.
//!
//! The fuel the run had left when the worker started is the meter's ceiling.
//! Once the counted work passes it, the meter cancels the operation, and
//! every check that stops a cancelled operation stops this one. That is safe
//! until publication: nothing a Git operation does is seen before it
//! publishes, and publication checks for cancellation only before its first
//! move. A publication that has begun finishes, and its work is settled
//! after, never refused: a short budget is taken to zero and the run stops
//! at its next fuel check.
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
    /// Bytes scanned without being decoded: ignore patterns matched, text
    /// checked for a diff.
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
        counter.fetch_add(n, Ordering::Relaxed);
        if self.fuel() > self.ceiling {
            self.exhausted.store(true, Ordering::Relaxed);
            if let Some(cancel) = &self.cancel {
                cancel.store(true, Ordering::Relaxed);
            }
        }
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
}
