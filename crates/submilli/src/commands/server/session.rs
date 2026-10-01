//! `submilli server session` — open and close a session on a running
//! `submilli-server`, the way an application does for one conversation.
//! `submilli server run-code --session` runs programs inside it.

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Deserialize;
use ureq::http::StatusCode;

use crate::commands::http::{ServerTarget, error_message, ok_or_report};
use crate::commands::server::run_code::parse_variables;

#[derive(Subcommand)]
pub enum SessionCmd {
    /// Open a session bound to a registered blueprint and print its id.
    Open(OpenArgs),
    /// Close a session, discarding its files and session state.
    Close(CloseArgs),
}

#[derive(clap::Args)]
pub struct OpenArgs {
    /// Name of a blueprint registered on the server.
    #[arg(long)]
    blueprint: String,

    /// Bind a blueprint variable for the whole session, `NAME=VALUE`
    /// (repeatable).
    #[arg(long = "var", value_name = "NAME=VALUE")]
    vars: Vec<String>,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(clap::Args)]
pub struct CloseArgs {
    /// Id printed by `submilli server session open`.
    session: String,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Deserialize)]
struct CreateResponse {
    session_id: String,
}

pub fn execute(cmd: SessionCmd) -> Result<ExitCode> {
    match cmd {
        SessionCmd::Open(args) => open(args),
        SessionCmd::Close(args) => close(args),
    }
}

/// Prints only the id on stdout, so `SESSION=$(submilli server session open …)`
/// captures it.
fn open(args: OpenArgs) -> Result<ExitCode> {
    let variables = parse_variables(&args.vars)?;
    let base = args.target.base();
    let agent = args.target.agent()?;

    let mut request = serde_json::json!({ "blueprint": args.blueprint });
    if !variables.is_empty() {
        request["variables"] = serde_json::to_value(&variables)?;
    }
    let resp = match agent
        .post(&format!("{base}/v1/sessions"))
        .send_json(request)
    {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };
    let Some(resp) = ok_or_report(resp) else {
        return Ok(ExitCode::from(1));
    };
    let created: CreateResponse = resp
        .into_body()
        .read_json()
        .context("server returned malformed JSON")?;
    println!("{}", created.session_id);
    Ok(ExitCode::SUCCESS)
}

fn close(args: CloseArgs) -> Result<ExitCode> {
    let base = args.target.base();
    let agent = args.target.agent()?;

    let url = session_url(base, &args.session, &[])?;
    let resp = match agent.delete(&url).call() {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };
    match resp.status() {
        StatusCode::NO_CONTENT => {
            println!("closed session {}", args.session);
            Ok(ExitCode::SUCCESS)
        }
        StatusCode::NOT_FOUND => {
            eprintln!(
                "error: the server has no session '{}': it was closed, or its idle timeout \
                 expired",
                args.session
            );
            Ok(ExitCode::from(1))
        }
        _ => {
            eprintln!("error: {}", error_message(resp));
            Ok(ExitCode::from(1))
        }
    }
}

/// `{base}/v1/sessions/{session}/{tail…}`, with the id encoded as one path
/// segment so an id holding `/` or `?` cannot address another endpoint.
pub(super) fn session_url(base: &str, session: &str, tail: &[&str]) -> Result<String> {
    let mut url = url::Url::parse(base).with_context(|| format!("invalid server URL `{base}`"))?;
    url.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("invalid server URL `{base}`"))?
        .pop_if_empty()
        .extend(["v1", "sessions", session])
        .extend(tail);
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::session_url;

    #[test]
    fn session_urls_append_to_the_base_path() {
        assert_eq!(
            session_url("http://127.0.0.1:8128", "abc", &[]).unwrap(),
            "http://127.0.0.1:8128/v1/sessions/abc"
        );
        assert_eq!(
            session_url("https://example.com/submilli", "abc", &["execute"]).unwrap(),
            "https://example.com/submilli/v1/sessions/abc/execute"
        );
    }

    #[test]
    fn session_ids_stay_one_path_segment() {
        assert_eq!(
            session_url("http://127.0.0.1:8128", "../x?y", &[]).unwrap(),
            "http://127.0.0.1:8128/v1/sessions/..%2Fx%3Fy"
        );
    }
}
