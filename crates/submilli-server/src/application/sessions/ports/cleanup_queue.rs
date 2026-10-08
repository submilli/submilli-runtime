use crate::application::error::StoreError;
use std::path::PathBuf;

#[async_trait::async_trait]
pub(crate) trait SessionCleanupQueue: Send + Sync {
    async fn contains(&self, id: &str) -> Result<bool, StoreError>;
    async fn enqueue(&self, task: SessionCleanup) -> Result<(), StoreError>;
}

/// Work owned by the application after closure. Orphans have no session row.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionCleanup {
    pub session_id: String,
    pub folder: Option<PathBuf>,
}
