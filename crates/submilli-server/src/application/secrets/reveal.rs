use super::SecretFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::secret::{SecretName, SecretValue};

/// The value of a secret, for a program whose blueprint declares it. The
/// capability check and redaction happen where the program asks for it.
pub(crate) struct RevealSecret<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> RevealSecret<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    /// `None` when nothing is stored under `name`, including a name no secret
    /// can have.
    pub async fn execute(&self, name: String) -> Result<Option<SecretValue>, SecretFailure> {
        let Ok(name) = SecretName::parse(name) else {
            return Ok(None);
        };
        let mut unit = self.units.begin().await?;
        Ok(unit
            .get_secret(&name)
            .await?
            .map(crate::domain::secret::Secret::into_value))
    }
}
