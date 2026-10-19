//! MCP OAuth grants: deposited by an operator's login, rotated when the
//! provider issues a new refresh token, listed and revoked through the
//! management API, and removed with their blueprint.
pub(crate) mod deposit;
pub(crate) mod queries;
pub(crate) mod revoke;
pub(crate) mod rotate;

use crate::application::error::StoreError;
use crate::domain::mcp_grant::GrantError;

#[derive(Debug, thiserror::Error)]
pub(crate) enum GrantFailure {
    #[error(transparent)]
    Rule(#[from] GrantError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("blueprint '{0}' does not exist")]
    UnknownBlueprint(String),
}
