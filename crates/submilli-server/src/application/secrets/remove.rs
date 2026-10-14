use super::SecretFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::secret::SecretName;

/// Remove a secret; whether one was stored under the name.
pub(crate) struct RemoveSecret<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> RemoveSecret<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(&self, name: String) -> Result<bool, SecretFailure> {
        let name = SecretName::parse(name)?;
        let mut unit = self.units.begin().await?;
        let removed = unit.remove_secret(&name).await?;
        unit.commit().await?;
        Ok(removed)
    }
}
