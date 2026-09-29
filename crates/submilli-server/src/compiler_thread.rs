//! Compilation recurses with program nesting and needs the interpreter's
//! documented stack, several times a runtime worker's 2 MiB. Each compile runs
//! on its own short-lived thread of that size, so the reservation is released
//! when the compile ends. SUB-1123 tracks pooling these threads.

use interpreter::compiler_limits::COMPILER_STACK_BYTES;

#[derive(Debug)]
pub(crate) enum CompilerThreadError {
    Spawn(std::io::Error),
    Panicked,
}

impl std::fmt::Display for CompilerThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "cannot start the compiler thread: {error}"),
            Self::Panicked => f.write_str("the compiler thread panicked"),
        }
    }
}

impl std::error::Error for CompilerThreadError {}

/// Runs `work` on a compiler-sized thread and waits for it, blocking the caller.
pub(crate) fn run<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T, CompilerThreadError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("submilli-compile".into())
            .stack_size(COMPILER_STACK_BYTES)
            .spawn_scoped(scope, work)
            .map_err(CompilerThreadError::Spawn)?
            .join()
            .map_err(|_| CompilerThreadError::Panicked)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_compile_at_the_limits_fits_from_a_worker_sized_caller() {
        let source = format!(
            "function main(): number {{ const a = 1; const s = `{}`; return s.length; }}",
            "${a}x".repeat(510)
        );
        let compiled = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                run(|| {
                    interpreter::compile_script(
                        &source,
                        "limits.ts",
                        interpreter::FileId(0),
                        &[],
                        &[],
                    )
                    .is_ok()
                })
                .unwrap()
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(compiled);
    }

    #[test]
    fn a_panicking_compile_is_reported_not_propagated() {
        let result = run(|| -> () { panic!("injected compiler panic") });
        assert!(matches!(result, Err(CompilerThreadError::Panicked)));
    }
}
