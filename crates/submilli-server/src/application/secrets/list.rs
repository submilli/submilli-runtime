use super::SecretFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;

/// The names of the stored secrets, never their values.
pub(crate) struct ListSecretNames<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> ListSecretNames<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretFailure> {
        let mut unit = self.units.begin().await?;
        Ok(unit.secret_names(prefix).await?)
    }
}
