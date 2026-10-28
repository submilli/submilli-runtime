//! Package discovery for the MCP tools and the REST surface. The stdlib
//! rendering lives in `interpreter::packages`; this adds `@mcp/<server>` virtual
//! packages (rendered from a blueprint's discovered catalog) and the shared JSON
//! response shapes so the two transports can't drift.

use interpreter::docs_view::Availability;
use serde_json::{Value, json};
use submilli_shared::docs_scope::{self, DocsScope, PackageView};

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
    /// A standard-library module the caller named without its `submilli:`
    /// scheme. Served, but the response must say the asked name is wrong and
    /// that `canonical` is the import name.
    CorrectedHost {
        canonical: String,
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
    host_lookup(name, scope).unwrap_or_else(|| fallback_lookup(name, &[], scope))
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
        .unwrap_or_else(|| fallback_lookup(name, &catalog_names(catalog), scope))
}

/// Like [`lookup_with_catalog`], but scoped to what the bound blueprint lets
/// programs use, `scope`, and resolving the registry packages it declares.
/// The global REST docs endpoint is not blueprint-bound, so registry packages
/// intentionally surface only through the MCP tool.
pub(crate) fn lookup_with_blueprint(
    name: &str,
    catalog: &McpCatalog,
    scope: &DocsScope,
) -> DocLookup {
    lookup_for_session(name, catalog, scope, None)
}

/// Like [`lookup_with_blueprint`], for a session whose programs may import its
/// application tools, `tools`, as `@tools`. `tools` is `None` when the
/// session declares none or its blueprint lets programs call none.
pub(crate) fn lookup_for_session(
    name: &str,
    catalog: &McpCatalog,
    scope: &DocsScope,
    tools: Option<&interpreter::PackageDeclaration>,
) -> DocLookup {
    if name == interpreter::runtime::TOOLS_PACKAGE {
        return match tools {
            Some(tools) => DocLookup::Mcp {
                description: tools_description(tools, scope.tools()),
                declarations: interpreter::packages::render_declarations(tools),
            },
            None => DocLookup::Unknown {
                message: "`@tools` is unavailable: this session declares no application tools \
                          that its blueprint lets programs call"
                    .to_owned(),
                suggestion: None,
            },
        };
    }
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
    fallback_lookup(name, &union_candidates(catalog, scope), scope)
}

fn host_lookup(name: &str, scope: &DocsScope) -> Option<DocLookup> {
    scope.stdlib().docs(name).map(|doc| DocLookup::Host {
        description: doc.description,
        declarations: doc.declarations,
    })
}

/// A bare standard-library tail (`fs` for `submilli:fs`) is that module: serve
/// it, and say the asked name is wrong. A tail shared with another visible name
/// is a miss that lists every full name. `None` when the name is not a tail.
fn bare_stdlib_lookup(name: &str, extra: &[String], scope: &DocsScope) -> Option<DocLookup> {
    use interpreter::packages::BareNameMatch;
    match scope.stdlib().match_bare_name(name, extra) {
        BareNameMatch::Unique(doc) => Some(DocLookup::CorrectedHost {
            canonical: doc.name,
            description: doc.description,
            declarations: doc.declarations,
        }),
        BareNameMatch::Ambiguous(names) => Some(DocLookup::Unknown {
            message: interpreter::packages::ambiguous_bare_name_message(name, &names),
            suggestion: None,
        }),
        BareNameMatch::Miss => None,
    }
}

/// A name no catalog claims as written: a bare standard-library tail is served
/// with a correction, otherwise the built-in catalog answers, with a suggestion
/// drawn from every catalog this call site can see.
fn fallback_lookup(name: &str, extra: &[String], scope: &DocsScope) -> DocLookup {
    use interpreter::packages::BuiltinLookup;
    if let Some(found) = bare_stdlib_lookup(name, extra, scope) {
        return found;
    }
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
        DocLookup::CorrectedHost {
            canonical,
            description,
            declarations,
        } => {
            // `name` is the import string. The asked name stays in
            // `requested_name` so a model cannot mistake it for the package.
            let correction = interpreter::packages::bare_name_note(name, &canonical);
            json!({
                "name": canonical,
                "requested_name": name,
                "source": "host",
                "correction": correction,
                "description": format!("{correction}\n\n{description}"),
                "declarations": declarations,
            })
        }
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
/// unknown. The shared per-name shape behind the MCP batch and the REST
/// single-name endpoint, so the two can't drift.
///
/// `mcp_packages` are the `@mcp/*` names the bound blueprint declares. Without
/// a binding the slice is empty and those names keep their existing behavior.
pub(crate) fn builtin_entry_json(
    name: &str,
    mcp_packages: &[String],
    fetch_with: &Fetch<'_>,
    scope: &DocsScope,
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
        BuiltinLookup::Unknown => package_correcting_call(name, mcp_packages, fetch_with, scope)
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
    fetch_with: &Fetch<'_>,
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
            &fetch_with.package_docs(name),
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
    fetch_with: &Fetch<'_>,
    scope: &DocsScope,
) -> Value {
    let results: Vec<Value> = names
        .iter()
        .map(|n| builtin_entry_json(n, mcp_packages, fetch_with, scope))
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
    let scope = DocsScope::unscoped_shared();
    let results = search_json_results(query, catalog, scope);
    search_body(results, fetch_with, scope, || mcp_entries(catalog))
}

pub(crate) fn search_json_with_blueprint(
    query: &str,
    catalog: &McpCatalog,
    scope: &DocsScope,
    fetch_with: Fetch<'_>,
) -> Value {
    search_json_for_session(query, catalog, scope, fetch_with, None)
}

/// Like [`search_json_with_blueprint`], for a session whose programs may import
/// its application tools, `tools`, as `@tools`. `tools` is `None` when the
/// session declares none or its blueprint lets programs call none.
pub(crate) fn search_json_for_session(
    query: &str,
    catalog: &McpCatalog,
    scope: &DocsScope,
    fetch_with: Fetch<'_>,
    tools: Option<&interpreter::PackageDeclaration>,
) -> Value {
    let mut results = search_json_results(query, catalog, scope);
    if let Some(tools) = tools.filter(|tools| tools_match(tools, query)) {
        results.push(json!({
            "name": interpreter::runtime::TOOLS_PACKAGE,
            "source": "tools",
            "description": tools_description(tools, scope.tools()),
        }));
    }
    results.extend(registry_search(query, scope));
    search_body(results, fetch_with, scope, || {
        let mut entries = mcp_entries(catalog);
        entries.extend(tools.map(|tools| CatalogEntry {
            name: interpreter::runtime::TOOLS_PACKAGE.to_owned(),
            source: "tools".to_owned(),
            description: tools_description(tools, scope.tools()),
        }));
        entries.extend(registry_entries(scope));
        entries
    })
}

/// One-line summary of a session's `@tools` package: the tool count, and what
/// the blueprint's rules for `tools.call` limit.
fn tools_description(
    tools: &interpreter::PackageDeclaration,
    availability: &Availability,
) -> String {
    let count = tools.values.len();
    let mut description = format!(
        "The application's own tools, answered by the harness ({count} tool{})",
        if count == 1 { "" } else { "s" }
    );
    if let Availability::Available { notes } = availability {
        for note in notes {
            description.push_str(". ");
            description.push_str(note.trim_end_matches('.'));
        }
    }
    description
}

/// Whether `query` matches the `@tools` package name or one of its tools.
fn tools_match(tools: &interpreter::PackageDeclaration, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || interpreter::runtime::TOOLS_PACKAGE.contains(&query)
        || "tools".contains(&query)
        || tools
            .values
            .keys()
            .any(|tool| tool.to_lowercase().contains(&query))
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

#[cfg(test)]
mod git_discovery_tests {
    use super::*;

    #[test]
    fn git_is_visible_only_with_a_configured_blueprint() {
        let catalog = McpCatalog::empty();
        let mut blueprint = submilli_blueprint::parse("name: test").unwrap();
        for query in ["", "git", "not-a-package"] {
            let response = search_json_with_blueprint(
                query,
                &catalog,
                &DocsScope::for_blueprint(&blueprint, &[]),
                Fetch::Mcp,
            );
            assert!(!response.to_string().contains("submilli:git"));
        }
        assert!(matches!(
            lookup_with_blueprint(
                "submilli:git",
                &catalog,
                &DocsScope::for_blueprint(&blueprint, &[])
            ),
            DocLookup::Unknown { .. }
        ));
        // A forgotten prefix must not reveal a module the blueprint hides.
        let hidden = docs_json(
            "git",
            lookup_with_blueprint("git", &catalog, &DocsScope::for_blueprint(&blueprint, &[])),
        );
        assert_eq!(hidden["error"], "unknown_package", "{hidden}");
        assert!(!hidden.to_string().contains("submilli:git"), "{hidden}");
        assert!(
            !docs_json(
                "submilli:gi",
                lookup_with_blueprint(
                    "submilli:gi",
                    &catalog,
                    &DocsScope::for_blueprint(&blueprint, &[])
                )
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
                search_json_with_blueprint(
                    query,
                    &catalog,
                    &DocsScope::for_blueprint(&blueprint, &[]),
                    Fetch::Mcp
                )
                .to_string()
                .contains("submilli:git")
            );
        }
        assert!(matches!(
            lookup_with_blueprint(
                "submilli:git",
                &catalog,
                &DocsScope::for_blueprint(&blueprint, &[])
            ),
            DocLookup::Host { .. }
        ));
        let bare = docs_json(
            "git",
            lookup_with_blueprint("git", &catalog, &DocsScope::for_blueprint(&blueprint, &[])),
        );
        assert_eq!(bare["name"], "submilli:git", "{bare}");
        assert_eq!(bare["requested_name"], "git", "{bare}");
        assert!(
            bare["correction"]
                .as_str()
                .unwrap_or("")
                .contains("Do not import `git`"),
            "{bare}"
        );
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
    fn a_bare_name_follows_the_same_grant_as_the_full_name() {
        let catalog = McpCatalog::empty();
        let hidden = submilli_blueprint::parse("name: test\n").unwrap();
        let denied = docs_json(
            "fs",
            lookup_with_blueprint("fs", &catalog, &DocsScope::for_blueprint(&hidden, &[])),
        );
        assert_eq!(denied["error"], "unknown_package", "{denied}");
        assert!(!denied.to_string().contains("submilli:fs"), "{denied}");

        let allowed = submilli_blueprint::parse(
            "name: test\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n",
        )
        .unwrap();
        let served = docs_json(
            "fs",
            lookup_with_blueprint("fs", &catalog, &DocsScope::for_blueprint(&allowed, &[])),
        );
        assert_eq!(served["name"], "submilli:fs", "{served}");
        assert!(served["error"].is_null(), "{served}");
    }

    #[test]
    fn a_bare_name_shared_with_another_package_is_ambiguous() {
        let body = docs_json(
            "fs",
            fallback_lookup(
                "fs",
                &["@acme/fs".to_string()],
                DocsScope::unscoped_shared(),
            ),
        );
        assert_eq!(body["error"], "unknown_package", "{body}");
        let message = body["message"].as_str().unwrap_or("");
        assert!(
            message.contains("@acme/fs") && message.contains("submilli:fs"),
            "{body}"
        );
        assert!(body["did_you_mean"].is_null(), "{body}");
    }

    #[test]
    fn discovery_respects_library_grants() {
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
            // Reading alone is useful: the read operations are shown.
            (
                "permissions:\n  main:\n    - capability: session.read\n      action: allow",
                [false, false, false, false, true],
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
                        let response = search_json_with_blueprint(
                            query,
                            &catalog,
                            &DocsScope::for_blueprint(&blueprint, &[]),
                            fetch,
                        );
                        assert_eq!(
                            response.to_string().contains(name),
                            visible,
                            "{policy}: {query}: {response}"
                        );
                    }
                }
                assert_eq!(
                    matches!(
                        lookup_with_blueprint(
                            name,
                            &catalog,
                            &DocsScope::for_blueprint(&blueprint, &[])
                        ),
                        DocLookup::Host { .. }
                    ),
                    visible,
                    "{policy}: {name}"
                );
                let typo = format!("{name}x");
                let response = docs_json(
                    &typo,
                    lookup_with_blueprint(
                        &typo,
                        &catalog,
                        &DocsScope::for_blueprint(&blueprint, &[]),
                    ),
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
