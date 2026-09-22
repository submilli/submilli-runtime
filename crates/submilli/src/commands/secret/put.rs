//! `submilli secret put <key>` — store a secret in the local secret store. The
//! value is prompted for, or read from stdin, so it never appears in the
//! process's argument list.

use std::process::ExitCode;

use anyhow::{Result, anyhow};

use crate::commands::local;

#[derive(clap::Args)]
pub struct Args {
    key: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let value = local::read_secret_value(&args.key)?;
    let store = local::open_secret_store()?;
    local::block_on(store.put(&args.key, &value))?.map_err(|e| anyhow!("{e}"))?;
    println!("Stored secret '{}'", args.key);
    Ok(ExitCode::SUCCESS)
}
