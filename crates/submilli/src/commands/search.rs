//! `submilli search [query]` — list stdlib and installed packages matching a
//! query (name, description, or exported symbol). Omit the query to list all.
//! Runs offline. See [`discovery::Scope`] for what `--blueprint` changes.

use std::path::PathBuf;
use std::process::ExitCode;

use interpreter::packages;

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
    let hits: Vec<_> = packages::search(&query)
        .into_iter()
        .filter(|module| scope.allows(&module.name))
        .collect();
    let needle = query.trim().to_lowercase();
    let installed: Vec<String> = scope
        .installed_packages()
        .iter()
        .filter(|artifact| installed_matches(artifact, &needle))
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

/// The same three fields the stdlib search matches: name, description, and an
/// exported symbol.
fn installed_matches(artifact: &submilli_build::Artifact, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let metadata = &artifact.metadata;
    metadata.package_name.to_lowercase().contains(needle)
        || metadata.description.to_lowercase().contains(needle)
        || artifact
            .package_declaration
            .values
            .keys()
            .chain(artifact.package_declaration.types.keys())
            .any(|symbol| symbol.to_lowercase().contains(needle))
}
