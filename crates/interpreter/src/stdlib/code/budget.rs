//! Scoped accounting for native working buffers.
use crate::runtime::StoreData;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wasmtime::{Caller, Result, bail};

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
        // Admit native buffers before allocation; exhausting tenant memory
        // terminates the run, while maxReadSize remains an ordinary error.
        caller
            .data()
            .tenant_limits
            .charge_host_bytes(bytes as u64)?;
        self.charged += bytes as u64;
        Ok(())
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
