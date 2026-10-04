//! Server logfmt records and their shared, reopenable output.

mod format;
mod output;

pub use format::{Logfmt, LogfmtFields, Stream, encode_record};
pub use output::{LogOutput, ReopenTask};
