//! `submilli server packages install <url> [package] [--sha <sha>] [--upgrade]` — ask a
//! running submilli-server to fetch a GitHub package, compile it, and install it
//! into the server's package store. The fetch + compile happen server-side; the
//! CLI only relays the request, and never sends its own GitHub token.

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    /// Set a src/config.ts parameter of the selected package.
    #[arg(long, value_name = "NAME=VALUE")]
    config: Vec<String>,
    /// Remove a saved override and use the source default.
    #[arg(long, value_name = "NAME")]
    reset_config: Vec<String>,
    /// GitHub repo: `org/repo`, `github.com/org/repo`, or a full URL. A private
    /// repository needs the server's own GitHub token (`github_token_file` in
    /// its config file); this command never sends yours.
    url: String,

    /// Install only this package (`@org/name`). Omit to install every package
    /// the repo declares.
    package: Option<String>,

    /// Pin to this commit SHA (or ref). Resolved from the default branch when
    /// omitted.
    #[arg(long)]
    sha: Option<String>,

    /// Replace an installed package when its source or configuration changes.
    #[arg(long)]
    upgrade: bool,

    /// Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1.
    #[arg(long)]
    deny_warnings: bool,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Serialize)]
struct InstallRequest {
    config: std::collections::BTreeMap<String, String>,
    reset_config: Vec<String>,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
    upgrade: bool,
    deny_warnings: bool,
}

#[derive(Debug, Deserialize)]
struct InstallResponse {
    sha: String,
    installed: Vec<String>,
    up_to_date: Vec<String>,
    #[serde(default)]
    warnings: Vec<String>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let inputs = submilli_build::ConfigInputs::parse(
        &args.config,
        &args.reset_config,
        args.package.is_some(),
    )?;
    let base = args.target.base();
    let url = format!("{base}/v1/packages/install");

    let agent = args.target.agent()?;
    let resp = match agent.post(&url).send_json(InstallRequest {
        config: inputs.values,
        reset_config: inputs.reset,
        url: args.url,
        sha: args.sha,
        package: args.package,
        upgrade: args.upgrade,
        deny_warnings: args.deny_warnings || submilli_build::deny_warnings_from_env(),
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
        for warning in &body.warnings {
            eprint!("{warning}");
        }
        for name in &body.up_to_date {
            eprintln!("up to date {name}");
        }
        for name in &body.installed {
            eprintln!("installed {name} @ {}", short(&body.sha));
        }
        Ok(ExitCode::SUCCESS)
    } else if status == 400 {
        let body: serde_json::Value = resp
            .into_body()
            .read_json()
            .context("server returned malformed JSON")?;
        if let Some(warnings) = body.get("warnings").and_then(serde_json::Value::as_array) {
            for warning in warnings.iter().filter_map(serde_json::Value::as_str) {
                eprint!("{warning}");
            }
        }
        if let Some(command) = body
            .get("configuration")
            .and_then(|configuration| configuration.get("command"))
            .and_then(serde_json::Value::as_str)
        {
            eprintln!("  {}{}", command, args.target.command_flags());
        }
        eprintln!(
            "error: {}",
            body.get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("package install failed")
        );
        Ok(ExitCode::from(1))
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}

fn short(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}
