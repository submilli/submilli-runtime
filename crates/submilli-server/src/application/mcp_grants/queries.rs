use super::GrantFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::mcp_grant::{GrantOverview, GrantVersion, McpGrant};

/// A blueprint's grant for an MCP server, if one is on file.
pub(crate) struct GetGrant<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> GetGrant<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<McpGrant>, GrantFailure> {
        let mut unit = self.units.begin().await?;
        Ok(unit.get_grant(blueprint, server).await?)
    }
}

/// The version of a blueprint's grant for an MCP server, if one is on file,
/// without opening its token: the cheap check a cached access token is held
/// against.
pub(crate) struct CurrentGrantVersion<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> CurrentGrantVersion<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<GrantVersion>, GrantFailure> {
        let mut unit = self.units.begin().await?;
        Ok(unit.grant_version(blueprint, server).await?)
    }
}

/// The grants on file, for one blueprint or all of them, without their tokens.
pub(crate) struct ListGrants<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> ListGrants<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(
        &self,
        blueprint: Option<&str>,
    ) -> Result<Vec<GrantOverview>, GrantFailure> {
        let mut unit = self.units.begin().await?;
        Ok(unit.list_grants(blueprint).await?)
    }
}
