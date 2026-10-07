use super::ports::{MCPCatalog, MCPServer, PreparedPackages};
use crate::application::error::StoreError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::session::{ClosedReason, SessionStatus};

#[derive(Debug, thiserror::Error)]
pub(crate) enum RemoveBlueprintError {
    #[error(transparent)]
    Storage(#[from] StoreError),
}

pub(crate) struct RemoveBlueprint<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    mcp_server: &'a dyn MCPServer,
    mcp_catalog: &'a dyn MCPCatalog,
    prepared_packages: &'a dyn PreparedPackages,
}

impl<'a> RemoveBlueprint<'a> {
    pub fn new(
        unit_of_work: &'a dyn UnitOfWorkFactory,
        mcp_server: &'a dyn MCPServer,
        mcp_catalog: &'a dyn MCPCatalog,
        prepared_packages: &'a dyn PreparedPackages,
    ) -> Self {
        Self {
            unit_of_work,
            mcp_server,
            mcp_catalog,
            prepared_packages,
        }
    }

    pub async fn execute(&self, name: &str) -> Result<bool, RemoveBlueprintError> {
        let mut unit = self.unit_of_work.begin().await?;
        if !unit.blueprint_exists(name).await? {
            return Ok(false);
        }
        let sessions = unit.sessions_for_blueprint(name).await?;
        for mut session in sessions {
            if matches!(session.status(), SessionStatus::Closed(_)) {
                continue;
            }
            session.close(ClosedReason::BlueprintRemoved);
            unit.save_session(session).await?;
        }
        if !unit.remove_blueprint(name).await? {
            return Ok(false);
        }
        unit.commit().await?;
        self.mcp_server.evict(name);
        self.mcp_catalog.evict(name);
        self.prepared_packages.evict(name);
        Ok(true)
    }
}
