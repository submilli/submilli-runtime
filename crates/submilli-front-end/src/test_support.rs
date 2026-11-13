//! Helpers for this crate's and the engine's tests.
#![allow(clippy::expect_used)]

/// Runs `test` on a thread of the stack compilation is given: substitution
/// and drop recurse once per level of a type at the depth limit. The compiler
/// runs in place there ([`crate::compiler_thread::run`]), so thread-local
/// fault injection the test sets up reaches it.
pub fn on_compiler_stack<T: Send>(test: impl FnOnce() -> T + Send) -> T {
    crate::compiler_thread::run(test).expect("the test thread starts and does not panic")
}
