//! Wall-clock watchdog that drives wasmtime's epoch-based interruption.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasmtime::Engine;

/// Bind with `let _watchdog = …`, not `let _ = …`; `_` drops immediately,
/// disarming the timer before the guarded Wasm call completes.
pub struct Watchdog {
    cancel: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // Not joining: avoids blocking fast programs on the sleeping timer.
        let _ = self.cancel.send(());
        let _ = self.handle.take();
    }
}

/// Return an I/O error if the operating system cannot create the timer thread.
pub fn arm(engine: &Engine, timeout: Duration) -> std::io::Result<Watchdog> {
    #[cfg(test)]
    if FAIL_SPAWN.get() {
        return Err(std::io::Error::from_raw_os_error(11));
    }
    let (tx, rx) = mpsc::channel::<()>();
    let engine = engine.clone();
    let handle = thread::Builder::new().spawn(move || match rx.recv_timeout(timeout) {
        Err(RecvTimeoutError::Timeout) => engine.increment_epoch(),
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
    })?;
    Ok(Watchdog {
        cancel: tx,
        handle: Some(handle),
    })
}

#[cfg(test)]
thread_local! {
    static FAIL_SPAWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileId, RuntimeConfig, compile_script};

    struct SpawnFailure(bool);

    impl SpawnFailure {
        fn inject() -> Self {
            Self(FAIL_SPAWN.replace(true))
        }
    }

    impl Drop for SpawnFailure {
        fn drop(&mut self) {
            FAIL_SPAWN.set(self.0);
        }
    }

    #[test]
    fn spawn_failure_preserves_io_cause_and_healthy_follow_up() {
        let config = RuntimeConfig {
            timeout: Some(Duration::from_secs(60)),
            ..Default::default()
        };
        let compiled = compile_script(
            "function main(): number { return 42; }",
            "watchdog.ts",
            FileId(0),
            &[],
            &[],
        )
        .unwrap();
        {
            let _failure = SpawnFailure::inject();
            let error = pollster::block_on(config.run_compiled(&compiled)).unwrap_err();
            assert_eq!(error.to_string(), "starting execution timeout watchdog");
            assert_eq!(
                error
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .raw_os_error(),
                Some(11)
            );
        }
        let result = pollster::block_on(config.run_compiled(&compiled)).unwrap();
        assert_eq!(result.value.as_deref(), Some("42"));
    }

    #[test]
    fn spawn_failure_precedes_guest_top_level_execution() {
        let config = RuntimeConfig {
            timeout: Some(Duration::from_secs(60)),
            ..Default::default()
        };
        let compiled = compile_script(
            "throw new Error(\"guest top level ran\"); function main(): void {}",
            "watchdog.ts",
            FileId(0),
            &[],
            &[],
        )
        .unwrap();
        let _failure = SpawnFailure::inject();
        let error = pollster::block_on(config.run(&compiled.wasm)).unwrap_err();
        assert!(error.is::<std::io::Error>());
        assert!(!format!("{error:#}").contains("guest top level ran"));
    }

    #[test]
    fn disabled_timeout_does_not_spawn() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let _failure = SpawnFailure::inject();
        assert!(config.arm_timeout(&engine).unwrap().is_none());
    }

    #[test]
    fn expired_watchdog_interrupts_guest() {
        let config = RuntimeConfig {
            timeout: Some(Duration::ZERO),
            ..Default::default()
        };
        let engine = config.engine().unwrap();
        let mut store = config.store(&engine, ()).unwrap();
        let mut watchdog = arm(&engine, Duration::ZERO).unwrap();
        watchdog.handle.take().unwrap().join().unwrap();
        let error = call_guest(&mut store).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(wasmtime::Trap::Interrupt)
        ));
    }

    #[test]
    fn dropped_watchdog_disarms_and_releases_thread() {
        let config = RuntimeConfig {
            timeout: Some(Duration::from_secs(3600)),
            ..Default::default()
        };
        let engine = config.engine().unwrap();
        let mut store = config.store(&engine, ()).unwrap();
        let mut watchdog = arm(&engine, config.timeout.unwrap()).unwrap();
        let handle = watchdog.handle.take().unwrap();
        drop(watchdog);
        handle.join().unwrap();
        call_guest(&mut store).unwrap();
    }

    fn call_guest(store: &mut wasmtime::Store<()>) -> wasmtime::Result<()> {
        use wasm_encoder::*;
        let mut module = wasm_encoder::Module::new();
        let mut types = TypeSection::new();
        types.ty().function([], []);
        module.section(&types);
        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);
        let mut exports = ExportSection::new();
        exports.export("run", ExportKind::Func, 0);
        module.section(&exports);
        let mut code = CodeSection::new();
        // The interpreter checks epochs at loop backedges, not every return.
        let mut function = Function::new([(1, ValType::I32)]);
        function.instruction(&Instruction::I32Const(1000));
        function.instruction(&Instruction::LocalSet(0));
        function.instruction(&Instruction::Loop(BlockType::Empty));
        function.instruction(&Instruction::LocalGet(0));
        function.instruction(&Instruction::I32Const(1));
        function.instruction(&Instruction::I32Sub);
        function.instruction(&Instruction::LocalTee(0));
        function.instruction(&Instruction::BrIf(0));
        function.instruction(&Instruction::End);
        function.instruction(&Instruction::End);
        code.function(&function);
        module.section(&code);
        let module = wasmtime::Module::new(store.engine(), module.finish())?;
        let instance = wasmtime::Instance::new(&mut *store, &module, &[])?;
        instance
            .get_typed_func::<(), ()>(&mut *store, "run")?
            .call(store, ())
    }
}
