use std::time::SystemTime;

use super::GrantFailure;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::mcp_grant::{GrantCredential, GrantVersion, McpGrant};

/// A login for a blueprint's MCP server: the first grant, or one replacing
/// whatever was on file.
pub(crate) struct DepositGrant<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> DepositGrant<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(
        &self,
        blueprint: &str,
        server: &str,
        credential: GrantCredential,
        scopes: Vec<String>,
    ) -> Result<GrantVersion, GrantFailure> {
        let login_id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now();
        let mut unit = self.units.begin().await?;
        if !unit.blueprint_exists(blueprint).await? {
            return Err(GrantFailure::UnknownBlueprint(blueprint.to_owned()));
        }
        // A new login replaces whatever is on file without opening it, so a
        // grant sealed under a lost key can still be replaced.
        let grant = match unit.grant_version(blueprint, server).await? {
            Some(previous) => McpGrant::supersede(
                previous,
                blueprint.to_owned(),
                server.to_owned(),
                login_id,
                credential,
                scopes,
                now,
            )?,
            None => McpGrant::deposit(
                blueprint.to_owned(),
                server.to_owned(),
                login_id,
                credential,
                scopes,
                now,
            )?,
        };
        let version = grant.version();
        unit.save_grant(grant).await?;
        unit.commit().await?;
        Ok(version)
    }
}
