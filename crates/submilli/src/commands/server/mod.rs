use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

pub mod apply;
pub mod blueprint;
pub mod docs;
pub mod mcp;
pub mod packages;
pub mod run_code;
pub mod secret;
pub mod session;
pub mod status;
pub mod stop;
pub mod trust;

#[derive(Subcommand)]
pub enum ServerCmd {
    /// Apply blueprint YAML documents to a running submilli-server.
    Apply(apply::Args),
    /// Manage approved HTTPS server public keys.
    #[command(subcommand)]
    Trust(trust::TrustCmd),
    /// Read package declarations, including a blueprint's MCP tools.
    Docs(docs::Args),
    /// Execute a Submilli script on a running submilli-server.
    #[command(name = "run-code")]
    RunCode(run_code::Args),
    /// Manage the server's package store and list the packages it can resolve.
    #[command(subcommand)]
    Packages(packages::PackagesCmd),
    /// Report a running server's status (pid, bind, sessions, blueprints).
    Status(status::Args),
    /// Ask a running server to drain and stop.
    Stop(stop::Args),
    /// Manage blueprints registered on the server.
    #[command(subcommand)]
    Blueprint(blueprint::BlueprintCmd),
    /// Manage secrets in the server's secret store.
    #[command(subcommand)]
    Secret(secret::SecretCmd),
    /// Open and close sessions, for `run-code --session`.
    #[command(subcommand)]
    Session(session::SessionCmd),
    /// Authenticate outbound OAuth MCP servers declared in a blueprint.
    #[command(subcommand)]
    Mcp(mcp::McpCmd),
}

pub fn execute(cmd: ServerCmd) -> Result<ExitCode> {
    match cmd {
        ServerCmd::Apply(args) => apply::execute(args),
        ServerCmd::Trust(cmd) => trust::execute(cmd),
        ServerCmd::Docs(args) => docs::execute(args),
        ServerCmd::RunCode(args) => run_code::execute(args),
        ServerCmd::Packages(cmd) => packages::execute(cmd),
        ServerCmd::Status(args) => status::execute(args),
        ServerCmd::Stop(args) => stop::execute(args),
        ServerCmd::Blueprint(cmd) => blueprint::execute(cmd),
        ServerCmd::Secret(cmd) => secret::execute(cmd),
        ServerCmd::Session(cmd) => session::execute(cmd),
        ServerCmd::Mcp(cmd) => mcp::execute(cmd),
    }
}
