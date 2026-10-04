//! Plain-text rendering of the shared discovery outcomes in
//! `interpreter::packages`, so `docs`, `builtins`, and `search` report the same
//! resolution decisions the MCP tools and REST endpoints do.
//!
//! The decisions themselves live in the interpreter; this module only chooses
//! words for them, and what a [`Scope`] lets a command show.

use std::path::PathBuf;

use anyhow::Context;
use interpreter::packages::{self, Resolution};
use submilli_blueprint::Blueprint;
use submilli_build::{Artifact, PackageStore};
use submilli_shared::library_visibility::LibraryVisibility;

/// What a discovery command shows. Without `--blueprint` it is the whole
/// library, opt-in `submilli:git` and every installed package included, even
/// when a `blueprint.yaml` sits in the directory: the author has to read an
/// API before deciding to enable it. With `--blueprint` it is what that
/// blueprint's programs could import, the view the server gives a bound
/// blueprint.
pub(crate) enum Scope {
    Library,
    Blueprint {
        path: PathBuf,
        blueprint: Box<Blueprint>,
        visibility: LibraryVisibility,
    },
}

impl Scope {
    pub(crate) fn load(path: Option<PathBuf>) -> anyhow::Result<Self> {
        let Some(path) = path else {
            return Ok(Self::Library);
        };
        let yaml = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let blueprint = submilli_blueprint::parse(&yaml)
            .with_context(|| format!("parsing {}", path.display()))?;
        let visibility = LibraryVisibility::for_blueprint(&blueprint);
        Ok(Self::Blueprint {
            path,
            blueprint: Box::new(blueprint),
            visibility,
        })
    }

    /// Whether a name is shown. Built-ins need no `import`, so every scope has
    /// them; packages (stdlib, `@mcp/<server>`, and installed `@scope/name`)
    /// follow the blueprint.
    pub(crate) fn allows(&self, name: &str) -> bool {
        let Self::Blueprint {
            blueprint,
            visibility,
            ..
        } = self
        else {
            return true;
        };
        if is_stdlib(name) {
            return visibility.allows(name);
        }
        if !name.starts_with('@') {
            return true;
        }
        match name.strip_prefix("@mcp/") {
            Some(server) => blueprint.mcp.contains_key(server),
            None => blueprint.packages.contains(name),
        }
    }

    /// Installed packages this scope shows, in name order.
    pub(crate) fn installed_packages(&self) -> Vec<Artifact> {
        let store = PackageStore::default();
        store
            .available_packages()
            .iter()
            .filter(|name| self.allows(name))
            // A directory that fails to load is skipped: the catalog is a
            // listing, not a diagnostic.
            .filter_map(|name| store.load(name).ok())
            .collect()
    }

    /// Why a package that exists is not shown, with the change that shows it.
    pub(crate) fn hidden_message(&self, name: &str) -> String {
        let path = match self {
            Self::Library => return format!("{name} is not available."),
            Self::Blueprint { path, .. } => path.display(),
        };
        let fix = match name {
            "submilli:git" => format!(
                "it sets no Git identity. Run `submilli blueprint git set --name <name> \
                 --email <email> --blueprint {path}`"
            ),
            "submilli:llm" => format!(
                "it needs a model and a rule allowing `llm.call`. Run `submilli blueprint \
                 capability list {name}`"
            ),
            _ if is_stdlib(name) => format!(
                "no rule for caller `main` allows one of its capabilities. Run `submilli \
                 blueprint capability list {name}`"
            ),
            _ => format!(
                "it does not declare the package. Run `submilli blueprint add-package {name} \
                 --blueprint {path}`"
            ),
        };
        format!(
            "{name} is not available to programs under {path}: {fix}, or drop `--blueprint` \
             to read the whole library."
        )
    }
}

fn is_stdlib(name: &str) -> bool {
    name.starts_with("submilli:")
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
pub(crate) fn report_miss(name: &str, outcome: Resolution, scope: &Scope) {
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
    eprintln!(
        "unknown package: {name}{}\n",
        suggestion_clause(name, scope)
    );
    print_catalog(scope);
}

/// A name the built-in catalog does not know. A package name gets the call to
/// make rather than the declarations: a package needs an `import` the caller
/// still has to write, so answering here would hide that step.
pub(crate) fn builtin_miss_message(name: &str) -> String {
    if packages::docs(name).is_some() {
        return packages::package_correcting_message(name, &format!("Run `submilli docs {name}`"));
    }
    format!(
        "unknown built-in: {name}{}",
        suggestion_clause(name, &Scope::Library)
    )
}

/// Where to find the catalog. Printed once per run, not once per bad name.
pub(crate) const BUILTINS_LISTING_HINT: &str =
    "Run `submilli builtins` to list available built-ins.";

/// List what does exist, plus where the language globals live.
pub(crate) fn print_catalog(scope: &Scope) {
    let catalog = packages::catalog_filtered(Vec::new(), |name| scope.allows(name));
    eprintln!("Available packages:");
    for entry in &catalog.entries {
        eprintln!("  {} — {}", entry.name, entry.description);
    }
    if catalog.remaining > 0 {
        eprintln!("  … and {} more", catalog.remaining);
    }
    let installed = scope.installed_packages();
    if !installed.is_empty() {
        eprintln!("\nInstalled packages:");
        for artifact in &installed {
            eprintln!("  {}", installed_summary(artifact));
        }
    }
    eprintln!("\n{}", packages::builtins_pointer("`submilli builtins`"));
}

fn suggestion_clause(name: &str, scope: &Scope) -> String {
    match packages::suggest_filtered(name, &[], |name| scope.allows(name)) {
        Some(hint) => format!(". Did you mean `{hint}`?"),
        None => String::new(),
    }
}
