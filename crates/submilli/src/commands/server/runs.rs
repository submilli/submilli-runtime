//! Inspect persisted executions and explicitly resume a stuck run.
use crate::commands::http::{ServerTarget, ok_or_report};
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum RunsCmd {
    /// List runs, including stuck runs and their reasons.
    List(ListArgs),
    /// Inspect a run and its retained result.
    Show(RunArgs),
    /// Resume a stuck run with its original code, identity, and limits.
    Resume(RunArgs),
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long)]
    session: Option<String>,
    #[arg(long, value_parser = ["queued", "running", "stuck", "completed", "cancelled", "aborted", "indeterminate", "expired"])]
    status: Option<String>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Args)]
pub struct RunArgs {
    run_id: String,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    target: ServerTarget,
}

pub fn execute(command: RunsCmd) -> Result<ExitCode> {
    match command {
        RunsCmd::List(args) => list(args),
        RunsCmd::Show(args) => show(args),
        RunsCmd::Resume(args) => resume(args),
    }
}

fn list(args: ListArgs) -> Result<ExitCode> {
    let agent = args.target.agent()?;
    let mut after: Option<String> = None;
    let mut all = Vec::new();
    loop {
        let mut url = url::Url::parse(&format!("{}/v1/runs", args.target.base()))?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(session) = &args.session {
                query.append_pair("session", session);
            }
            if let Some(status) = &args.status {
                query.append_pair("status", status);
            }
            if let Some(after) = &after {
                query.append_pair("after", after);
            }
        }
        let Some(response) = ok_or_report(agent.get(url.as_str()).call()?) else {
            return Ok(ExitCode::FAILURE);
        };
        let page: Value = response
            .into_body()
            .read_json()
            .context("invalid run page")?;
        let runs = page["runs"].as_array().context("run page has no runs")?;
        if args.json {
            all.extend(runs.iter().cloned());
        } else {
            for run in runs {
                print_run(run);
            }
        }
        let next = page["next"].as_str().map(str::to_owned);
        if next.is_none() {
            break;
        }
        if next == after {
            bail!("server repeated its pagination cursor");
        }
        after = next;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&all)?);
    }
    Ok(ExitCode::SUCCESS)
}

fn show(args: RunArgs) -> Result<ExitCode> {
    let agent = args.target.agent()?;
    let url = run_url(args.target.base(), &args.run_id, None)?;
    let Some(response) = ok_or_report(agent.get(&url).call()?) else {
        return Ok(ExitCode::FAILURE);
    };
    let run: Value = response
        .into_body()
        .read_json()
        .context("invalid run response")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        print_run(&run);
        if let Some(response) = run.get("response") {
            println!("{}", serde_json::to_string_pretty(response)?);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn resume(args: RunArgs) -> Result<ExitCode> {
    let agent = args.target.agent()?;
    let url = run_url(args.target.base(), &args.run_id, None)?;
    let Some(response) = ok_or_report(agent.get(&url).call()?) else {
        return Ok(ExitCode::FAILURE);
    };
    let run: Value = response
        .into_body()
        .read_json()
        .context("invalid run response")?;
    if run["status"] != "stuck" {
        bail!("run is not stuck; current status is {}", run["status"]);
    }
    let revision = run["revision"].as_i64().context("run has no revision")?;
    let url = run_url(args.target.base(), &args.run_id, Some("recover"))?;
    let Some(response) = ok_or_report(
        agent
            .post(&url)
            .send_json(serde_json::json!({"revision": revision}))?,
    ) else {
        return Ok(ExitCode::FAILURE);
    };
    let run: Value = response
        .into_body()
        .read_json()
        .context("invalid recovery response")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        print_run(&run);
    }
    Ok(ExitCode::SUCCESS)
}

fn run_url(base: &str, id: &str, tail: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(base)?;
    let mut path = url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("server URL cannot have path segments"))?;
    path.pop_if_empty().extend(["v1", "runs", id]);
    if let Some(tail) = tail {
        path.push(tail);
    }
    drop(path);
    Ok(url.into())
}

fn print_run(run: &Value) {
    println!(
        "{}  {}  session={}  attempt={}{}",
        run["execution_id"].as_str().unwrap_or("?"),
        run["status"].as_str().unwrap_or("?"),
        run["session_id"].as_str().unwrap_or("?"),
        run["attempt"],
        run["reason"]
            .as_str()
            .map(|reason| format!("  {reason}"))
            .unwrap_or_default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn run_id_is_one_encoded_path_segment() {
        assert_eq!(
            run_url("https://example.com", "a/b?c", Some("recover")).unwrap(),
            "https://example.com/v1/runs/a%2Fb%3Fc/recover"
        );
    }
}
