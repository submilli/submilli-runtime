//! Read declarations from the server's blueprint-scoped package catalog.

use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::commands::http::{ServerTarget, error_message};
use ureq::http::StatusCode;

#[derive(clap::Args)]
pub struct Args {
    /// Package name, including `@mcp/<server>` virtual packages.
    name: String,
    /// Registered blueprint whose package catalog to query.
    #[arg(long)]
    blueprint: String,
    #[command(flatten)]
    target: ServerTarget,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let mut url = url::Url::parse(args.target.base()).context("invalid server URL")?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("server URL cannot contain a path"))?
        .pop_if_empty()
        .extend(["v1", "blueprints", &args.blueprint, "packages", "docs"]);
    url.query_pairs_mut()
        .clear()
        .append_pair("name", &args.name);
    let response = args.target.agent()?.get(url.as_str()).call();
    let mut response = response.context("fetching package docs")?;
    // Only a refused token is rewritten, because the fix for it is on this
    // side. Every other failure is printed as the server sent it.
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        eprintln!("error: {}", error_message(response));
        return Ok(ExitCode::FAILURE);
    }
    let success = response.status().is_success();
    let body = response
        .body_mut()
        .read_to_string()
        .context("reading package docs")?;
    if success {
        println!("{body}");
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("{body}");
        Ok(ExitCode::FAILURE)
    }
}
