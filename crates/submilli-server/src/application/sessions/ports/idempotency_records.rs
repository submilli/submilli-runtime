use crate::application::error::StoreError;

#[async_trait::async_trait]
pub(crate) trait IdempotencyRecords: Send + Sync {
    async fn session_ids(&self) -> Result<Vec<String>, StoreError>;
}
