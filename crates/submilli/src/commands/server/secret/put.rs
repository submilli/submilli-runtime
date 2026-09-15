//! `submilli server secret put <key> [--server <url>]` — store a secret in the
//! server's secret store. The value is read from stdin so it never appears in
//! the process's argument list.

use std::io::Read;
use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(clap::Args)]
pub struct Args {
    key: String,

    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[allow(dead_code)]
    error: String,
    message: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let mut value = String::new();
    std::io::stdin()
        .read_to_string(&mut value)
        .context("reading secret value from stdin")?;
    // Drop a single trailing newline so `echo secret | …` stores `secret`.
    let value = value.strip_suffix('\n').unwrap_or(&value);

    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/secrets");

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let resp = match agent
        .post(&url)
        .send_json(serde_json::json!({ "key": args.key, "value": value }))
    {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        println!("Stored secret '{}'", args.key);
        Ok(ExitCode::SUCCESS)
    } else {
        match resp.into_body().read_json::<ErrorResponse>() {
            Ok(err) => eprintln!("error: {}", err.message),
            Err(_) => eprintln!("error: server returned HTTP {status}"),
        }
        Ok(ExitCode::from(1))
    }
}
