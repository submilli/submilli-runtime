//! `submilli server mcp …` — authenticate outbound OAuth MCP servers declared in
//! a blueprint. The OAuth dance runs here on the operator's machine; the server
//! only stores the resulting refresh token and reports PENDING/ACTIVE state.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;

mod auth_status;
mod authenticate;
mod deauthenticate;
pub mod oauth;

#[derive(Subcommand)]
pub enum McpCmd {
    /// Run the OAuth flow for a blueprint's MCP server and store its refresh
    /// token. Re-run to re-authenticate (e.g. after changing scopes) — it
    /// overwrites the existing credential.
    Authenticate(authenticate::Args),
    /// Remove an MCP server's stored refresh token (blueprint returns to PENDING).
    Deauthenticate(deauthenticate::Args),
    /// Show each declared MCP server's authentication state.
    #[command(name = "auth-status")]
    AuthStatus(auth_status::Args),
}

pub fn execute(cmd: McpCmd) -> Result<ExitCode> {
    match cmd {
        McpCmd::Authenticate(args) => authenticate::execute(args),
        McpCmd::Deauthenticate(args) => deauthenticate::execute(args),
        McpCmd::AuthStatus(args) => auth_status::execute(args),
    }
}

/// The PENDING/ACTIVE state the auth endpoints return.
#[derive(Debug, Deserialize)]
struct AuthStateBody {
    state: String,
    #[serde(default)]
    unauthenticated: Vec<String>,
}

impl AuthStateBody {
    /// A one-line summary for the operator.
    fn summary(&self, blueprint: &str) -> String {
        if self.unauthenticated.is_empty() {
            format!("blueprint '{blueprint}' is {}", self.state.to_uppercase())
        } else {
            format!(
                "blueprint '{blueprint}' is {} (still awaiting: {})",
                self.state.to_uppercase(),
                self.unauthenticated.join(", ")
            )
        }
    }
}
