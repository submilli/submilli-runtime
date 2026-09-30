//! Scoped accounting for native working buffers and bounded host work.
use crate::runtime::StoreData;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wasmtime::{Caller, Result, Trap, bail};

pub(super) struct Budget {
    counter: Arc<AtomicU64>,
    charged: u64,
    pub max_bytes: usize,
}
impl Budget {
    pub fn new(caller: &Caller<'_, StoreData>) -> Self {
        Self {
            counter: caller.data().tenant_limits.host_attached_counter(),
            charged: 0,
            max_bytes: caller.data().fs_max_read_size as usize,
        }
    }
    pub fn charge(&mut self, caller: &mut Caller<'_, StoreData>, bytes: usize) -> Result<()> {
        // An estimate of working space taken before the work starts, so a
        // refusal is an error the program can catch and act on (a smaller
        // file), not the run reaching its memory cap.
        caller
            .data()
            .tenant_limits
            .charge_host_bytes(bytes as u64)
            .map_err(|refused| wasmtime::Error::msg(format!("code: {refused}")))?;
        self.charged += bytes as u64;
        Ok(())
    }
    pub fn work(caller: &mut Caller<'_, StoreData>, units: usize) -> Result<()> {
        let remaining = caller.get_fuel()?;
        let Some(remaining) = remaining.checked_sub(units as u64) else {
            caller.set_fuel(0)?;
            return Err(Trap::OutOfFuel.into());
        };
        caller.set_fuel(remaining)
    }
    pub fn check_size(&self, bytes: usize) -> Result<()> {
        if bytes > self.max_bytes {
            bail!("code: result or input exceeds maxReadSize; use a smaller file or query");
        }
        Ok(())
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.charged, Ordering::Relaxed);
    }
}

/// Conservative JSON/text expansion budget, checked before constructing retained records.
pub(super) struct OutputBudget {
    bytes: usize,
}
impl OutputBudget {
    pub fn new() -> Self {
        Self { bytes: 128 }
    }
    pub fn reserve(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        budget: &mut Budget,
        bytes: usize,
    ) -> Result<bool> {
        if self.bytes.saturating_add(bytes) > budget.max_bytes {
            return Ok(false);
        }
        budget.charge(caller, bytes.saturating_mul(2))?;
        self.bytes += bytes;
        Ok(true)
    }
    pub fn line_bytes(text: &str) -> usize {
        text.len().saturating_mul(6).saturating_add(256)
    }
}
