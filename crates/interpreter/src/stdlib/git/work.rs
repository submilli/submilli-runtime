//! Worker-side fuel bounds for measured algorithms, with post-effect forgiveness.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use wasmtime::{Result, Trap};

pub(super) struct AlgorithmWork {
    spent: AtomicU64,
    limit: u64,
    forgiving: AtomicBool,
}
impl AlgorithmWork {
    pub fn new(limit: u64) -> Self {
        Self {
            spent: AtomicU64::new(0),
            limit,
            forgiving: AtomicBool::new(false),
        }
    }

    pub fn charge(&self, units: u64) -> Result<()> {
        let forgiving = self.forgiving.load(Ordering::Relaxed);
        let previous = self
            .spent
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |spent| {
                let next = spent.saturating_add(units);
                Some(if forgiving {
                    next
                } else {
                    next.min(self.limit)
                })
            })
            .map_err(|_| {
                crate::runtime::host::fatal_host_error("git: failed to account algorithm work")
            })?;
        if !forgiving && units > self.limit.saturating_sub(previous) {
            return Err(Trap::OutOfFuel.into());
        }
        Ok(())
    }

    pub fn before_effect(&self) {
        // Sending a request or publishing can have an uncertain outcome.
        self.forgiving.store(true, Ordering::Relaxed);
    }

    pub fn spent(&self) -> u64 {
        self.spent.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_before_work_and_preserves_work_after_an_effect() {
        let work = AlgorithmWork::new(10);
        work.charge(7).unwrap();
        assert!(work.charge(4).unwrap_err().is::<Trap>());
        assert_eq!(work.spent(), 10);
        work.before_effect();
        work.charge(20).unwrap();
        assert_eq!(work.spent(), 30);
    }
}
