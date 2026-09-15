//! `submilli mcp provider add/list/remove` — edit the local OAuth provider
//! config (`~/.submilli/mcp_oauth.yaml`). Secret *values* never pass through
//! here: `--client-secret` takes a reference (`${secrets.NAME}` / `${env.VAR}`),
//! and the referenced value lives in the local secret store (`submilli secret
//! put`) or the environment.

use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Subcommand;
use submilli_shared::mcp::oauth as mcp_oauth;

use super::provider_config::{ProviderEntry, load_file, save_file};

#[derive(Subcommand)]
pub enum ProviderCmd {
    /// Add or replace the provider matching a host.
    Add(AddArgs),
    /// List configured providers (secret references are shown; values are not).
    List,
    /// Remove the provider matching a host.
    Remove(RemoveArgs),
}

#[derive(clap::Args)]
pub struct AddArgs {
    /// OAuth host to match (e.g. `github.com`), compared against the token
    /// endpoint's host — not the MCP URL.
    #[arg(long = "match")]
    match_host: String,
    /// OAuth client id. A literal, or a `${secrets.X}` / `${env.X}` reference.
    #[arg(long)]
    client_id: String,
    /// OAuth client secret, as a reference: `${secrets.NAME}` (from the local
    /// store) or `${env.VAR}`. Omit for a public (PKCE-only) client.
    #[arg(long)]
    client_secret: Option<String>,
    /// OAuth scope to request (repeatable).
    #[arg(long = "scope")]
    scopes: Vec<String>,
}

#[derive(clap::Args)]
pub struct RemoveArgs {
    /// The `match` host of the provider to remove.
    #[arg(long = "match")]
    match_host: String,
}

pub fn execute(cmd: ProviderCmd) -> Result<ExitCode> {
    match cmd {
        ProviderCmd::Add(args) => add(args),
        ProviderCmd::List => list(),
        ProviderCmd::Remove(args) => remove(args),
    }
}

fn add(args: AddArgs) -> Result<ExitCode> {
    // A client secret must be a well-formed reference, never a literal value —
    // otherwise it lands in plaintext in the config file (and a *malformed* ref
    // is silently sent verbatim as a literal at exchange time). The value
    // belongs in the secret store; the config only names it.
    if let Some(secret) = &args.client_secret
        && !mcp_oauth::is_secret_ref(secret)
    {
        bail!(
            "--client-secret must be a reference, not a literal value: use \
             `${{secrets.NAME}}` (store the value with `submilli secret put NAME`) \
             or `${{env.VAR}}`"
        );
    }
    let mut file = load_file()?;
    let entry = ProviderEntry {
        match_host: args.match_host.clone(),
        client_id: args.client_id,
        client_secret: args.client_secret,
        scopes: args.scopes,
    };
    // One provider per host: replace an existing match rather than duplicate it.
    match file
        .providers
        .iter_mut()
        .find(|p| p.match_host == args.match_host)
    {
        Some(existing) => *existing = entry,
        None => file.providers.push(entry),
    }
    save_file(&file)?;
    println!("✓ configured OAuth provider for {}", args.match_host);
    Ok(ExitCode::SUCCESS)
}

fn list() -> Result<ExitCode> {
    let file = load_file()?;
    if file.providers.is_empty() {
        println!(
            "no OAuth providers configured ({})",
            super::provider_config::config_path().display()
        );
        return Ok(ExitCode::SUCCESS);
    }
    for p in &file.providers {
        let secret = if p.client_secret.is_some() {
            "confidential"
        } else {
            "public (no secret)"
        };
        let scopes = if p.scopes.is_empty() {
            String::new()
        } else {
            format!(" scopes=[{}]", p.scopes.join(", "))
        };
        println!(
            "{:<24} client_id={} {}{}",
            p.match_host, p.client_id, secret, scopes
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn remove(args: RemoveArgs) -> Result<ExitCode> {
    let mut file = load_file()?;
    let before = file.providers.len();
    file.providers.retain(|p| p.match_host != args.match_host);
    if file.providers.len() == before {
        bail!("no OAuth provider matches host '{}'", args.match_host);
    }
    save_file(&file)?;
    println!("✓ removed OAuth provider for {}", args.match_host);
    Ok(ExitCode::SUCCESS)
}
