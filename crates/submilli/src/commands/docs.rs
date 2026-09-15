//! `submilli docs <name>` — print a stdlib package's TypeScript-style
//! declarations and description. Runs offline; no server needed.

use std::process::ExitCode;

use interpreter::packages::{self, Resolution};

use super::discovery;

#[derive(clap::Args)]
pub struct Args {
    /// Package name, e.g. `submilli:http`. A language built-in (`Temporal`,
    /// `Temporal.Instant`) resolves here too.
    name: String,
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    match packages::resolve(&args.name) {
        Resolution::Module(doc) => {
            println!("{} — {}\n", doc.name, doc.description);
            println!("{}", doc.declarations);
            Ok(ExitCode::SUCCESS)
        }
        // A built-in needs no `import`, so serving it here costs the caller
        // nothing — the same redirect `packages.docs` makes over MCP and REST.
        Resolution::Builtin { name, declarations } => {
            println!("{}\n", packages::builtin_no_import_note(&name));
            println!("{declarations}");
            Ok(ExitCode::SUCCESS)
        }
        other => {
            discovery::report_miss(&args.name, other);
            Ok(ExitCode::FAILURE)
        }
    }
}
