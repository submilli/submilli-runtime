//! `submilli blueprint *` — local blueprint **authoring** (operates on a
//! `blueprint.yaml` file), distinct from `submilli server blueprint *` which
//! manages blueprints on a running server. See the v1 release plan §F/§G.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

pub mod add_mcp;
pub mod add_package;
pub mod auth_proxy;
pub mod capability;
mod capability_names;
mod declared_packages;
mod file;
mod filter_fields;
pub mod git;
pub mod init;
pub mod lint;
mod package_secrets;
pub mod prompt;
pub mod secret;
pub mod variable;

#[derive(Subcommand)]
pub enum BlueprintCmd {
    /// Scaffold a minimal deny-by-default `blueprint.yaml` in the current
    /// directory (`--full` lists every stdlib capability). Offline — no
    /// server needed.
    Init(init::Args),
    /// Configure Git identity and HTTPS username in a local blueprint.
    #[command(subcommand)]
    Git(git::GitCmd),
    /// Validate a blueprint file's syntax and references. Offline.
    Lint(lint::Args),
    /// Add an outbound MCP server to a local `blueprint.yaml`.
    #[command(name = "add-mcp")]
    AddMcp(add_mcp::Args),
    /// Add an already-installed package to a local `blueprint.yaml`.
    #[command(name = "add-package")]
    AddPackage(add_package::Args),
    /// Manage the session variables declared in a local `blueprint.yaml`'s
    /// `variables:` block.
    #[command(subcommand)]
    Variable(variable::VariableCmd),
    /// Manage the secrets declared in a local `blueprint.yaml`'s `secrets:`
    /// block.
    #[command(subcommand)]
    Secret(secret::SecretCmd),
    /// Manage host-keyed outbound-auth rules in a local `blueprint.yaml`.
    #[command(name = "auth-proxy", subcommand)]
    AuthProxy(auth_proxy::AuthProxyCmd),
    /// Browse gateable capabilities and edit `permissions:` rules in a local
    /// `blueprint.yaml`.
    #[command(subcommand)]
    Capability(capability::CapabilityCmd),
    /// Print the LLM-facing `execute` tool description (the MCP "system
    /// prompt") for a blueprint, with placeholders resolved as the server
    /// resolves them. Offline — no server needed.
    Prompt(prompt::Args),
}

pub fn execute(cmd: BlueprintCmd) -> Result<ExitCode> {
    match cmd {
        BlueprintCmd::Init(args) => init::execute(args),
        BlueprintCmd::Git(cmd) => git::execute(cmd),
        BlueprintCmd::Lint(args) => lint::execute(args),
        BlueprintCmd::AddMcp(args) => add_mcp::execute(args),
        BlueprintCmd::AddPackage(args) => add_package::execute(args),
        BlueprintCmd::Variable(cmd) => variable::execute(cmd),
        BlueprintCmd::Secret(cmd) => secret::execute(cmd),
        BlueprintCmd::AuthProxy(cmd) => auth_proxy::execute(cmd),
        BlueprintCmd::Capability(cmd) => capability::execute(cmd),
        BlueprintCmd::Prompt(args) => prompt::execute(args),
    }
}
