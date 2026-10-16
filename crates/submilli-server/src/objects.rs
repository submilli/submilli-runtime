//! Server composition and configuration for the durability crate.
use crate::database::ServerDatabase;
use std::{path::PathBuf, sync::Arc};
use submilli_durable::object::ObjectId;
pub use submilli_durable::objects::{
    ObjectLimits, ObjectRecovery, ObjectStoreError, PreparedObject, PreparedPayload, StoredPayload,
};
use submilli_shared::secret_store::SecretCipher;

pub use submilli_durable::objects::{DEFAULT_INLINE_BYTES, MAX_INLINE_BYTES};

#[derive(Clone, Debug)]
pub struct DurableObjectConfig {
    /// Must already exist when explicitly configured.
    pub directory: PathBuf,
    pub inline_bytes: usize,
    pub limits: ObjectLimits,
}

impl Default for DurableObjectConfig {
    fn default() -> Self {
        Self {
            directory: crate::config::default_server_root().join("objects"),
            inline_bytes: DEFAULT_INLINE_BYTES,
            limits: ObjectLimits::default(),
        }
    }
}

impl DurableObjectConfig {
    pub fn validate(&self) -> Result<(), ObjectStoreError> {
        self.limits.validate()?;
        if self.inline_bytes > MAX_INLINE_BYTES {
            return Err(ObjectStoreError::Configuration(
                "inline threshold exceeds 1 MiB".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DurableObjects {
    inner: submilli_durable::objects::DurableObjects,
    cipher: Arc<SecretCipher>,
}
impl std::ops::Deref for DurableObjects {
    type Target = submilli_durable::objects::DurableObjects;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DurableObjects {
    pub async fn open(
        config: DurableObjectConfig,
        database: Arc<ServerDatabase>,
        cipher: Arc<SecretCipher>,
    ) -> Result<Self, ObjectStoreError> {
        if database.is_ephemeral() {
            return Err(ObjectStoreError::Configuration(
                "durable objects require a persistent database".into(),
            ));
        }
        let units = crate::adapters::unit_of_work::durable::units(database, cipher.clone());
        let config = submilli_durable::objects::DurableObjectConfig {
            directory: config.directory,
            inline_bytes: config.inline_bytes,
            limits: config.limits,
        };
        submilli_durable::objects::DurableObjects::open(config, units, cipher.clone())
            .await
            .map(|inner| Self { inner, cipher })
    }
    pub(crate) fn inner(&self) -> &submilli_durable::objects::DurableObjects {
        &self.inner
    }
    pub(crate) fn units(&self, database: Arc<ServerDatabase>) -> submilli_durable::UnitFactory {
        crate::adapters::unit_of_work::durable::units(database, self.cipher.clone())
    }
    pub fn new_id() -> Result<ObjectId, ObjectStoreError> {
        submilli_durable::objects::DurableObjects::new_id()
    }
}
#[cfg(test)]
pub(crate) mod tests;
