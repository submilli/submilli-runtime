//! Labels of the runs the playground starts itself (KTD5). A run another client sends
//! is labeled by its token's name (`app`, `stand-in`, `chat`, `browser-chat`); these
//! name the playground's own, through `run_program` and `test_program`.

#![allow(dead_code, reason = "used by the controls that start runs (U10, U17)")]

/// A program the coding assistant ran with `exec`.
pub(crate) const ASSISTANT: &str = "assistant";
/// An example program run from the page.
pub(crate) const EXAMPLE: &str = "example";
/// A recorded run tested again with `test`.
pub(crate) const TEST: &str = "test";
/// A recorded program run again live with `rerun`.
pub(crate) const RERUN: &str = "rerun";

/// Every label the playground gives its own runs.
pub(crate) const ALL: [&str; 4] = [ASSISTANT, EXAMPLE, TEST, RERUN];
