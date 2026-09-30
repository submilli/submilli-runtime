//! `submilli server secret list [--prefix <p>] [--server <url>]` — print secret
//! keys one per line (never values).

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    /// Only list keys starting with this prefix.
    #[arg(long)]
    prefix: Option<String>,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    keys: Vec<String>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.target.base();
    let mut url = format!("{base}/v1/secrets");
    if let Some(prefix) = &args.prefix {
        url = format!("{url}?prefix={}", urlencode(prefix));
    }

    let agent = args.target.agent()?;
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        let body: ListResponse = resp
            .into_body()
            .read_json()
            .context("server returned malformed JSON")?;
        for key in body.keys {
            println!("{key}");
        }
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}

/// Percent-encode the bytes that matter in a query value: `/` stays a literal
/// key separator, so encode space, `&`, `=`, `?`, `#`, `%`, `+`.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'&' | b'=' | b'?' | b'#' | b'%' | b'+' | b' ' => {
                out.push_str(&format!("%{b:02X}"));
            }
            _ => out.push(b as char),
        }
    }
    out
}
