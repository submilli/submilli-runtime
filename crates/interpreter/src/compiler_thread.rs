//! The thread the compiler runs on: [`submilli_front_end::compiler_thread`],
//! with its failures as compile errors.

use crate::compiler_error::{CompileError, CompilerFailure};

pub use submilli_front_end::compiler_thread::{CompilerThreadError, run as on_compiler_stack};

/// Runs `work` on a thread with the compiler's stack, or in place on one
/// already. A thread that can't start, or work that panics, is
/// [`CompilerFailure::Unavailable`].
pub fn run<T: Send>(
    work: impl FnOnce() -> Result<T, CompileError> + Send,
) -> Result<T, CompileError> {
    submilli_front_end::compiler_thread::run(work).map_err(|error| {
        CompileError::from(CompilerFailure::Unavailable {
            message: error.to_string(),
        })
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_compiler_that_cannot_run_is_unavailable_not_a_program_error() {
        let error =
            run(|| -> Result<(), CompileError> { panic!("injected compiler panic") }).unwrap_err();
        assert!(error.diagnostics.is_empty());
        assert!(error.unavailable().is_some(), "{error:?}");
    }
}
