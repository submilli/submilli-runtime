//! Read declarations from the server's blueprint-scoped package catalog.

use std::process::ExitCode;

use anyhow::{Context, Result};

#[derive(clap::Args)]
pub struct Args {
    /// Package name, including `@mcp/<server>` virtual packages.
    name: String,
    /// Registered blueprint whose package catalog to query.
    #[arg(long)]
    blueprint: String,
    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let mut url = url::Url::parse(&args.server).context("invalid server URL")?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("server URL cannot contain a path"))?
        .pop_if_empty()
        .extend(["v1", "blueprints", &args.blueprint, "packages", "docs"]);
    url.query_pairs_mut()
        .clear()
        .append_pair("name", &args.name);
    let response = ureq::get(url.as_str())
        .config()
        .http_status_as_error(false)
        .build()
        .call();
    let mut response = response.context("fetching package docs")?;
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
