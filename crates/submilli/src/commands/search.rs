//! `submilli search [query]` — list stdlib and installed packages matching a
//! query (name, description, or exported symbol). Omit the query to list all.
//! Runs offline. See [`discovery::Scope`] for what `--blueprint` changes.

use std::path::PathBuf;
use std::process::ExitCode;

use super::discovery::{self, Scope};

#[derive(clap::Args)]
pub struct Args {
    /// Substring to match against module names, descriptions, and exported
    /// symbols. Omit to list every package.
    query: Option<String>,
    /// List only what programs under this blueprint could import. Its
    /// `@mcp/<server>` packages are not listed: read one with `submilli docs
    /// @mcp/<server> --blueprint <path>`. Without it, the whole library is listed.
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

impl Args {
    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        vec![("has_query", self.query.is_some())]
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let scope = Scope::load(args.blueprint)?;
    let query = args.query.unwrap_or_default();
    let hits = scope.stdlib_search(&query);
    let needle = query.trim().to_lowercase();
    let installed: Vec<String> = scope
        .installed_packages()
        .iter()
        .filter(|package| package.matches(&needle))
        .map(discovery::installed_summary)
        .collect();
    if hits.is_empty() && installed.is_empty() {
        eprintln!("no packages match {query:?}\n");
        discovery::print_catalog(&scope);
        return Ok(ExitCode::SUCCESS);
    }
    for m in hits {
        println!("{} — {}", m.name, m.description);
    }
    for line in installed {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}
