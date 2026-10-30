//! Decide calls waiting for an approver: list, inspect, approve, or deny them.
use crate::commands::http::{ServerTarget, ok_or_report};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::process::ExitCode;

#[derive(Subcommand)]
pub enum ApprovalCmd {
    /// List calls waiting for approval, oldest first.
    List(ListArgs),
    /// Show one call waiting for approval: what it does, the rule that asked,
    /// and its session.
    Show(CallArgs),
    /// Approve a waiting call: it runs, and the program carries on.
    Approve(DecideArgs),
    /// Deny a waiting call: the program's call throws `PermissionDeniedError`.
    Deny(DecideArgs),
}

#[derive(Args)]
pub struct ListArgs {
    /// Only calls waiting for this approver.
    #[arg(long, value_parser = ["human", "admin"])]
    approver: Option<String>,
    /// Only calls of this session.
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Args)]
pub struct CallArgs {
    /// The call's id, as `list` shows it.
    call_id: String,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Args)]
pub struct DecideArgs {
    /// The call's id, as `list` shows it.
    call_id: String,
    /// Why. A denial's reason reaches the program.
    #[arg(long)]
    reason: Option<String>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    target: ServerTarget,
}

pub fn execute(command: ApprovalCmd) -> Result<ExitCode> {
    match command {
        ApprovalCmd::List(args) => list(args),
        ApprovalCmd::Show(args) => show(args),
        ApprovalCmd::Approve(args) => decide(args, Decision::Approve),
        ApprovalCmd::Deny(args) => decide(args, Decision::Deny),
    }
}

#[derive(Clone, Copy)]
enum Decision {
    Approve,
    Deny,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
        }
    }

    fn done(self) -> &'static str {
        match self {
            Self::Approve => "Approved",
            Self::Deny => "Denied",
        }
    }
}

fn list(args: ListArgs) -> Result<ExitCode> {
    let agent = args.target.agent()?;
    let mut url = url::Url::parse(&approval_url(args.target.base(), None)?)?;
    {
        let mut query = url.query_pairs_mut();
        if let Some(approver) = &args.approver {
            query.append_pair("approver", approver);
        }
        if let Some(session) = &args.session {
            query.append_pair("session", session);
        }
    }
    let Some(response) = ok_or_report(agent.get(url.as_str()).call()?) else {
        return Ok(ExitCode::FAILURE);
    };
    let page: Value = response
        .into_body()
        .read_json()
        .context("invalid approvals response")?;
    let approvals = page["approvals"]
        .as_array()
        .context("approvals response has no approvals")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(approvals)?);
    } else if approvals.is_empty() {
        println!("No calls are waiting for approval.");
    } else {
        for approval in approvals {
            print_line(approval);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn show(args: CallArgs) -> Result<ExitCode> {
    let agent = args.target.agent()?;
    let url = approval_url(args.target.base(), Some(&args.call_id))?;
    let Some(response) = ok_or_report(agent.get(&url).call()?) else {
        return Ok(ExitCode::FAILURE);
    };
    let approval: Value = response
        .into_body()
        .read_json()
        .context("invalid approval response")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&approval)?);
        return Ok(ExitCode::SUCCESS);
    }
    print_line(&approval);
    let call = &approval["approval"];
    println!(
        "  context: {}",
        serde_json::to_string_pretty(&call["context"])?
    );
    let rule = &call["rule"];
    println!(
        "  rule: {} under `{}`{}{}",
        rule["index"]
            .as_u64()
            .and_then(|index| index.checked_add(1))
            .map_or_else(|| "?".to_owned(), |number| number.to_string()),
        rule["caller"].as_str().unwrap_or("?"),
        rule["name"]
            .as_str()
            .map(|name| format!(" ({name})"))
            .unwrap_or_default(),
        rule["filter"]
            .as_str()
            .map(|filter| format!(", when {filter}"))
            .unwrap_or_default(),
    );
    if let Some(variables) = approval["variables"].as_object()
        && !variables.is_empty()
    {
        println!("  session variables: {}", Value::Object(variables.clone()));
    }
    Ok(ExitCode::SUCCESS)
}

fn decide(args: DecideArgs, decision: Decision) -> Result<ExitCode> {
    let agent = args.target.agent()?;
    let url = approval_url(args.target.base(), Some(&args.call_id))?;
    let mut body = serde_json::json!({ "decision": decision.as_str() });
    if let Some(reason) = &args.reason {
        body["reason"] = Value::String(reason.clone());
    }
    let Some(response) = ok_or_report(agent.post(&url).send_json(body)?) else {
        return Ok(ExitCode::FAILURE);
    };
    let run: Value = response
        .into_body()
        .read_json()
        .context("invalid run response")?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        println!(
            "{} {}. The run is now {}.",
            decision.done(),
            args.call_id,
            run["status"].as_str().unwrap_or("?")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn approval_url(base: &str, call_id: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(base)?;
    let mut path = url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("server URL cannot have path segments"))?;
    path.pop_if_empty().extend(["v1", "approvals"]);
    if let Some(call_id) = call_id {
        path.push(call_id);
    }
    drop(path);
    Ok(url.into())
}

fn print_line(approval: &Value) {
    let call = &approval["approval"];
    println!(
        "{}  {}  {} by {}  session={}",
        approval["call_id"].as_str().unwrap_or("?"),
        call["approver"].as_str().unwrap_or("?"),
        call["capability"].as_str().unwrap_or("?"),
        call["caller"].as_str().unwrap_or("?"),
        approval["session_id"].as_str().unwrap_or("?"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn call_id_is_one_encoded_path_segment() {
        assert_eq!(
            approval_url("https://example.com", Some("a/b:c")).unwrap(),
            "https://example.com/v1/approvals/a%2Fb:c"
        );
        assert_eq!(
            approval_url("https://example.com/", None).unwrap(),
            "https://example.com/v1/approvals"
        );
    }
}
