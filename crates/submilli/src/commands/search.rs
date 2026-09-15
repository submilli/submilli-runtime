//! `submilli search [query]` — list stdlib packages matching a query (name,
//! description, or exported symbol). Omit the query to list all. Runs offline.

use std::process::ExitCode;

use interpreter::packages;

use super::discovery;

#[derive(clap::Args)]
pub struct Args {
    /// Substring to match against module names, descriptions, and exported
    /// symbols. Omit to list every package.
    query: Option<String>,
}

impl Args {
    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        vec![("has_query", self.query.is_some())]
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let query = args.query.unwrap_or_default();
    let hits = packages::search(&query);
    if hits.is_empty() {
        eprintln!("no packages match {query:?}\n");
        discovery::print_catalog();
        return Ok(ExitCode::SUCCESS);
    }
    for m in hits {
        println!("{} — {}", m.name, m.description);
    }
    Ok(ExitCode::SUCCESS)
}
