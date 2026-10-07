use crate::application::error::StoreError;
use crate::domain::session::Session;

/// Independent access to committed session aggregates.
#[async_trait::async_trait]
pub(crate) trait SessionRepository: Send + Sync {
    async fn get(&self, id: &str) -> Result<Option<Session>, StoreError>;
}
