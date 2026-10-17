//! Server composition for durable workspace storage.
use crate::database::ServerDatabase;
use interpreter::runtime::Vfs;
use std::sync::Arc;
pub use submilli_durable::filesystems::{
    Content, DirectorySnapshot, FilesystemError, FilesystemReader, Mutation, Outcome,
    RecoveryError, ReplayInput,
};
use submilli_shared::secret_store::SecretCipher;

#[derive(Clone)]
pub struct DurableFilesystem(submilli_durable::filesystems::DurableFilesystem);
impl std::ops::Deref for DurableFilesystem {
    type Target = submilli_durable::filesystems::DurableFilesystem;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DurableFilesystem {
    pub async fn open(
        vfs: Vfs,
        run: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        context: FilesystemContext,
    ) -> Result<Self, FilesystemError> {
        let units = context.units(database);
        submilli_durable::filesystems::DurableFilesystem::open(
            vfs,
            run,
            owner,
            units,
            context.inner.clone(),
        )
        .await
        .map(Self)
    }
    pub async fn retained_ephemeral(
        parent: &std::path::Path,
        run: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        context: FilesystemContext,
    ) -> Result<Self, FilesystemError> {
        let units = context.units(database);
        submilli_durable::filesystems::DurableFilesystem::retained_ephemeral(
            parent,
            run,
            owner,
            units,
            context.inner.clone(),
        )
        .await
        .map(Self)
    }
    pub async fn resume(
        workspace: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        context: FilesystemContext,
    ) -> Result<Self, FilesystemError> {
        let units = context.units(database);
        submilli_durable::filesystems::DurableFilesystem::resume(
            workspace,
            owner,
            units,
            context.inner.clone(),
        )
        .await
        .map(Self)
    }
    pub async fn recover_cleanup(
        workspace: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        context: FilesystemContext,
    ) -> Result<(), FilesystemError> {
        let units = context.units(database);
        submilli_durable::filesystems::DurableFilesystem::recover_cleanup(
            workspace,
            owner,
            units,
            context.inner.clone(),
        )
        .await
    }
}
#[cfg(test)]
mod tests;

/// Shared filesystem coordination and persistence composition. It owns no
/// payload directory and never serializes filesystem input bytes.
#[derive(Clone)]
pub struct FilesystemContext {
    inner: submilli_durable::filesystems::FilesystemContext,
    cipher: Arc<SecretCipher>,
}
impl FilesystemContext {
    pub async fn open(
        database: Arc<ServerDatabase>,
        cipher: Arc<SecretCipher>,
    ) -> Result<Self, FilesystemError> {
        let units = crate::adapters::unit_of_work::durable::units(database, cipher.clone());
        let inner = submilli_durable::filesystems::FilesystemContext::open(units).await?;
        Ok(Self { inner, cipher })
    }
    fn units(&self, database: Arc<ServerDatabase>) -> submilli_durable::UnitFactory {
        crate::adapters::unit_of_work::durable::units(database, self.cipher.clone())
    }
    #[cfg(test)]
    fn test_gate(&self) -> &Arc<tokio::sync::Mutex<()>> {
        self.inner.test_gate()
    }
    pub fn store_identity(&self) -> &str {
        self.inner.store_identity()
    }
}
