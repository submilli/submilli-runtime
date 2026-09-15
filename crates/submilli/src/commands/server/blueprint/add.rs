//! Read a YAML blueprint file and POST it to a running submilli-server.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(clap::Args)]
pub struct Args {
    file: PathBuf,

    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

#[derive(Debug, Deserialize)]
struct AddResponse {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[allow(dead_code)]
    error: String,
    message: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let yaml = fs::read_to_string(&args.file)
        .with_context(|| format!("reading {}", args.file.display()))?;

    if let Err(err) = submilli_blueprint::parse(&yaml) {
        eprintln!("error: {err}");
        return Ok(ExitCode::from(1));
    }

    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/blueprints");

    // Disable ureq's default "non-2xx is an error" so we can read the
    // structured `{ error, message }` body the server returns on 400/409.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
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
        match resp.into_body().read_json::<ErrorResponse>() {
            Ok(err) => eprintln!("error: {}", err.message),
            Err(_) => eprintln!("error: server returned HTTP {status}"),
        }
        Ok(ExitCode::from(1))
    }
}
