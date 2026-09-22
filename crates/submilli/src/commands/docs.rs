//! `submilli docs <name>` — print a stdlib or installed package's
//! TypeScript-style declarations and description. Runs offline; no server
//! needed.

use std::process::ExitCode;

use interpreter::packages::{self, Resolution};
use submilli_build::PackageStore;

use super::discovery;

#[derive(clap::Args)]
pub struct Args {
    /// Package name, e.g. `submilli:http` or an installed `@org/name`. A
    /// language built-in (`Temporal`, `Temporal.Instant`) resolves here too.
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
            // Installed packages come after the stdlib and the built-ins: a
            // `submilli:*` or built-in name can never be shadowed by a store entry.
            if let Ok(artifact) = PackageStore::default().load(&args.name) {
                println!("{}\n", discovery::installed_summary(&artifact));
                println!(
                    "{}",
                    packages::render_declarations(&artifact.package_declaration)
                );
                return Ok(ExitCode::SUCCESS);
            }
            discovery::report_miss(&args.name, other);
            Ok(ExitCode::FAILURE)
        }
    }
}
