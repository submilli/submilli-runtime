//! Helpers for this crate's tests.
#![allow(clippy::expect_used)]

/// Runs `test` on a thread of the stack compilation is given: substitution
/// and drop recurse once per level of a type at the depth limit.
pub(crate) fn on_compiler_stack<T: Send>(test: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(crate::compiler_limits::COMPILER_STACK_BYTES)
            .spawn_scoped(scope, test)
            .expect("spawn a test thread")
            .join()
            .expect("the test thread does not panic")
    })
}
