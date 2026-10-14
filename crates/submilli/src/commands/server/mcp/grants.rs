//! `submilli server mcp grants …` — list the MCP OAuth grants a server holds, or
//! delete one. Grants are never printed with their tokens.

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Deserialize;
use submilli_shared::mcp_auth::GrantSummary;

use crate::commands::http::{ServerTarget, error_message};

#[derive(Subcommand)]
pub enum GrantsCmd {
    /// List the grants, one per blueprint and MCP server.
    List(ListArgs),
    /// Delete a grant, even one whose blueprint no longer declares the server.
    Delete(DeleteArgs),
}

#[derive(clap::Args)]
pub struct ListArgs {
    /// Only list this blueprint's grants.
    #[arg(long)]
    blueprint: Option<String>,
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(clap::Args)]
pub struct DeleteArgs {
    /// Blueprint the grant belongs to.
    blueprint: String,
    /// The MCP server's local name (its key in the `mcp:` block).
    server: String,
    #[command(flatten)]
    target: ServerTarget,
}

#[derive(Debug, Deserialize)]
struct GrantsResponse {
    grants: Vec<GrantSummary>,
}

pub fn execute(cmd: GrantsCmd) -> Result<ExitCode> {
    match cmd {
        GrantsCmd::List(args) => list(&args),
        GrantsCmd::Delete(args) => delete(&args),
    }
}

fn list(args: &ListArgs) -> Result<ExitCode> {
    let mut url = format!("{}/v1/mcp-grants", args.target.base());
    if let Some(blueprint) = &args.blueprint {
        url = format!(
            "{url}?blueprint={}",
            url::form_urlencoded::byte_serialize(blueprint.as_bytes()).collect::<String>()
        );
    }
    let resp = match args.target.agent()?.get(&url).call() {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };
    if resp.status().as_u16() != 200 {
        eprintln!("error: {}", error_message(resp));
        return Ok(ExitCode::from(1));
    }
    let body: GrantsResponse = resp
        .into_body()
        .read_json()
        .context("server returned malformed JSON")?;
    for grant in body.grants {
        println!("{}", describe(&grant));
    }
    Ok(ExitCode::SUCCESS)
}

/// One line per grant: `<blueprint>/<server>  <kind>  [scopes]`.
pub(crate) fn describe(grant: &GrantSummary) -> String {
    let mut line = format!(
        "{}/{}\t{}",
        grant.blueprint,
        grant.server,
        grant.kind.as_str()
    );
    if !grant.scopes.is_empty() {
        line.push_str(&format!("\tscopes: {}", grant.scopes.join(" ")));
    }
    line
}

fn delete(args: &DeleteArgs) -> Result<ExitCode> {
    let segment = |name: &str| {
        url::form_urlencoded::byte_serialize(name.as_bytes())
            .collect::<String>()
            .replace('+', "%20")
    };
    let url = format!(
        "{}/v1/mcp-grants/{}/{}",
        args.target.base(),
        segment(&args.blueprint),
        segment(&args.server)
    );
    let resp = match args.target.agent()?.delete(&url).call() {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };
    if resp.status().as_u16() != 200 {
        eprintln!("error: {}", error_message(resp));
        return Ok(ExitCode::from(1));
    }
    println!(
        "✓ deleted the grant for '{}' in blueprint '{}'",
        args.server, args.blueprint
    );
    Ok(ExitCode::SUCCESS)
}
