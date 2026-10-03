//! Fuel for work done by host functions. Wasm instructions burn fuel on their
//! own; a host function charges for its work here, from the same budget, so
//! that fuel bounds the CPU a program spends wherever it spends it.
//!
//! One fuel is roughly the cost of one interpreted Wasm instruction, about
//! 2.5 ns. Host work is priced by the native time it takes, through a small
//! set of cost classes: each class is a rate per unit of work, and a host
//! function charges `CALL` plus the classes that describe what it does with
//! the sizes it knows. The rates are placeholders until SUB-1270 measures
//! them; `plans/sub-1269-host-fuel-costs.md` gives the formula of every host
//! function.
//!
//! Charges are batched: a charge is refused before the work when the budget
//! is short, but the engine's fuel is only lowered once [`HOST_FUEL_BATCH`]
//! units have accumulated and Wasm has burned more than that since the last
//! application, because `set_fuel` restarts the engine's async yield countdown
//! (`RuntimeConfig::async_yield_fuel`, as in wasmtime) and a loop calling a
//! host function every few instructions would otherwise never yield. The
//! engine may therefore run past the budget by what is pending before Wasm
//! fuel runs out, at most [`HOST_FUEL_PENDING_CAP`]; usage reporting clamps
//! to the budget.

use std::future::Future;

use wasmtime::{AsContextMut, Caller, Func, FuncType, Trap, Val};

use super::StoreData;

/// Fuel per unit of work, as `fuel` units for every `per` units of work, so a
/// class worth less than one fuel per unit still charges: the cost rounds up,
/// and no non-empty work is free.
#[derive(Clone, Copy, Debug)]
pub struct Rate {
    fuel: u64,
    per: u64,
}

impl Rate {
    const fn per_unit(fuel: u64) -> Rate {
        Rate { fuel, per: 1 }
    }

    const fn per_units(fuel: u64, per: u64) -> Rate {
        Rate { fuel, per }
    }

    /// The fuel for `n` units of work, rounded up, saturating.
    pub fn cost(self, n: u64) -> u64 {
        n.saturating_mul(self.fuel).div_ceil(self.per)
    }
}

/// Flat charge of every host call; an O(1) host function costs this alone.
/// `CALL`, `TZ` and `GATE` are single-shot amounts for `charge_host_fuel`;
/// the `Rate`s below scale with a size and go through `charge`.
pub const CALL: u64 = 16;
/// memcpy-like movement of code units or bytes.
pub const COPY: Rate = Rate::per_units(1, 8);
/// Per unit read, compared, hashed or transformed with simple logic, including
/// UTF-8/UTF-16 conversion.
pub const SCAN: Rate = Rate::per_unit(1);
/// Per byte of parsing or formatting that builds structure (JSON, URLs,
/// numbers, dates, diffs).
pub const PARSE: Rate = Rate::per_unit(6);
/// Per element, entry or field touched, boxed or allocated as a GC value.
pub const ELEM: Rate = Rate::per_unit(10);
/// Regex matching per byte of haystack. The engine is the `regex` crate: no
/// backtracking, so the worst case is the program size times the haystack,
/// and the program size is capped at compile time.
pub const REGEX: Rate = Rate::per_unit(8);
/// Compiling a regex, on top of `PARSE` of its source: the program it
/// builds is bounded by a size cap, not by the source length.
pub const REGEX_COMPILE: u64 = 2_000;
/// Cryptographic hashing per byte.
pub const HASH: Rate = Rate::per_unit(4);
/// Bytes sent, received, read or written; waiting costs nothing.
pub const IO: Rate = Rate::per_units(1, 4);
/// One filesystem metadata operation (open, stat, readdir entry, rename, ...).
pub const SYSCALL: Rate = Rate::per_unit(400);
/// One time-zone resolution.
pub const TZ: u64 = 200;
/// One capability check.
pub const GATE: u64 = 400;

/// The host overhead of sorting `n` elements: `ELEM` per comparison and move,
/// `n·log2(n)` of them. Comparators and key conversions charge themselves.
pub fn sort_cost(n: u64) -> u64 {
    let log = u64::from(n.max(2).ilog2());
    ELEM.cost(n.saturating_mul(log))
}

/// Multiplication, division or radix conversion of big integers of `a` and
/// `b` limbs: a limb operation per limb pair while the operands are small,
/// then, once the library switches to Karatsuba and Toom-3, close to linear
/// in the larger operand. A placeholder shape until SUB-1270 measures it.
pub fn bigint_product_cost(a: u64, b: u64) -> u64 {
    let (small, large) = (a.min(b).max(1), a.max(b).max(1));
    let schoolbook = small.saturating_mul(large.min(64));
    let beyond = large
        .saturating_sub(64)
        .saturating_mul(u64::from(large.ilog2()) + 1);
    ELEM.cost(schoolbook.saturating_add(beyond))
}

/// Charges the flat per-call cost.
pub fn charge_call(ctx: impl AsContextMut<Data = StoreData>) -> wasmtime::Result<()> {
    charge_host_fuel(ctx, CALL)
}

/// Charges `n` units of work at `rate`, before the work.
pub fn charge(
    ctx: impl AsContextMut<Data = StoreData>,
    rate: Rate,
    n: u64,
) -> wasmtime::Result<()> {
    charge_host_fuel(ctx, rate.cost(n))
}

/// Builds a host function that charges `CALL` before `body` runs. For host
/// functions created with `Func::new` outside the linker (iterator steps and
/// the like); linker registrations charge in `register_host_fn`.
pub(crate) fn host_func(
    mut store: impl AsContextMut<Data = StoreData>,
    ty: FuncType,
    body: impl Fn(&mut Caller<'_, StoreData>, &[Val], &mut [Val]) -> wasmtime::Result<()>
    + Send
    + Sync
    + 'static,
) -> Func {
    let abi = ty.clone();
    Func::new(&mut store, ty, move |mut caller, params, results| {
        super::host::check_host_abi(&abi, params, results)?;
        charge_call(&mut caller)?;
        body(&mut caller, params, results)
    })
}

/// Async sibling of [`host_func`], for the vtable hooks (`toString`,
/// `toJSON`, `equals`, `hash`) that guest code reaches by `call_ref`.
pub(crate) fn host_func_async<F>(
    mut store: impl AsContextMut<Data = StoreData>,
    ty: FuncType,
    body: F,
) -> Func
where
    F: for<'a> Fn(
            Caller<'a, StoreData>,
            &'a [Val],
            &'a mut [Val],
        ) -> Box<dyn Future<Output = wasmtime::Result<()>> + Send + 'a>
        + Send
        + Sync
        + 'static,
{
    let abi = ty.clone();
    Func::new_async(&mut store, ty, move |mut caller, params, results| {
        if let Err(error) = super::host::check_host_abi(&abi, params, results) {
            return Box::new(async move { Err(error) });
        }
        if let Err(error) = charge_call(&mut caller) {
            return Box::new(async move { Err(error) });
        }
        body(caller, params, results)
    })
}

/// Charges `units` for work that followed an effect: a response received, a
/// file written, a completion returned. The effect has happened and its
/// result is in hand, so the charge never refuses: a short budget is taken
/// to zero, the call completes, and the run stops at the next fuel check.
pub fn settle(
    ctx: impl AsContextMut<Data = StoreData>,
    rate: Rate,
    n: u64,
) -> wasmtime::Result<()> {
    settle_host_fuel(ctx, rate.cost(n))
}

/// Marshal an already-observed effect's result using helpers that normally
/// charge before work. Within this host-only scope, short fuel settles to zero
/// without discarding the result. Do not run guest callbacks in this scope.
pub(crate) fn settle_result<T>(
    caller: &mut Caller<'_, StoreData>,
    body: impl FnOnce(&mut Caller<'_, StoreData>) -> wasmtime::Result<T>,
) -> wasmtime::Result<T> {
    let previous = caller.data().settling_host_result;
    caller.data_mut().settling_host_result = true;
    let result = body(caller);
    caller.data_mut().settling_host_result = previous;
    result
}

/// [`settle`] for an amount of fuel.
pub fn settle_host_fuel(
    mut ctx: impl AsContextMut<Data = StoreData>,
    units: u64,
) -> wasmtime::Result<()> {
    let mut ctx = ctx.as_context_mut();
    let pending = ctx.data().host_fuel_pending;
    let available = ctx.get_fuel()?.saturating_sub(pending);
    if units <= available {
        return charge_host_fuel(ctx, units);
    }
    ctx.set_fuel(0)?;
    let data = ctx.data_mut();
    data.host_fuel_pending = 0;
    data.host_fuel = data.host_fuel.saturating_add(available);
    Ok(())
}

/// Host charges accumulate on the store and are applied to the engine once
/// they reach this many units and Wasm has burned this much since the last
/// application. It must not be below `RuntimeConfig::async_yield_fuel`: each
/// application restarts the yield countdown, and the countdown needs a full
/// interval between applications.
pub const HOST_FUEL_BATCH: u64 = 10_000;
/// Pending charges are applied regardless once they reach this, which bounds
/// how far past its budget a run can get before Wasm fuel runs out. Such an
/// application does restart the yield countdown, so a loop that charges this
/// much between yields yields only on the deadline.
pub const HOST_FUEL_PENDING_CAP: u64 = 1_000 * HOST_FUEL_BATCH;

/// Charges `units` of host work against the run's fuel, before the work.
///
/// The charge is refused when the budget left, pending charges included,
/// cannot cover it: the store is left at zero and the call fails with the
/// engine's own out-of-fuel trap, so the run ends the way a Wasm loop would
/// and guest code cannot catch it, even when the host body re-entered it.
/// Otherwise the charge joins the pending batch and reaches the engine once
/// the batch is full (see [`HOST_FUEL_BATCH`] and the module doc). Every
/// charge, pending or applied, counts as host fuel for
/// [`super::limits::ExecutionUsage`].
pub fn charge_host_fuel(
    mut ctx: impl AsContextMut<Data = StoreData>,
    units: u64,
) -> wasmtime::Result<()> {
    if units == 0 {
        return Ok(());
    }
    let mut ctx = ctx.as_context_mut();
    let pending = ctx.data().host_fuel_pending;
    let engine_fuel = ctx.get_fuel()?;
    let available = engine_fuel.saturating_sub(pending);
    if units > available {
        ctx.set_fuel(0)?;
        let data = ctx.data_mut();
        data.host_fuel_pending = 0;
        data.host_fuel = data.host_fuel.saturating_add(available);
        if data.settling_host_result {
            return Ok(());
        }
        return Err(Trap::OutOfFuel.into());
    }
    // Neither overflows: `units <= engine_fuel - pending`.
    let batch = pending + units;
    let data = ctx.data_mut();
    data.host_fuel = data.host_fuel.saturating_add(units);
    // Only Wasm lowers the engine's fuel between applications, so the mark
    // measures Wasm burned since; before the first application a full batch
    // applies at once. The engine yields on the instruction after the slice
    // hits zero, so one more unit than a batch proves a yield happened.
    let wasm_burned = data
        .host_fuel_applied_at
        .map_or(u64::MAX, |mark| mark.saturating_sub(engine_fuel));
    let apply = batch >= HOST_FUEL_PENDING_CAP
        || (batch >= HOST_FUEL_BATCH && wasm_burned > HOST_FUEL_BATCH);
    if !apply {
        data.host_fuel_pending = batch;
        return Ok(());
    }
    data.host_fuel_pending = 0;
    data.host_fuel_applied_at = Some(engine_fuel - batch);
    ctx.set_fuel(engine_fuel - batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs};
    use wasmtime::{Func, FuncType, Val, ValType};

    #[test]
    fn result_settlement_preserves_values_and_restores_refusing_charges() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let complete = Func::new(
            &mut store,
            FuncType::new(&engine, [], [ValType::I32]),
            |mut caller, _, results| {
                results[0] = settle_result(&mut caller, |caller| {
                    charge(caller, PARSE, 100)?;
                    Ok(Val::I32(42))
                })?;
                assert!(!caller.data().settling_host_result);
                Ok(())
            },
        );
        store.set_fuel(1).unwrap();
        let mut results = [Val::I32(0)];
        complete.call(&mut store, &[], &mut results).unwrap();
        assert_eq!(results[0].i32(), Some(42));
        assert_eq!(store.get_fuel().unwrap(), 0);
        assert_eq!(store.data().host_fuel, 1);
        assert!(charge_call(&mut store).unwrap_err().is::<Trap>());

        let fail = Func::new(
            &mut store,
            FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                let result = settle_result(&mut caller, |caller| {
                    charge(caller, COPY, 100)?;
                    Err::<(), _>(crate::runtime::host::range_error("copy entry limit"))
                });
                assert!(!caller.data().settling_host_result);
                result
            },
        );
        let error = fail.call(&mut store, &[], &mut []).unwrap_err();
        assert!(error.to_string().contains("copy entry limit"), "{error}");
        assert!(!store.data().settling_host_result);
        assert!(charge_call(&mut store).unwrap_err().is::<Trap>());
    }

    #[test]
    fn settling_never_refuses_and_leaves_a_short_budget_at_zero() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let settle = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::I64], []),
            |mut caller, params, _| settle_host_fuel(&mut caller, params[0].unwrap_i64() as u64),
        );
        let charge = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::I64], []),
            |mut caller, params, _| charge_host_fuel(&mut caller, params[0].unwrap_i64() as u64),
        );

        // Within the budget it is an ordinary charge, batched like any other.
        store.set_fuel(1_000).unwrap();
        settle.call(&mut store, &[Val::I64(300)], &mut []).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 1_000);
        assert_eq!(store.data().host_fuel_pending, 300);
        assert_eq!(store.data().host_fuel, 300);

        // Beyond it, pending included, it takes what is there and succeeds;
        // the next ordinary charge is the one that traps.
        settle
            .call(&mut store, &[Val::I64(5_000)], &mut [])
            .unwrap();
        assert_eq!(store.get_fuel().unwrap(), 0);
        assert_eq!(store.data().host_fuel_pending, 0);
        assert_eq!(store.data().host_fuel, 1_000);
        let error = charge
            .call(&mut store, &[Val::I64(1)], &mut [])
            .unwrap_err();
        assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::OutOfFuel));
    }

    #[test]
    fn the_batch_covers_the_default_yield_interval() {
        assert!(RuntimeConfig::default().async_yield_fuel <= Some(HOST_FUEL_BATCH));
    }

    #[test]
    fn rates_round_up_and_never_make_work_free() {
        assert_eq!(COPY.cost(0), 0);
        assert_eq!(COPY.cost(1), 1);
        assert_eq!(COPY.cost(8), 1);
        assert_eq!(COPY.cost(9), 2);
        assert_eq!(PARSE.cost(3), 18);
        assert_eq!(SCAN.cost(u64::MAX), u64::MAX);
        assert_eq!(sort_cost(0), 0);
        assert_eq!(sort_cost(1), ELEM.cost(1));
        assert_eq!(sort_cost(1024), ELEM.cost(10 * 1024));
    }

    #[test]
    fn host_charges_are_batched_counted_and_refused_before_the_work() {
        use crate::runtime::limits::ExecutionUsage;
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
        let charge_units = |store: &mut wasmtime::Store<StoreData>, units: u64| {
            charge.call(&mut *store, &[Val::I64(units as i64)], &mut [])
        };
        // Stands in for Wasm burning fuel between host calls.
        let burn = |store: &mut wasmtime::Store<StoreData>, units: u64| {
            let left = store.get_fuel().unwrap() - units;
            store.set_fuel(left).unwrap();
        };

        // Below a batch the engine's fuel is untouched; the counter is exact,
        // and so is the usage report.
        store.set_fuel(100_000).unwrap();
        charge_units(&mut store, 9_999).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 100_000);
        assert_eq!(store.data().host_fuel, 9_999);
        assert_eq!(store.data().host_fuel_pending, 9_999);
        let usage = ExecutionUsage::capture(&store, 100_000).unwrap();
        assert_eq!(
            (usage.fuel, usage.wasm_fuel, usage.host_fuel),
            (9_999, 0, 9_999)
        );
        // The first full batch is applied.
        charge_units(&mut store, 1).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 90_000);
        assert_eq!(store.data().host_fuel_pending, 0);
        // The next one waits until Wasm has burned more than a batch since,
        // so the yield countdown gets a full run.
        charge_units(&mut store, 20_000).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 90_000);
        assert_eq!(store.data().host_fuel_pending, 20_000);
        burn(&mut store, 10_000);
        charge_units(&mut store, 1).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 80_000);
        burn(&mut store, 1);
        charge_units(&mut store, 1).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 59_997);
        assert_eq!(store.data().host_fuel_pending, 0);
        let charged_so_far = store.data().host_fuel;

        // A charge the remaining budget cannot cover, pending included, is
        // refused before the work: of the 300, 100 is already pending, so the
        // refused charge takes the 200 left and the store ends at zero.
        store.set_fuel(300).unwrap();
        charge_units(&mut store, 100).unwrap();
        let error = charge_units(&mut store, 201).unwrap_err();
        assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::OutOfFuel));
        assert_eq!(store.get_fuel().unwrap(), 0);
        assert_eq!(store.data().host_fuel, charged_so_far + 100 + 200);
        assert_eq!(store.data().host_fuel_pending, 0);

        // Wasm may burn into a pending batch: the run overran by that much,
        // the next charge finds nothing left, and the report stays within
        // the budget. (A fresh budget, so the counters start over.)
        store.set_fuel(1_000).unwrap();
        store.data_mut().host_fuel = 0;
        charge_units(&mut store, 600).unwrap();
        burn(&mut store, 700);
        let error = charge_units(&mut store, 1).unwrap_err();
        assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::OutOfFuel));
        assert_eq!(store.data().host_fuel, 600);
        let usage = ExecutionUsage::capture(&store, 1_000).unwrap();
        assert_eq!(
            (usage.fuel, usage.wasm_fuel, usage.host_fuel),
            (1_000, 400, 600)
        );

        // Zero-cost work is free and leaves the engine alone.
        store.set_fuel(50).unwrap();
        charge_units(&mut store, 0).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 50);
    }
}
