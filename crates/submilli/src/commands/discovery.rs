//! Plain-text rendering of the shared discovery outcomes in
//! `interpreter::packages`, so `docs`, `builtins`, and `search` report the same
//! resolution decisions the MCP tools and REST endpoints do.
//!
//! The decisions themselves live in the interpreter; this module only chooses
//! words for them, and what a [`Scope`] lets a command show.

use std::path::PathBuf;

use anyhow::Context;
use interpreter::Stdlib;
use interpreter::docs_view::Unscoped;
use interpreter::packages::{self, Catalog, ModuleDoc, ModuleSummary, Resolution, StdlibView};
use submilli_blueprint::Blueprint;
use submilli_build::PackageStore;
use submilli_shared::docs_scope::{DocsScope, PackageView};

/// What a discovery command shows. Without `--blueprint` it is the whole
/// library, opt-in `submilli:git` and every installed package included, even
/// when a `blueprint.yaml` sits in the directory: the author has to read an
/// API before deciding to enable it. With `--blueprint` it is what that
/// blueprint's programs could use, the view the server gives a bound
/// blueprint: only the operations its rules could allow, and only the parts
/// of a package's documentation about them.
pub(crate) enum Scope {
    Library {
        stdlib: StdlibView,
    },
    Blueprint {
        path: PathBuf,
        blueprint: Box<Blueprint>,
        docs: Box<DocsScope>,
    },
}

impl Scope {
    /// The whole library.
    fn library() -> Self {
        Self::Library {
            stdlib: Stdlib::core().view(&Unscoped),
        }
    }

    pub(crate) fn load(path: Option<PathBuf>) -> anyhow::Result<Self> {
        let Some(path) = path else {
            return Ok(Self::library());
        };
        let yaml = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let blueprint = submilli_blueprint::parse(&yaml)
            .with_context(|| format!("parsing {}", path.display()))?;
        let artifacts = submilli_shared::docs_scope::load_declared(
            &blueprint.packages,
            &PackageStore::default(),
        );
        let docs = DocsScope::for_blueprint(&blueprint, &artifacts);
        Ok(Self::Blueprint {
            path,
            blueprint: Box::new(blueprint),
            docs: Box::new(docs),
        })
    }

    /// Whether a name is shown. Built-ins need no `import`, so every scope has
    /// them; packages (stdlib, `@mcp/<server>`, and installed `@scope/name`)
    /// follow the blueprint.
    pub(crate) fn allows(&self, name: &str) -> bool {
        let Self::Blueprint {
            blueprint, docs, ..
        } = self
        else {
            return true;
        };
        if is_stdlib(name) {
            return docs.allows(name);
        }
        if !name.starts_with('@') {
            return true;
        }
        match name.strip_prefix("@mcp/") {
            Some(server) => blueprint.mcp.contains_key(server),
            None => docs.declares(name) && docs.allows(name),
        }
    }

    /// The standard library modules the scope shows.
    fn stdlib(&self) -> &StdlibView {
        match self {
            Self::Library { stdlib } => stdlib,
            Self::Blueprint { docs, .. } => docs.stdlib(),
        }
    }

    /// A standard library module's documentation, narrowed to the scope.
    pub(crate) fn stdlib_docs(&self, name: &str) -> Option<ModuleDoc> {
        self.stdlib().docs(name)
    }

    /// A bare name (`fs`) against the modules this scope shows, plus the other
    /// full names it can resolve, so a colliding package is not hidden.
    pub(crate) fn match_bare_name(&self, name: &str) -> packages::BareNameMatch {
        let stdlib = self.stdlib();
        // A miss cannot become a hit by adding other catalogs' names, and the
        // caller's miss path lists the installed packages itself.
        if !stdlib.has_bare_tail(name) {
            return packages::BareNameMatch::Miss;
        }
        let others = self.other_package_names();
        stdlib.match_bare_name(name, &others)
    }

    /// The full names a bare name could also mean: the installed packages this
    /// scope shows. MCP servers are discovered server-side, so a declared but
    /// unreachable `@mcp/<server>` is not a name the CLI can resolve.
    fn other_package_names(&self) -> Vec<String> {
        match self {
            Self::Library { .. } => {
                let store = PackageStore::default();
                // Names only: the catalog view needs rendered declarations, but
                // a collision check does not. An `@mcp/*` artifact is not a
                // package `docs` can resolve; those names route to discovery.
                store
                    .available_packages()
                    .into_iter()
                    .filter(|name| !name.starts_with("@mcp/") && store.load(name).is_ok())
                    .collect()
            }
            Self::Blueprint { docs, .. } => docs
                .usable_packages()
                .map(|package| package.name.clone())
                .collect(),
        }
    }

    /// Standard library modules matching `query`, within the scope.
    pub(crate) fn stdlib_search(&self, query: &str) -> Vec<ModuleSummary> {
        self.stdlib().search(query)
    }

    /// An installed package as the scope shows it, if the scope has it.
    pub(crate) fn installed_package(&self, name: &str) -> Option<PackageView> {
        match self {
            Self::Library { .. } => PackageStore::default()
                .load(name)
                .ok()
                .map(|artifact| PackageView::new(&artifact, &Unscoped)),
            Self::Blueprint { docs, .. } => docs.package(name).cloned(),
        }
    }

    /// Installed packages this scope shows, in name order.
    pub(crate) fn installed_packages(&self) -> Vec<PackageView> {
        match self {
            Self::Library { .. } => {
                let store = PackageStore::default();
                store
                    .available_packages()
                    .iter()
                    // A directory that fails to load is skipped: the catalog is
                    // a listing, not a diagnostic.
                    .filter_map(|name| store.load(name).ok())
                    .map(|artifact| PackageView::new(&artifact, &Unscoped))
                    .collect()
            }
            Self::Blueprint { docs, .. } => docs.usable_packages().cloned().collect(),
        }
    }

    fn catalog(&self) -> Catalog {
        self.stdlib()
            .catalog_filtered(Vec::new(), |name| self.allows(name))
    }

    fn suggest(&self, name: &str) -> Option<String> {
        self.stdlib()
            .suggest_filtered(name, &[], |name| self.allows(name))
    }

    /// Why a package that exists is not shown, with the change that shows it.
    pub(crate) fn hidden_message(&self, name: &str) -> String {
        let path = match self {
            Self::Library { .. } => return format!("{name} is not available."),
            Self::Blueprint { path, .. } => path.display(),
        };
        let declared = match self {
            Self::Blueprint { docs, .. } => docs.declares(name),
            Self::Library { .. } => false,
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
            "submilli:embedding" => format!(
                "it needs an embedding alias and a rule allowing `embedding.embed`. Run \
                 `submilli blueprint capability list {name}`"
            ),
            _ if is_stdlib(name) || declared => format!(
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
pub(crate) fn installed_summary(package: &PackageView) -> String {
    if package.description.trim().is_empty() {
        format!("{} — v{}", package.name, package.version)
    } else {
        format!("{} — {}", package.name, package.description)
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
        suggestion_clause(name, &Scope::library())
    )
}

/// Where to find the catalog. Printed once per run, not once per bad name.
pub(crate) const BUILTINS_LISTING_HINT: &str =
    "Run `submilli builtins` to list available built-ins.";

/// List what does exist, plus where the language globals live.
pub(crate) fn print_catalog(scope: &Scope) {
    let catalog = scope.catalog();
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
        for package in &installed {
            eprintln!("  {}", installed_summary(package));
        }
    }
    eprintln!("\n{}", packages::builtins_pointer("`submilli builtins`"));
}

fn suggestion_clause(name: &str, scope: &Scope) -> String {
    match scope.suggest(name) {
        Some(hint) => format!(". Did you mean `{hint}`?"),
        None => String::new(),
    }
}
