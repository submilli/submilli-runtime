//! `submilli secret delete <key>` — remove a secret from the local secret store.

use std::process::ExitCode;

use anyhow::{Result, anyhow};

use crate::commands::local;

#[derive(clap::Args)]
pub struct Args {
    key: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let store = local::open_secret_store()?;
    local::block_on(store.delete(&args.key))?.map_err(|e| anyhow!("{e}"))?;
    println!("Deleted secret '{}'", args.key);
    Ok(ExitCode::SUCCESS)
}
