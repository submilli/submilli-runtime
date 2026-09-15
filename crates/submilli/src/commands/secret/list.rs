//! `submilli secret list [--prefix <p>]` — print local secret keys one per line
//! (never values).

use std::process::ExitCode;

use anyhow::{Result, anyhow};

use crate::commands::local;

#[derive(clap::Args)]
pub struct Args {
    /// Only list keys starting with this prefix.
    #[arg(long)]
    prefix: Option<String>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let store = local::open_secret_store()?;
    let keys = local::block_on(store.list(args.prefix.as_deref()))?.map_err(|e| anyhow!("{e}"))?;
    for key in keys {
        println!("{key}");
    }
    Ok(ExitCode::SUCCESS)
}
