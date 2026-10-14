//! Transactional queries and writes for application use cases.
use crate::application::error::StoreError;
use crate::domain::session::Session;
use std::time::SystemTime;

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
    async fn sessions_due_for_expiry(
        &mut self,
        now: SystemTime,
    ) -> Result<Vec<Session>, StoreError>;
    async fn list_sessions(&mut self) -> Result<Vec<Session>, StoreError>;
    async fn sessions_for_blueprint(&mut self, name: &str) -> Result<Vec<Session>, StoreError>;
    async fn save_session(&mut self, session: Session) -> Result<(), StoreError>;
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError>;
    async fn remove_blueprint(&mut self, name: &str) -> Result<bool, StoreError>;
    async fn get_request(
        &mut self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<crate::domain::idempotent_request::IdempotentRequest>, StoreError>;
    async fn save_request(
        &mut self,
        request: crate::domain::idempotent_request::IdempotentRequest,
    ) -> Result<(), StoreError>;
    async fn remove_request(&mut self, session_id: &str, key: &str) -> Result<(), StoreError>;
    async fn remove_session_requests(&mut self, session_id: &str) -> Result<(), StoreError>;
    async fn unfinished_requests(
        &mut self,
    ) -> Result<Vec<crate::domain::idempotent_request::IdempotentRequest>, StoreError>;
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}
