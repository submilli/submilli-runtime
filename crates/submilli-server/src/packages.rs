//! Package discovery for the MCP tools and the REST surface. The stdlib
//! rendering lives in `interpreter::packages`; this adds `@mcp/<server>` virtual
//! packages (rendered from a blueprint's discovered catalog) and the shared JSON
//! response shapes so the two transports can't drift.

use serde_json::{Value, json};
use submilli_blueprint::Blueprint;
use submilli_build::PackageStore;
use submilli_shared::library_visibility::LibraryVisibility;

use interpreter::packages::CatalogEntry;

use crate::mcp::McpCatalog;

/// Which transport is asking, so a correcting call names a call the caller can
/// actually make. The MCP tool and the REST endpoint reach the same resolution
/// but are invoked differently; naming the other one is its own dead end.
#[derive(Clone, Copy)]
pub(crate) enum Fetch<'a> {
    Mcp,
    /// The REST routes, which are nested under the blueprint named here.
    Rest(&'a str),
}

impl Fetch<'_> {
    fn package_docs(self, name: &str) -> String {
        match self {
            Fetch::Mcp => {
                format!("Fetch it with submilli__typescript__packages__docs(\"{name}\")")
            }
            Fetch::Rest(blueprint) => {
                format!("Fetch it from `GET /v1/blueprints/{blueprint}/packages/docs?name={name}`")
            }
        }
    }

    pub(crate) fn builtins_pointer(self) -> String {
        match self {
            Fetch::Mcp => {
                interpreter::packages::builtins_pointer("`submilli__typescript__builtins__docs`")
            }
            Fetch::Rest(blueprint) => interpreter::packages::builtins_pointer(&format!(
                "`GET /v1/blueprints/{blueprint}/builtins/docs?name=<name>`"
            )),
        }
    }
}

pub(crate) enum DocLookup {
    Host {
        description: String,
        declarations: String,
    },
    /// A discovered `@mcp/<server>` package's typed surface.
    Mcp {
        description: String,
        declarations: String,
    },
    Registry {
        description: String,
        declarations: String,
        documentation: String,
    },
    /// A language built-in reached through a package-shaped surface: always in
    /// scope, so serving it costs the caller nothing (unlike the reverse
    /// direction, where a package needs an `import` the model must write).
    Builtin { declarations: String },
    /// `@mcp/<server>` for a server this blueprint doesn't declare.
    McpUnknown,
    Unknown {
        message: String,
        /// Closest name across every catalog this call site could see.
        suggestion: Option<String>,
    },
}

/// Look up a stdlib package's docs. `@mcp/*` names always resolve to
/// [`DocLookup::McpUnknown`] here — they're blueprint-scoped, so use
/// [`lookup_with_catalog`] where a blueprint is bound.
pub(crate) fn lookup(name: &str) -> DocLookup {
    if name.starts_with("@mcp/") {
        return DocLookup::McpUnknown;
    }
    host_lookup(name, LibraryVisibility::unscoped())
        .unwrap_or_else(|| builtin_fallback(name, &[], LibraryVisibility::unscoped()))
}

/// Like [`lookup`], but resolves `@mcp/<server>` names against a blueprint's
/// discovered catalog.
pub(crate) fn lookup_with_catalog(name: &str, catalog: &McpCatalog) -> DocLookup {
    if name.starts_with("@mcp/") {
        if let Some(reason) = catalog.unavailable_reason(name.trim_start_matches("@mcp/")) {
            return DocLookup::Unknown {
                message: format!("`{name}` is declared but unavailable: {reason}"),
                suggestion: None,
            };
        }
        return match catalog.package(name) {
            Some(pkg) => DocLookup::Mcp {
                description: pkg.description(),
                declarations: interpreter::packages::render_declarations(&pkg.defs),
            },
            None => DocLookup::McpUnknown,
        };
    }
    host_lookup(name, LibraryVisibility::unscoped()).unwrap_or_else(|| {
        builtin_fallback(name, &catalog_names(catalog), LibraryVisibility::unscoped())
    })
}

/// Like [`lookup_with_catalog`], but also resolves registry packages declared
/// by the bound blueprint. The global REST docs endpoint is not blueprint-bound,
/// so registry packages intentionally surface only through the MCP tool.
pub(crate) fn lookup_with_blueprint(
    name: &str,
    catalog: &McpCatalog,
    blueprint: &Blueprint,
    store: &PackageStore,
) -> DocLookup {
    if name.starts_with("@mcp/") {
        return lookup_with_catalog(name, catalog);
    }
    if let Some(found) = host_lookup(name, LibraryVisibility::for_blueprint(blueprint)) {
        return found;
    }
    if blueprint.packages.contains(name) {
        // A declared package whose artifact is missing is not an unknown name:
        // suggesting alternatives (or the name itself) would hide the real fix.
        return match registry_lookup(name, store) {
            Some(found) => found,
            None => DocLookup::Unknown {
                message: format!(
                    "`{name}` is declared by this blueprint but its artifact could not be \
                     loaded — run `submilli install` to fetch it."
                ),
                suggestion: None,
            },
        };
    }
    builtin_fallback(
        name,
        &union_candidates(catalog, blueprint),
        LibraryVisibility::for_blueprint(blueprint),
    )
}

fn host_lookup(name: &str, visibility: LibraryVisibility) -> Option<DocLookup> {
    if !visibility.allows(name) {
        return None;
    }
    interpreter::packages::docs(name).map(|doc| DocLookup::Host {
        description: doc.description,
        declarations: doc.declarations,
    })
}

/// A name no package source claims. Check the built-in catalog before erroring
/// — a built-in needs no `import`, so serving it here costs the caller nothing
/// — and carry a suggestion drawn from every catalog this call site can see.
fn builtin_fallback(name: &str, extra: &[String], visibility: LibraryVisibility) -> DocLookup {
    use interpreter::packages::BuiltinLookup;
    match interpreter::packages::builtin_lookup(name) {
        BuiltinLookup::Found(declarations) => DocLookup::Builtin { declarations },
        BuiltinLookup::UnknownMember {
            path,
            member,
            members,
        } => DocLookup::Unknown {
            message: interpreter::packages::unknown_member_message(&path, &member, &members),
            suggestion: None,
        },
        BuiltinLookup::Unknown => DocLookup::Unknown {
            message: format!("unknown package: {name}"),
            suggestion: interpreter::packages::suggest_filtered(name, extra, |name| {
                visibility.allows(name)
            }),
        },
    }
}

fn catalog_names(catalog: &McpCatalog) -> Vec<String> {
    catalog.search("").into_iter().map(|m| m.name).collect()
}

/// Every name this call site could have resolved, for did-you-mean.
fn union_candidates(catalog: &McpCatalog, blueprint: &Blueprint) -> Vec<String> {
    let mut names = catalog_names(catalog);
    names.extend(blueprint.packages.iter().cloned());
    names
}

/// `packages.docs` body — the same shape over MCP and REST (REST adds a status
/// code; see `handlers::packages`).
pub(crate) fn docs_json(name: &str, lookup: DocLookup) -> Value {
    match lookup {
        DocLookup::Host {
            description,
            declarations,
        } => json!({
            "name": name,
            "source": "host",
            "description": description,
            "declarations": declarations,
        }),
        DocLookup::Mcp {
            description,
            declarations,
        } => json!({
            "name": name,
            "source": "mcp",
            "description": description,
            "declarations": declarations,
        }),
        DocLookup::Registry {
            description,
            declarations,
            ..
        } => json!({
            "name": name,
            "source": "registry",
            "description": description,
            "declarations": declarations,
        }),
        DocLookup::Builtin { declarations } => json!({
            "name": name,
            "source": "builtin",
            // The `source` tag is a machine field; say it in prose too, or a
            // model reading only the description writes an `import` for a global.
            "description": interpreter::packages::builtin_no_import_note(name),
            "declarations": declarations,
        }),
        DocLookup::McpUnknown => json!({
            "error": "unknown_mcp_server",
            "message": format!("unknown MCP server: {name}"),
        }),
        DocLookup::Unknown {
            message,
            suggestion: Some(hint),
        } => json!({
            "error": "unknown_package",
            "message": format!("{message}. Did you mean `{hint}`?"),
            "did_you_mean": hint,
        }),
        DocLookup::Unknown {
            message,
            suggestion: None,
        } => json!({
            "error": "unknown_package",
            "message": message,
        }),
    }
}

pub(crate) fn docs_markdown(lookup: &DocLookup) -> Option<String> {
    match lookup {
        DocLookup::Registry {
            documentation,
            declarations,
            ..
        } => Some(docs_body_markdown(documentation, declarations)),
        // `@mcp/<server>` docs render as markdown too (the one-line description as
        // the intro), so the tool returns a string like registry packages do.
        DocLookup::Mcp {
            description,
            declarations,
        } => Some(docs_body_markdown(description, declarations)),
        _ => None,
    }
}

/// One built-in's entry — `{ name, declarations }`, or `{ name, error }` when
/// unknown. The shared per-name shape behind the MCP batch and the REST
/// single-name endpoint, so the two can't drift.
///
/// `mcp_packages` are the `@mcp/*` names the bound blueprint declares. Without
/// a binding the slice is empty and those names keep their existing behavior.
pub(crate) fn builtin_entry_json(
    name: &str,
    mcp_packages: &[String],
    fetch_with: &Fetch<'_>,
    visibility: LibraryVisibility,
) -> Value {
    use interpreter::packages::BuiltinLookup;
    match interpreter::packages::builtin_lookup(name) {
        BuiltinLookup::Found(declarations) => json!({ "name": name, "declarations": declarations }),
        BuiltinLookup::UnknownMember {
            path,
            member,
            members,
        } => json!({
            "name": name,
            "error": "unknown_builtin",
            "message": interpreter::packages::unknown_member_message(&path, &member, &members),
        }),
        BuiltinLookup::Unknown => {
            package_correcting_call(name, mcp_packages, fetch_with, visibility)
                .unwrap_or_else(|| unknown_builtin_entry(name, mcp_packages, visibility))
        }
    }
}

/// A name this tool recognizes as a package. Forgiveness is asymmetric on
/// purpose: `packages.docs` on a built-in serves it, because a built-in needs
/// no `import`. Going the other way, the tool names the call to make instead of
/// serving declarations — a package needs an `import` the model has to write,
/// and answering here would hide the step that distinguishes the namespaces.
///
/// Registry names are deliberately not recognized (they'd need the package
/// store threaded in), so they land on the both-miss path with a suggestion.
fn package_correcting_call(
    name: &str,
    mcp_packages: &[String],
    fetch_with: &Fetch<'_>,
    visibility: LibraryVisibility,
) -> Option<Value> {
    if !visibility.allows(name) {
        return None;
    }
    let is_package = interpreter::packages::docs(name).is_some()
        || (name.starts_with("@mcp/") && mcp_packages.iter().any(|pkg| pkg == name));
    if !is_package {
        return None;
    }
    // A distinct code, so a consumer matching on it can tell a correcting call
    // from a name that exists nowhere.
    Some(json!({
        "name": name,
        "error": "not_a_builtin",
        "message": interpreter::packages::package_correcting_message(
            name,
            &fetch_with.package_docs(name),
        ),
    }))
}

fn unknown_builtin_entry(
    name: &str,
    mcp_packages: &[String],
    visibility: LibraryVisibility,
) -> Value {
    let message = format!("unknown built-in: {name}");
    // Built-in corrections have never advertised the opt-in Git module.
    match interpreter::packages::suggest_filtered(name, mcp_packages, |name| {
        name != "submilli:git" && visibility.allows(name)
    }) {
        Some(hint) => json!({
            "name": name,
            "error": "unknown_builtin",
            "message": format!("{message}. Did you mean `{hint}`?"),
            "did_you_mean": hint,
        }),
        None => json!({ "name": name, "error": "unknown_builtin", "message": message }),
    }
}

/// `builtins.docs` body — `{ results: [{ name, declarations }] }`, one entry
/// per requested name. Unknown names come back as `{ name, error }` inline
/// rather than failing the batch.
pub(crate) fn builtins_docs_json(
    names: &[String],
    mcp_packages: &[String],
    fetch_with: &Fetch<'_>,
    visibility: LibraryVisibility,
) -> Value {
    let results: Vec<Value> = names
        .iter()
        .map(|n| builtin_entry_json(n, mcp_packages, fetch_with, visibility))
        .collect();
    json!({ "results": results })
}

/// The `@mcp/*` names a bound blueprint declares, for the builtins tool's
/// package recognition.
pub(crate) fn mcp_package_names(catalog: &McpCatalog) -> Vec<String> {
    catalog_names(catalog)
}

/// The built-in catalog — `{ types: [...], namespaces: [...] }`. Mirrors the
/// `{builtins}` prompt placeholder; backs the REST built-ins listing.
pub(crate) fn builtins_list_json() -> Value {
    let b = interpreter::packages::builtins();
    json!({ "types": b.types, "namespaces": b.namespaces })
}

/// `packages.search` body — `{ results: [{ name, source, description }] }`.
/// Stdlib modules only; for a blueprint's discovered `@mcp/<server>` packages use
/// [`search_json_with_catalog`].
pub(crate) fn search_json(query: &str, fetch_with: Fetch<'_>) -> Value {
    search_json_with_catalog(query, &McpCatalog::empty(), fetch_with)
}

/// Like [`search_json`], but also folds in a blueprint's discovered
/// `@mcp/<server>` packages, tagged `source: "mcp"`, after the stdlib hits.
pub(crate) fn search_json_with_catalog(
    query: &str,
    catalog: &McpCatalog,
    fetch_with: Fetch<'_>,
) -> Value {
    let results = search_json_results(query, catalog, LibraryVisibility::unscoped());
    search_body(results, fetch_with, LibraryVisibility::unscoped(), || {
        mcp_entries(catalog)
    })
}

pub(crate) fn search_json_with_blueprint(
    query: &str,
    catalog: &McpCatalog,
    blueprint: &Blueprint,
    store: &PackageStore,
    fetch_with: Fetch<'_>,
) -> Value {
    // Load each declared artifact once and derive both views from it: the hit
    // list and, only on a miss, the catalog listing.
    let artifacts: Vec<_> = blueprint
        .packages
        .iter()
        .filter_map(|name| store.load(name).ok())
        .collect();
    let mut results =
        search_json_results(query, catalog, LibraryVisibility::for_blueprint(blueprint));
    results.extend(registry_search(query, &artifacts));
    search_body(
        results,
        fetch_with,
        LibraryVisibility::for_blueprint(blueprint),
        || {
            let mut entries = mcp_entries(catalog);
            entries.extend(registry_entries(&artifacts));
            entries
        },
    )
}

/// Wrap a completed result set. A zero-hit search answers with what *is*
/// available rather than a bare empty list.
///
/// Callers hand their set in already complete — the miss branch cannot live in
/// [`search_json_results`], which returns before `search_json_with_blueprint`
/// appends its registry hits, so a registry-only match would emit the catalog
/// beside a real hit.
///
/// `available` is a closure so a hit never pays to assemble the listing.
fn search_body(
    results: Vec<Value>,
    fetch_with: Fetch<'_>,
    visibility: LibraryVisibility,
    available: impl FnOnce() -> Vec<CatalogEntry>,
) -> Value {
    if !results.is_empty() {
        return json!({ "results": results });
    }
    let catalog =
        interpreter::packages::catalog_filtered(available(), |name| visibility.allows(name));
    let listing: Vec<Value> = catalog
        .entries
        .iter()
        .map(|e| json!({ "name": e.name, "source": e.source, "description": e.description }))
        .collect();
    let mut body = json!({
        "results": [],
        "available_packages": listing,
        "builtins": fetch_with.builtins_pointer(),
    });
    if catalog.remaining > 0 {
        body["available_packages_omitted"] = json!(catalog.remaining);
    }
    body
}

fn mcp_entries(catalog: &McpCatalog) -> Vec<CatalogEntry> {
    catalog
        .search("")
        .into_iter()
        .map(|m| CatalogEntry {
            name: m.name,
            source: "mcp".to_string(),
            description: m.description,
        })
        .collect()
}

fn registry_entries(artifacts: &[submilli_build::Artifact]) -> Vec<CatalogEntry> {
    artifacts
        .iter()
        .map(|artifact| CatalogEntry {
            name: artifact.metadata.package_name.clone(),
            source: "registry".to_string(),
            description: registry_description(&artifact.metadata),
        })
        .collect()
}

fn search_json_results(
    query: &str,
    catalog: &McpCatalog,
    visibility: LibraryVisibility,
) -> Vec<Value> {
    let mut results: Vec<Value> = interpreter::packages::search(query)
        .into_iter()
        .filter(|module| visibility.allows(&module.name))
        .map(|m| json!({ "name": m.name, "source": "host", "description": m.description }))
        .collect();
    results.extend(
        catalog
            .search(query)
            .into_iter()
            .map(|m| json!({ "name": m.name, "source": "mcp", "description": m.description })),
    );
    results
}

fn registry_lookup(name: &str, store: &PackageStore) -> Option<DocLookup> {
    let artifact = store.load(name).ok()?;
    Some(DocLookup::Registry {
        description: registry_description(&artifact.metadata),
        declarations: interpreter::packages::render_declarations(&artifact.package_declaration),
        documentation: artifact.documentation,
    })
}

fn registry_search(query: &str, artifacts: &[submilli_build::Artifact]) -> Vec<Value> {
    let q = query.trim().to_lowercase();
    artifacts
        .iter()
        .filter(|artifact| registry_matches(artifact, &q))
        .map(|artifact| {
            json!({
                "name": artifact.metadata.package_name,
                "source": "registry",
                "description": registry_description(&artifact.metadata),
            })
        })
        .collect()
}

fn registry_matches(artifact: &submilli_build::Artifact, query: &str) -> bool {
    query.is_empty()
        || artifact
            .metadata
            .package_name
            .to_lowercase()
            .contains(query)
        || artifact.metadata.description.to_lowercase().contains(query)
        || artifact
            .metadata
            .keywords
            .iter()
            .any(|keyword| keyword.to_lowercase().contains(query))
        || artifact
            .package_declaration
            .values
            .keys()
            .chain(artifact.package_declaration.types.keys())
            .any(|name| name.to_lowercase().contains(query))
}

fn registry_description(metadata: &submilli_build::ArtifactMetadata) -> String {
    if metadata.description.trim().is_empty() {
        format!(
            "Registry package {} v{}.",
            metadata.package_name, metadata.package_version
        )
    } else {
        metadata.description.clone()
    }
}

/// Render docs as markdown: an intro paragraph (registry documentation, or an
/// MCP server's one-line description) followed by the typed declarations.
fn docs_body_markdown(intro: &str, declarations: &str) -> String {
    let mut out = intro.trim().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("## Declarations\n\n```ts\n");
    out.push_str(declarations.trim());
    out.push_str("\n```\n");
    out
}

#[cfg(test)]
mod git_discovery_tests {
    use super::*;

    #[test]
    fn git_is_visible_only_with_a_configured_blueprint() {
        let directory = tempfile::tempdir().unwrap();
        let store = PackageStore::new(directory.path());
        let catalog = McpCatalog::empty();
        let mut blueprint = submilli_blueprint::parse("name: test").unwrap();
        for query in ["", "git", "not-a-package"] {
            let response =
                search_json_with_blueprint(query, &catalog, &blueprint, &store, Fetch::Mcp);
            assert!(!response.to_string().contains("submilli:git"));
        }
        assert!(matches!(
            lookup_with_blueprint("submilli:git", &catalog, &blueprint, &store),
            DocLookup::Unknown { .. }
        ));
        assert!(
            !docs_json(
                "submilli:gi",
                lookup_with_blueprint("submilli:gi", &catalog, &blueprint, &store)
            )
            .to_string()
            .contains("submilli:git")
        );
        blueprint.git = Some(submilli_blueprint::GitConfig {
            identity: submilli_blueprint::GitIdentity {
                name: "Agent".into(),
                email: "agent@example.com".into(),
            },
            username: None,
        });
        for query in ["", "git", "not-a-package"] {
            assert!(
                search_json_with_blueprint(query, &catalog, &blueprint, &store, Fetch::Mcp)
                    .to_string()
                    .contains("submilli:git")
            );
        }
        assert!(matches!(
            lookup_with_blueprint("submilli:git", &catalog, &blueprint, &store),
            DocLookup::Host { .. }
        ));
        assert!(
            !search_json("", Fetch::Mcp)
                .to_string()
                .contains("submilli:git")
        );
    }
}

#[cfg(test)]
mod policy_visibility_tests {
    use super::*;

    #[test]
    fn discovery_respects_library_grants() {
        let directory = tempfile::tempdir().unwrap();
        let store = PackageStore::new(directory.path());
        let catalog = McpCatalog::empty();
        for (policy, expected) in [
            ("", [false, false, false, false, false]),
            ("default: allow", [true, true, true, false, true]),
            ("default: ask-human", [true, true, true, false, true]),
            (
                "permissions:\n  main:\n    - capability: http.get\n      action: ask-human",
                [true, false, false, false, false],
            ),
            (
                "permissions:\n  main:\n    - capability: fs.mkdir\n      action: allow",
                [false, true, false, false, false],
            ),
            (
                "permissions:\n  main:\n    - capability: fs.read\n      action: allow",
                [false, true, true, false, false],
            ),
            (
                "default: allow\nllm:\n  providers:\n    test:\n      type: anthropic\n  models:\n    test-model:\n      provider: test",
                [true, true, true, true, true],
            ),
            (
                "permissions:\n  main:\n    - capability: session.read\n      action: allow",
                [false, false, false, false, false],
            ),
            (
                "permissions:\n  main:\n    - capability: session.read\n      action: allow\n    - capability: session.write\n      action: ask-human",
                [false, false, false, false, true],
            ),
        ] {
            let blueprint = submilli_blueprint::parse(&format!("name: test\n{policy}\n")).unwrap();
            for (name, visible) in [
                "submilli:http",
                "submilli:fs",
                "submilli:code",
                "submilli:llm",
                "submilli:session",
            ]
            .into_iter()
            .zip(expected)
            {
                for fetch in [Fetch::Rest("test"), Fetch::Mcp] {
                    let symbol = match name {
                        "submilli:http" => "download",
                        "submilli:fs" => "readText",
                        "submilli:llm" => "models",
                        "submilli:session" => "remove",
                        _ => "diffText",
                    };
                    for query in ["", name, symbol, "nothingmatchesthis"] {
                        let response =
                            search_json_with_blueprint(query, &catalog, &blueprint, &store, fetch);
                        assert_eq!(
                            response.to_string().contains(name),
                            visible,
                            "{policy}: {query}: {response}"
                        );
                    }
                }
                assert_eq!(
                    matches!(
                        lookup_with_blueprint(name, &catalog, &blueprint, &store),
                        DocLookup::Host { .. }
                    ),
                    visible,
                    "{policy}: {name}"
                );
                let typo = format!("{name}x");
                let response = docs_json(
                    &typo,
                    lookup_with_blueprint(&typo, &catalog, &blueprint, &store),
                );
                assert_eq!(
                    response["did_you_mean"].as_str() == Some(name),
                    visible,
                    "{response}"
                );
            }
        }
    }
}
