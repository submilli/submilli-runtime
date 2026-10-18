//! Server composition for the idempotent filesystem journal.
use crate::database::ServerDatabase;
use interpreter::runtime::Vfs;
use interpreter::runtime::durable::RunId;
use std::sync::Arc;
pub use submilli_durable::{DurableFilesystem, FilesystemError};
use submilli_shared::secret_store::SecretCipher;

pub async fn open(
    vfs: Vfs,
    run_id: &RunId,
    database: Arc<ServerDatabase>,
    context: FilesystemContext,
) -> Result<DurableFilesystem, FilesystemError> {
    let units = context.units(database);
    DurableFilesystem::open(vfs, run_id, units, context.inner).await
}
#[cfg(test)]
mod tests;

/// Shared filesystem coordination and persistence composition. It owns no
/// payload directory and never serializes filesystem input bytes.
#[derive(Clone)]
pub struct FilesystemContext {
    inner: submilli_durable::filesystem::FilesystemContext,
    cipher: Arc<SecretCipher>,
}
impl FilesystemContext {
    pub fn new(cipher: Arc<SecretCipher>) -> Self {
        Self {
            inner: Default::default(),
            cipher,
        }
    }
    fn units(&self, database: Arc<ServerDatabase>) -> submilli_durable::UnitFactory {
        crate::adapters::unit_of_work::durable::units(database, self.cipher.clone())
    }
}
