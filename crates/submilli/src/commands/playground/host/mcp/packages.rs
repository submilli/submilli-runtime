//! Package discovery for the MCP tools. The stdlib rendering lives in
//! `interpreter::packages`; this adds `@mcp/<server>` virtual packages (rendered
//! from a blueprint's discovered catalog) and the JSON response shapes.
//!
//! Copied from `submilli-server`'s `packages.rs`, without the REST surface and
//! the application tools only a REST session declares.

use serde_json::{Value, json};
use submilli_shared::docs_scope::{self, DocsScope, PackageView};

use interpreter::packages::CatalogEntry;

use submilli_shared::mcp::McpCatalog;

/// The call that fetches a package's docs, for a correcting message.
fn package_docs_call(name: &str) -> String {
    format!("Fetch it with submilli__typescript__packages__docs(\"{name}\")")
}

fn builtins_pointer() -> String {
    interpreter::packages::builtins_pointer("`submilli__typescript__builtins__docs`")
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
    /// A declared package whose installed documentation has malformed
    /// directives, so no part of it can be shown safely.
    InvalidDocumentation { message: String },
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
    let scope = DocsScope::unscoped_shared();
    host_lookup(name, scope).unwrap_or_else(|| builtin_fallback(name, &[], scope))
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
    let scope = DocsScope::unscoped_shared();
    host_lookup(name, scope)
        .unwrap_or_else(|| builtin_fallback(name, &catalog_names(catalog), scope))
}

/// Like [`lookup_with_catalog`], but scoped to what the bound blueprint lets
/// programs use, `scope`, and resolving the registry packages it declares.
pub(crate) fn lookup_with_blueprint(
    name: &str,
    catalog: &McpCatalog,
    scope: &DocsScope,
) -> DocLookup {
    if name.starts_with("@mcp/") {
        return lookup_with_catalog(name, catalog);
    }
    if let Some(found) = host_lookup(name, scope) {
        return found;
    }
    if scope.declares(name) {
        // A declared package whose artifact is missing is not an unknown name:
        // suggesting alternatives (or the name itself) would hide the real fix.
        return match scope.package(name) {
            Some(package) => registry_lookup(package),
            None => DocLookup::Unknown {
                message: docs_scope::not_loaded_message(name),
                suggestion: None,
            },
        };
    }
    builtin_fallback(name, &union_candidates(catalog, scope), scope)
}

fn host_lookup(name: &str, scope: &DocsScope) -> Option<DocLookup> {
    scope.stdlib().docs(name).map(|doc| DocLookup::Host {
        description: doc.description,
        declarations: doc.declarations,
    })
}

/// A name no package source claims. Check the built-in catalog before erroring
/// — a built-in needs no `import`, so serving it here costs the caller nothing
/// — and carry a suggestion drawn from every catalog this call site can see.
fn builtin_fallback(name: &str, extra: &[String], scope: &DocsScope) -> DocLookup {
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
            suggestion: scope.suggest(name, extra),
        },
    }
}

fn catalog_names(catalog: &McpCatalog) -> Vec<String> {
    catalog.search("").into_iter().map(|m| m.name).collect()
}

/// Every name this call site could have resolved, for did-you-mean.
fn union_candidates(catalog: &McpCatalog, scope: &DocsScope) -> Vec<String> {
    let mut names = catalog_names(catalog);
    names.extend(scope.usable_packages().map(|package| package.name.clone()));
    names
}

/// `packages.docs` body.
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
        DocLookup::InvalidDocumentation { message } => json!({
            "error": "invalid_documentation",
            "message": message,
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
/// unknown. The per-name shape behind the MCP batch.
///
/// `mcp_packages` are the `@mcp/*` names the bound blueprint declares. Without
/// a binding the slice is empty and those names keep their existing behavior.
pub(crate) fn builtin_entry_json(name: &str, mcp_packages: &[String], scope: &DocsScope) -> Value {
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
        BuiltinLookup::Unknown => package_correcting_call(name, mcp_packages, scope)
            .unwrap_or_else(|| unknown_builtin_entry(name, mcp_packages, scope)),
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
    scope: &DocsScope,
) -> Option<Value> {
    let is_package = scope.stdlib().contains(name)
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
            &package_docs_call(name),
        ),
    }))
}

fn unknown_builtin_entry(name: &str, mcp_packages: &[String], scope: &DocsScope) -> Value {
    let message = format!("unknown built-in: {name}");
    // Built-in corrections have never advertised the opt-in Git module.
    match scope.stdlib().suggest_filtered(name, mcp_packages, |name| {
        name != "submilli:git" && scope.allows(name)
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
    scope: &DocsScope,
) -> Value {
    let results: Vec<Value> = names
        .iter()
        .map(|n| builtin_entry_json(n, mcp_packages, scope))
        .collect();
    json!({ "results": results })
}

/// The `@mcp/*` names a bound blueprint declares, for the builtins tool's
/// package recognition.
pub(crate) fn mcp_package_names(catalog: &McpCatalog) -> Vec<String> {
    catalog_names(catalog)
}

/// The built-in catalog — `{ types: [...], namespaces: [...] }`. Mirrors the
/// `{builtins}` prompt placeholder.
pub(crate) fn builtins_list_json() -> Value {
    let b = interpreter::packages::builtins();
    json!({ "types": b.types, "namespaces": b.namespaces })
}

/// `packages.search` body — `{ results: [{ name, source, description }] }`.
/// Stdlib modules only; for a blueprint's discovered `@mcp/<server>` packages use
/// [`search_json_with_catalog`].
pub(crate) fn search_json(query: &str) -> Value {
    search_json_with_catalog(query, &McpCatalog::empty())
}

/// Like [`search_json`], but also folds in a blueprint's discovered
/// `@mcp/<server>` packages, tagged `source: "mcp"`, after the stdlib hits.
pub(crate) fn search_json_with_catalog(query: &str, catalog: &McpCatalog) -> Value {
    let scope = DocsScope::unscoped_shared();
    let results = search_json_results(query, catalog, scope);
    search_body(results, scope, || mcp_entries(catalog))
}

pub(crate) fn search_json_with_blueprint(
    query: &str,
    catalog: &McpCatalog,
    scope: &DocsScope,
) -> Value {
    let mut results = search_json_results(query, catalog, scope);
    results.extend(registry_search(query, scope));
    search_body(results, scope, || {
        let mut entries = mcp_entries(catalog);
        entries.extend(registry_entries(scope));
        entries
    })
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
    scope: &DocsScope,
    available: impl FnOnce() -> Vec<CatalogEntry>,
) -> Value {
    if !results.is_empty() {
        return json!({ "results": results });
    }
    let catalog = scope.catalog(available());
    let listing: Vec<Value> = catalog
        .entries
        .iter()
        .map(|e| json!({ "name": e.name, "source": e.source, "description": e.description }))
        .collect();
    let mut body = json!({
        "results": [],
        "available_packages": listing,
        "builtins": builtins_pointer(),
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

fn registry_entries(scope: &DocsScope) -> Vec<CatalogEntry> {
    scope
        .usable_packages()
        .map(|package| CatalogEntry {
            name: package.name.clone(),
            source: "registry".to_string(),
            description: package.summary(),
        })
        .collect()
}

fn search_json_results(query: &str, catalog: &McpCatalog, scope: &DocsScope) -> Vec<Value> {
    let mut results: Vec<Value> = scope
        .stdlib()
        .search(query)
        .into_iter()
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

fn registry_lookup(package: &PackageView) -> DocLookup {
    if !package.usable {
        return DocLookup::Unknown {
            message: docs_scope::nothing_allowed_message(&package.name),
            suggestion: None,
        };
    }
    match &package.documentation {
        Ok(documentation) => DocLookup::Registry {
            description: package.summary(),
            declarations: package.declarations.clone(),
            documentation: documentation.clone(),
        },
        Err(errors) => DocLookup::InvalidDocumentation {
            message: docs_scope::invalid_documentation_message(&package.name, errors),
        },
    }
}

fn registry_search(query: &str, scope: &DocsScope) -> Vec<Value> {
    let q = query.trim().to_lowercase();
    scope
        .usable_packages()
        .filter(|package| package.matches(&q))
        .map(|package| {
            json!({
                "name": package.name,
                "source": "registry",
                "description": package.summary(),
            })
        })
        .collect()
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
