//! `submilli mcp deauthenticate --blueprint <path> <server>` — delete an MCP
//! server's stored credential; the blueprint returns to PENDING.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, anyhow};
use submilli_shared::mcp_auth::{blueprint_auth_state, credential_key};

use crate::commands::local;
use crate::commands::mcp::{load_blueprint, summary};

#[derive(clap::Args)]
pub struct Args {
    /// The MCP server's local name (its key in the blueprint's `mcp:` block).
    server: String,
    /// Blueprint file that declares the MCP server.
    #[arg(long)]
    blueprint: PathBuf,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match local::block_on(run(&args))? {
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

async fn run(args: &Args) -> Result<String> {
    let blueprint = load_blueprint(&args.blueprint)?;
    let store = local::open_secret_store()?;
    store
        .delete(&credential_key(&blueprint.name, &args.server))
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let state = blueprint_auth_state(&blueprint, Some(&store)).await;
    Ok(format!(
        "removed '{}' credential — {}",
        args.server,
        summary(&blueprint.name, &state)
    ))
}
