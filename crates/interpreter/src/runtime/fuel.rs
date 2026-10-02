//! Fuel for work done by host functions. Wasm instructions burn fuel on their
//! own; a host function charges for its work here, from the same budget, so
//! that fuel bounds the CPU a program spends wherever it spends it.

use wasmtime::{AsContextMut, Trap};

use super::StoreData;

/// Charges `units` of host work against the run's fuel, before the work.
///
/// Not enough fuel leaves the store at zero and fails with the engine's own
/// out-of-fuel trap, so the run ends the way a Wasm loop would: guest code
/// cannot catch the trap, even when the host body re-entered it. What was
/// taken is recorded as host fuel for [`super::limits::ExecutionUsage`]; on
/// exhaustion that is only the fuel that was left, so `wasm + host` still
/// adds up to the budget.
///
/// `set_fuel` also restarts the engine's async yield countdown
/// (`RuntimeConfig::async_yield_fuel`), so a program that charges more often
/// than every interval seldom yields on fuel; the deadline still interrupts
/// it. Keeping the countdown needs an engine call that consumes fuel in
/// place, which is still to be added.
pub fn charge_host_fuel(
    mut ctx: impl AsContextMut<Data = StoreData>,
    units: u64,
) -> wasmtime::Result<()> {
    let mut ctx = ctx.as_context_mut();
    let remaining = ctx.get_fuel()?;
    let taken = units.min(remaining);
    ctx.set_fuel(remaining - taken)?;
    ctx.data_mut().host_fuel = ctx.data().host_fuel.saturating_add(taken);
    if taken < units {
        return Err(Trap::OutOfFuel.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs};
    use wasmtime::{Func, FuncType, Val, ValType};

    #[test]
    fn host_charges_draw_from_the_store_fuel_and_are_counted() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let charge = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::I64], []),
            |mut caller, params, _| charge_host_fuel(&mut caller, params[0].unwrap_i64() as u64),
        );

        store.set_fuel(300).unwrap();
        charge.call(&mut store, &[Val::I64(100)], &mut []).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 200);
        assert_eq!(store.data().host_fuel, 100);
        charge.call(&mut store, &[Val::I64(200)], &mut []).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 0);
        assert_eq!(store.data().host_fuel, 300);

        // Exhaustion zeroes the store and counts only what was there to take.
        store.set_fuel(50).unwrap();
        let error = charge
            .call(&mut store, &[Val::I64(80)], &mut [])
            .unwrap_err();
        assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::OutOfFuel));
        assert_eq!(store.get_fuel().unwrap(), 0);
        assert_eq!(store.data().host_fuel, 350);
    }
}
