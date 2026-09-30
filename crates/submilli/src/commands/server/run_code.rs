//! `submilli server run-code` — POST a script to a running `submilli-server`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use crate::commands::http::{ServerTarget, ok_or_report};

#[derive(clap::Args)]
pub struct Args {
    script: PathBuf,

    #[arg(long)]
    blueprint: String,

    #[command(flatten)]
    target: ServerTarget,

    /// Bind a blueprint variable for this run, `NAME=VALUE` (repeatable), the
    /// way an application binds it when it opens a session.
    #[arg(long = "var", value_name = "NAME=VALUE")]
    vars: Vec<String>,

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
    let variables = parse_variables(&args.vars)?;

    let base = args.target.base();
    let execute_url = format!("{base}/v1/execute");

    let agent = args
        .target
        .agent_with_timeout(args.timeout.map(Duration::from_secs))?;

    let mut request = serde_json::json!({
        "code": source,
        "blueprint": args.blueprint,
    });
    if !variables.is_empty() {
        request["variables"] = serde_json::to_value(&variables)?;
    }
    let resp = match agent.post(&execute_url).send_json(request) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let Some(resp) = ok_or_report(resp) else {
        return Ok(ExitCode::from(1));
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

/// `--var NAME=VALUE` pairs as the `variables` object of the request. The server
/// checks them against the blueprint's declarations, so only the shape is
/// checked here.
fn parse_variables(raw: &[String]) -> Result<BTreeMap<String, String>> {
    let mut variables = BTreeMap::new();
    for pair in raw {
        let (name, value) = pair
            .split_once('=')
            .with_context(|| format!("--var '{pair}' must be in `NAME=VALUE` form"))?;
        if name.is_empty() {
            bail!("--var '{pair}' has an empty name");
        }
        variables.insert(name.to_owned(), value.to_owned());
    }
    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::parse_variables;

    #[test]
    fn var_pairs_split_on_the_first_equals() {
        let vars = parse_variables(&["customerId=cus_1".into(), "note=a=b".into()]).unwrap();
        assert_eq!(vars["customerId"], "cus_1");
        assert_eq!(vars["note"], "a=b");
        assert!(parse_variables(&[]).unwrap().is_empty());
    }

    #[test]
    fn malformed_var_pairs_are_refused() {
        assert!(
            parse_variables(&["customerId".into()])
                .unwrap_err()
                .to_string()
                .contains("NAME=VALUE")
        );
        assert!(
            parse_variables(&["=x".into()])
                .unwrap_err()
                .to_string()
                .contains("empty name")
        );
    }
}
