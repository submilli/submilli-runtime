//! `submilli server blueprint list [--server <url>]` — print blueprint
//! names one per line so output composes with shell pipelines.

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::commands::http::{ServerTarget, ok_or_report};

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    target: ServerTarget,
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
    let base = args.target.base();
    let url = format!("{base}/v1/blueprints");

    let agent = args.target.agent()?;
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let Some(resp) = ok_or_report(resp) else {
        return Ok(ExitCode::from(1));
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
