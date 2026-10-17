use super::*;
use tokio::sync::{Mutex, RwLock};

/// One context per local SQLite store, shared by every workspace opener.
/// It coordinates same-process workers; it is not a multi-node lease.
#[derive(Clone)]
pub struct FilesystemContext {
    identity: String,
    pub(super) filesystem_gate: Arc<Mutex<()>>,
    pub(super) filesystem_resources: Arc<RwLock<()>>,
    pub(super) filesystem_quotas: Arc<Mutex<quotas::SharedQuotas>>,
}
impl FilesystemContext {
    pub async fn open(units: UnitFactory) -> Result<Self, FilesystemError> {
        let mut unit = units().await?;
        let identity = if let Some(identity) = unit.filesystem_store_identity().await? {
            identity
        } else {
            let mut bytes = [0; 16];
            getrandom::getrandom(&mut bytes).map_err(|error| StoreError::Io(error.to_string()))?;
            let identity = uuid::Builder::from_random_bytes(bytes)
                .into_uuid()
                .to_string();
            unit.identify_filesystem_store(identity.clone()).await?;
            identity
        };
        validate_id(&identity)?;
        unit.commit().await?;
        Ok(Self {
            identity,
            filesystem_gate: Arc::default(),
            filesystem_resources: Arc::default(),
            filesystem_quotas: Arc::default(),
        })
    }
    pub fn store_identity(&self) -> &str {
        &self.identity
    }
    #[cfg(feature = "test-support")]
    pub fn test_gate(&self) -> &Arc<Mutex<()>> {
        &self.filesystem_gate
    }
}
