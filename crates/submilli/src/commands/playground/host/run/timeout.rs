//! Per-store deadlines on the host's shared engine clock.
//!
//! Copied from `submilli-server`'s `execution_timeout.rs`.
//!
//! A run's time limit counts its own work. Time spent waiting outside the
//! program on a host call that pauses the clock (an application tool call,
//! [`StoreData::clock_paused`](interpreter::runtime::StoreData::clock_paused))
//! does not count.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use wasmtime::{Engine, Store, UpdateDeadline};

const TICK: Duration = Duration::from_secs(1);

pub(super) fn start_ticker(engine: &Engine) -> std::io::Result<JoinHandle<()>> {
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
pub(super) fn arm<T: 'static>(
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
mod tests;
