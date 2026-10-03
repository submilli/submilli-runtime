//! `submilli server status` — query a running server's `/v1/status` endpoint.

use std::process::ExitCode;

use anyhow::Context;
use serde::Deserialize;

use crate::commands::http::{ServerTarget, ok_or_report};

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    status: String,
    bind_addr: Option<String>,
    pid: u32,
    active_sessions: usize,
    #[serde(default)]
    blueprints: Vec<String>,
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let base = args.target.base();
    let url = format!("{base}/v1/status");
    let agent = match args.target.agent() {
        Ok(agent) => agent,
        Err(error) if crate::commands::http::connection_refused(&error) => {
            println!("stopped (no server at {base})");
            return Ok(ExitCode::from(1));
        }
        Err(error) => return Err(error),
    };

    let resp = match agent.get(&url).call() {
        Ok(response) => response,
        Err(ureq::Error::Io(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            println!("stopped (no server at {base})");
            return Ok(ExitCode::from(1));
        }
        Err(error) => return Err(error.into()),
    };
    // A server that refuses the token is running; saying "stopped" would hide
    // the real problem.
    let Some(resp) = ok_or_report(resp) else {
        return Ok(ExitCode::from(1));
    };

    let status: StatusResponse = resp
        .into_body()
        .read_json()
        .context("server returned malformed JSON")?;

    println!("status:          {}", status.status);
    println!(
        "bind:            {}",
        status.bind_addr.as_deref().unwrap_or("unknown")
    );
    println!("pid:             {}", status.pid);
    println!("active sessions: {}", status.active_sessions);
    if status.blueprints.is_empty() {
        println!("blueprints:      (none)");
    } else {
        println!("blueprints:      {}", status.blueprints.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}
