//! The work a Git worker does, counted for fuel.
//!
//! The worker runs on the blocking pool, away from the store, so it counts
//! its own work here as it goes, and the store thread charges it when the
//! worker returns. By then the operation may have published, so the charge
//! is settled, never refused: a short budget is taken to zero and the run
//! stops at its next fuel check.
use crate::runtime::StoreData;
use crate::runtime::fuel;
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::AsContextMut;

#[derive(Debug, Default)]
pub(super) struct Meter {
    /// Filesystem operations: an entry listed or stat'ed, a file opened, a
    /// rename, a directory created.
    syscalls: AtomicU64,
    /// Bytes read from or written to files.
    io: AtomicU64,
    /// Bytes decoded: trees, commits, the index, blobs, packs.
    parse: AtomicU64,
    /// Bytes hashed.
    hash: AtomicU64,
    /// Entries built: paths in a file set, index entries, objects indexed.
    elements: AtomicU64,
}

impl Meter {
    pub(super) fn syscalls(&self, n: u64) {
        self.syscalls.fetch_add(n, Ordering::Relaxed);
    }

    pub(super) fn io(&self, bytes: u64) {
        self.io.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn parse(&self, bytes: u64) {
        self.parse.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn hash(&self, bytes: u64) {
        self.hash.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn elements(&self, n: u64) {
        self.elements.fetch_add(n, Ordering::Relaxed);
    }

    /// The fuel the counted work costs.
    pub(super) fn fuel(&self) -> u64 {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        [
            fuel::SYSCALL.cost(load(&self.syscalls)),
            fuel::IO.cost(load(&self.io)),
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
