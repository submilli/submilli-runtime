//! Plain-text rendering of the shared discovery outcomes in
//! `interpreter::packages`, so `docs`, `builtins`, and `search` report the same
//! resolution decisions the MCP tools and REST endpoints do.
//!
//! The decisions themselves live in the interpreter; this module only chooses
//! words for them. The CLI has no blueprint binding, so `@mcp/<server>`
//! packages are out of reach here; packages installed in the local store need
//! no blueprint and are listed alongside the stdlib.

use interpreter::packages::{self, Resolution};
use submilli_build::{Artifact, PackageStore};

/// Every package in the local store whose artifact loads, in name order. A
/// directory that fails to load is skipped: the catalog is a listing, not a
/// diagnostic.
pub(crate) fn installed_packages() -> Vec<Artifact> {
    let store = PackageStore::default();
    store
        .available_packages()
        .iter()
        .filter_map(|name| store.load(name).ok())
        .collect()
}

/// One line for an installed package, falling back to name and version when
/// its manifest carries no description.
pub(crate) fn installed_summary(artifact: &Artifact) -> String {
    let metadata = &artifact.metadata;
    if metadata.description.trim().is_empty() {
        format!("{} — v{}", metadata.package_name, metadata.package_version)
    } else {
        format!("{} — {}", metadata.package_name, metadata.description)
    }
}

/// Report a name that resolved to neither a package nor a built-in.
pub(crate) fn report_miss(name: &str, outcome: Resolution) {
    if let Resolution::UnknownMember {
        path,
        member,
        members,
    } = outcome
    {
        eprintln!(
            "{}",
            packages::unknown_member_message(&path, &member, &members)
        );
        return;
    }
    eprintln!("unknown package: {name}{}\n", suggestion_clause(name));
    print_catalog();
}

/// A name the built-in catalog does not know. A package name gets the call to
/// make rather than the declarations: a package needs an `import` the caller
/// still has to write, so answering here would hide that step.
pub(crate) fn builtin_miss_message(name: &str) -> String {
    if packages::docs(name).is_some() {
        return packages::package_correcting_message(name, &format!("Run `submilli docs {name}`"));
    }
    format!("unknown built-in: {name}{}", suggestion_clause(name))
}

/// Where to find the catalog. Printed once per run, not once per bad name.
pub(crate) const BUILTINS_LISTING_HINT: &str =
    "Run `submilli builtins` to list available built-ins.";

/// List what does exist, plus where the language globals live.
pub(crate) fn print_catalog() {
    let catalog = packages::catalog(Vec::new());
    eprintln!("Available packages:");
    for entry in &catalog.entries {
        eprintln!("  {} — {}", entry.name, entry.description);
    }
    if catalog.remaining > 0 {
        eprintln!("  … and {} more", catalog.remaining);
    }
    let installed = installed_packages();
    if !installed.is_empty() {
        eprintln!("\nInstalled packages:");
        for artifact in &installed {
            eprintln!("  {}", installed_summary(artifact));
        }
    }
    eprintln!("\n{}", packages::builtins_pointer("`submilli builtins`"));
}

fn suggestion_clause(name: &str) -> String {
    match packages::suggest(name, &[]) {
        Some(hint) => format!(". Did you mean `{hint}`?"),
        None => String::new(),
    }
}
