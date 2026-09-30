//! Read a YAML blueprint file and POST it to a running submilli-server.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    file: PathBuf,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct AddResponse {
    name: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let yaml = fs::read_to_string(&args.file)
        .with_context(|| format!("reading {}", args.file.display()))?;

    if let Err(err) = submilli_blueprint::parse(&yaml) {
        eprintln!("error: {err}");
        return Ok(ExitCode::from(1));
    }

    let base = args.target.base();
    let url = format!("{base}/v1/blueprints");

    let agent = args.target.agent()?;
    let resp = match agent
        .post(&url)
        .send_json(serde_json::json!({ "yaml": yaml }))
    {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        let body: AddResponse = resp
            .into_body()
            .read_json()
            .context("server returned malformed JSON")?;
        println!("Added blueprint '{}'", body.name);
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}
