//! Per-store [`ResourceLimiter`] enforcing a single aggregate cap on
//! GC heap, linear memory, and host-attached bytes (e.g. compiled regexes).
//! `host_attached_bytes` is `Arc<AtomicU64>` so externref `Drop` impls can
//! decrement it without a `&mut TenantLimits` borrow.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wasmtime::{ResourceLimiter, Store};

use super::StoreData;

pub const DEFAULT_MAX_STORE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct MemoryCapExceeded {
    pub requested: u64,
    pub already_observed: u64,
    pub already_host_attached: u64,
    pub cap: u64,
}

impl std::fmt::Display for MemoryCapExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "memory cap exceeded: requested {} host bytes; \
             already observed {} bytes + {} host bytes against {} cap",
            self.requested, self.already_observed, self.already_host_attached, self.cap
        )
    }
}

impl std::error::Error for MemoryCapExceeded {}

/// The run reached its memory cap. Like spent fuel or an expired deadline this
/// ends the run; a program cannot catch it. The refusal that raised it is the
/// error's source.
#[derive(Debug, Clone, Copy)]
pub struct MemoryExhausted;

impl std::fmt::Display for MemoryExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("memory exhausted")
    }
}

/// Whether `err` is the tenant's cap refusing memory: the engine growing the
/// GC heap, or the host charging bytes it holds for the program.
pub fn is_memory_exhausted(err: &wasmtime::Error) -> bool {
    err.is::<MemoryExhausted>()
        || err.is::<wasmtime::GcHeapOutOfMemory<()>>()
        || err.is::<MemoryCapExceeded>()
}

/// Names `err` as [`MemoryExhausted`] when the cap refused memory, keeping the
/// refusal as its source; any other error is returned unchanged.
pub(crate) fn name_memory_exhaustion(err: wasmtime::Error) -> wasmtime::Error {
    if !is_memory_exhausted(&err) || err.is::<MemoryExhausted>() {
        return err;
    }
    err.context(MemoryExhausted)
}

pub struct TenantLimits {
    pub max_total_bytes: u64,
    observed_bytes: u64,
    peak_bytes: AtomicU64,
    host_attached_bytes: Arc<AtomicU64>,
}

impl TenantLimits {
    pub fn new(max_total_bytes: u64) -> Self {
        Self {
            max_total_bytes,
            observed_bytes: 0,
            peak_bytes: AtomicU64::new(0),
            host_attached_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn observed_bytes(&self) -> u64 {
        self.observed_bytes
    }

    /// High-water mark of admitted engine memory plus charged host bytes, not RSS.
    pub fn peak_bytes(&self) -> u64 {
        self.peak_bytes.load(Ordering::Relaxed)
    }

    pub fn host_attached_bytes(&self) -> u64 {
        self.host_attached_bytes.load(Ordering::Relaxed)
    }

    pub fn host_attached_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.host_attached_bytes)
    }

    /// Charges happen on the store thread, but cancelled Git workers can refund
    /// their reservations concurrently during execution cleanup.
    pub fn charge_host_bytes(&self, n: u64) -> Result<(), MemoryCapExceeded> {
        self.host_attached_bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                let next = current.checked_add(n)?;
                (self.observed_bytes.saturating_add(next) <= self.max_total_bytes).then_some(next)
            })
            .map(|previous| {
                self.peak_bytes.fetch_max(
                    self.observed_bytes
                        .saturating_add(previous)
                        .saturating_add(n),
                    Ordering::Relaxed,
                );
            })
            .map_err(|current| MemoryCapExceeded {
                requested: n,
                already_observed: self.observed_bytes,
                already_host_attached: current,
                cap: self.max_total_bytes,
            })
    }

    pub fn release_host_bytes(&self, n: u64) {
        let _ = self.host_attached_bytes.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_sub(n)),
        );
    }
}

impl ResourceLimiter for TenantLimits {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let delta = (desired as u64).saturating_sub(current as u64);
        let next = self.observed_bytes.saturating_add(delta);
        // host_attached_bytes is read-only here; writes belong to charge/release_host_bytes.
        let host = self.host_attached_bytes.load(Ordering::Relaxed);
        if next.saturating_add(host) > self.max_total_bytes {
            return Ok(false);
        }
        self.observed_bytes = next;
        self.peak_bytes
            .fetch_max(next.saturating_add(host), Ordering::Relaxed);
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }
}

/// Call once, immediately after constructing the `Store`, before instantiating any module.
pub fn install_tenant_limits(store: &mut Store<StoreData>) {
    store.limiter(|data| &mut data.tenant_limits);
}

/// Usage survives successful execution, traps, and cleanup of host allocations.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionUsage {
    pub fuel: u64,
    pub memory_peak: u64,
}

impl ExecutionUsage {
    pub fn capture(store: &Store<StoreData>, initial_fuel: u64) -> wasmtime::Result<Self> {
        let remaining = store.get_fuel()?;
        let fuel = initial_fuel
            .checked_sub(remaining)
            .ok_or_else(|| wasmtime::Error::msg("remaining fuel exceeds initial budget"))?;
        Ok(Self {
            fuel,
            memory_peak: store.data().tenant_limits.peak_bytes(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_peak_retains_released_host_allocations_and_excludes_refusals() {
        let mut limits = TenantLimits::new(1000);
        assert!(limits.memory_growing(0, 200, None).unwrap());
        limits.charge_host_bytes(500).unwrap();
        limits.release_host_bytes(500);
        assert_eq!(limits.peak_bytes(), 700);
        limits.charge_host_bytes(100).unwrap();
        assert!(limits.memory_growing(200, 650, None).unwrap());
        assert_eq!(limits.peak_bytes(), 750);
        assert!(limits.charge_host_bytes(300).is_err());
        assert!(!limits.memory_growing(650, 950, None).unwrap());
        assert_eq!(limits.peak_bytes(), 750);
    }
}
