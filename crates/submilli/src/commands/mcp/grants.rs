//! `submilli mcp grants [--blueprint <name>]` — list the MCP OAuth grants
//! `submilli mcp authenticate` stored locally, never their tokens.

use std::process::ExitCode;

use anyhow::{Result, anyhow};

use crate::commands::local;
use crate::commands::server::mcp::grants::describe;

#[derive(clap::Args)]
pub struct Args {
    /// Only list this blueprint's grants.
    #[arg(long)]
    blueprint: Option<String>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let grants = local::open_grant_store()?;
    let listed =
        local::block_on(grants.list(args.blueprint.as_deref()))?.map_err(|e| anyhow!("{e}"))?;
    for grant in listed {
        println!("{}", describe(&grant));
    }
    Ok(ExitCode::SUCCESS)
}
