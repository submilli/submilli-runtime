use crate::application::error::StoreError;
use crate::application::sessions::ports::{SessionCleanup, SessionCleanupQueue};
use crate::session_store::DurableSessionStore;

pub(crate) struct StoredSessionCleanupQueue<'a>(pub &'a dyn DurableSessionStore);

#[async_trait::async_trait]
impl SessionCleanupQueue for StoredSessionCleanupQueue<'_> {
    async fn contains(&self, id: &str) -> Result<bool, StoreError> {
        Ok(self.0.cleanup_task(id).await?.is_some())
    }
    async fn enqueue(&self, task: SessionCleanup) -> Result<(), StoreError> {
        self.0.schedule_cleanup(task).await
    }
}
