//! `submilli server packages install <url> [package] [--sha <sha>] [--upgrade]` — ask a
//! running submilli-server to fetch a GitHub package, compile it, and install it
//! into the server's package store. The fetch + compile happen server-side; the
//! CLI only relays the request.

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(clap::Args)]
pub struct Args {
    /// GitHub repo: `org/repo`, `github.com/org/repo`, or a full URL.
    url: String,

    /// Install only this package (`@org/name`). Omit to install every package
    /// the repo declares.
    package: Option<String>,

    /// Pin to this commit SHA (or ref). Resolved from the default branch when
    /// omitted.
    #[arg(long)]
    sha: Option<String>,

    /// Re-install over a package already present at a different commit.
    #[arg(long)]
    upgrade: bool,

    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

#[derive(Debug, Serialize)]
struct InstallRequest {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
    upgrade: bool,
}

#[derive(Debug, Deserialize)]
struct InstallResponse {
    sha: String,
    installed: Vec<String>,
    up_to_date: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[allow(dead_code)]
    error: String,
    message: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/packages/install");

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let resp = match agent.post(&url).send_json(InstallRequest {
        url: args.url,
        sha: args.sha,
        package: args.package,
        upgrade: args.upgrade,
    }) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        let body: InstallResponse = resp
            .into_body()
            .read_json()
            .context("server returned malformed JSON")?;
        for name in &body.up_to_date {
            eprintln!("up to date {name}");
        }
        for name in &body.installed {
            eprintln!("installed {name} @ {}", short(&body.sha));
        }
        Ok(ExitCode::SUCCESS)
    } else {
        match resp.into_body().read_json::<ErrorResponse>() {
            Ok(err) => eprintln!("error: {}", err.message),
            Err(_) => eprintln!("error: server returned HTTP {status}"),
        }
        Ok(ExitCode::from(1))
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}
