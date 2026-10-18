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
    cipher: Arc<SecretCipher>,
) -> Result<DurableFilesystem, FilesystemError> {
    let units = crate::adapters::unit_of_work::durable::units(database, cipher);
    DurableFilesystem::open(vfs, run_id, units).await
}
#[cfg(test)]
mod tests;
