//! Persistence contract owned by durability, independent of a database driver.
use crate::state::*;
use futures::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("durable persistence failed: {0}")]
    Database(Box<dyn std::error::Error + Send + Sync>),
    #[error("durable persistence I/O failed: {0}")]
    Io(String),
    #[error("{0}")]
    Credentials(String),
    #[error("the durable record changed since it was read")]
    Conflict,
    #[error("the durable store is busy")]
    Busy,
}

/// Creates a fresh transaction. No server repository or connection escapes it.
pub type UnitFactory =
    Arc<dyn Fn() -> BoxFuture<'static, Result<Box<dyn UnitOfWork>, StoreError>> + Send + Sync>;

/// Reads see pending writes. Commit consumes the unit; dropping it discards
/// uncommitted changes. An uncertain commit must be resolved by rereading the
/// durable identity in a fresh unit, never by repeating an external mutation.
#[async_trait::async_trait]
pub trait UnitOfWork: Send {
    async fn filesystem_store_identity(&mut self) -> Result<Option<String>, StoreError>;
    async fn identify_filesystem_store(&mut self, identity: String) -> Result<(), StoreError>;
    async fn get_run(&mut self, run: &str) -> Result<Option<RunState>, StoreError>;
    async fn get_workspace(&mut self, id: &str) -> Result<Option<WorkspaceState>, StoreError>;
    async fn save_workspace(&mut self, workspace: WorkspaceState) -> Result<(), StoreError>;
    async fn get_effect(&mut self, run: &str, id: &str) -> Result<Option<Effect>, StoreError>;
    async fn save_effect(&mut self, effect: Effect) -> Result<(), StoreError>;
    async fn pending_effect(&mut self, run: &str) -> Result<Option<String>, StoreError>;
    async fn effect_count(&mut self, run: &str) -> Result<u64, StoreError>;
    async fn effects_after(
        &mut self,
        run: &str,
        operation: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Effect>, StoreError>;
    async fn get_plan(&mut self, workspace: &str, id: &str) -> Result<Option<Plan>, StoreError>;
    async fn save_plan(&mut self, workspace: &str, plan: Plan) -> Result<(), StoreError>;
    async fn pending_plan(&mut self, workspace: &str) -> Result<Option<Plan>, StoreError>;
    async fn plans_after(
        &mut self,
        workspace: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Plan>, StoreError>;
    async fn get_reader(&mut self, workspace: &str, id: &str)
    -> Result<Option<Reader>, StoreError>;
    async fn save_reader(&mut self, workspace: &str, reader: Reader) -> Result<(), StoreError>;
    async fn get_listing(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Listing>, StoreError>;
    async fn save_listing(&mut self, workspace: &str, listing: Listing) -> Result<(), StoreError>;
    async fn pending_resource(
        &mut self,
        workspace: &str,
    ) -> Result<Option<PendingResource>, StoreError>;
    async fn get_writer(&mut self, workspace: &str, id: &str)
    -> Result<Option<Writer>, StoreError>;
    async fn save_writer(&mut self, workspace: &str, writer: Writer) -> Result<(), StoreError>;
    async fn closing_writer(&mut self, workspace: &str) -> Result<Option<Writer>, StoreError>;
    async fn writer_for_effect(
        &mut self,
        workspace: &str,
        effect: &str,
    ) -> Result<Option<Writer>, StoreError>;
    async fn get_chunk(
        &mut self,
        workspace: &str,
        writer: &str,
        id: &str,
    ) -> Result<Option<Chunk>, StoreError>;
    async fn save_chunk(&mut self, workspace: &str, chunk: Chunk) -> Result<(), StoreError>;
    async fn writer_chunks(
        &mut self,
        workspace: &str,
        writer: &str,
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError>;
    async fn chunks_after(
        &mut self,
        workspace: &str,
        after: (&str, &str),
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError>;
    async fn resource_counts(&mut self, workspace: &str) -> Result<ResourceCounts, StoreError>;
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

#[cfg(test)]
mod tests;
