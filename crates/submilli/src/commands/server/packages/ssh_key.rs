//! `submilli server packages ssh-key` — print the public half of the key a
//! running server installs private GitHub packages with, to add as a deploy
//! key. The private key never leaves the server.

use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct SshKeyResponse {
    public_key: String,
    fingerprint: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.target.base();
    let url = format!("{base}/v1/packages/ssh-key");

    let agent = args.target.agent()?;
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    if resp.status().as_u16() != 200 {
        eprintln!("error: {}", error_message(resp));
        return Ok(ExitCode::from(1));
    }
    let body: SshKeyResponse = resp
        .into_body()
        .read_json()
        .context("server returned malformed JSON")?;
    // The key alone on stdout, so it can be piped into `gh repo deploy-key add`.
    println!("{}", body.public_key);
    eprintln!("fingerprint {}", body.fingerprint);
    Ok(ExitCode::SUCCESS)
}
