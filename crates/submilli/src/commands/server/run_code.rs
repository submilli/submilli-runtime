//! `submilli server run-code` — POST a script to a running `submilli-server`.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(clap::Args)]
pub struct Args {
    script: PathBuf,

    #[arg(long)]
    blueprint: String,

    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,

    /// Omit for no client-side timeout; the server still enforces its own fuel/epoch limits.
    #[arg(long)]
    timeout: Option<u64>,
}

/// Owned to keep the CLI independent of the server crate at compile time.
#[derive(Debug, Deserialize)]
struct ExecuteResponse {
    #[serde(default)]
    session_id: Option<String>,
    result: Option<Value>,
    #[serde(default)]
    console: Vec<String>,
    error: Option<ExecuteError>,
    #[serde(default)]
    discovery_warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExecuteError {
    #[allow(dead_code)]
    kind: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct LastRunResponse {
    #[serde(default)]
    console: Vec<String>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let source = fs::read_to_string(&args.script)
        .with_context(|| format!("reading {}", args.script.display()))?;

    let base = args.server.trim_end_matches('/');
    let execute_url = format!("{base}/v1/execute");

    let mut config = ureq::Agent::config_builder();
    if let Some(secs) = args.timeout {
        config = config.timeout_global(Some(Duration::from_secs(secs)));
    }
    let agent: ureq::Agent = config.build().into();

    let resp = match agent.post(&execute_url).send_json(serde_json::json!({
        "code": source,
        "blueprint": args.blueprint,
    })) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let response: ExecuteResponse = resp
        .into_body()
        .read_json()
        .context("server returned malformed JSON")?;

    for warning in &response.discovery_warnings {
        eprintln!("warning: {warning}");
    }

    if let Some(error) = response.error {
        for line in &response.console {
            eprintln!("{line}");
        }
        eprint!("{}", error.message);
        if !error.message.ends_with('\n') {
            eprintln!();
        }
        return Ok(ExitCode::from(1));
    }

    // /v1/execute suppresses console on success; pull it from
    // /v1/sessions/{session_id}/last-run so the CLI can surface it on stderr.
    if let Some(session_id) = response.session_id.as_deref() {
        let last_run_url = format!("{base}/v1/sessions/{session_id}/last-run");
        if let Ok(last) = agent.get(&last_run_url).call()
            && let Ok(body) = last.into_body().read_json::<LastRunResponse>()
        {
            for line in &body.console {
                eprintln!("{line}");
            }
        }
    }

    match response.result {
        Some(Value::String(s)) if !s.is_empty() => {
            // String-returning main: print contents unquoted so
            // terminal output looks natural (`hello` not `"hello"`).
            print!("{s}");
            if !s.ends_with('\n') {
                println!();
            }
        }
        Some(Value::String(_)) | None => {}
        Some(other) => {
            println!("{}", serde_json::to_string(&other).unwrap());
        }
    }

    Ok(ExitCode::SUCCESS)
}
