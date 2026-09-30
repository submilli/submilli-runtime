//! Read a YAML blueprint file and PUT it to a running submilli-server,
//! creating it or replacing an existing blueprint of the same name. The
//! name comes from the file's `name:` field — no positional argument.

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
struct ApplyResponse {
    name: String,
    created: bool,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let yaml = fs::read_to_string(&args.file)
        .with_context(|| format!("reading {}", args.file.display()))?;

    let blueprint = match submilli_blueprint::parse(&yaml) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let base = args.target.base();
    let url = format!("{base}/v1/blueprints/{}", blueprint.name);

    let agent = args.target.agent()?;
    let resp = match agent
        .put(&url)
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
        let body: ApplyResponse = resp
            .into_body()
            .read_json()
            .context("server returned malformed JSON")?;
        let verb = if body.created { "Added" } else { "Updated" };
        println!("{verb} blueprint '{}'", body.name);
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}
