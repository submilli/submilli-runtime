//! Idempotent filesystem operations with application-owned persistence.
pub mod codec;
pub mod filesystem;
mod io;
mod readers;
pub mod unit_of_work;
mod writers;
pub use filesystem::{DurableFilesystem, FilesystemContext, FilesystemError};
pub use io::RecoveryError;
pub use readers::{DirectoryEntry, FilesystemReader, ReaderPosition};
pub use unit_of_work::{UnitFactory, UnitOfWork};
#[cfg(feature = "test-support")]
static CHECKPOINT: std::sync::OnceLock<fn(&str)> = std::sync::OnceLock::new();
#[cfg(feature = "test-support")]
pub fn set_test_checkpoint(callback: fn(&str)) {
    let _ = CHECKPOINT.set(callback);
}
pub(crate) fn checkpoint(_point: &str) {
    #[cfg(feature = "test-support")]
    if let Some(callback) = CHECKPOINT.get() {
        callback(_point);
    }
}
