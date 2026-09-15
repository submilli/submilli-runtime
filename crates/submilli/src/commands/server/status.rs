//! `submilli server status` — query a running server's `/v1/status` endpoint.

use std::process::ExitCode;

use anyhow::Context;
use serde::Deserialize;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
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
    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/status");
    let agent: ureq::Agent = ureq::Agent::config_builder().build().into();

    let Ok(resp) = agent.get(&url).call() else {
        println!("stopped (no server at {base})");
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
