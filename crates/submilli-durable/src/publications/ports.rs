use super::ObjectStoreError;
use crate::object::{ContentIdentity, ObjectId};
use async_trait::async_trait;
use std::io::{Read, Write};

pub type PayloadSource = Box<dyn Read + Send>;
pub type PayloadTarget = Box<dyn Write + Send>;

pub struct PublishedContent {
    pub identity: ContentIdentity,
    pub stored_bytes: u64,
}

/// Access protects publication, readers, abandonment, and collection for the
/// entire operation, including outstanding I/O after caller cancellation.
#[async_trait]
pub(crate) trait ObjectStorage: Send + Sync {
    fn maximum_stored_bytes(&self, plaintext_bytes: u64) -> Result<u64, ObjectStoreError>;
    async fn access(&self, id: &ObjectId) -> Result<Box<dyn ObjectAccess>, ObjectStoreError>;
}

#[async_trait]
pub(crate) trait ObjectAccess: Send {
    async fn publish(
        &mut self,
        source: PayloadSource,
        maximum_bytes: u64,
    ) -> Result<PublishedContent, ObjectStoreError>;
    async fn inspect(
        &mut self,
        maximum_bytes: u64,
    ) -> Result<Option<PublishedContent>, ObjectStoreError>;
    async fn copy_into(
        &mut self,
        content: ContentIdentity,
        target: PayloadTarget,
    ) -> Result<PayloadTarget, ObjectStoreError>;
    async fn remove(&mut self) -> Result<(), ObjectStoreError>;
}
