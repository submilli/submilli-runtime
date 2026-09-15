//! `submilli server mcp auth-status <blueprint>` — show each declared MCP
//! server's authentication state.

use std::process::ExitCode;

use anyhow::Result;
use serde::Deserialize;

use crate::commands::http::{client, read_or_error};

#[derive(clap::Args)]
pub struct Args {
    /// Blueprint to report on.
    blueprint: String,
    /// Base URL of the running submilli-server.
    #[arg(long = "server", default_value = "http://127.0.0.1:8128")]
    server_url: String,
}

#[derive(Debug, Deserialize)]
struct StatusBody {
    state: String,
    servers: Vec<ServerRow>,
}

#[derive(Debug, Deserialize)]
struct ServerRow {
    name: String,
    kind: String,
    #[serde(default)]
    authenticated: Option<bool>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    match run(&args) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run(args: &Args) -> Result<()> {
    let agent = client();
    let base = args.server_url.trim_end_matches('/');
    let body: StatusBody = read_or_error(
        agent
            .get(&format!("{base}/v1/mcp/{}/auth-status", args.blueprint))
            .call()
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    )?;

    println!("{}: {}", args.blueprint, body.state.to_uppercase());
    for server in &body.servers {
        println!("  {:<24} {}", server.name, describe(server));
    }
    Ok(())
}

fn describe(server: &ServerRow) -> String {
    match (server.kind.as_str(), server.authenticated) {
        ("oauth", Some(true)) => "oauth — authenticated".into(),
        ("oauth", _) => "oauth — NOT AUTHENTICATED".into(),
        ("static", _) => "n/a (static API key)".into(),
        _ => "n/a (no auth)".into(),
    }
}
