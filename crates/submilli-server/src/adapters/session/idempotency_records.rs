use crate::application::error::StoreError;
use crate::application::sessions::ports::IdempotencyRecords;
use crate::idempotency_store::IdempotencyStore;

pub(crate) struct StoredIdempotencyRecords<'a>(pub &'a dyn IdempotencyStore);

#[async_trait::async_trait]
impl IdempotencyRecords for StoredIdempotencyRecords<'_> {
    async fn session_ids(&self) -> Result<Vec<String>, StoreError> {
        self.0.session_ids().await
    }
}
