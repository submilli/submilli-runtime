//! `submilli server blueprint show <name> [--server <url>]` — print a
//! registered blueprint's YAML to stdout.

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    name: String,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct ShowResponse {
    #[allow(dead_code)]
    name: String,
    yaml: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.target.base();
    let url = format!("{base}/v1/blueprints/{}", args.name);

    let agent = args.target.agent()?;
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        let body: ShowResponse = resp
            .into_body()
            .read_json()
            .context("server returned malformed JSON")?;
        print!("{}", body.yaml);
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}
