//! Per-store deadlines on the server's shared engine clock.
//!
//! A run's time limit counts its own work. Time spent waiting outside the
//! program on a host call that pauses the clock (an application tool call,
//! [`StoreData::clock_paused`](interpreter::runtime::StoreData::clock_paused))
//! does not count.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use wasmtime::{Engine, Store, UpdateDeadline};

const TICK: Duration = Duration::from_secs(1);

pub(crate) fn start_ticker(engine: &Engine) -> std::io::Result<JoinHandle<()>> {
    let engine = engine.weak();
    thread::Builder::new()
        .name("execution-epoch".into())
        .spawn(move || {
            loop {
                // Sleep between increments, rather than catching up missed ticks:
                // compressed ticks could expire a newly armed store early.
                thread::sleep(TICK);
                let Some(engine) = engine.upgrade() else {
                    break;
                };
                engine.increment_epoch();
            }
        })
}

/// Arm `store` to stop after `timeout` of work. `paused` reads how long the
/// store has spent waiting with the clock paused.
///
/// The tick deadline stays the authority while nothing has paused, so a store
/// that never waits stops exactly as a plain epoch deadline would. Once it has
/// paused, reaching the deadline re-derives the work done from the wall clock
/// and, if the budget is not spent, pushes the deadline out by what remains.
pub(crate) fn arm<T: 'static>(
    store: &mut Store<T>,
    timeout: Option<Duration>,
    paused: fn(&T) -> Duration,
) {
    store.set_epoch_deadline(deadline_ticks(timeout));
    let Some(budget) = timeout.filter(|timeout| !timeout.is_zero()) else {
        return;
    };
    let armed = Instant::now();
    let paused_before = paused(store.data());
    store.epoch_deadline_callback(move |store| {
        let paused = paused(store.data()).saturating_sub(paused_before);
        if paused.is_zero() {
            return Ok(UpdateDeadline::Interrupt);
        }
        let worked = armed.elapsed().saturating_sub(paused);
        Ok(match budget.checked_sub(worked) {
            Some(left) if !left.is_zero() => UpdateDeadline::Continue(deadline_ticks(Some(left))),
            _ => UpdateDeadline::Interrupt,
        })
    });
}

fn deadline_ticks(timeout: Option<Duration>) -> u64 {
    let Some(timeout) = timeout.filter(|timeout| !timeout.is_zero()) else {
        return u64::MAX;
    };
    // The next tick can be imminent. Reserve a full extra tick so alignment
    // never shortens the requested duration. Embedders can supply fractions.
    timeout
        .as_secs()
        .saturating_add(u64::from(timeout.subsec_nanos() != 0))
        .saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use interpreter::runtime::RuntimeConfig;
    use wasmtime::{Instance, Module, Trap};

    fn can_run(store: &mut Store<()>) -> bool {
        let module = test_module(store.engine(), false);
        let instance = Instance::new(&mut *store, &module, &[]).unwrap();
        let run = instance
            .get_typed_func::<(), ()>(&mut *store, "run")
            .unwrap();
        match run.call(store, ()) {
            Ok(()) => true,
            Err(error) => {
                assert!(matches!(
                    error.downcast_ref::<Trap>(),
                    Some(Trap::Interrupt)
                ));
                false
            }
        }
    }

    fn test_module(engine: &Engine, calls_host: bool) -> Module {
        use wasm_encoder::*;
        let mut module = wasm_encoder::Module::new();
        let mut types = TypeSection::new();
        types.ty().function([], []);
        module.section(&types);
        if calls_host {
            let mut imports = ImportSection::new();
            imports.import("host", "wait", EntityType::Function(0));
            module.section(&imports);
        }
        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);
        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, u32::from(calls_host));
        module.section(&exports);
        let mut code = CodeSection::new();
        let mut function = Function::new([(1, ValType::I32)]);
        if calls_host {
            function.instruction(&Instruction::Call(0));
            // Ensure guest execution reaches an epoch check after the host call.
            function.instruction(&Instruction::Loop(BlockType::Empty));
            function.instruction(&Instruction::Br(0));
            function.instruction(&Instruction::End);
        } else {
            function.instruction(&Instruction::I32Const(10));
            function.instruction(&Instruction::LocalSet(0));
            function.instruction(&Instruction::Loop(BlockType::Empty));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::I32Const(1));
            function.instruction(&Instruction::I32Sub);
            function.instruction(&Instruction::LocalTee(0));
            function.instruction(&Instruction::BrIf(0));
            function.instruction(&Instruction::End);
        }
        function.instruction(&Instruction::End);
        code.function(&function);
        module.section(&code);
        wasmtime::Module::new(engine, module.finish()).unwrap()
    }

    #[tokio::test]
    async fn pending_host_call_is_not_cancelled_by_expired_epoch() {
        use std::sync::Arc;
        use wasmtime::{Caller, Extern, Func};
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config.store_async(&engine, ()).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let host = Func::wrap_async(&mut store, {
            let entered = entered.clone();
            let release = release.clone();
            move |_: Caller<'_, ()>, (): ()| {
                let entered = entered.clone();
                let release = release.clone();
                Box::new(async move {
                    entered.notify_one();
                    release.notified().await;
                })
            }
        });
        let module = test_module(&engine, true);
        let instance = Instance::new_async(&mut store, &module, &[Extern::Func(host)])
            .await
            .unwrap();
        let run = instance
            .get_typed_func::<(), ()>(&mut store, "run")
            .unwrap();
        arm(&mut store, Some(Duration::from_secs(1)), |_| Duration::ZERO);
        let mut execution = Box::pin(run.call_async(&mut store, ()));
        tokio::select! {
            _ = entered.notified() => {},
            result = &mut execution => panic!("host did not wait: {result:?}"),
        }
        engine.increment_epoch();
        engine.increment_epoch();
        assert!(futures::poll!(&mut execution).is_pending());
        release.notify_one();
        let error = tokio::time::timeout(Duration::from_secs(5), execution)
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<Trap>(),
            Some(Trap::Interrupt)
        ));
    }

    /// A host call that pauses the clock waits past the deadline; the run then
    /// keeps its unspent budget instead of stopping at its next check.
    #[tokio::test]
    async fn time_paused_in_a_host_call_does_not_count() {
        use std::sync::Arc;
        use wasmtime::{Caller, Extern, Func};
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config.store_async(&engine, Duration::ZERO).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let host = Func::wrap_async(&mut store, {
            let entered = entered.clone();
            let release = release.clone();
            move |mut caller: Caller<'_, Duration>, (): ()| {
                let entered = entered.clone();
                let release = release.clone();
                Box::new(async move {
                    let waited = Instant::now();
                    entered.notify_one();
                    release.notified().await;
                    *caller.data_mut() += waited.elapsed();
                })
            }
        });
        let module = test_module(&engine, true);
        let instance = Instance::new_async(&mut store, &module, &[Extern::Func(host)])
            .await
            .unwrap();
        let run = instance
            .get_typed_func::<(), ()>(&mut store, "run")
            .unwrap();
        let budget = Duration::from_millis(300);
        arm(&mut store, Some(budget), |paused| *paused);
        let mut execution = Box::pin(run.call_async(&mut store, ()));
        tokio::select! {
            _ = entered.notified() => {},
            result = &mut execution => panic!("host did not wait: {result:?}"),
        }
        // The wait outlasts the deadline's ticks.
        for _ in 0..3 {
            engine.increment_epoch();
        }
        release.notify_one();
        let released = Instant::now();
        // The program now loops; it stops once it has worked past its budget.
        let ticker = {
            let engine = engine.clone();
            std::thread::spawn(move || {
                for _ in 0..10 {
                    std::thread::sleep(Duration::from_millis(100));
                    engine.increment_epoch();
                }
            })
        };
        let error = tokio::time::timeout(Duration::from_secs(5), execution)
            .await
            .unwrap()
            .unwrap_err();
        ticker.join().unwrap();
        assert!(matches!(
            error.downcast_ref::<Trap>(),
            Some(Trap::Interrupt)
        ));
        assert!(released.elapsed() >= budget, "{:?}", released.elapsed());
    }

    #[test]
    fn deadlines_round_up_and_do_not_overflow() {
        assert_eq!(deadline_ticks(None), u64::MAX);
        assert_eq!(deadline_ticks(Some(Duration::ZERO)), u64::MAX);
        assert_eq!(deadline_ticks(Some(Duration::from_secs(30))), 31);
        assert_eq!(deadline_ticks(Some(Duration::from_millis(30100))), 32);
        assert_eq!(deadline_ticks(Some(Duration::MAX)), u64::MAX);
    }

    #[test]
    fn staggered_stores_expire_independently_and_disabled_store_survives() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut older = config.store(&engine, ()).unwrap();
        let mut newer = config.store(&engine, ()).unwrap();
        let mut disabled = config.store(&engine, ()).unwrap();
        arm(&mut older, Some(Duration::from_secs(2)), |_| Duration::ZERO);
        engine.increment_epoch();
        arm(&mut newer, Some(Duration::from_secs(2)), |_| Duration::ZERO);
        engine.increment_epoch();
        assert!(can_run(&mut older));
        engine.increment_epoch();
        assert!(!can_run(&mut older));
        assert!(can_run(&mut newer));
        engine.increment_epoch();
        assert!(!can_run(&mut newer));
        assert!(can_run(&mut disabled));
    }

    #[test]
    fn a_resumed_attempt_gets_a_fresh_timeout() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let timeout = Some(Duration::from_secs(2));
        let mut first = config.store(&engine, ()).unwrap();
        arm(&mut first, timeout, |_| Duration::ZERO);
        for _ in 0..3 {
            engine.increment_epoch();
        }
        assert!(!can_run(&mut first));
        let mut resumed = config.store(&engine, ()).unwrap();
        arm(&mut resumed, timeout, |_| Duration::ZERO);
        for _ in 0..2 {
            engine.increment_epoch();
            assert!(can_run(&mut resumed));
        }
        engine.increment_epoch();
        assert!(!can_run(&mut resumed));
    }

    #[test]
    fn ticker_releases_engine_and_exits() {
        let engine = RuntimeConfig::default().engine().unwrap();
        let weak = engine.weak();
        let ticker = start_ticker(&engine).unwrap();
        drop(engine);
        ticker.join().unwrap();
        assert!(weak.upgrade().is_none());
    }
}
