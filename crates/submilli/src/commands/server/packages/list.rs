//! `submilli server packages list` — the registry packages installed in a
//! running server's store, one per line (plain text, like the other discovery
//! surfaces).

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    packages: Vec<InstalledPackage>,
}

#[derive(Debug, Deserialize)]
struct InstalledPackage {
    name: String,
    version: String,
    description: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/packages");

    let body: ListResponse = match ureq::get(&url).call() {
        Ok(mut resp) => resp
            .body_mut()
            .read_json()
            .context("server returned malformed JSON")?,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    if body.packages.is_empty() {
        println!("no packages installed");
        return Ok(ExitCode::SUCCESS);
    }
    for pkg in body.packages {
        if pkg.description.is_empty() {
            println!("{} v{}", pkg.name, pkg.version);
        } else {
            println!("{} v{} — {}", pkg.name, pkg.version, pkg.description);
        }
    }
    Ok(ExitCode::SUCCESS)
}
