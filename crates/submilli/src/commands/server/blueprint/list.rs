//! `submilli server blueprint list [--server <url>]` — print blueprint
//! names one per line so output composes with shell pipelines.

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
    blueprints: Vec<BlueprintSummary>,
}

#[derive(Debug, Deserialize)]
struct BlueprintSummary {
    name: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/blueprints");

    let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let body: ListResponse = resp
        .into_body()
        .read_json()
        .context("server returned malformed JSON")?;
    for entry in body.blueprints {
        println!("{}", entry.name);
    }
    Ok(ExitCode::SUCCESS)
}
