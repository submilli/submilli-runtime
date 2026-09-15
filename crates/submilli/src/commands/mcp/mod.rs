//! `submilli mcp …` — authenticate outbound OAuth MCP servers declared in a
//! local blueprint, with no running server. The whole OAuth dance (discovery,
//! optional Dynamic Client Registration, browser PKCE, and the token exchange)
//! runs here, and the credential is written to the local secret store under the
//! same `mcp_oauth/<blueprint>/<server>/credential` key the server uses — so a
//! script that runs against the server in production authenticates identically
//! during local iteration.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use submilli_blueprint::Blueprint;
use submilli_shared::mcp_auth::AuthState;

mod auth_status;
mod authenticate;
mod deauthenticate;
mod provider;
mod provider_config;

#[derive(Subcommand)]
pub enum McpCmd {
    /// Run the OAuth flow for a blueprint's MCP server and store its credential
    /// locally. Re-run to re-authenticate (e.g. after changing scopes) — it
    /// overwrites the existing credential.
    Authenticate(authenticate::Args),
    /// Remove an MCP server's stored credential (blueprint returns to PENDING).
    Deauthenticate(deauthenticate::Args),
    /// Show each declared MCP server's authentication state.
    #[command(name = "auth-status")]
    AuthStatus(auth_status::Args),
    /// Manage local OAuth provider apps (`~/.submilli/mcp_oauth.yaml`) — the
    /// client id / secret used to authenticate confidential OAuth servers.
    #[command(subcommand)]
    Provider(provider::ProviderCmd),
}

pub fn execute(cmd: McpCmd) -> Result<ExitCode> {
    match cmd {
        McpCmd::Authenticate(args) => authenticate::execute(args),
        McpCmd::Deauthenticate(args) => deauthenticate::execute(args),
        McpCmd::AuthStatus(args) => auth_status::execute(args),
        McpCmd::Provider(cmd) => provider::execute(cmd),
    }
}

/// Parse a blueprint file for the local MCP commands.
fn load_blueprint(path: &Path) -> Result<Blueprint> {
    let yaml =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    submilli_blueprint::parse(&yaml).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// A one-line PENDING/ACTIVE summary for the operator.
fn summary(blueprint: &str, state: &AuthState) -> String {
    match state {
        AuthState::Active => format!("blueprint '{blueprint}' is ACTIVE"),
        AuthState::Pending { unauthenticated } => format!(
            "blueprint '{blueprint}' is PENDING (still awaiting: {})",
            unauthenticated.join(", ")
        ),
    }
}
