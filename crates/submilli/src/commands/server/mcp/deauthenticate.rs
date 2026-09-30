//! `submilli server mcp deauthenticate <blueprint> <server>` — delete an MCP
//! server's stored refresh token; the blueprint returns to PENDING.

use std::process::ExitCode;

use anyhow::Result;

use super::AuthStateBody;
use crate::commands::http::{ServerTarget, read_or_error};

#[derive(clap::Args)]
pub struct Args {
    /// Blueprint that declares the MCP server.
    blueprint: String,
    /// The MCP server's local name (its key in the `mcp:` block).
    server: String,
    #[command(flatten)]
    target: ServerTarget,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
        Ok(message) => {
            println!("✓ {message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(args: &Args) -> Result<String> {
    let agent = args.target.agent()?;
    let base = args.target.base();
    let state: AuthStateBody = read_or_error(
        agent
            .delete(&format!(
                "{base}/v1/mcp/{}/{}/refresh-token",
                args.blueprint, args.server
            ))
            .call()
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    )?;
    Ok(format!(
        "removed '{}' refresh token — {}",
        args.server,
        state.summary(&args.blueprint)
    ))
}
