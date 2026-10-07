//! The detached playground's stderr is its log. Every line goes through here: a
//! note as it is, a warning after `warning: `.

use std::io::{self, Write};

/// Logs something the playground did.
pub(crate) fn note(message: &str) {
    let _ = writeln!(io::stderr().lock(), "{message}");
}

/// Logs something that went wrong and that nothing else reports.
pub(crate) fn warn(message: &str) {
    let _ = writeln!(io::stderr().lock(), "warning: {message}");
}
