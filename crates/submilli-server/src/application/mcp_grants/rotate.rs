use std::time::SystemTime;

use super::GrantFailure;
use crate::application::error::StoreError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::mcp_grant::GrantVersion;

/// What rotating a grant's refresh token came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RotationOutcome {
    Rotated(GrantVersion),
    /// The grant was replaced or revoked since it was redeemed; nothing was
    /// written.
    Changed,
}

/// Keep the refresh token the provider issued when it redeemed the grant at
/// `expected`, unless the grant has moved on since.
pub(crate) struct RotateGrant<'a> {
    units: &'a dyn UnitOfWorkFactory,
}

impl<'a> RotateGrant<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(
        &self,
        blueprint: &str,
        server: &str,
        expected: &GrantVersion,
        refresh_token: String,
    ) -> Result<RotationOutcome, GrantFailure> {
        let mut unit = self.units.begin().await?;
        let Some(mut grant) = unit.get_grant(blueprint, server).await? else {
            return Ok(RotationOutcome::Changed);
        };
        if &grant.version() != expected {
            return Ok(RotationOutcome::Changed);
        }
        grant.rotate_refresh_token(refresh_token, SystemTime::now())?;
        let version = grant.version();
        match unit.save_grant(grant).await {
            Ok(()) => {}
            Err(StoreError::Conflict) => return Ok(RotationOutcome::Changed),
            Err(error) => return Err(error.into()),
        }
        unit.commit().await?;
        Ok(RotationOutcome::Rotated(version))
    }
}
