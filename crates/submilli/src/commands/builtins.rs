//! `submilli builtins [names...]` — print TypeScript-style declarations for
//! language built-ins (the globals in scope without an `import`), or list the
//! catalog when no names are given. Runs offline; no server needed.

use std::process::ExitCode;

use interpreter::packages::{self, BuiltinLookup};

use super::discovery;

#[derive(clap::Args)]
pub struct Args {
    /// Built-in names to describe, e.g. `Array Map Temporal`. A dotted member
    /// path (`Temporal.Instant`) prints just that member. Omit to list the
    /// full catalog.
    names: Vec<String>,
}

impl Args {
    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        vec![("has_names", !self.names.is_empty())]
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    if args.names.is_empty() {
        let b = packages::builtins();
        println!("Types: {}", b.types.join(", "));
        println!("Namespaces: {}", b.namespaces.join(", "));
        return Ok(ExitCode::SUCCESS);
    }

    let mut ok = true;
    for (i, name) in args.names.iter().enumerate() {
        if i > 0 {
            println!();
        }
        match packages::builtin_lookup(name) {
            BuiltinLookup::Found(declarations) => println!("{declarations}"),
            BuiltinLookup::UnknownMember {
                path,
                member,
                members,
            } => {
                eprintln!(
                    "{}",
                    packages::unknown_member_message(&path, &member, &members)
                );
                ok = false;
            }
            BuiltinLookup::Unknown => {
                eprintln!("{}", discovery::builtin_miss_message(name));
                ok = false;
            }
        }
    }
    if !ok {
        eprintln!("\n{}", discovery::BUILTINS_LISTING_HINT);
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
