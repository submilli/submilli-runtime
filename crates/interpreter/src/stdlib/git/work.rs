//! Worker-side algorithm admission shares the operation's complete fuel meter.
use std::sync::Arc;
use wasmtime::Result;

pub(super) struct AlgorithmWork {
    meter: Arc<super::meter::Meter>,
}
impl AlgorithmWork {
    pub fn new(limit: u64) -> Self {
        Self::with_meter(Arc::new(super::meter::Meter::new(limit, None)))
    }

    pub fn with_meter(meter: Arc<super::meter::Meter>) -> Self {
        Self { meter }
    }

    pub fn charge(&self, units: u64) -> Result<()> {
        self.meter.charge_algorithm(units)
    }

    pub fn before_effect(&self) {
        self.meter.before_effect();
    }

    #[cfg(test)]
    pub fn spent(&self) -> u64 {
        self.meter.algorithm_fuel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_before_work_and_preserves_work_after_an_effect() {
        let work = AlgorithmWork::new(10);
        work.charge(7).unwrap();
        assert!(work.charge(4).unwrap_err().is::<wasmtime::Trap>());
        assert_eq!(work.spent(), 10);
        work.before_effect();
        work.charge(20).unwrap();
        assert_eq!(work.spent(), 30);
    }
}
