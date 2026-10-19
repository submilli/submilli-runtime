use super::GrantFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;

/// Forget a blueprint's grant for an MCP server; whether one was on file.
pub(crate) struct RevokeGrant<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> RevokeGrant<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(&self, blueprint: &str, server: &str) -> Result<bool, GrantFailure> {
        let mut unit = self.units.begin().await?;
        let revoked = unit.remove_grant(blueprint, server).await?;
        unit.commit().await?;
        Ok(revoked)
    }
}
