use std::time::SystemTime;

use super::SecretFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::secret::{Secret, SecretName, SecretValue};

/// Create a secret, or replace its value.
pub(crate) struct StoreSecret<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> StoreSecret<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(&self, name: String, value: String) -> Result<(), SecretFailure> {
        let secret = Secret::new(SecretName::parse(name)?, SecretValue::new(value));
        let mut unit = self.units.begin().await?;
        unit.save_secret(secret, SystemTime::now()).await?;
        unit.commit().await?;
        Ok(())
    }
}
