//! Plain-text rendering of the shared discovery outcomes in
//! `interpreter::packages`, so `docs`, `builtins`, and `search` report the same
//! resolution decisions the MCP tools and REST endpoints do.
//!
//! The decisions themselves live in the interpreter; this module only chooses
//! words for them. The CLI has no blueprint binding, so its catalog is
//! stdlib-only — correct here rather than a gap.

use interpreter::packages::{self, Resolution};

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
    eprintln!("\n{}", packages::builtins_pointer("`submilli builtins`"));
}

fn suggestion_clause(name: &str) -> String {
    match packages::suggest(name, &[]) {
        Some(hint) => format!(". Did you mean `{hint}`?"),
        None => String::new(),
    }
}
