//! `submilli server packages uninstall <name>` — remove an installed package
//! from a running server's store.

use std::process::ExitCode;

use anyhow::Result;
use serde::Deserialize;

#[derive(clap::Args)]
pub struct Args {
    /// Package to remove, e.g. `@submilli/jina`.
    name: String,

    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

#[derive(Debug, Deserialize)]
struct UninstallResponse {
    #[serde(default)]
    still_available_from: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[allow(dead_code)]
    error: String,
    message: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.server.trim_end_matches('/');
    let url = format!("{base}/v1/packages/{}", args.name);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
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
        match resp.into_body().read_json::<ErrorResponse>() {
            Ok(err) => eprintln!("error: {}", err.message),
            Err(_) => eprintln!("error: server returned HTTP {status}"),
        }
        Ok(ExitCode::from(1))
    }
}
