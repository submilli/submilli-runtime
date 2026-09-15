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

pub fn arm(engine: &Engine, timeout: Duration) -> Watchdog {
    let (tx, rx) = mpsc::channel::<()>();
    let engine = engine.clone();
    let handle = thread::spawn(move || match rx.recv_timeout(timeout) {
        Err(RecvTimeoutError::Timeout) => engine.increment_epoch(),
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
    });
    Watchdog {
        cancel: tx,
        handle: Some(handle),
    }
}
