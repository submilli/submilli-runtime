//! `submilli server packages uninstall <name>` — remove an installed package
//! from a running server's store.

use std::process::ExitCode;

use anyhow::Result;
use serde::Deserialize;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    /// Package to remove, e.g. `@submilli/jina`.
    name: String,

    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct UninstallResponse {
    #[serde(default)]
    still_available_from: Option<String>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.target.base();
    let url = format!("{base}/v1/packages/{}", args.name);

    let agent = args.target.agent()?;
    let resp = match agent.delete(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        println!("uninstalled {}", args.name);
        let body = resp.into_body().read_json::<UninstallResponse>();
        if let Ok(UninstallResponse {
            still_available_from: Some(root),
        }) = body
        {
            eprintln!("note: {} is still readable from {root}", args.name);
        }
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}
