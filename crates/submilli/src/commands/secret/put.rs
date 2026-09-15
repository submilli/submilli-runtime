//! `submilli secret put <key>` — store a secret in the local secret store. The
//! value is read from stdin so it never appears in the process's argument list.

use std::io::Read;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};

use crate::commands::local;

#[derive(clap::Args)]
pub struct Args {
    key: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let mut value = String::new();
    std::io::stdin()
        .read_to_string(&mut value)
        .context("reading secret value from stdin")?;
    // Drop a single trailing newline so `echo secret | …` stores `secret`.
    let value = value.strip_suffix('\n').unwrap_or(&value);

    let store = local::open_secret_store()?;
    local::block_on(store.put(&args.key, value))?.map_err(|e| anyhow!("{e}"))?;
    println!("Stored secret '{}'", args.key);
    Ok(ExitCode::SUCCESS)
}
