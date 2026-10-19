//! The server's [`SecretStore`]: operator secrets in the database, reached
//! through the secret use-cases. Programs resolve `store:` secrets and the
//! management API writes them through this one seam, which an embedder may
//! replace with its own store.
pub(crate) mod legacy;

use std::sync::Arc;

use submilli_shared::secret_store::{SecretStore, SecretStoreError};

use crate::application::error::StoreError;
use crate::application::secrets::SecretFailure;
use crate::application::secrets::list::ListSecretNames;
use crate::application::secrets::remove::RemoveSecret;
use crate::application::secrets::reveal::RevealSecret;
use crate::application::secrets::store::StoreSecret;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::secret::SecretValue;

pub(crate) struct DatabaseSecretStore {
    units: Arc<dyn UnitOfWorkFactory>,
}

impl DatabaseSecretStore {
    pub fn new(units: Arc<dyn UnitOfWorkFactory>) -> Self {
        Self { units }
    }
}

fn failure(error: SecretFailure) -> SecretStoreError {
    match error {
        SecretFailure::Rule(rule) => SecretStoreError::InvalidName(rule.to_string()),
        SecretFailure::Store(StoreError::Credentials(message)) => SecretStoreError::Crypto(message),
        SecretFailure::Store(store) => SecretStoreError::Io(store.to_string()),
    }
}

#[async_trait::async_trait]
impl SecretStore for DatabaseSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        RevealSecret::new(self.units.as_ref())
            .execute(key.to_owned())
            .await
            .map(|value| value.map(SecretValue::into_inner))
            .map_err(failure)
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        StoreSecret::new(self.units.as_ref())
            .execute(key.to_owned(), value.to_owned())
            .await
            .map_err(failure)
    }

    async fn delete(&self, key: &str) -> Result<bool, SecretStoreError> {
        RemoveSecret::new(self.units.as_ref())
            .execute(key.to_owned())
            .await
            .map_err(failure)
    }

    async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretStoreError> {
        ListSecretNames::new(self.units.as_ref())
            .execute(prefix)
            .await
            .map_err(failure)
    }
}
