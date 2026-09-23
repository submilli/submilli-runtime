//! `submilli server packages list` — the registry packages a running server can
//! resolve, one per line on stdout (plain text, like the other discovery
//! surfaces). A package the server only reads from a fallback store (the
//! CLI's own, on a dev machine) is marked with where it comes from, and when
//! more than one store is searched the list of stores goes to stderr so stdout
//! stays one package per line.

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
    /// Every root the server searches, its own store first. Absent from older
    /// servers, which have exactly one.
    #[serde(default)]
    roots: Vec<String>,
    packages: Vec<InstalledPackage>,
}

#[derive(Debug, Deserialize)]
struct InstalledPackage {
    name: String,
    version: String,
    description: String,
    #[serde(default)]
    root: String,
    /// Older servers report neither field; everything they list is theirs.
    #[serde(default = "managed_by_default")]
    managed: bool,
}

fn managed_by_default() -> bool {
    true
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

    if body.roots.len() > 1 {
        eprintln!("stores: {}", body.roots.join(", "));
    }
    if body.packages.is_empty() {
        println!("no packages installed");
        return Ok(ExitCode::SUCCESS);
    }
    for pkg in body.packages {
        let mut line = format!("{} v{}", pkg.name, pkg.version);
        if !pkg.description.is_empty() {
            line.push_str(&format!(" — {}", pkg.description));
        }
        if !pkg.managed {
            line.push_str(&format!(" (from {})", pkg.root));
        }
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}
