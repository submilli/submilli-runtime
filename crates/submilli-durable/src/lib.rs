//! Durable filesystem effects with replay-supplied inputs.
//! Persistence is supplied by the embedding application.
pub mod recovery;
pub mod sealing;
pub mod unit_of_work;
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
pub mod codec;
pub mod effects;
pub mod filesystems;
pub mod journal;
pub mod state;

pub mod record_codec;
