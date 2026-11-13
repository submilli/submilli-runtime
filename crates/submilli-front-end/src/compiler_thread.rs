//! Running the compiler on a thread with the stack it needs.
//!
//! The compiler recurses with the program's nesting, so it needs
//! [`COMPILER_STACK_BYTES`] of stack. A front end, and the engine's back end,
//! run their work through [`run`], so callers don't size stacks.

use std::cell::Cell;

use crate::compiler_limits::COMPILER_STACK_BYTES;

thread_local! {
    static ON_COMPILER_STACK: Cell<bool> = const { Cell::new(false) };
}

/// Why [`run`] has no result.
#[derive(Debug)]
pub enum CompilerThreadError {
    /// The thread could not start.
    Spawn(std::io::Error),
    /// The work panicked.
    Panicked,
}

impl std::fmt::Display for CompilerThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "could not start the compiler: {error}"),
            Self::Panicked => f.write_str("the compiler panicked"),
        }
    }
}

impl std::error::Error for CompilerThreadError {}

/// Runs `work` on a thread with the compiler's stack and returns its result.
///
/// On a thread [`run`] started, `work` runs in place: the back end calling a
/// front end, which calls [`run`] again, stays on one thread.
pub fn run<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T, CompilerThreadError> {
    if ON_COMPILER_STACK.get() {
        return Ok(work());
    }
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("submilli-compile".into())
            .stack_size(COMPILER_STACK_BYTES)
            .spawn_scoped(scope, || {
                ON_COMPILER_STACK.set(true);
                work()
            })
            .map_err(CompilerThreadError::Spawn)?
            .join()
            .map_err(|_| CompilerThreadError::Panicked)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_on_one_compiler_thread_however_deep_the_calls() {
        let outer = std::thread::current().id();
        let (first, nested) = run(|| {
            let first = std::thread::current().id();
            let nested = run(|| std::thread::current().id()).unwrap();
            (first, nested)
        })
        .unwrap();
        assert_ne!(first, outer, "the work leaves the caller's thread");
        assert_eq!(nested, first, "a nested call stays on the compiler thread");
    }

    #[test]
    fn a_panic_is_reported_not_propagated() {
        let result = run(|| -> () { panic!("injected compiler panic") });
        assert!(matches!(result, Err(CompilerThreadError::Panicked)));
        // The caller's thread can compile again.
        assert_eq!(run(|| 1).unwrap(), 1);
    }
}
