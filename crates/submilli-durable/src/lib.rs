//! Filesystem operations that can safely be called again with the same effect ID.
//!
//! [`history::History`] records ordered native outcomes and decides which calls
//! to retry. The filesystem journal stays independent: this crate
//! remembers enough about filesystem work to recognize completed calls and finish
//! interrupted calls without applying their mutation twice. Opening a filesystem
//! does not scan for unfinished work or resume a run.
//!
//! Start with [`DurableFilesystem`] for shared mounts, journal access, and local
//! coordination. Its `operation(work)` method creates a
//! [`filesystem::FilesystemOperation`] for one call's fuel budget. Read
//! `filesystem/append_bytes.rs` to follow a saved-offset state machine. Other operation
//! files sit beside it, with their state types and encoders/decoders. `readers.rs`
//! and `writers.rs` adapt resource handles and share native resource access.
//!
//! [`unit_of_work`] defines the journal records and transaction contract; the
//! embedding application implements it. [`codec`] handles their stored encoding.
//! `io.rs` provides contained paths, private staging files, and disk accounting.
//!
//! A run ID separates runs. An effect ID identifies one logical call within that
//! run: retries reuse it and distinct calls use different IDs. Resource handles
//! use their opening effect's ID. Input bytes are supplied again by the caller;
//! the journal stores their fingerprints, not a second copy of the inputs.
pub mod codec;
pub mod filesystem;
pub mod history;
mod io;
mod readers;
pub mod unit_of_work;
mod writers;
pub use filesystem::{DurableFilesystem, FilesystemError};
pub use io::{RecoveryError, filesystem_config_fingerprint, retire_filesystem};
pub use readers::{DirectoryEntry, ReaderPosition};
pub use unit_of_work::{UnitFactory, UnitOfWork};
#[cfg(feature = "test-support")]
static CHECKPOINT: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();
#[cfg(feature = "test-support")]
pub fn set_test_checkpoint(callback: fn(&str)) {
    let _ = CHECKPOINT.set(callback);
}
/// Test hook for crash injection; does not save recovery state.
pub(crate) fn test_checkpoint(_point: &str) {
    #[cfg(feature = "test-support")]
    if let Some(callback) = CHECKPOINT.get() {
        callback(_point);
    }
}
