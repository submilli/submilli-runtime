//! `submilli mcp auth-status --blueprint <path>` — show each declared MCP
//! server's authentication state from the local store.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use submilli_shared::mcp_auth::{ServerKind, blueprint_auth_state, is_authenticated, server_kind};

use crate::commands::local;
use crate::commands::mcp::load_blueprint;

#[derive(clap::Args)]
pub struct Args {
    /// Blueprint file to report on.
    #[arg(long)]
    blueprint: PathBuf,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match local::block_on(run(&args))? {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

async fn run(args: &Args) -> Result<()> {
    let blueprint = load_blueprint(&args.blueprint)?;
    let store = local::open_secret_store()?;
    let state = blueprint_auth_state(&blueprint, Some(&store)).await;
    let overall = if state.is_pending() {
        "PENDING"
    } else {
        "ACTIVE"
    };
    println!("{}: {overall}", blueprint.name);

    for (name, server) in &blueprint.mcp {
        let description = match server_kind(server) {
            ServerKind::OAuth => {
                if is_authenticated(&blueprint.name, name, Some(&store)).await {
                    "oauth — authenticated".to_string()
                } else {
                    "oauth — NOT AUTHENTICATED".to_string()
                }
            }
            ServerKind::StaticKey => "n/a (static API key)".to_string(),
            ServerKind::None => "n/a (no auth)".to_string(),
        };
        println!("  {name:<24} {description}");
    }
    Ok(())
}
