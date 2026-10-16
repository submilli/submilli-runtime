//! Server composition for durable workspace storage.
use crate::{database::ServerDatabase, objects::DurableObjects};
use interpreter::runtime::Vfs;
use std::sync::Arc;
pub use submilli_durable::filesystems::{
    Content, DirectorySnapshot, FilesystemError, FilesystemReader, Mutation, Outcome, RecoveryError,
};

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
        objects: DurableObjects,
    ) -> Result<Self, FilesystemError> {
        let units = objects.units(database);
        submilli_durable::filesystems::DurableFilesystem::open(
            vfs,
            run,
            owner,
            units,
            objects.inner().clone(),
        )
        .await
        .map(Self)
    }
    pub async fn retained_ephemeral(
        parent: &std::path::Path,
        run: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        objects: DurableObjects,
    ) -> Result<Self, FilesystemError> {
        let units = objects.units(database);
        submilli_durable::filesystems::DurableFilesystem::retained_ephemeral(
            parent,
            run,
            owner,
            units,
            objects.inner().clone(),
        )
        .await
        .map(Self)
    }
    pub async fn resume(
        workspace: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        objects: DurableObjects,
    ) -> Result<Self, FilesystemError> {
        let units = objects.units(database);
        submilli_durable::filesystems::DurableFilesystem::resume(
            workspace,
            owner,
            units,
            objects.inner().clone(),
        )
        .await
        .map(Self)
    }
    pub async fn recover_cleanup(
        workspace: &str,
        owner: &str,
        database: Arc<ServerDatabase>,
        objects: DurableObjects,
    ) -> Result<(), FilesystemError> {
        let units = objects.units(database);
        submilli_durable::filesystems::DurableFilesystem::recover_cleanup(
            workspace,
            owner,
            units,
            objects.inner().clone(),
        )
        .await
    }
}
#[cfg(test)]
mod tests;
