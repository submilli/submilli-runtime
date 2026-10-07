//! Transactional queries and writes for application use cases.
use crate::application::error::StoreError;
use crate::domain::session::Session;

#[async_trait::async_trait]
pub(crate) trait UnitOfWorkFactory: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn UnitOfWork>, StoreError>;
}

/// Reads see this unit's pending writes. Commit consumes the unit; dropping it
/// discards uncommitted work. Compatibility adapters document weaker commit
/// guarantees. Do not open another database operation while this scope holds
/// the transaction's connection.
#[async_trait::async_trait]
pub(crate) trait UnitOfWork: Send {
    async fn get_session(&mut self, id: &str) -> Result<Option<Session>, StoreError>;
    async fn list_sessions(&mut self) -> Result<Vec<Session>, StoreError>;
    async fn sessions_for_blueprint(&mut self, name: &str) -> Result<Vec<Session>, StoreError>;
    async fn save_session(&mut self, session: Session) -> Result<(), StoreError>;
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError>;
    async fn remove_blueprint(&mut self, name: &str) -> Result<bool, StoreError>;
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}
