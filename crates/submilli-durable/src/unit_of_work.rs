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
    async fn get_operation(
        &mut self,
        scope: &str,
        id: &str,
    ) -> Result<Option<Operation>, StoreError>;
    async fn save_operation(&mut self, effect: Operation) -> Result<(), StoreError>;
    async fn pending_operation(&mut self, scope: &str) -> Result<Option<String>, StoreError>;
    async fn operation_count(&mut self, scope: &str) -> Result<u64, StoreError>;

    async fn get_reader(&mut self, scope: &str, id: &str) -> Result<Option<Reader>, StoreError>;
    async fn save_reader(&mut self, scope: &str, reader: Reader) -> Result<(), StoreError>;
    /// Encoded snapshot size, without loading or decoding its payload.
    async fn listing_bytes(&mut self, scope: &str, id: &str) -> Result<u64, StoreError>;
    async fn get_listing(&mut self, scope: &str, id: &str) -> Result<Option<Listing>, StoreError>;
    async fn save_listing(&mut self, scope: &str, listing: Listing) -> Result<(), StoreError>;
    async fn pending_resource(
        &mut self,
        scope: &str,
    ) -> Result<Option<PendingResource>, StoreError>;
    async fn get_writer(&mut self, scope: &str, id: &str) -> Result<Option<Writer>, StoreError>;
    async fn save_writer(&mut self, scope: &str, writer: Writer) -> Result<(), StoreError>;
    async fn closing_writer(&mut self, scope: &str) -> Result<Option<Writer>, StoreError>;
    async fn writer_for_effect(
        &mut self,
        scope: &str,
        effect: &str,
    ) -> Result<Option<Writer>, StoreError>;
    async fn get_chunk(
        &mut self,
        scope: &str,
        writer: &str,
        id: &str,
    ) -> Result<Option<Chunk>, StoreError>;
    async fn save_chunk(&mut self, scope: &str, chunk: Chunk) -> Result<(), StoreError>;
    async fn writer_chunks(
        &mut self,
        scope: &str,
        writer: &str,
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError>;

    async fn resource_counts(&mut self, scope: &str) -> Result<ResourceCounts, StoreError>;
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

#[cfg(test)]
mod tests;
