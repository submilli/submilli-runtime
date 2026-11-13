//! Running the compiler on a thread with the stack it needs.
//!
//! The compiler recurses with the program's nesting, so it needs
//! [`COMPILER_STACK_BYTES`] of stack. Every public entry point runs its work
//! through [`run`], so callers don't size stacks.

use std::cell::Cell;

use crate::compiler_error::{CompileError, CompilerFailure};
use crate::compiler_limits::COMPILER_STACK_BYTES;

thread_local! {
    static ON_COMPILER_STACK: Cell<bool> = const { Cell::new(false) };
}

/// Why [`on_compiler_stack`] has no result.
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
/// On a thread this started, `work` runs in place: the back end calling a
/// front end, which calls it again, stays on one thread.
pub fn on_compiler_stack<T: Send>(
    work: impl FnOnce() -> T + Send,
) -> Result<T, CompilerThreadError> {
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

/// [`on_compiler_stack`] for compiler work. A thread that can't start, or work
/// that panics, is [`CompilerFailure::Unavailable`].
pub fn run<T: Send>(
    work: impl FnOnce() -> Result<T, CompileError> + Send,
) -> Result<T, CompileError> {
    on_compiler_stack(work).map_err(|error| {
        CompileError::from(CompilerFailure::Unavailable {
            message: error.to_string(),
        })
    })?
}

/// Runs a test on a compiler thread. The compiler runs in place there, so
/// thread-local fault injection the test sets up reaches it.
#[cfg(test)]
pub(crate) fn on_test_stack<T: Send>(test: impl FnOnce() -> T + Send) -> T {
    on_compiler_stack(test).expect("the test thread starts and does not panic")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_on_one_compiler_thread_however_deep_the_calls() {
        let outer = std::thread::current().id();
        let (first, nested) = on_compiler_stack(|| {
            let first = std::thread::current().id();
            let nested = on_compiler_stack(|| std::thread::current().id()).unwrap();
            (first, nested)
        })
        .unwrap();
        assert_ne!(first, outer, "the work leaves the caller's thread");
        assert_eq!(nested, first, "a nested call stays on the compiler thread");
    }

    #[test]
    fn a_panic_is_reported_not_propagated() {
        let result = on_compiler_stack(|| -> () { panic!("injected compiler panic") });
        assert!(matches!(result, Err(CompilerThreadError::Panicked)));
        assert_eq!(on_compiler_stack(|| 1).unwrap(), 1);
    }

    #[test]
    fn a_compiler_that_cannot_run_is_unavailable_not_a_program_error() {
        let error =
            run(|| -> Result<(), CompileError> { panic!("injected compiler panic") }).unwrap_err();
        assert!(error.diagnostics.is_empty());
        assert!(error.unavailable().is_some(), "{error:?}");
    }
}
