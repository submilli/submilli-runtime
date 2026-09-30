//! Package discovery over the standard library, plus the prelude built-in
//! catalog (`builtins` / `builtin_docs`).
//!
//! In MVP the resolvable set is the stdlib — there is no registry yet. Each
//! module's exported declarations render to `.d.subm`-style text directly from
//! its [`PackageDeclaration`] (the public symbol model). Shared by the MCP
//! `packages.*` / `builtins.docs` tools, the server's REST surface, and the
//! `submilli docs` / `search` / `builtins` CLI commands.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::runtime::prelude::declaration::prelude_package_declaration;
use crate::stdlib::stdlib_package_declarations;
use crate::types::escape_string_literal;
use crate::{
    ClassExtends, DocCapabilityBindingKind, DocCapabilityLiteral, DocComment, FileId,
    NamespaceSymbol, PackageDeclaration, Param, Span, Type, TypeKind, TypePredicate, TypeSymbol,
    ValueKind, ValueSymbol,
};

/// Internal plumbing imported by the fs/http shims; never user-facing.
const INTERNAL_MODULE: &str = "submilli:security";

/// A module's full documentation: its one-line description and `.d.subm`
/// declarations.
pub struct ModuleDoc {
    pub name: String,
    pub description: String,
    pub declarations: String,
}

/// A search result — name + one-line description.
pub struct ModuleSummary {
    pub name: String,
    pub description: String,
}

/// Documentation for a stdlib module, or `None` if `name` isn't one (callers
/// handle `@mcp/*` and unknown names).
pub fn docs(name: &str) -> Option<ModuleDoc> {
    docs_with_git(name, false)
}

/// Blueprint-scoped variant; Git is visible only when configured.
pub fn docs_with_git(name: &str, git_enabled: bool) -> Option<ModuleDoc> {
    user_modules()
        .into_iter()
        .find(|d| d.package_name == name && (git_enabled || name != "submilli:git"))
        .map(|defs| ModuleDoc {
            name: defs.package_name.clone(),
            description: module_description(&defs.package_name).to_string(),
            declarations: render_declarations(&defs),
        })
}

/// Modules whose name, description, or an exported symbol contains `query`
/// (case-insensitive). An empty query lists every module.
pub fn search(query: &str) -> Vec<ModuleSummary> {
    search_with_git(query, false)
}

/// Blueprint-scoped variant; Git is visible only when configured.
pub fn search_with_git(query: &str, git_enabled: bool) -> Vec<ModuleSummary> {
    let q = query.trim().to_lowercase();
    user_modules()
        .iter()
        .filter(|defs| {
            (git_enabled || defs.package_name != "submilli:git") && matches_query(defs, &q)
        })
        .map(|defs| ModuleSummary {
            name: defs.package_name.clone(),
            description: module_description(&defs.package_name).to_string(),
        })
        .collect()
}

/// The language built-ins always in scope without an `import`: headline prelude
/// types (`Array`, `Map`, `String`, …) and the namespace globals (`Math`,
/// `Temporal`, and the `JSON` compiler intrinsic). Backs the `{builtins}` prompt
/// placeholder and the `builtins.docs` tool.
pub struct Builtins {
    pub types: Vec<String>,
    pub namespaces: Vec<String>,
}

/// `JSON` is a compiler intrinsic (special-cased in the typechecker, not a
/// prelude `PackageDeclaration` entry), so it's catalogued and rendered by hand.
const JSON_BUILTIN: &str = "JSON";

/// Prelude types that exist for the type system but that an agent never writes
/// by name: the receiver type of the `console` global, an options bag, regex
/// match results, and the iteration protocol. Filtered out of the catalog.
const SUPPORTING_TYPES: &[&str] = &[
    "Console",
    "Base64Options",
    "RegExpMatch",
    "Iterator",
    "Iterable",
    "IteratorResult",
    "IteratorYieldResult",
    "IteratorReturnResult",
];

fn is_headline_type(name: &str) -> bool {
    !name.contains('#')                   // flattened namespace member (`Temporal#Instant`)
        && !name.ends_with("Constructor") // `new`/static plumbing, folded into its base
        && !SUPPORTING_TYPES.contains(&name)
}

/// The catalog of built-ins an agent can reference — see [`Builtins`].
pub fn builtins() -> Builtins {
    let defs = builtin_package_declaration();
    // `defs.types`/`namespaces` are `BTreeMap`s, so keys arrive sorted.
    let mut types: Vec<String> = defs
        .types
        .keys()
        .filter(|n| is_headline_type(n))
        .cloned()
        .collect();
    types.push("Record".into());
    types.sort();
    let mut namespaces: Vec<String> = defs.namespaces.keys().cloned().collect();
    namespaces.push(JSON_BUILTIN.to_string());
    namespaces.sort();
    Builtins { types, namespaces }
}

/// `.d.subm` declarations for a single built-in (`Array`, `Temporal`, `JSON`, …),
/// or `None` if `name` isn't one. Accepts a dotted member path
/// (`Temporal.Instant`) as well as a plain name — see [`builtin_lookup`].
pub fn builtin_docs(name: &str) -> Option<String> {
    match builtin_lookup(name) {
        BuiltinLookup::Found(declarations) => Some(declarations),
        BuiltinLookup::UnknownMember { .. } | BuiltinLookup::Unknown => None,
    }
}

/// Why resolving a built-in name succeeded or failed. The distinction matters
/// to callers: an unresolvable *member* can name the members that do exist,
/// which turns a dead end into a one-edit repair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinLookup {
    /// Rendered `.d.subm` declarations for the resolved name.
    Found(String),
    /// `path` resolved, but it has no member called `member`.
    UnknownMember {
        path: String,
        member: String,
        members: Vec<String>,
    },
    /// The leading segment names no built-in at all.
    Unknown,
}

/// Resolve a built-in name, which may be a dotted path into a namespace
/// (`Temporal.Instant`, `Temporal.Now.instant`, `Math.max`, `Array.isArray`).
///
/// A plain name renders exactly as it always has. A dotted path renders only
/// the member slice, wrapped in its enclosing `namespace` chain — the
/// declarations refer to each other by qualified name (`Temporal.Instant`), so
/// an unwrapped slice would teach `Instant.from(...)` instead of
/// `Temporal.Instant.from(...)`.
pub fn builtin_lookup(name: &str) -> BuiltinLookup {
    // `#` is the internal namespace-flattening separator (`Temporal#Instant`);
    // it's never part of the surface an agent names.
    if name.contains('#') {
        return BuiltinLookup::Unknown;
    }
    // A trailing or doubled dot is a typo the path walk can absorb rather than
    // reject (see the forgiveness principle in CLAUDE.md).
    let segments: Vec<&str> = name.split('.').filter(|s| !s.is_empty()).collect();
    let Some((head, rest)) = segments.split_first() else {
        return BuiltinLookup::Unknown;
    };

    if head.eq_ignore_ascii_case("Record") && rest.is_empty() {
        return BuiltinLookup::Found("/** Record<K, V> accepts string keys. With K = string, reads return V | null and writes require V. Finite string-literal keys are all required. Equivalent open syntax: { [key: string]: V }. */\ntype Record<K extends string, V> = { [P in K]: V };\n".into());
    }
    if head.eq_ignore_ascii_case(JSON_BUILTIN) {
        let defs = json_package_declaration();
        return walk_namespace(JSON_BUILTIN, &defs.namespaces[JSON_BUILTIN], rest);
    }

    let defs = builtin_package_declaration();
    if let Some(canonical) = resolve_ignore_case(defs.namespaces.keys(), head) {
        return walk_namespace(&canonical, &defs.namespaces[&canonical], rest);
    }

    let Some(canonical) = resolve_type_key(&defs.types, head) else {
        return BuiltinLookup::Unknown;
    };
    match rest {
        [] => {
            let mut out = String::new();
            render_type_with_ctor(&mut out, &defs.types, &canonical, "");
            BuiltinLookup::Found(out.trim_end().to_string())
        }
        [member, deeper @ ..] => resolve_type_member(&defs.types, &canonical, member, deeper),
    }
}

/// Walk `rest` down from the namespace `head`, which is already resolved.
fn walk_namespace<'a>(head: &str, head_ns: &'a NamespaceSymbol, rest: &[&str]) -> BuiltinLookup {
    let mut chain: Vec<String> = vec![head.to_string()];
    let mut ns: &'a NamespaceSymbol = head_ns;

    for (idx, seg) in rest.iter().enumerate() {
        let tail = &rest[idx + 1..];

        if let Some(canonical) = resolve_ignore_case(ns.namespaces.keys(), seg) {
            let sub = &ns.namespaces[&canonical];
            if tail.is_empty() {
                let mut body = String::new();
                render_namespace(&mut body, &canonical, sub, "");
                return found_in(&chain, &body);
            }
            chain.push(canonical);
            ns = sub;
            continue;
        }

        if let Some(canonical) = resolve_type_key(&ns.types, seg) {
            if tail.is_empty() {
                let mut body = String::new();
                render_namespace_member(&mut body, ns, &canonical);
                return found_in(&chain, &body);
            }
            // The type name stays off `chain`: a type is not a namespace, and
            // the rendered stub already names the interface the member hangs off.
            return match resolve_type_member(&ns.types, &canonical, tail[0], &tail[1..]) {
                BuiltinLookup::Found(body) => found_in(&chain, &body),
                miss => qualify(&chain, miss),
            };
        }

        if let Some(canonical) = resolve_ignore_case(ns.values.keys(), seg) {
            if tail.is_empty() {
                let sym = &ns.values[&canonical];
                let mut body = String::new();
                push_doc(&mut body, value_doc(&sym.kind), "");
                render_value(&mut body, &canonical, &sym.kind, "");
                return found_in(&chain, &body);
            }
            // A value is a leaf: it has no members to walk into.
            chain.push(canonical);
            return BuiltinLookup::UnknownMember {
                path: chain.join("."),
                member: tail.join("."),
                members: Vec::new(),
            };
        }

        return BuiltinLookup::UnknownMember {
            path: chain.join("."),
            member: (*seg).to_string(),
            members: namespace_member_names(ns),
        };
    }

    let mut out = String::new();
    render_namespace(&mut out, head, head_ns, "");
    BuiltinLookup::Found(out.trim_end().to_string())
}

/// Resolve `member` on the type `type_name`, checking the interface itself and
/// then its `*Constructor` (the `new`/static side, where `Array.isArray` lives).
fn resolve_type_member(
    types: &BTreeMap<String, TypeSymbol>,
    type_name: &str,
    member: &str,
    deeper: &[&str],
) -> BuiltinLookup {
    if !deeper.is_empty() {
        return BuiltinLookup::UnknownMember {
            path: format!("{type_name}.{member}"),
            member: deeper.join("."),
            members: Vec::new(),
        };
    }
    let ctor = format!("{type_name}Constructor");
    for owner in [type_name, ctor.as_str()] {
        let Some(sym) = types.get(owner) else {
            continue;
        };
        if let Some(body) = render_type_member(owner, &sym.kind, member) {
            return BuiltinLookup::Found(body.trim_end().to_string());
        }
    }
    BuiltinLookup::UnknownMember {
        path: type_name.to_string(),
        member: member.to_string(),
        members: type_member_names(types, type_name),
    }
}

/// Render the slice of `ns` named `canonical`: the namespace's own binding for
/// the name, the interface, and its `*Constructor`.
///
/// Deliberately not [`render_type_with_ctor`] — that synthesises its own
/// `const <name>: <name>Constructor;` line, which inside a namespace both
/// duplicates the namespace's existing binding and drops the qualification
/// (`Temporal.InstantConstructor`) that binding carries.
fn render_namespace_member(out: &mut String, ns: &NamespaceSymbol, canonical: &str) {
    if let Some(sym) = ns.values.get(canonical) {
        push_doc(out, value_doc(&sym.kind), "");
        render_value(out, canonical, &sym.kind, "");
    }
    if let Some(sym) = ns.types.get(canonical) {
        push_doc(out, type_doc(&sym.kind), "");
        render_type(out, canonical, &sym.kind, "");
    }
    let ctor = format!("{canonical}Constructor");
    if let Some(sym) = ns.types.get(&ctor) {
        push_doc(out, type_doc(&sym.kind), "");
        render_type(out, &ctor, &sym.kind, "");
    }
}

/// One method or property of an interface, rendered inside a stub of its owner
/// so the reader sees which type the member hangs off.
fn render_type_member(owner: &str, kind: &TypeKind, member: &str) -> Option<String> {
    if let TypeKind::Class { .. } = kind {
        return render_class_member(owner, kind, member);
    }
    let TypeKind::Interface {
        generics,
        methods,
        properties,
        ..
    } = kind
    else {
        return None;
    };
    let mut body = String::new();
    if let Some(name) = resolve_ignore_case(properties.keys(), member) {
        let prop = &properties[&name];
        push_doc(&mut body, &prop.doc, "  ");
        let ro = if prop.readonly { "readonly " } else { "" };
        let opt = if prop.optional { "?" } else { "" };
        let _ = writeln!(body, "  {ro}{name}{opt}: {};", prop.ty);
    } else {
        let name = resolve_ignore_case(methods.keys(), member)?;
        let m = &methods[&name];
        push_doc(&mut body, &m.doc, "  ");
        let _ = writeln!(
            body,
            "  {name}{}({}): {};",
            generics_str(&m.generics),
            params_str(&m.params),
            m.ret
        );
    }
    Some(format!(
        "interface {owner}{} {{\n{body}}}\n",
        generics_str(generics)
    ))
}

/// One public member of a class, rendered inside a stub of its owner. Mirrors
/// the visibility filtering of the full class rendering: a private member is
/// not part of the surface an agent can call, so it stays unresolvable.
fn render_class_member(owner: &str, kind: &TypeKind, member: &str) -> Option<String> {
    let TypeKind::Class {
        generics,
        fields,
        methods,
        method_visibility,
        statics,
        static_visibility,
        static_fields,
        ..
    } = kind
    else {
        return None;
    };
    let mut body = String::new();
    if let Some(name) = resolve_ignore_case(static_fields.keys(), member)
        .filter(|n| static_fields[n].visibility != crate::Visibility::Private)
    {
        let field = &static_fields[&name];
        push_doc(&mut body, &field.doc, "  ");
        let ro = if field.readonly { "readonly " } else { "" };
        let _ = writeln!(body, "  static {ro}{name}: {};", field.ty);
    } else if let Some(name) = resolve_ignore_case(statics.keys(), member)
        .filter(|n| static_visibility.get(n) != Some(&crate::Visibility::Private))
    {
        let m = &statics[&name];
        push_doc(&mut body, &m.doc, "  ");
        let _ = writeln!(
            body,
            "  static {name}{}({}): {};",
            generics_str(&m.generics),
            params_str(&m.params),
            m.ret
        );
    } else if let Some(name) = resolve_ignore_case(fields.keys(), member)
        .filter(|n| fields[n].visibility != crate::Visibility::Private)
    {
        let field = &fields[&name];
        push_doc(&mut body, &field.doc, "  ");
        let ro = if field.readonly { "readonly " } else { "" };
        let opt = if field.optional { "?" } else { "" };
        let _ = writeln!(body, "  {ro}{name}{opt}: {};", field.ty);
    } else {
        let name = resolve_ignore_case(methods.keys(), member)
            .filter(|n| method_visibility.get(n) != Some(&crate::Visibility::Private))?;
        let m = &methods[&name];
        push_doc(&mut body, &m.doc, "  ");
        let _ = writeln!(
            body,
            "  {name}{}({}): {};",
            generics_str(&m.generics),
            params_str(&m.params),
            m.ret
        );
    }
    Some(format!(
        "class {owner}{} {{\n{body}}}\n",
        generics_str(generics)
    ))
}

/// Members an agent can name on `type_name`, including its `*Constructor` side.
fn type_member_names(types: &BTreeMap<String, TypeSymbol>, type_name: &str) -> Vec<String> {
    let ctor = format!("{type_name}Constructor");
    let mut names: Vec<String> =
        [type_name, ctor.as_str()]
            .iter()
            .filter_map(|owner| types.get(*owner))
            .flat_map(|sym| match &sym.kind {
                TypeKind::Interface {
                    methods,
                    properties,
                    ..
                } => properties
                    .keys()
                    .chain(methods.keys())
                    .cloned()
                    .collect::<Vec<_>>(),
                // Private members are not part of the surface an agent can call, so
                // naming them here would offer a repair the typechecker rejects.
                TypeKind::Class {
                    fields,
                    methods,
                    method_visibility,
                    statics,
                    static_visibility,
                    static_fields,
                    ..
                } => {
                    static_fields
                        .iter()
                        .filter(|(_, f)| f.visibility != crate::Visibility::Private)
                        .map(|(n, _)| n)
                        .chain(statics.keys().filter(|n| {
                            static_visibility.get(*n) != Some(&crate::Visibility::Private)
                        }))
                        .chain(
                            fields
                                .iter()
                                .filter(|(_, f)| f.visibility != crate::Visibility::Private)
                                .map(|(n, _)| n),
                        )
                        .chain(methods.keys().filter(|n| {
                            method_visibility.get(*n) != Some(&crate::Visibility::Private)
                        }))
                        .cloned()
                        .collect::<Vec<_>>()
                }
                _ => Vec::new(),
            })
            .collect();
    names.sort();
    names.dedup();
    names
}

/// Members an agent can name on `ns`, with the `*Constructor` plumbing folded
/// away — the namespace binds each constructor to its base name already.
fn namespace_member_names(ns: &NamespaceSymbol) -> Vec<String> {
    let mut names: Vec<String> = ns
        .values
        .keys()
        .chain(ns.types.keys())
        .chain(ns.namespaces.keys())
        .filter(|n| !n.ends_with("Constructor"))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Find the type key matching `name`, accepting the base name of a
/// `*Constructor` entry so `Instant` resolves when only `InstantConstructor`
/// is declared.
fn resolve_type_key(types: &BTreeMap<String, TypeSymbol>, name: &str) -> Option<String> {
    resolve_ignore_case(types.keys(), name).or_else(|| {
        resolve_ignore_case(types.keys(), &format!("{name}Constructor"))
            .map(|c| c.trim_end_matches("Constructor").to_string())
    })
}

/// Wrap a rendered member slice in its enclosing `namespace` chain.
fn found_in(chain: &[String], body: &str) -> BuiltinLookup {
    let mut out = body.trim_end().to_string();
    for name in chain.iter().rev() {
        out = format!("namespace {name} {{\n{}\n}}", indent_block(&out, "  "));
    }
    BuiltinLookup::Found(out)
}

/// Re-anchor a miss reported against a bare type name onto its full path.
fn qualify(chain: &[String], miss: BuiltinLookup) -> BuiltinLookup {
    match miss {
        BuiltinLookup::UnknownMember {
            path,
            member,
            members,
        } if !chain.is_empty() => BuiltinLookup::UnknownMember {
            path: format!("{}.{path}", chain.join(".")),
            member,
            members,
        },
        other => other,
    }
}

/// How a discovery surface should answer one name, once the stdlib and
/// built-in catalogs have both been consulted.
///
/// The server layers `@mcp/*` and registry packages on top; those live in
/// crates that depend on this one, so they cannot be resolved from here.
pub enum Resolution {
    /// An importable stdlib module.
    Module(ModuleDoc),
    /// A language built-in, always in scope without an `import`.
    Builtin { name: String, declarations: String },
    /// The head resolved but the member did not.
    UnknownMember {
        path: String,
        member: String,
        members: Vec<String>,
    },
    /// Neither catalog knows the name.
    Unknown,
}

/// Try `name` as a stdlib module, then as a built-in. Every discovery surface
/// runs this same ladder so MCP, REST, and the CLI reach the same answer.
pub fn resolve(name: &str) -> Resolution {
    if let Some(doc) = docs(name) {
        return Resolution::Module(doc);
    }
    match builtin_lookup(name) {
        BuiltinLookup::Found(declarations) => Resolution::Builtin {
            name: name.to_string(),
            declarations,
        },
        BuiltinLookup::UnknownMember {
            path,
            member,
            members,
        } => Resolution::UnknownMember {
            path,
            member,
            members,
        },
        BuiltinLookup::Unknown => Resolution::Unknown,
    }
}

/// A did-you-mean candidate for a name that resolved nowhere, drawn from the
/// stdlib and built-in catalogs plus any `extra` names the caller can see
/// (`@mcp/*`, registry packages).
///
/// Two things bare edit distance cannot do on its own: a dotted name fails on
/// its head segment, and a package named without its scheme (`http` for
/// `submilli:http`) sits far outside any sane threshold.
/// Globals this language deliberately omits, each pointing at what replaces it.
/// Edit distance cannot find these — `Date` is nearer `Math` than `Temporal` —
/// and an LLM carrying JS habits reaches for them by name, so the redirect is
/// the difference between a dead end and the right answer.
const OMITTED_GLOBALS: &[(&str, &str)] = &[("Date", "Temporal")];

pub fn suggest(name: &str, extra: &[String]) -> Option<String> {
    suggest_with_git(name, extra, false)
}

/// Blueprint-scoped variant; Git is visible only when configured.
pub fn suggest_with_git(name: &str, extra: &[String], git_enabled: bool) -> Option<String> {
    suggest_filtered(name, extra, |module| {
        git_enabled || module != "submilli:git"
    })
}

/// Suggest only names visible to the caller, retaining built-in corrections.
pub fn suggest_filtered(
    name: &str,
    extra: &[String],
    visible: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut candidates: Vec<String> = search_with_git("", true)
        .into_iter()
        .map(|m| m.name)
        .collect();
    let builtins = builtins();
    candidates.extend(builtins.types);
    candidates.extend(builtins.namespaces);
    candidates.extend(extra.iter().cloned());
    candidates.retain(|candidate| visible(candidate));

    // Only the head segment is in question: `Temporel.Instant` misses because
    // of `Temporel`, and the tail only inflates the distance past threshold.
    let query = name.split('.').find(|s| !s.is_empty()).unwrap_or(name);
    // `closest_match` floors its threshold at 2 edits, so a one- or two-character
    // query sits within reach of an unrelated name. Below that length there is no
    // signal to match on.
    if query.len() < 3 {
        return None;
    }

    if let Some((_, replacement)) = OMITTED_GLOBALS
        .iter()
        .find(|(omitted, _)| omitted.eq_ignore_ascii_case(name))
    {
        return Some((*replacement).to_string());
    }

    // Never offer the query back to the caller: a name that resolved nowhere is
    // not its own repair, however it was spelled.
    let is_self = |c: &String| c.eq_ignore_ascii_case(name);
    if let Some(hit) = candidates
        .iter()
        .find(|c| !is_self(c) && name_tail(c).eq_ignore_ascii_case(query))
    {
        return Some(hit.clone());
    }
    // Match full names and scheme-stripped tails in one pass, so the globally
    // closest key wins: `htp` should reach `submilli:http` (one edit from its
    // tail) rather than whichever unrelated full name lands inside threshold.
    let mut keys: Vec<(&str, &str)> = Vec::new();
    for c in candidates.iter().filter(|c| !is_self(c)) {
        keys.push((c.as_str(), c.as_str()));
        let tail = name_tail(c);
        if tail != c.as_str() {
            keys.push((tail, c.as_str()));
        }
    }
    let hit = crate::did_you_mean::closest_match(query, keys.iter().map(|(k, _)| *k))?;
    keys.iter()
        .find(|(k, _)| *k == hit)
        .map(|(_, owner)| (*owner).to_string())
}

/// The part of a package name after its scheme or scope: `submilli:http` and
/// `@mcp/linear` reduce to `http` and `linear`.
fn name_tail(name: &str) -> &str {
    let after_scheme = name.rsplit_once(':').map_or(name, |(_, t)| t);
    after_scheme
        .rsplit_once('/')
        .map_or(after_scheme, |(_, t)| t)
}

/// A summary-only entry in the "here is what exists" listing a zero-hit
/// discovery response carries. Never declarations — this is a pointer, not a
/// payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: String,
    pub source: String,
    pub description: String,
}

/// A bounded available-package listing.
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
    /// Entries dropped past [`CATALOG_LIMIT`], reported as a count so the
    /// response stays bounded as the registry grows.
    pub remaining: usize,
}

/// Upper bound on listed entries; the remainder is reported as a count.
pub const CATALOG_LIMIT: usize = 50;

/// Where the language globals live. A zero-hit package search says this rather
/// than folding built-ins into its results, which would teach an agent to
/// write an `import` for a global.
///
/// `fetch_with` names the call the asking surface actually accepts — the tool
/// (`builtins.docs`) or the CLI command (`submilli builtins`). Pointing a CLI
/// user at a tool name they cannot run would be its own dead end.
pub fn builtins_pointer(fetch_with: &str) -> String {
    let head = "Language built-ins (Array, Map, String, Temporal, JSON, …)";
    format!("{head} are always in scope without an import — fetch them with {fetch_with}.")
}

/// A package name asked of a built-ins surface. The forgiveness here is
/// asymmetric on purpose: a built-in asked of `packages.docs` is simply served,
/// because it needs no `import`, while a package needs an `import` the caller
/// still has to write — so this names the call to make rather than hiding that
/// step behind the declarations.
///
/// `fetch_with` is the call the asking surface actually accepts, exactly as in
/// [`builtins_pointer`].
pub fn package_correcting_message(name: &str, fetch_with: &str) -> String {
    format!(
        "`{name}` is a package, not a language built-in. {fetch_with} for its declarations, \
         and write `import ... from \"{name}\"` to use it."
    )
}

/// Source tag for the stdlib modules this crate can see.
pub const SOURCE_STDLIB: &str = "stdlib";

/// The stdlib catalog plus whatever `extra` sources the caller can see, capped.
pub fn catalog(extra: Vec<CatalogEntry>) -> Catalog {
    catalog_with_git(extra, false)
}

/// Blueprint-scoped variant; Git is visible only when configured.
pub fn catalog_with_git(extra: Vec<CatalogEntry>, git_enabled: bool) -> Catalog {
    catalog_filtered(extra, |module| git_enabled || module != "submilli:git")
}

/// Filter before applying the catalog limit so omitted counts reflect visibility.
pub fn catalog_filtered(extra: Vec<CatalogEntry>, visible: impl Fn(&str) -> bool) -> Catalog {
    let mut entries: Vec<CatalogEntry> = search_with_git("", true)
        .into_iter()
        .map(|m| CatalogEntry {
            name: m.name,
            source: SOURCE_STDLIB.to_string(),
            description: m.description,
        })
        .collect();
    entries.extend(extra);
    entries.retain(|entry| visible(&entry.name));
    let remaining = entries.len().saturating_sub(CATALOG_LIMIT);
    entries.truncate(CATALOG_LIMIT);
    Catalog { entries, remaining }
}

/// Name the head, then list what it actually has, so the repair is one edit.
pub fn unknown_member_message(path: &str, member: &str, members: &[String]) -> String {
    if members.is_empty() {
        return format!("`{path}` has no member `{member}` — `{path}` has no members.");
    }
    format!(
        "`{path}` has no member `{member}`. Available members of `{path}`: {}.",
        members.join(", ")
    )
}

/// Prose to accompany a built-in served through a package-shaped surface. A
/// `source` tag is a machine field; an agent reading the message still has to
/// be told not to write an `import`.
pub fn builtin_no_import_note(name: &str) -> String {
    format!("`{name}` is a language built-in, always in scope — do not write an `import` for it.")
}

fn indent_block(body: &str, pad: &str) -> String {
    body.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn builtin_package_declaration() -> PackageDeclaration {
    let mut defs = prelude_package_declaration();
    let prelude_decl = crate::runtime::prelude::package_declaration();
    for (name, namespace) in prelude_decl.namespaces {
        defs.namespaces.entry(name).or_insert(namespace);
    }
    defs
}

/// Find the key that matches `name` case-insensitively, preferring an exact
/// match so canonical casing always wins when both exist.
fn resolve_ignore_case<'a>(keys: impl Iterator<Item = &'a String>, name: &str) -> Option<String> {
    let mut fallback = None;
    for key in keys {
        if key == name {
            return Some(key.clone());
        }
        if fallback.is_none() && key.eq_ignore_ascii_case(name) {
            fallback = Some(key.clone());
        }
    }
    fallback
}

/// The stdlib modules an agent may import, minus internal plumbing.
fn user_modules() -> Vec<PackageDeclaration> {
    stdlib_package_declarations()
        .into_iter()
        .filter(|d| d.package_name != INTERNAL_MODULE)
        .collect()
}

/// One-line module summaries — the single source for the stdlib tour shown in
/// `packages.search`/`docs` and (manually mirrored) MVP.md §I.
fn module_description(name: &str) -> &'static str {
    match name {
        "submilli:crypto" => "Hashing, HMAC, and random bytes.",
        "submilli:code" => {
            "Workspace tools: numbered reads, search, glob, tree, anchored edits and unified diffs."
        }
        "submilli:fs" => "Sandbox filesystem: read/write/list/stat/remove/exists/info.",
        "submilli:git" => {
            "Capability-controlled VFS repositories: history, staging, commits, branches and HTTPS fetch."
        }
        "submilli:http" => "Outbound HTTP: get/post/put/patch/delete/head.",
        "submilli:llm" => "Gated model calls: call/batch, and models() to discover them.",
        "submilli:secrets" => "Policy-gated access to blueprint-declared secrets.",
        "submilli:session" => "Session-scoped key-value state: get/has/set/remove/list.",
        "submilli:url" => "URL parse/build and query-string handling. Pure compute.",
        "submilli:uuid" => "UUID v4/v7 generation and validation.",
        _ => "",
    }
}

fn matches_query(defs: &PackageDeclaration, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let in_symbols = defs
        .values
        .keys()
        .chain(defs.types.keys())
        .any(|k| k.to_lowercase().contains(q));
    defs.package_name.to_lowercase().contains(q)
        || module_description(&defs.package_name)
            .to_lowercase()
            .contains(q)
        || in_symbols
}

/// Render a module's exports as `.d.subm`-style declarations. Public so an
/// embedder can render dynamically-built packages (e.g. `@mcp/<server>`) the same
/// way the stdlib docs render.
pub fn render_declarations(defs: &PackageDeclaration) -> String {
    let mut out = String::new();
    for (name, sym) in &defs.values {
        push_doc(&mut out, value_doc(&sym.kind), "");
        render_value(&mut out, name, &sym.kind, "");
    }
    for (name, sym) in &defs.types {
        push_doc(&mut out, type_doc(&sym.kind), "");
        render_type(&mut out, name, &sym.kind, "");
    }
    out.trim_end().to_string()
}

/// Render editor-facing TypeScript declarations for every stdlib module a
/// package project can import. Each package gets a separate `declare module`
/// block. `submilli:test` is included even though only `build test` makes it
/// importable, and `submilli:security` even though only packages may import
/// it — the editor should complete both; `build check` remains the gate
/// against importing them elsewhere.
pub fn render_stdlib_d_ts() -> String {
    let mut out = String::new();
    for defs in &editor_stdlib_modules() {
        render_declare_module(&mut out, defs);
    }
    out.trim_end().to_string()
}

/// Render editor-facing TypeScript declarations for `packages`, one
/// `declare module` block each, in the order given.
///
/// A declaration names the types it borrows from another module without
/// saying where they come from, so each block imports them. A borrowed type is
/// matched to its module by the reference's mangled name, not by its text:
/// many modules export a `Page` or an `Item`. `context` holds further modules
/// those types may come from that get no block here, such as the project's
/// own packages.
pub fn render_packages_d_ts(
    packages: &[&PackageDeclaration],
    context: &[&PackageDeclaration],
) -> String {
    let stdlib = editor_stdlib_modules();
    let exporters = TypeExporters::new(
        stdlib
            .iter()
            .chain(packages.iter().copied())
            .chain(context.iter().copied()),
    );
    let globals = global_classes();
    let mut out = String::new();
    for defs in packages {
        let _ = writeln!(out, "declare module \"{}\" {{", defs.package_name);
        let imports = block_imports(defs, &exporters);
        for (local, (public, module)) in &imports.types {
            if local == public {
                let _ = writeln!(out, "  import type {{ {local} }} from \"{module}\";");
            } else {
                let _ = writeln!(
                    out,
                    "  import type {{ {public} as {local} }} from \"{module}\";"
                );
            }
        }
        let parent = |extends: &ClassExtends| {
            if let Some(local) = imports.parents.get(extends.parent.as_str()) {
                return Some(ts_named_type(local, &extends.args));
            }
            // A package value of the global's name would hide it; the class
            // then renders without its parent rather than extend the value.
            let global = globals.get(extends.parent.as_str())?;
            if declares(defs, global, true) {
                return None;
            }
            Some(ts_named_type(global, &extends.args))
        };
        render_ts_declarations(&mut out, defs, "  ", "export ", &parent);
        let _ = writeln!(out, "}}\n");
    }
    out.trim_end().to_string()
}

/// The built-in classes a package's class may extend, such as `Error`, by
/// mangled name. They're globals, so a parent among them needs no import.
fn global_classes() -> BTreeMap<String, String> {
    let defs = builtin_package_declaration();
    defs.types
        .into_iter()
        .filter(|(name, symbol)| {
            matches!(symbol.kind, TypeKind::Class { .. })
                && !is_hidden_prelude_type(&defs.package_name, name)
        })
        .map(|(name, symbol)| (symbol.mangled_name.as_str().to_string(), name))
        .collect()
}

fn editor_stdlib_modules() -> Vec<PackageDeclaration> {
    let mut modules = user_modules();
    modules.push(crate::stdlib::test::package_declaration());
    modules.push(crate::stdlib::security::package_declaration());
    modules.sort_by(|a, b| a.package_name.cmp(&b.package_name));
    modules
}

/// Where each exported type can be imported from.
struct TypeExporters<'a> {
    /// A type symbol's mangled name, mapped to the type and its module.
    by_mangled: BTreeMap<&'a str, (ExportedType<'a>, &'a str)>,
    /// Each module's exported types by name, for a reference whose mangled
    /// name is the internal-module form of a type the package root re-exports.
    by_module: BTreeMap<&'a str, BTreeMap<&'a str, ExportedType<'a>>>,
}

#[derive(Clone, Copy)]
struct ExportedType<'a> {
    name: &'a str,
    /// A class or enum is a value too, and an import of one conflicts with a
    /// local value of the same name. An interface or alias doesn't.
    is_value: bool,
}

impl<'a> TypeExporters<'a> {
    fn new(modules: impl Iterator<Item = &'a PackageDeclaration>) -> Self {
        let mut by_mangled = BTreeMap::new();
        let mut by_module: BTreeMap<&str, BTreeMap<&str, ExportedType>> = BTreeMap::new();
        for defs in modules {
            let module = defs.package_name.as_str();
            for (name, symbol) in &defs.types {
                let exported = ExportedType {
                    name: name.as_str(),
                    is_value: matches!(
                        symbol.kind,
                        TypeKind::Class { .. }
                            | TypeKind::NumberEnum { .. }
                            | TypeKind::StringEnum { .. }
                    ),
                };
                by_mangled.insert(symbol.mangled_name.as_str(), (exported, module));
                by_module
                    .entry(module)
                    .or_default()
                    .insert(name.as_str(), exported);
            }
        }
        Self {
            by_mangled,
            by_module,
        }
    }

    /// The type a reference points at and its module, if a known module
    /// exports it.
    fn resolve(&self, reference: &TypeReference) -> Option<(ExportedType<'a>, &'a str)> {
        if let Some(&exported) = self.by_mangled.get(reference.mangled.as_str()) {
            return Some(exported);
        }
        let (module, types) = self.by_module.get_key_value(reference.package.as_str())?;
        Some((*types.get(reference.name.as_str())?, module))
    }
}

/// A by-name type as a declaration refers to it: the declaring symbol's
/// mangled name and package, and the name the rendered text uses for it,
/// which is the local name an aliased import gave it.
struct TypeReference {
    mangled: String,
    package: String,
    name: String,
}

/// The imports one `declare module` block needs.
struct BlockImports<'a> {
    /// Keyed by the name the block's text uses, valued by the public name and
    /// module to import it from.
    types: BTreeMap<String, (&'a str, &'a str)>,
    /// Each class parent's mangled name, mapped to the name the block imports
    /// it under.
    parents: BTreeMap<&'a str, String>,
}

/// The imports `defs`'s block needs.
///
/// A type reference is imported under the name the text already uses for it.
/// That name is the package's own if it declares a type of that name, or a
/// value when the borrowed type is a value too (a class or enum); TypeScript
/// keeps a type and a value of one name apart otherwise. Two references that
/// use one local name for different types, which only separate source files
/// can produce, get the first one's import.
///
/// A class's parent records no local name, so the block picks one: the name
/// an import of the same type already has, else its public name, else that
/// name with a numeric suffix that nothing in the block uses.
fn block_imports<'a>(
    defs: &'a PackageDeclaration,
    exporters: &TypeExporters<'a>,
) -> BlockImports<'a> {
    let mut types = BTreeMap::new();
    for reference in rendered_type_references(defs) {
        let Some((exported, module)) = exporters.resolve(&reference) else {
            continue;
        };
        if module != defs.package_name && !declares(defs, &reference.name, exported.is_value) {
            types
                .entry(reference.name)
                .or_insert((exported.name, module));
        }
    }
    let mut parents = BTreeMap::new();
    for (mangled, exported, module) in class_parents(defs, exporters) {
        if module == defs.package_name {
            parents.insert(mangled, exported.name.to_string());
            continue;
        }
        let already_imported = types
            .iter()
            .find(|(_, target)| **target == (exported.name, module))
            .map(|(local, _)| local.clone());
        let local = if let Some(local) = already_imported {
            local
        } else {
            let local = free_local_name(defs, &types, exported.name);
            types.insert(local.clone(), (exported.name, module));
            local
        };
        parents.insert(mangled, local);
    }
    BlockImports { types, parents }
}

/// `public` if the block can import a class under it, else `public` with the
/// first numeric suffix nothing in the block uses. Each name the block holds
/// rules out at most one suffix, so one past their count is always free.
fn free_local_name(
    defs: &PackageDeclaration,
    imports: &BTreeMap<String, (&str, &str)>,
    public: &str,
) -> String {
    let is_free = |name: &str| !imports.contains_key(name) && !declares(defs, name, true);
    if is_free(public) {
        return public.to_string();
    }
    let taken = imports
        .len()
        .saturating_add(defs.types.len())
        .saturating_add(defs.values.len())
        .saturating_add(defs.namespaces.len());
    (1..=taken.saturating_add(1))
        .map(|suffix| format!("{public}{suffix}"))
        .find(|name| is_free(name))
        .unwrap_or_else(|| format!("{public}{}", taken.saturating_add(1)))
}

/// Whether `defs` declares `name` itself in a way that would clash with
/// importing a type under it.
fn declares(defs: &PackageDeclaration, name: &str, borrowed_is_value: bool) -> bool {
    defs.types.contains_key(name)
        || (borrowed_is_value
            && (defs.values.contains_key(name) || defs.namespaces.contains_key(name)))
}

/// The parent of each class `defs` exports that extends a known module's
/// class, by mangled name, with that class and its module.
fn class_parents<'a>(
    defs: &'a PackageDeclaration,
    exporters: &TypeExporters<'a>,
) -> Vec<(&'a str, ExportedType<'a>, &'a str)> {
    defs.types
        .values()
        .filter_map(|symbol| match &symbol.kind {
            TypeKind::Class {
                extends: Some(extends),
                ..
            } => {
                let parent = extends.parent.as_str();
                let &(exported, module) = exporters.by_mangled.get(parent)?;
                Some((parent, exported, module))
            }
            _ => None,
        })
        .collect()
}

/// The by-name types the rendered declarations of `defs` mention: in its
/// values, namespaces, and the public surface of its types, which is what
/// `render_ts_declarations` prints. A private function or member doesn't
/// appear in the text, so its types don't count.
///
/// Every by-name `Type` serializes with its declaring symbol's `mangled` name,
/// so walking the serialized form finds them in every signature and member
/// without a visitor over each declaration shape. A reference renders as its
/// name and type arguments only, so the walk goes into `args` and not into an
/// alias's body. A class's parent isn't a `Type`; `class_parents` finds
/// those.
fn rendered_type_references(defs: &PackageDeclaration) -> Vec<TypeReference> {
    let types: BTreeMap<&String, TypeSymbol> = defs
        .types
        .iter()
        .map(|(name, symbol)| (name, public_surface(symbol)))
        .collect();
    let rendered = [
        serde_json::to_value(&defs.values),
        serde_json::to_value(&types),
        serde_json::to_value(&defs.namespaces),
    ];
    // A part that won't serialize still renders; it just imports nothing.
    let mut pending: Vec<&serde_json::Value> = rendered.iter().flatten().collect();
    let mut references = Vec::new();
    while let Some(value) = pending.pop() {
        match value {
            serde_json::Value::Object(fields) => {
                let Some(serde_json::Value::String(mangled)) = fields.get("mangled") else {
                    pending.extend(fields.values());
                    continue;
                };
                let text = |key: &str| {
                    fields
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                references.push(TypeReference {
                    mangled: mangled.clone(),
                    package: text("package"),
                    name: text("name"),
                });
                pending.extend(fields.get("args"));
            }
            serde_json::Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    references
}

/// `symbol` as `render_ts_type` prints it: a class keeps only its public
/// members.
fn public_surface(symbol: &TypeSymbol) -> TypeSymbol {
    let mut symbol = symbol.clone();
    if let TypeKind::Class {
        fields,
        narrowing_checks,
        methods,
        method_visibility,
        accessors,
        statics,
        static_visibility,
        static_fields,
        ..
    } = &mut symbol.kind
    {
        let is_public = |visibility: Option<&crate::Visibility>| {
            visibility != Some(&crate::Visibility::Private)
        };
        accessors.retain(|accessor| is_public(fields.get(accessor.name()).map(|f| &f.visibility)));
        fields.retain(|_, field| is_public(Some(&field.visibility)));
        static_fields.retain(|_, field| is_public(Some(&field.visibility)));
        methods.retain(|name, _| is_public(method_visibility.get(name)));
        statics.retain(|name, _| is_public(static_visibility.get(name)));
        narrowing_checks.clear();
    }
    symbol
}

/// Render editor-facing TypeScript ambient declarations for always-in-scope
/// built-ins.
pub fn render_lib_submilli_d_ts() -> String {
    let defs = builtin_package_declaration();
    let globals = global_classes();
    let parent = |extends: &ClassExtends| {
        let global = globals.get(extends.parent.as_str())?;
        Some(ts_named_type(global, &extends.args))
    };
    let mut out = String::new();
    render_ts_declarations(&mut out, &defs, "", "declare ", &parent);
    render_ts_compiler_globals(&mut out, &defs);
    let json = json_package_declaration();
    render_ts_declarations(&mut out, &json, "", "declare ", &|_| None);
    out.trim_end().to_string()
}

fn render_ts_compiler_globals(out: &mut String, defs: &PackageDeclaration) {
    if !defs.types.contains_key("Function") {
        out.push_str("interface Function {}\n\n");
    }
    out.push_str("interface CallableFunction extends Function {}\n\n");
    out.push_str("interface NewableFunction extends Function {}\n\n");
    out.push_str("interface IArguments {}\n\n");
    out.push_str(
        "/** Throw `Error(message)` if `condition` is false. Compiler intrinsic — always in scope, not imported. */\n\
         declare function assert(condition: boolean, message?: string): void;\n\n",
    );
    render_ts_checker_plumbing(out, defs);
}

// Members the TS checker requires structurally but submilli handles
// intrinsically, so the prelude declarations lack them: with `target` ≥
// es2015 `for-of` needs `[Symbol.iterator]()` (submilli's protocol is a
// plain `iterator()` method; arrays iterate at the language level), and
// `arr[i]` needs an index signature. Merged into `Array` and into every
// prelude interface declaring `iterator()`. Users never write these; only
// the checker reads them.
fn render_ts_checker_plumbing(out: &mut String, defs: &PackageDeclaration) {
    out.push_str("declare const Symbol: { readonly iterator: unique symbol };\n\n");
    out.push_str(
        "interface Array<T> {\n  [index: number]: T;\n  [Symbol.iterator](): Iterator<T>;\n}\n\n",
    );
    for (name, sym) in &defs.types {
        if let TypeKind::Interface {
            generics, methods, ..
        } = &sym.kind
            && let Some(m) = methods.get("iterator")
        {
            let _ = writeln!(
                out,
                "interface {name}{} {{\n  [Symbol.iterator](): {};\n}}\n",
                ts_interface_generics(name, generics, "declare "),
                ts_type(&m.ret)
            );
        }
    }
}

fn render_declare_module(out: &mut String, defs: &PackageDeclaration) {
    let _ = writeln!(out, "declare module \"{}\" {{", defs.package_name);
    render_ts_declarations(out, defs, "  ", "export ", &|_| None);
    let _ = writeln!(out, "}}\n");
}

/// `class_parent` renders a class's `extends` target, or declines, in which
/// case the class renders without one.
fn render_ts_declarations(
    out: &mut String,
    defs: &PackageDeclaration,
    indent: &str,
    export_prefix: &str,
    class_parent: &dyn Fn(&ClassExtends) -> Option<String>,
) {
    for (name, sym) in &defs.values {
        if is_hidden_prelude_value(&defs.package_name, name) {
            continue;
        }
        push_doc(out, value_doc(&sym.kind), indent);
        render_ts_value(out, name, &sym.kind, indent, export_prefix);
    }
    // Only the runtime's own modules pair a type with an `XConstructor`
    // interface for its static side; in a package they're two ordinary types.
    let pairs_constructors = defs.package_name.starts_with("submilli:");
    for (name, sym) in &defs.types {
        if is_hidden_prelude_type(&defs.package_name, name) {
            continue;
        }
        if pairs_constructors && name.ends_with("Constructor") {
            continue;
        }
        if pairs_constructors && defs.types.contains_key(&format!("{name}Constructor")) {
            render_ts_type_with_ctor(
                out,
                &defs.types,
                name,
                indent,
                export_prefix,
                !defs.values.contains_key(name),
            );
        } else {
            push_doc(out, type_doc(&sym.kind), indent);
            let parent = match &sym.kind {
                TypeKind::Class {
                    extends: Some(extends),
                    ..
                } => class_parent(extends),
                _ => None,
            };
            render_ts_type(
                out,
                name,
                &sym.kind,
                indent,
                export_prefix,
                parent.as_deref(),
            );
        }
    }
    for (name, ns) in &defs.namespaces {
        render_ts_namespace(out, name, ns, indent, export_prefix);
    }
}

fn is_hidden_prelude_value(package_name: &str, name: &str) -> bool {
    package_name == crate::mangle::PRELUDE_PACKAGE
        && (name.starts_with("string_")
            || matches!(
                name,
                "string_concat" | "string_eq" | "string_length" | "string_cmp"
            ))
}

fn is_hidden_prelude_type(package_name: &str, name: &str) -> bool {
    package_name == crate::mangle::PRELUDE_PACKAGE && name.contains('#')
}

fn render_ts_value(
    out: &mut String,
    name: &str,
    kind: &ValueKind,
    indent: &str,
    export_prefix: &str,
) {
    let reserved_export = export_prefix == "export " && is_ts_reserved_word(name);
    let declared_name = if reserved_export {
        format!("{name}_")
    } else {
        name.to_string()
    };
    let decl_export_prefix = if reserved_export { "" } else { export_prefix };
    match kind {
        ValueKind::Function {
            generics,
            params,
            ret,
            type_predicate,
            ..
        } => {
            let ret = ts_return_type(params, ret, type_predicate.as_ref());
            let _ = writeln!(
                out,
                "{indent}{decl_export_prefix}function {declared_name}{}({}): {ret};",
                generics_str(generics),
                ts_params_str(params)
            );
        }
        ValueKind::Let { ty, .. } => {
            let _ = writeln!(
                out,
                "{indent}{decl_export_prefix}let {declared_name}: {};",
                ts_type(ty)
            );
        }
        ValueKind::Const { ty, .. } => {
            let _ = writeln!(
                out,
                "{indent}{decl_export_prefix}const {declared_name}: {};",
                ts_type(ty)
            );
        }
    }
    if reserved_export {
        let _ = writeln!(out, "{indent}export {{ {declared_name} as {name} }};");
    }
    out.push('\n');
}

/// `parent` is the rendered `extends` target of a class, if it has one.
fn render_ts_type(
    out: &mut String,
    name: &str,
    kind: &TypeKind,
    indent: &str,
    export_prefix: &str,
    parent: Option<&str>,
) {
    let inner = format!("{indent}  ");
    match kind {
        TypeKind::Interface {
            generics,
            methods,
            properties,
            index,
            ..
        } => {
            let _ = writeln!(
                out,
                "{indent}{export_prefix}interface {name}{} {{",
                ts_interface_generics(name, generics, export_prefix)
            );
            if let Some(index) = index {
                let ro = if index.readonly { "readonly " } else { "" };
                let _ = writeln!(out, "{inner}{ro}[key: string]: {};", ts_type(&index.value));
            }
            for (pname, prop) in properties {
                push_doc(out, &prop.doc, &inner);
                let ro = if prop.readonly { "readonly " } else { "" };
                let opt = if prop.optional { "?" } else { "" };
                let _ = writeln!(out, "{inner}{ro}{pname}{opt}: {};", ts_type(&prop.ty));
            }
            for (mname, m) in methods {
                push_doc(out, &m.doc, &inner);
                let ret = ts_return_type(&m.params, &m.ret, m.predicate.as_ref());
                match mname.as_str() {
                    "@call" => {
                        let _ = writeln!(
                            out,
                            "{inner}{}({}): {ret};",
                            generics_str(&m.generics),
                            ts_params_str(&m.params)
                        );
                    }
                    "new" => {
                        let _ = writeln!(
                            out,
                            "{inner}new {}({}): {ret};",
                            generics_str(&m.generics),
                            ts_params_str(&m.params)
                        );
                    }
                    _ => {
                        let _ = writeln!(
                            out,
                            "{inner}{mname}{}({}): {ret};",
                            generics_str(&m.generics),
                            ts_params_str(&m.params)
                        );
                    }
                }
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
        TypeKind::NumberEnum { variants, .. } => {
            let _ = writeln!(out, "{indent}{export_prefix}enum {name} {{");
            for (v, value) in variants {
                let _ = writeln!(out, "{inner}{v} = {value},");
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
        TypeKind::StringEnum { variants, .. } => {
            let _ = writeln!(out, "{indent}{export_prefix}enum {name} {{");
            for (v, value) in variants {
                let _ = writeln!(out, "{inner}{v} = \"{}\",", escape_string_literal(value));
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
        TypeKind::Alias { generics, ty, .. } => {
            let _ = writeln!(
                out,
                "{indent}{export_prefix}type {name}{} = {};\n",
                generics_str(generics),
                ts_type(ty)
            );
        }
        TypeKind::Class {
            generics,
            fields,
            methods,
            method_visibility,
            statics,
            static_visibility,
            static_fields,
            accessors,
            constructor,
            ..
        } => {
            // Private members are part of the in-memory class but never the public
            // API an agent can call, so they're omitted from the rendered surface
            // (`packages.docs` and the generated `.d.ts`). Privacy itself is enforced
            // by the typechecker; this only keeps private names/types out of the docs.
            let extends = parent.map(|parent| format!(" extends {parent}"));
            let _ = writeln!(
                out,
                "{indent}{export_prefix}class {name}{}{} {{",
                generics_str(generics),
                extends.unwrap_or_default()
            );
            for (fname, field) in static_fields {
                if field.visibility == crate::Visibility::Private {
                    continue;
                }
                push_doc(out, &field.doc, &inner);
                let ro = if field.readonly { "readonly " } else { "" };
                let _ = writeln!(out, "{inner}static {ro}{fname}: {};", ts_type(&field.ty));
            }
            for (mname, m) in statics {
                if static_visibility.get(mname) == Some(&crate::Visibility::Private) {
                    continue;
                }
                push_doc(out, &m.doc, &inner);
                let ret = ts_return_type(&m.params, &m.ret, m.predicate.as_ref());
                let _ = writeln!(
                    out,
                    "{inner}static {mname}{}({}): {ret};",
                    generics_str(&m.generics),
                    ts_params_str(&m.params)
                );
            }
            for (fname, field) in fields {
                if field.visibility == crate::Visibility::Private
                    || accessors.iter().any(|a| a.name() == fname)
                {
                    continue;
                }
                push_doc(out, &field.doc, &inner);
                let ro = if field.readonly { "readonly " } else { "" };
                let opt = if field.optional { "?" } else { "" };
                let _ = writeln!(out, "{inner}{ro}{fname}{opt}: {};", ts_type(&field.ty));
            }
            render_class_accessors(out, fields, accessors, &inner, ts_type);
            let _ = writeln!(out, "{inner}constructor({});", ts_params_str(constructor));
            for (mname, m) in methods {
                if method_visibility.get(mname) == Some(&crate::Visibility::Private) {
                    continue;
                }
                push_doc(out, &m.doc, &inner);
                let ret = ts_return_type(&m.params, &m.ret, m.predicate.as_ref());
                let _ = writeln!(
                    out,
                    "{inner}{mname}{}({}): {ret};",
                    generics_str(&m.generics),
                    ts_params_str(&m.params)
                );
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
    }
}

/// Render a class's accessor properties for the public surface (docs / `.d.ts`).
/// Same-typed get+set renders as a writable property, get-only as `readonly`,
/// and a write-only or differently-typed pair as explicit `get`/`set` lines.
/// Private accessors (visibility lives on the property's `FieldSig`) are skipped.
fn render_class_accessors(
    out: &mut String,
    fields: &std::collections::BTreeMap<String, crate::FieldSig>,
    accessors: &[crate::AccessorSig],
    inner: &str,
    fmt_ty: impl Fn(&Type) -> String,
) {
    use crate::AccessorSig;
    let mut by_name: std::collections::BTreeMap<&str, (Option<&Type>, Option<&crate::Param>)> =
        std::collections::BTreeMap::new();
    for acc in accessors {
        let entry = by_name.entry(acc.name()).or_insert((None, None));
        match acc {
            AccessorSig::Getter { ret_ty, .. } => entry.0 = Some(ret_ty),
            AccessorSig::Setter { param, .. } => entry.1 = Some(param),
        }
    }
    for (name, (get, set)) in by_name {
        if fields.get(name).map(|f| f.visibility) == Some(crate::Visibility::Private) {
            continue;
        }
        match (get, set) {
            (Some(r), Some(p)) if r == &p.ty => {
                let _ = writeln!(out, "{inner}{name}: {};", fmt_ty(r));
            }
            (Some(r), Some(p)) => {
                let _ = writeln!(out, "{inner}get {name}(): {};", fmt_ty(r));
                let _ = writeln!(out, "{inner}set {name}({}: {});", p.name, fmt_ty(&p.ty));
            }
            (Some(r), None) => {
                let _ = writeln!(out, "{inner}readonly {name}: {};", fmt_ty(r));
            }
            (None, Some(p)) => {
                let _ = writeln!(out, "{inner}set {name}({}: {});", p.name, fmt_ty(&p.ty));
            }
            (None, None) => {}
        }
    }
}

fn render_ts_type_with_ctor(
    out: &mut String,
    types: &BTreeMap<String, TypeSymbol>,
    name: &str,
    indent: &str,
    export_prefix: &str,
    synthesize_binding: bool,
) {
    if let Some(sym) = types.get(name) {
        push_doc(out, type_doc(&sym.kind), indent);
        render_ts_type(out, name, &sym.kind, indent, export_prefix, None);
    }
    let ctor = format!("{name}Constructor");
    if let Some(sym) = types.get(&ctor) {
        push_doc(out, type_doc(&sym.kind), indent);
        render_ts_type(out, &ctor, &sym.kind, indent, export_prefix, None);
        if synthesize_binding {
            let _ = writeln!(out, "{indent}{export_prefix}const {name}: {ctor};\n");
        }
    }
}

fn render_ts_namespace(
    out: &mut String,
    name: &str,
    ns: &NamespaceSymbol,
    indent: &str,
    export_prefix: &str,
) {
    push_doc(out, &ns.doc, indent);
    let _ = writeln!(out, "{indent}{export_prefix}namespace {name} {{");
    let inner = format!("{indent}  ");
    for (vname, sym) in &ns.values {
        push_doc(out, value_doc(&sym.kind), &inner);
        render_ts_value(out, vname, &sym.kind, &inner, "");
    }
    for (tname, sym) in &ns.types {
        push_doc(out, type_doc(&sym.kind), &inner);
        render_ts_type(out, tname, &sym.kind, &inner, "", None);
    }
    for (nname, sub) in &ns.namespaces {
        render_ts_namespace(out, nname, sub, &inner, "");
    }
    let _ = writeln!(out, "{indent}}}\n");
}

fn ts_params_str(params: &[Param]) -> String {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let prefix = if p.rest { "..." } else { "" };
            let opt = if p.default.is_some() { "?" } else { "" };
            let name = if p.name.is_empty() {
                format!("arg{i}")
            } else {
                p.name.clone()
            };
            format!("{prefix}{name}{opt}: {}", ts_type(&p.ty))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn ts_return_type(params: &[Param], ret: &Type, predicate: Option<&TypePredicate>) -> String {
    let Some(predicate) = predicate else {
        return ts_type(ret);
    };
    let Some(param) = params.get(predicate.parameter_index as usize) else {
        return ts_type(ret);
    };
    let name = if param.name.is_empty() {
        format!("arg{}", predicate.parameter_index)
    } else {
        param.name.clone()
    };
    let asserted = ts_type(&predicate.asserted_type);
    match param.ty {
        Type::TypeVar(_) | Type::GenericParam { .. } => {
            format!("{name} is {} & {asserted}", ts_type(&param.ty))
        }
        _ => format!("{name} is {asserted}"),
    }
}

fn ts_type(ty: &Type) -> String {
    match ty {
        Type::Refined { original, ty } => format!("({} & {})", ts_type(original), ts_type(ty)),
        Type::Number => "number".to_string(),
        Type::NumberLiteral(n) => n.0.to_string(),
        Type::BigInt => "bigint".to_string(),
        Type::String => "string".to_string(),
        Type::StringLiteral(s) => format!("\"{}\"", escape_string_literal(s)),
        Type::Uint8Array => "Uint8Array".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::BooleanLiteral(value) => value.to_string(),
        Type::Null => "null".to_string(),
        Type::Void => "void".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Function {
            params,
            ret,
            predicate,
            has_rest,
        } => {
            let params = params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let prefix = if *has_rest && i == params.len().saturating_sub(1) {
                        "..."
                    } else {
                        ""
                    };
                    format!("{prefix}arg{i}: {}", ts_type(p))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let ret = predicate.as_deref().map_or_else(
                || ts_type(ret),
                |p| format!("arg{} is {}", p.parameter_index, ts_type(&p.asserted_type)),
            );
            format!("({params}) => {ret}")
        }
        Type::Object { fields, index } => {
            let mut members: Vec<String> = fields
                .iter()
                .map(|(name, field)| {
                    let opt = if field.optional { "?" } else { "" };
                    let ro = if field.readonly { "readonly " } else { "" };
                    format!("{ro}{name}{opt}: {}", ts_type(&field.ty))
                })
                .collect();
            if let Some(index) = index {
                let ro = if index.readonly { "readonly " } else { "" };
                members.push(format!("{ro}[key: string]: {}", ts_type(&index.value)));
            }
            if members.is_empty() {
                "{}".into()
            } else {
                format!("{{ {} }}", members.join("; "))
            }
        }
        Type::Array(elem) => format!("{}[]", ts_type_array_element(elem)),
        Type::Tuple(elements) => {
            let elements = elements.iter().map(ts_type).collect::<Vec<_>>().join(", ");
            format!("[{elements}]")
        }
        Type::Readonly(inner) => format!("readonly {}", ts_type(inner)),
        Type::Error => "never".to_string(),
        Type::Never => "never".to_string(),
        Type::TypeVar(name) | Type::GenericParam { name, .. } => name.clone(),
        Type::InterfaceRef { name, args, .. }
        | Type::ClassRef { name, args, .. }
        | Type::Alias { name, args, .. } => ts_named_type(name, args),
        Type::NumberEnum { name, .. } | Type::StringEnum { name, .. } => name.clone(),
        Type::Union(members) => members
            .iter()
            .map(ts_type_union_member)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::AliasRef { name, args, .. } => ts_named_type(name, args),
    }
}

fn ts_named_type(name: &str, args: &[Type]) -> String {
    if args.is_empty() {
        return name.to_string();
    }
    let args = args.iter().map(ts_type).collect::<Vec<_>>().join(", ");
    format!("{name}<{args}>")
}

fn ts_type_array_element(ty: &Type) -> String {
    match ty {
        Type::Function { .. } | Type::Union(_) | Type::Readonly(_) => format!("({})", ts_type(ty)),
        _ => ts_type(ty),
    }
}

fn ts_type_union_member(ty: &Type) -> String {
    match ty {
        Type::Function { .. } => format!("({})", ts_type(ty)),
        _ => ts_type(ty),
    }
}

fn is_ts_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "as"
            | "async"
            | "await"
            | "from"
            | "get"
            | "let"
            | "of"
            | "set"
            | "static"
            | "using"
            | "yield"
    )
}

fn json_package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(crate::mangle::PRELUDE_PACKAGE);
    let json_prefix = crate::mangle::prelude(JSON_BUILTIN);
    let mut json = NamespaceSymbol {
        name: JSON_BUILTIN.to_string(),
        mangled_prefix: json_prefix.clone(),
        declaration_span: Span::at(FileId::JSON),
        values: BTreeMap::new(),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: None,
    };
    json.values.insert(
        "stringify".to_string(),
        ValueSymbol {
            name: "stringify".to_string(),
            mangled_name: crate::mangle::extend(&json_prefix, "stringify"),
            declaration_span: Span::at(FileId::JSON),
            kind: ValueKind::Function {
                generics: vec!["T".to_string()],
                params: vec![
                    Param::new("value", Type::TypeVar("T".to_string())),
                    Param {
                        name: "replacer".to_string(),
                        ty: Type::Null,
                        default: Some(crate::DefaultValue::Null),
                        rest: false,
                    },
                    Param {
                        name: "space".to_string(),
                        ty: Type::union(vec![Type::Number, Type::String, Type::Null]),
                        default: Some(crate::DefaultValue::Null),
                        rest: false,
                    },
                ],
                ret: Type::String,
                type_predicate: None,
                doc: crate::doc(FileId::JSON, "/** Serialize a value to a JSON string. */"),
            },
        },
    );
    json.values.insert(
        "parse".to_string(),
        ValueSymbol {
            name: "parse".to_string(),
            mangled_name: crate::mangle::extend(&json_prefix, "parse"),
            declaration_span: Span::at(FileId::JSON),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("text", Type::String)],
                ret: Type::Unknown,
                type_predicate: None,
                doc: crate::doc(
                    FileId::JSON,
                    "/** Parse a JSON string as unknown; use `JSON.parse(s) as T` to validate a target type. */",
                ),
            },
        },
    );
    defs.namespaces.insert(JSON_BUILTIN.to_string(), json);
    defs
}

fn render_value(out: &mut String, name: &str, kind: &ValueKind, indent: &str) {
    match kind {
        ValueKind::Function {
            generics,
            params,
            ret,
            ..
        } => {
            let _ = writeln!(
                out,
                "{indent}function {name}{}({}): {ret};\n",
                generics_str(generics),
                params_str(params)
            );
        }
        ValueKind::Let { ty, .. } => {
            let _ = writeln!(out, "{indent}let {name}: {ty};\n");
        }
        ValueKind::Const { ty, .. } => {
            let _ = writeln!(out, "{indent}const {name}: {ty};\n");
        }
    }
}

fn render_type(out: &mut String, name: &str, kind: &TypeKind, indent: &str) {
    let inner = format!("{indent}  ");
    match kind {
        TypeKind::Interface {
            generics,
            methods,
            properties,
            ..
        } => {
            let _ = writeln!(out, "{indent}interface {name}{} {{", generics_str(generics));
            for (pname, prop) in properties {
                push_doc(out, &prop.doc, &inner);
                let ro = if prop.readonly { "readonly " } else { "" };
                let opt = if prop.optional { "?" } else { "" };
                let _ = writeln!(out, "{inner}{ro}{pname}{opt}: {};", prop.ty);
            }
            for (mname, m) in methods {
                push_doc(out, &m.doc, &inner);
                let _ = writeln!(
                    out,
                    "{inner}{mname}{}({}): {};",
                    generics_str(&m.generics),
                    params_str(&m.params),
                    m.ret
                );
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
        TypeKind::NumberEnum { variants, .. } => {
            let _ = writeln!(out, "{indent}enum {name} {{");
            for (v, value) in variants {
                let _ = writeln!(out, "{inner}{v} = {value},");
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
        TypeKind::StringEnum { variants, .. } => {
            let _ = writeln!(out, "{indent}enum {name} {{");
            for (v, value) in variants {
                let _ = writeln!(out, "{inner}{v} = \"{value}\",");
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
        TypeKind::Alias { generics, ty, .. } => {
            let _ = writeln!(
                out,
                "{indent}type {name}{} = {ty};\n",
                generics_str(generics)
            );
        }
        TypeKind::Class {
            generics,
            fields,
            methods,
            method_visibility,
            statics,
            static_visibility,
            static_fields,
            accessors,
            constructor,
            ..
        } => {
            // Private members exist in the class but are never callable from a
            // consumer, so they're omitted from the rendered API surface
            // (`packages.docs`). Privacy is enforced by the typechecker; this only
            // keeps private names/types out of the agent-facing docs.
            let _ = writeln!(out, "{indent}class {name}{} {{", generics_str(generics));
            for (fname, field) in static_fields {
                if field.visibility == crate::Visibility::Private {
                    continue;
                }
                push_doc(out, &field.doc, &inner);
                let ro = if field.readonly { "readonly " } else { "" };
                let _ = writeln!(out, "{inner}static {ro}{fname}: {};", field.ty);
            }
            for (mname, m) in statics {
                if static_visibility.get(mname) == Some(&crate::Visibility::Private) {
                    continue;
                }
                push_doc(out, &m.doc, &inner);
                let _ = writeln!(
                    out,
                    "{inner}static {mname}{}({}): {};",
                    generics_str(&m.generics),
                    params_str(&m.params),
                    m.ret
                );
            }
            for (fname, field) in fields {
                if field.visibility == crate::Visibility::Private
                    || accessors.iter().any(|a| a.name() == fname)
                {
                    continue;
                }
                push_doc(out, &field.doc, &inner);
                let ro = if field.readonly { "readonly " } else { "" };
                let opt = if field.optional { "?" } else { "" };
                let _ = writeln!(out, "{inner}{ro}{fname}{opt}: {};", field.ty);
            }
            render_class_accessors(out, fields, accessors, &inner, ToString::to_string);
            let _ = writeln!(out, "{inner}constructor({});", params_str(constructor));
            for (mname, m) in methods {
                if method_visibility.get(mname) == Some(&crate::Visibility::Private) {
                    continue;
                }
                push_doc(out, &m.doc, &inner);
                let _ = writeln!(
                    out,
                    "{inner}{mname}{}({}): {};",
                    generics_str(&m.generics),
                    params_str(&m.params),
                    m.ret
                );
            }
            let _ = writeln!(out, "{indent}}}\n");
        }
    }
}

/// Render `name`'s interface followed by its `{name}Constructor` (the `new` /
/// static side, e.g. `Array.isArray`) and the `const name: nameConstructor`
/// binding that ties them — the lib.d.ts shape an LLM expects. Either half may
/// be absent.
fn render_type_with_ctor(
    out: &mut String,
    types: &BTreeMap<String, TypeSymbol>,
    name: &str,
    indent: &str,
) {
    if let Some(sym) = types.get(name) {
        push_doc(out, type_doc(&sym.kind), indent);
        render_type(out, name, &sym.kind, indent);
    }
    let ctor = format!("{name}Constructor");
    if let Some(sym) = types.get(&ctor) {
        push_doc(out, type_doc(&sym.kind), indent);
        render_type(out, &ctor, &sym.kind, indent);
        let _ = writeln!(out, "{indent}const {name}: {ctor};\n");
    }
}

fn render_namespace(out: &mut String, name: &str, ns: &NamespaceSymbol, indent: &str) {
    let _ = writeln!(out, "{indent}namespace {name} {{");
    let inner = format!("{indent}  ");
    for (vname, sym) in &ns.values {
        push_doc(out, value_doc(&sym.kind), &inner);
        render_value(out, vname, &sym.kind, &inner);
    }
    for (tname, sym) in &ns.types {
        // The namespace's own values already bind each `XConstructor` to `X`,
        // so render every type (constructors included) plainly — no merge.
        push_doc(out, type_doc(&sym.kind), &inner);
        render_type(out, tname, &sym.kind, &inner);
    }
    for (nname, sub) in &ns.namespaces {
        render_namespace(out, nname, sub, &inner);
    }
    let _ = writeln!(out, "{indent}}}\n");
}

fn generics_str(generics: &[String]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generics.join(", "))
    }
}

// TS ≥5.6 hard-codes that the global `Iterable`/`Iterator` types take three
// type parameters; phantom defaults satisfy the checker while keeping
// single-argument usage (`Iterable<T>`) valid.
fn ts_interface_generics(name: &str, generics: &[String], export_prefix: &str) -> String {
    if export_prefix == "declare "
        && matches!(name, "Iterable" | "Iterator")
        && let [t] = generics
    {
        return format!("<{t}, TReturn = any, TNext = any>");
    }
    generics_str(generics)
}

fn params_str(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| {
            let prefix = if p.rest { "..." } else { "" };
            let opt = if p.default.is_some() { "?" } else { "" };
            format!("{prefix}{}{opt}: {}", p.name, p.ty)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn push_doc(out: &mut String, doc: &Option<DocComment>, indent: &str) {
    let Some(doc) = doc else { return };
    let mut lines: Vec<String> = doc.summary.lines().map(str::to_string).collect();
    for p in &doc.params {
        lines.push(format!("@param {} {}", p.name, p.description));
    }
    for cap in &doc.capabilities {
        let mut line = format!("@capability {}", cap.capability);
        if !cap.bindings.is_empty() {
            let bindings = cap
                .bindings
                .iter()
                .map(|b| {
                    let value = match &b.kind {
                        DocCapabilityBindingKind::Parameter { param, path, .. }
                            if *param == b.field && path.is_empty() =>
                        {
                            String::new()
                        }
                        DocCapabilityBindingKind::Parameter { param, path, .. } => {
                            let suffix = path
                                .iter()
                                .map(|part| format!(".{part}"))
                                .collect::<String>();
                            format!(": ${param}{suffix}")
                        }
                        DocCapabilityBindingKind::Type { name, .. } => format!(": {name}"),
                        DocCapabilityBindingKind::Literal { value, .. } => {
                            format!(": {}", literal_str(value))
                        }
                    };
                    format!("{}{}", b.field, value)
                })
                .collect::<Vec<_>>()
                .join(", ");
            line.push_str(&format!(" {{ {bindings} }}"));
        }
        if !cap.description.is_empty() {
            line.push_str(" - ");
            line.push_str(&cap.description);
        }
        lines.push(line);
    }
    if let Some(r) = &doc.returns {
        lines.push(format!("@returns {}", r.description));
    }
    if lines.is_empty() {
        return;
    }
    let _ = writeln!(out, "{indent}/**");
    for line in lines {
        let _ = writeln!(out, "{indent} * {line}");
    }
    let _ = writeln!(out, "{indent} */");
}

fn literal_str(value: &DocCapabilityLiteral) -> String {
    match value {
        DocCapabilityLiteral::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        DocCapabilityLiteral::Number(n) => n.clone(),
        DocCapabilityLiteral::Boolean(v) => v.to_string(),
        DocCapabilityLiteral::Null => "null".to_string(),
    }
}

fn value_doc(kind: &ValueKind) -> &Option<DocComment> {
    match kind {
        ValueKind::Function { doc, .. }
        | ValueKind::Let { doc, .. }
        | ValueKind::Const { doc, .. } => doc,
    }
}

fn type_doc(kind: &TypeKind) -> &Option<DocComment> {
    match kind {
        TypeKind::Interface { doc, .. }
        | TypeKind::Class { doc, .. }
        | TypeKind::NumberEnum { doc, .. }
        | TypeKind::StringEnum { doc, .. }
        | TypeKind::Alias { doc, .. } => doc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_filters_before_counting_omitted_entries() {
        let extra = (0..CATALOG_LIMIT)
            .map(|index| CatalogEntry {
                name: format!("@acme/package{index}"),
                source: "registry".into(),
                description: String::new(),
            })
            .collect();
        let catalog = catalog_filtered(extra, |name| !name.starts_with("submilli:"));
        assert_eq!(catalog.entries.len(), CATALOG_LIMIT);
        assert_eq!(catalog.remaining, 0);
        assert!(
            catalog
                .entries
                .iter()
                .all(|entry| entry.name.starts_with("@acme/"))
        );
    }

    #[test]
    fn http_docs_render_signatures() {
        let doc = docs("submilli:http").expect("http module");
        assert_eq!(doc.name, "submilli:http");
        assert!(!doc.description.is_empty());
        assert!(doc.declarations.contains("function get("));
        assert!(doc.declarations.contains("function post("));
        assert!(doc.declarations.contains("@capability http.get"));
        assert!(doc.declarations.contains("@capability http.download"));
    }

    #[test]
    fn fs_and_secrets_docs_render_capabilities() {
        let fs = docs("submilli:fs").expect("fs module");
        assert!(fs.declarations.contains("@capability fs.read"));
        assert!(fs.declarations.contains("@capability fs.write"));
        assert!(fs.declarations.contains("@capability fs.list"));

        let secrets = docs("submilli:secrets").expect("secrets module");
        assert!(secrets.declarations.contains("@capability secrets.get"));
    }

    #[test]
    fn security_is_not_user_facing() {
        assert!(docs("submilli:security").is_none());
        assert!(!search("").iter().any(|m| m.name == INTERNAL_MODULE));
    }

    #[test]
    fn search_matches_symbol_names() {
        let hits = search("sha256");
        assert!(
            hits.iter().any(|m| m.name == "submilli:crypto"),
            "{:?}",
            names(&hits)
        );
    }

    #[test]
    fn empty_query_lists_all_user_modules() {
        let names = names(&search(""));
        assert!(names.contains(&"submilli:crypto".to_string()));
        assert!(names.contains(&"submilli:http".to_string()));
        assert!(!names.contains(&INTERNAL_MODULE.to_string()));
    }

    fn names(hits: &[ModuleSummary]) -> Vec<String> {
        hits.iter().map(|m| m.name.clone()).collect()
    }

    #[test]
    fn builtins_lists_headline_types_and_namespaces() {
        let b = builtins();
        for expected in ["Array", "Map", "Set", "String", "Number", "Error"] {
            assert!(
                b.types.contains(&expected.to_string()),
                "missing {expected}"
            );
        }
        // Plumbing and flattened namespace members stay out of the catalog.
        for hidden in [
            "ArrayConstructor",
            "Console",
            "Iterator",
            "Temporal#Instant",
        ] {
            assert!(!b.types.contains(&hidden.to_string()), "leaked {hidden}");
        }
        for ns in ["Math", "Temporal", "JSON"] {
            assert!(
                b.namespaces.contains(&ns.to_string()),
                "missing namespace {ns}"
            );
        }
    }

    #[test]
    fn every_builtin_has_renderable_docs() {
        let b = builtins();
        for name in b.types.iter().chain(b.namespaces.iter()) {
            assert!(
                builtin_docs(name).is_some_and(|d| !d.is_empty()),
                "no docs for built-in {name}"
            );
        }
    }

    #[test]
    fn dotted_path_returns_the_member_slice() {
        let slice = builtin_docs("Temporal.Instant").expect("Temporal.Instant");
        // The binding, the interface, and the constructor side — the whole
        // slice, not whichever map is walked first.
        assert!(slice.contains("const Instant: Temporal.InstantConstructor;"));
        assert!(slice.contains("interface Instant {"));
        assert!(slice.contains("interface InstantConstructor {"));
        // Wrapped in its namespace, so the reader learns `Temporal.Instant.from`.
        assert!(slice.starts_with("namespace Temporal {"), "{slice}");
        // Doc comments survive the slice.
        assert!(slice.contains("A point in time, to nanosecond precision"));
        // Members of sibling types stay out.
        assert!(!slice.contains("interface ZonedDateTime {"), "{slice}");

        let full = builtin_docs("Temporal").expect("Temporal");
        assert!(
            slice.len() * 4 < full.len(),
            "slice {} vs full {}",
            slice.len(),
            full.len()
        );
    }

    #[test]
    fn dotted_path_resolves_nested_namespaces_and_values() {
        let now = builtin_docs("Temporal.Now").expect("Temporal.Now");
        assert!(now.starts_with("namespace Temporal {"));
        assert!(now.contains("namespace Now {"));
        assert!(now.contains("function instant(): Temporal.Instant;"));

        let one = builtin_docs("Temporal.Now.instant").expect("Temporal.Now.instant");
        assert!(one.contains("function instant(): Temporal.Instant;"));
        assert!(!one.contains("plainDateISO"), "{one}");

        let max = builtin_docs("Math.max").expect("Math.max");
        assert!(max.contains("function max("), "{max}");
        assert!(!max.contains("function min("), "{max}");

        let parse = builtin_docs("JSON.parse").expect("JSON.parse");
        assert!(parse.contains("function parse("), "{parse}");
        assert!(!parse.contains("function stringify("), "{parse}");
    }

    #[test]
    fn dotted_path_resolves_members_of_a_type_head() {
        // `isArray` lives on the constructor side; the stub names that owner
        // rather than pretending the member sits on `Array` itself.
        let is_array = builtin_docs("Array.isArray").expect("Array.isArray");
        assert!(
            is_array.starts_with("interface ArrayConstructor"),
            "{is_array}"
        );
        assert!(is_array.contains("isArray"));

        let repeat = builtin_docs("String.repeat").expect("String.repeat");
        assert!(repeat.starts_with("interface String"), "{repeat}");
        assert!(
            repeat.contains("repeat(count: number): string;"),
            "{repeat}"
        );

        // A type reached through a namespace is not itself a namespace, so it
        // must not gain a `namespace Instant {` wrapper.
        let from = builtin_docs("Temporal.Instant.from").expect("Temporal.Instant.from");
        assert!(from.starts_with("namespace Temporal {"), "{from}");
        assert!(from.contains("interface InstantConstructor {"), "{from}");
        assert!(!from.contains("namespace Instant"), "{from}");
    }

    #[test]
    fn every_member_a_miss_names_is_itself_resolvable() {
        // The miss message is a repair instruction: if it names a member, that
        // member must resolve. This closes the loop over every TypeKind, so a
        // kind the member renderer does not handle cannot pass unnoticed.
        let b = builtins();
        for name in b.types.iter().chain(b.namespaces.iter()) {
            let BuiltinLookup::UnknownMember { members, .. } =
                builtin_lookup(&format!("{name}.__definitely_not_a_member"))
            else {
                continue;
            };
            for member in members {
                let path = format!("{name}.{member}");
                assert!(
                    matches!(builtin_lookup(&path), BuiltinLookup::Found(_)),
                    "`{path}` is offered as a member of `{name}` but does not resolve"
                );
            }
        }
    }

    #[test]
    fn unknown_member_lists_the_members_that_exist() {
        let BuiltinLookup::UnknownMember {
            path,
            member,
            members,
        } = builtin_lookup("Temporal.Foo")
        else {
            panic!("expected an unknown-member miss");
        };
        assert_eq!(path, "Temporal");
        assert_eq!(member, "Foo");
        assert!(members.contains(&"Instant".to_string()));
        assert!(members.contains(&"Now".to_string()));
        // Constructor plumbing is bound to its base name already.
        assert!(
            !members.contains(&"InstantConstructor".to_string()),
            "{members:?}"
        );

        // A value is a leaf: it reports having no members rather than the
        // enclosing namespace's.
        let BuiltinLookup::UnknownMember { path, members, .. } = builtin_lookup("Math.max.foo")
        else {
            panic!("expected an unknown-member miss");
        };
        assert_eq!(path, "Math.max");
        assert!(members.is_empty());
    }

    #[test]
    fn a_bad_member_is_reported_against_its_full_path() {
        for (query, expected_path) in [
            ("Temporal.Instant.nope", "Temporal.Instant"),
            ("Temporal.Now.nope", "Temporal.Now"),
            ("Array.nope", "Array"),
            ("Temporal.Instant.from.x", "Temporal.Instant.from"),
            // A deep path rooted at a top-level type reports the member that
            // actually failed, not the one that resolved.
            ("Array.isArray.foo", "Array.isArray"),
        ] {
            let BuiltinLookup::UnknownMember { path, member, .. } = builtin_lookup(query) else {
                panic!("expected an unknown-member miss for {query}");
            };
            assert_eq!(path, expected_path, "for {query}");
            assert!(!member.is_empty(), "for {query}");
        }
    }

    #[test]
    fn unknown_head_is_not_an_unknown_member() {
        assert_eq!(builtin_lookup("Bogus.Thing"), BuiltinLookup::Unknown);
        // The internal flattening separator stays rejected.
        assert_eq!(builtin_lookup("Temporal#Instant"), BuiltinLookup::Unknown);
        assert_eq!(builtin_lookup(""), BuiltinLookup::Unknown);
    }

    #[test]
    fn plain_names_are_unchanged_and_segments_are_case_insensitive() {
        // Every catalog name still renders, and a namespace still renders whole.
        let full = builtin_docs("Temporal").expect("Temporal");
        assert!(full.starts_with("namespace Temporal {"));
        for member in ["Instant", "ZonedDateTime", "PlainDate", "Duration"] {
            assert!(
                full.contains(&format!("interface {member} {{")),
                "missing {member}"
            );
        }
        assert_eq!(
            builtin_docs("temporal.instant"),
            builtin_docs("Temporal.Instant")
        );
        // A trailing dot is absorbed rather than rejected.
        assert_eq!(builtin_docs("Temporal."), builtin_docs("Temporal"));
    }

    #[test]
    fn resolve_prefers_a_module_then_falls_back_to_builtins() {
        assert!(matches!(resolve("submilli:http"), Resolution::Module(_)));
        assert!(matches!(resolve("Temporal"), Resolution::Builtin { .. }));
        assert!(matches!(
            resolve("Temporal.Instant"),
            Resolution::Builtin { .. }
        ));
        assert!(matches!(
            resolve("Temporal.Foo"),
            Resolution::UnknownMember { .. }
        ));
        assert!(matches!(resolve("nope"), Resolution::Unknown));
        assert!(matches!(resolve("submilli:security"), Resolution::Unknown));
    }

    #[test]
    fn suggest_reaches_past_bare_edit_distance() {
        let extra = vec!["@mcp/linear".to_string()];
        // A name written without its scheme is the likeliest package typo and
        // sits 9 edits from the full name, far outside any threshold.
        assert_eq!(suggest("http", &extra).as_deref(), Some("submilli:http"));
        assert_eq!(suggest("htp", &extra).as_deref(), Some("submilli:http"));
        assert_eq!(suggest("linear", &extra).as_deref(), Some("@mcp/linear"));
        // A dotted miss is matched on its head alone.
        assert_eq!(suggest("Temporel", &extra).as_deref(), Some("Temporal"));
        assert_eq!(
            suggest("Temporel.Instant", &extra).as_deref(),
            Some("Temporal")
        );
        assert_eq!(suggest("Aray", &extra).as_deref(), Some("Array"));
        assert_eq!(suggest("zzzzzzzzz", &extra), None);
        // Internal plumbing never surfaces as a candidate, even when the query
        // is its exact name — the public `secrets` module is offered instead.
        assert_ne!(
            suggest(INTERNAL_MODULE, &[]).as_deref(),
            Some(INTERNAL_MODULE)
        );
    }

    #[test]
    fn a_deliberately_omitted_global_points_at_its_replacement() {
        // `Date` is two edits from `Math` and eight from `Temporal`, so edit
        // distance alone sends the model to arithmetic.
        assert_eq!(suggest("Date", &[]).as_deref(), Some("Temporal"));
        assert_eq!(suggest("date", &[]).as_deref(), Some("Temporal"));
    }

    #[test]
    fn suggest_declines_degenerate_and_self_queries() {
        // `closest_match` floors its threshold at 2 edits, so without a length
        // guard any short query lands on an unrelated name.
        for q in ["", ".", "a", "xy"] {
            assert_eq!(suggest(q, &[]), None, "for {q:?}");
        }
        // An empty leading segment is skipped rather than matched on.
        assert_eq!(suggest(".Temporel", &[]).as_deref(), Some("Temporal"));
        // A name that resolved nowhere is never its own repair.
        let declared = vec!["@acme/mypkg".to_string()];
        assert_eq!(suggest("@acme/mypkg", &declared), None);
    }

    #[test]
    fn catalog_is_summary_only_and_bounded() {
        let stdlib = catalog(Vec::new());
        assert_eq!(stdlib.remaining, 0);
        assert!(stdlib.entries.iter().all(|e| e.source == SOURCE_STDLIB));
        assert!(stdlib.entries.iter().any(|e| e.name == "submilli:http"));
        assert!(stdlib.entries.iter().all(|e| !e.description.is_empty()));
        assert!(!stdlib.entries.iter().any(|e| e.name == INTERNAL_MODULE));

        let extra: Vec<CatalogEntry> = (0..CATALOG_LIMIT)
            .map(|i| CatalogEntry {
                name: format!("@acme/p{i}"),
                source: "registry".to_string(),
                description: "x".to_string(),
            })
            .collect();
        let big = catalog(extra);
        assert_eq!(big.entries.len(), CATALOG_LIMIT);
        assert_eq!(big.remaining, stdlib.entries.len());
    }

    #[test]
    fn miss_messages_name_the_repair() {
        let msg = unknown_member_message("Temporal", "Foo", &["Instant".into(), "Now".into()]);
        assert!(msg.contains("Temporal"), "{msg}");
        assert!(msg.contains("Instant, Now"), "{msg}");
        let leaf = unknown_member_message("Math.max", "foo", &[]);
        assert!(leaf.contains("no members"), "{leaf}");
        // A `source` tag is a machine field; the prose must say it out loud.
        assert!(builtin_no_import_note("Temporal").contains("import"));
    }

    #[test]
    fn stdlib_capability_tags_parse_cleanly() {
        for module in user_modules() {
            for symbol in module.values.values() {
                let Some(doc) = value_doc(&symbol.kind) else {
                    continue;
                };
                for cap in &doc.capabilities {
                    assert!(
                        cap.diagnostics.is_empty(),
                        "{}.{}: {:?}",
                        module.package_name,
                        symbol.name,
                        cap.diagnostics
                    );
                }
            }
        }
    }

    #[test]
    fn every_importable_stdlib_symbol_has_docs() {
        for module in user_modules() {
            for symbol in module.values.values() {
                assert!(
                    value_doc(&symbol.kind).is_some(),
                    "{}.{} is missing docs",
                    module.package_name,
                    symbol.name
                );
            }
            for symbol in module.types.values() {
                assert!(
                    type_doc(&symbol.kind).is_some(),
                    "{}.{} is missing docs",
                    module.package_name,
                    symbol.name
                );
            }
        }
    }

    #[test]
    fn builtin_docs_folds_constructor_into_array() {
        let docs = builtin_docs("Array").expect("Array built-in");
        assert!(docs.contains("interface Array<"));
        assert!(docs.contains("interface ArrayConstructor"));
        assert!(docs.contains("const Array: ArrayConstructor;"));
    }

    #[test]
    fn builtin_docs_renders_temporal_namespace() {
        let docs = builtin_docs("Temporal").expect("Temporal built-in");
        assert!(docs.starts_with("namespace Temporal {"));
        assert!(docs.contains("interface Instant"));
        assert!(docs.contains("namespace Now {"));
        // Nested members are indented under the namespace.
        assert!(docs.contains("  interface Instant"));
        // The constructor binding comes from the namespace's own values — not
        // also synthesized by the constructor-merge (which would duplicate it).
        assert_eq!(docs.matches("const Instant").count(), 1, "{docs}");
    }

    #[test]
    fn builtin_docs_handles_json_intrinsic() {
        let docs = builtin_docs("JSON").expect("JSON built-in");
        assert!(docs.contains(
            "function stringify<T>(value: T, replacer?: null, space?: number | string | null): string;"
        ));
        assert!(docs.contains("function parse(text: string): unknown;"));
    }

    #[test]
    fn builtin_docs_is_case_insensitive() {
        // A lowercase name resolves to the canonically-cased built-in.
        assert_eq!(builtin_docs("array"), builtin_docs("Array"));
        assert_eq!(builtin_docs("temporal"), builtin_docs("Temporal"));
        assert_eq!(builtin_docs("json"), builtin_docs("JSON"));
        // Constructor-suffixed lookup folds in case-insensitively too.
        assert_eq!(builtin_docs("ARRAY"), builtin_docs("Array"));
    }

    #[test]
    fn builtin_docs_unknown_is_none() {
        assert!(builtin_docs("Promise").is_none());
        assert!(builtin_docs("Temporal#Instant").is_none());
    }

    #[test]
    fn packages_d_ts_imports_the_types_a_package_borrows() {
        let mut package = PackageDeclaration::with_package("@acme/files");
        let download = crate::stdlib::http::package_declaration()
            .values
            .get("download")
            .expect("submilli:http declares download")
            .clone();
        package.values.insert("fetch".to_string(), download);

        let docs = render_packages_d_ts(&[&package], &[]);

        assert!(docs.starts_with("declare module \"@acme/files\" {\n"));
        assert!(docs.contains("  import type { DownloadResult } from \"submilli:http\";\n"));
        assert!(docs.contains("  import type { DownloadOptions } from \"submilli:http\";\n"));
        assert!(!docs.contains("import type { Response }"));
    }

    #[test]
    fn a_borrowed_type_is_imported_from_its_own_module_when_others_share_its_name() {
        // `submilli:session` and `@acme/other` export a `Page` too; the
        // reference's mangled name picks the leaf's.
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Page".to_string(), alias_symbol("@acme/leaf", "Page"));
        let mut other = PackageDeclaration::with_package("@acme/other");
        other
            .types
            .insert("Page".to_string(), alias_symbol("@acme/other", "Page"));
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        let page = reference(
            "@acme/leaf",
            crate::mangle::package_symbol("@acme/leaf", "Page"),
            "Page",
        );
        mid.values
            .insert("page".to_string(), const_symbol("@acme/mid", "page", page));

        let docs = render_packages_d_ts(&[&leaf, &other, &mid], &[]);

        assert!(docs.contains("  import type { Page } from \"@acme/leaf\";\n"));
        assert!(!docs.contains("from \"submilli:session\""));
        assert!(!docs.contains("from \"@acme/other\""));
    }

    fn alias_symbol(package: &str, name: &str) -> crate::TypeSymbol {
        crate::TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(package, name),
            declaration_span: crate::Span::at(crate::FileId(0)),
            kind: crate::TypeKind::Alias {
                generics: Vec::new(),
                ty: Type::Number,
                doc: None,
            },
        }
    }

    fn const_symbol(package: &str, name: &str, ty: Type) -> ValueSymbol {
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(package, name),
            declaration_span: crate::Span::at(crate::FileId(0)),
            kind: ValueKind::Const { ty, doc: None },
        }
    }

    fn reference(package: &str, mangled: crate::MangledName, local_name: &str) -> Type {
        Type::alias_ref(
            crate::types::Package(package.to_string()),
            local_name,
            mangled,
            Vec::new(),
        )
    }

    #[test]
    fn an_aliased_type_is_imported_under_the_name_the_text_uses() {
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Page".to_string(), alias_symbol("@acme/leaf", "Page"));
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        let page = reference(
            "@acme/leaf",
            crate::mangle::package_symbol("@acme/leaf", "Page"),
            "LeafPage",
        );
        mid.values
            .insert("page".to_string(), const_symbol("@acme/mid", "page", page));

        let docs = render_packages_d_ts(&[&mid], &[&leaf]);

        assert!(docs.contains("  import type { Page as LeafPage } from \"@acme/leaf\";\n"));
        assert!(!docs.contains("declare module \"@acme/leaf\""));
    }

    #[test]
    fn a_type_reexported_from_an_internal_module_is_imported_from_the_package() {
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Status".to_string(), alias_symbol("@acme/leaf", "Status"));
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        let status = reference(
            "@acme/leaf",
            crate::mangle::package_module_symbol("@acme/leaf", "model", "Status"),
            "Status",
        );
        mid.values.insert(
            "status".to_string(),
            const_symbol("@acme/mid", "status", status),
        );

        let docs = render_packages_d_ts(&[&leaf, &mid], &[]);

        assert!(docs.contains("  import type { Status } from \"@acme/leaf\";\n"));
    }

    #[test]
    fn only_the_rendered_declarations_bring_imports() {
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        let response = reference(
            "submilli:http",
            crate::mangle::package_symbol("submilli:http", "Response"),
            "Response",
        );
        mid.runtime_globals.insert(
            crate::mangle::package_symbol("@acme/mid", "hidden"),
            response.clone(),
        );

        let docs = render_packages_d_ts(&[&mid], &[]);

        assert!(!docs.contains("import type"), "{docs}");
    }

    #[test]
    fn a_value_named_like_a_borrowed_interface_leaves_its_import() {
        // TypeScript keeps a type and a value of one name apart, so the
        // interface still needs its import next to the package's own value.
        let download = reference(
            "submilli:http",
            crate::mangle::package_symbol("submilli:http", "DownloadResult"),
            "DownloadResult",
        );
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        mid.values.insert(
            "DownloadResult".to_string(),
            const_symbol("@acme/mid", "DownloadResult", download),
        );

        let docs = render_packages_d_ts(&[&mid], &[]);

        assert!(
            docs.contains("  import type { DownloadResult } from \"submilli:http\";\n"),
            "{docs}"
        );
    }

    #[test]
    fn type_arguments_bring_imports_and_alias_bodies_do_not() {
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Page".to_string(), alias_symbol("@acme/leaf", "Page"));
        leaf.types
            .insert("Item".to_string(), alias_symbol("@acme/leaf", "Item"));
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        let item = reference(
            "@acme/leaf",
            crate::mangle::package_symbol("@acme/leaf", "Item"),
            "Item",
        );
        let page_of_items = Type::alias_ref(
            crate::types::Package("@acme/leaf".to_string()),
            "Page",
            crate::mangle::package_symbol("@acme/leaf", "Page"),
            vec![Type::Array(Box::new(item))],
        );
        mid.values.insert(
            "page".to_string(),
            const_symbol("@acme/mid", "page", page_of_items),
        );
        // A resolved alias renders as its name; what it stands for doesn't
        // appear in the text.
        let response = reference(
            "submilli:http",
            crate::mangle::package_symbol("submilli:http", "Response"),
            "Response",
        );
        let wrapped = Type::Alias {
            mangled: crate::mangle::package_symbol("@acme/leaf", "Item"),
            package: crate::types::Package("@acme/leaf".to_string()),
            name: "Item".to_string(),
            args: Vec::new(),
            ty: Box::new(response),
        };
        mid.values.insert(
            "item".to_string(),
            const_symbol("@acme/mid", "item", wrapped),
        );

        let docs = render_packages_d_ts(&[&mid], &[&leaf]);

        assert!(docs.contains("  import type { Page } from \"@acme/leaf\";\n"));
        assert!(docs.contains("  import type { Item } from \"@acme/leaf\";\n"));
        assert!(!docs.contains("Response"), "{docs}");
    }

    fn class_symbol(
        package: &str,
        name: &str,
        extends: Option<crate::ClassExtends>,
    ) -> crate::TypeSymbol {
        crate::TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(package, name),
            declaration_span: crate::Span::at(crate::FileId(0)),
            kind: TypeKind::Class {
                generics: Vec::new(),
                fields: BTreeMap::new(),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: Vec::new(),
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends,
                implements: Vec::new(),
                doc: None,
            },
        }
    }

    #[test]
    fn a_class_renders_and_imports_its_parent() {
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Base".to_string(), class_symbol("@acme/leaf", "Base", None));
        leaf.types
            .insert("Item".to_string(), alias_symbol("@acme/leaf", "Item"));
        let item = reference(
            "@acme/leaf",
            crate::mangle::package_symbol("@acme/leaf", "Item"),
            "Item",
        );
        let extends = crate::ClassExtends {
            parent: crate::mangle::package_symbol("@acme/leaf", "Base"),
            args: vec![item],
        };
        let mut sub = class_symbol("@acme/mid", "Sub", Some(extends));
        if let TypeKind::Class { fields, .. } = &mut sub.kind {
            // A private member isn't rendered, so its type isn't imported.
            let response = reference(
                "submilli:http",
                crate::mangle::package_symbol("submilli:http", "Response"),
                "Response",
            );
            fields.insert(
                "hidden".to_string(),
                crate::FieldSig {
                    ty: response,
                    visibility: crate::Visibility::Private,
                    readonly: false,
                    optional: false,
                    doc: None,
                },
            );
        }
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        mid.types.insert("Sub".to_string(), sub);

        let docs = render_packages_d_ts(&[&mid], &[&leaf]);

        assert!(
            docs.contains("  import type { Base } from \"@acme/leaf\";\n"),
            "{docs}"
        );
        assert!(
            docs.contains("  import type { Item } from \"@acme/leaf\";\n"),
            "{docs}"
        );
        assert!(
            docs.contains("  export class Sub extends Base<Item> {"),
            "{docs}"
        );
        assert!(!docs.contains("Response"), "{docs}");
    }

    fn subclass_of(package: &str, name: &str, parent: crate::MangledName) -> crate::TypeSymbol {
        class_symbol(package, name, Some(crate::ClassExtends::plain(parent)))
    }

    /// `@acme/leaf` and `@acme/other`, which each export a class `Base`.
    fn two_bases() -> (PackageDeclaration, PackageDeclaration) {
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Base".to_string(), class_symbol("@acme/leaf", "Base", None));
        let mut other = PackageDeclaration::with_package("@acme/other");
        other.types.insert(
            "Base".to_string(),
            class_symbol("@acme/other", "Base", None),
        );
        (leaf, other)
    }

    #[test]
    fn a_parent_named_like_a_local_value_is_imported_under_another_name() {
        let (leaf, _) = two_bases();
        let mut package = PackageDeclaration::with_package("@acme/shadowed");
        package.values.insert(
            "Base".to_string(),
            const_symbol("@acme/shadowed", "Base", Type::Number),
        );
        let base = crate::mangle::package_symbol("@acme/leaf", "Base");
        package.types.insert(
            "Sub".to_string(),
            subclass_of("@acme/shadowed", "Sub", base),
        );

        let docs = render_packages_d_ts(&[&package], &[&leaf]);

        assert!(
            docs.contains("  import type { Base as Base1 } from \"@acme/leaf\";\n"),
            "{docs}"
        );
        assert!(
            docs.contains("  export class Sub extends Base1 {"),
            "{docs}"
        );
    }

    #[test]
    fn parents_that_share_a_public_name_get_different_local_names() {
        let (leaf, other) = two_bases();
        let mut package = PackageDeclaration::with_package("@acme/both");
        package.types.insert(
            "A".to_string(),
            subclass_of(
                "@acme/both",
                "A",
                crate::mangle::package_symbol("@acme/leaf", "Base"),
            ),
        );
        package.types.insert(
            "B".to_string(),
            subclass_of(
                "@acme/both",
                "B",
                crate::mangle::package_symbol("@acme/other", "Base"),
            ),
        );

        let docs = render_packages_d_ts(&[&package], &[&leaf, &other]);

        assert!(
            docs.contains("  import type { Base } from \"@acme/leaf\";\n"),
            "{docs}"
        );
        assert!(
            docs.contains("  import type { Base as Base1 } from \"@acme/other\";\n"),
            "{docs}"
        );
        assert!(docs.contains("  export class A extends Base {"), "{docs}");
        assert!(docs.contains("  export class B extends Base1 {"), "{docs}");
    }

    #[test]
    fn a_parent_already_imported_under_an_alias_keeps_it() {
        let (leaf, _) = two_bases();
        let base = crate::mangle::package_symbol("@acme/leaf", "Base");
        let mut package = PackageDeclaration::with_package("@acme/aliased");
        package.values.insert(
            "base".to_string(),
            const_symbol(
                "@acme/aliased",
                "base",
                reference("@acme/leaf", base.clone(), "LB"),
            ),
        );
        package
            .types
            .insert("Sub".to_string(), subclass_of("@acme/aliased", "Sub", base));

        let docs = render_packages_d_ts(&[&package], &[&leaf]);

        assert!(
            docs.contains("  import type { Base as LB } from \"@acme/leaf\";\n"),
            "{docs}"
        );
        assert!(docs.contains("  export class Sub extends LB {"), "{docs}");
        assert!(!docs.contains("import type { Base }"), "{docs}");
    }

    #[test]
    fn a_class_extending_a_builtin_error_renders_its_parent_as_the_global() {
        let error = builtin_package_declaration()
            .types
            .get("Error")
            .expect("the prelude declares Error")
            .mangled_name
            .clone();
        let mut package = PackageDeclaration::with_package("@acme/errors");
        package.types.insert(
            "BillingError".to_string(),
            subclass_of("@acme/errors", "BillingError", error),
        );

        let docs = render_packages_d_ts(&[&package], &[]);

        assert!(
            docs.contains("  export class BillingError extends Error {"),
            "{docs}"
        );
        assert!(!docs.contains("import type"), "{docs}");
    }

    #[test]
    fn a_package_value_named_like_the_global_parent_leaves_the_class_without_it() {
        let error = builtin_package_declaration()
            .types
            .get("Error")
            .expect("the prelude declares Error")
            .mangled_name
            .clone();
        let mut package = PackageDeclaration::with_package("@acme/errors");
        package.values.insert(
            "Error".to_string(),
            const_symbol("@acme/errors", "Error", Type::Number),
        );
        package.types.insert(
            "BillingError".to_string(),
            subclass_of("@acme/errors", "BillingError", error),
        );

        let docs = render_packages_d_ts(&[&package], &[]);

        assert!(docs.contains("  export class BillingError {"), "{docs}");
    }

    #[test]
    fn builtin_errors_extend_error_in_the_editor_declarations() {
        let lib = render_lib_submilli_d_ts();
        for class in [
            "RangeError",
            "TypeError",
            "SyntaxError",
            "PermissionDeniedError",
        ] {
            assert!(
                lib.contains(&format!("declare class {class} extends Error {{")),
                "{class}"
            );
        }
        assert!(lib.contains("declare class Error {"));
    }

    #[test]
    fn a_value_named_like_a_borrowed_class_keeps_the_class_out() {
        // A class is a value too, so importing it would clash with the const.
        let mut leaf = PackageDeclaration::with_package("@acme/leaf");
        leaf.types
            .insert("Base".to_string(), class_symbol("@acme/leaf", "Base", None));
        let base = Type::class_ref(
            crate::types::Package("@acme/leaf".to_string()),
            "Base",
            crate::mangle::package_symbol("@acme/leaf", "Base"),
            Vec::new(),
        );
        let mut mid = PackageDeclaration::with_package("@acme/mid");
        mid.values
            .insert("Base".to_string(), const_symbol("@acme/mid", "Base", base));

        let docs = render_packages_d_ts(&[&mid], &[&leaf]);

        assert!(!docs.contains("import type"), "{docs}");
    }

    #[test]
    fn a_package_type_named_like_a_constructor_is_an_ordinary_type() {
        let mut package = PackageDeclaration::with_package("@acme/leaf");
        package
            .types
            .insert("Client".to_string(), alias_symbol("@acme/leaf", "Client"));
        package.types.insert(
            "ClientConstructor".to_string(),
            alias_symbol("@acme/leaf", "ClientConstructor"),
        );

        let docs = render_packages_d_ts(&[&package], &[]);

        assert!(docs.contains("  export type Client = number;"), "{docs}");
        assert!(
            docs.contains("  export type ClientConstructor = number;"),
            "{docs}"
        );
        assert!(!docs.contains("const Client"), "{docs}");
    }

    #[test]
    fn stdlib_d_ts_declares_each_user_module() {
        let docs = render_stdlib_d_ts();
        for module in user_modules() {
            assert!(
                docs.contains(&format!("declare module \"{}\" {{", module.package_name)),
                "missing declare module for {}",
                module.package_name
            );
        }
        assert!(docs.contains("declare module \"submilli:security\" {\n"));
        assert!(docs.contains("  export function check<T>("));
        assert!(docs.contains("declare module \"submilli:uuid\" {\n"));
        assert!(docs.contains("  export function v4("));
        assert!(docs.contains("  function delete_("));
        assert!(docs.contains("  export { delete_ as delete };"));
        assert!(!docs.contains("export function delete("));
    }

    #[test]
    fn lib_submilli_d_ts_renders_ambient_builtins() {
        let docs = render_lib_submilli_d_ts();
        assert!(docs.contains("interface Array<"));
        assert!(docs.contains("interface ArrayConstructor"));
        assert!(docs.contains("declare const Array: ArrayConstructor;"));
        assert_eq!(
            docs.matches("declare const Array: ArrayConstructor;")
                .count(),
            1
        );
        assert!(docs.contains("declare const console: Console;"));
        assert!(docs.contains("declare namespace Math {"));
        assert!(docs.contains("declare namespace Temporal {"));
        assert!(docs.contains("declare namespace JSON {"));
        assert!(docs.contains(
            "function stringify<T>(value: T, replacer?: null, space?: number | string | null): string;"
        ));
        assert!(docs.contains("interface Function {}"));
        assert!(docs.contains("interface IArguments {}"));
        assert!(docs.contains("declare class Error {"));
        assert!(docs.contains("static isError(value: unknown): value is Error;"));
        assert!(!docs.contains("ErrorConstructor"));
        assert!(!docs.contains("declare module"));
        assert!(!docs.contains("function string_concat"));
    }

    #[test]
    fn d_ts_renderer_uses_typescript_call_and_predicate_signatures() {
        let docs = render_lib_submilli_d_ts();
        assert!(docs.contains("(value: string | bigint): number;"));
        assert!(docs.contains("isArray<T>(value: T): value is T & unknown[];"));
        assert!(!docs.contains("@call"));
    }

    #[test]
    fn class_declarations_omit_private_members() {
        use crate::{FieldSig, MethodSig, Visibility};

        let field = |ty, visibility| FieldSig {
            ty,
            visibility,
            readonly: false,
            optional: false,
            doc: None,
        };
        let method = |ret| MethodSig {
            generics: Vec::new(),
            params: Vec::new(),
            ret,
            predicate: None,
            doc: None,
        };

        let mut fields = BTreeMap::new();
        fields.insert("name".to_string(), field(Type::String, Visibility::Public));
        fields.insert(
            "sound".to_string(),
            field(Type::String, Visibility::Private),
        );
        let mut methods = BTreeMap::new();
        methods.insert("speak".to_string(), method(Type::String));
        methods.insert("secret".to_string(), method(Type::Number));
        let mut method_visibility = BTreeMap::new();
        method_visibility.insert("speak".to_string(), Visibility::Public);
        method_visibility.insert("secret".to_string(), Visibility::Private);

        let mut defs = PackageDeclaration::with_package("@acme/zoo");
        defs.types.insert(
            "Animal".to_string(),
            TypeSymbol {
                name: "Animal".to_string(),
                mangled_name: crate::mangle::package_symbol("@acme/zoo", "Animal"),
                declaration_span: Span::new(FileId(0), 0, 0).unwrap(),
                kind: TypeKind::Class {
                    generics: Vec::new(),
                    fields,
                    narrowing_checks: BTreeMap::new(),
                    methods,
                    method_visibility,
                    accessors: Vec::new(),
                    constructor: vec![Param::new("name", Type::String)],
                    statics: BTreeMap::new(),
                    static_visibility: BTreeMap::new(),
                    static_fields: BTreeMap::new(),
                    extends: None,
                    implements: Vec::new(),
                    doc: None,
                },
            },
        );

        let rendered = render_declarations(&defs);
        // Public surface and the constructor stay.
        assert!(rendered.contains("class Animal {"), "{rendered}");
        assert!(rendered.contains("name: string;"), "{rendered}");
        assert!(rendered.contains("speak(): string;"), "{rendered}");
        assert!(
            rendered.contains("constructor(name: string);"),
            "{rendered}"
        );
        // Private members and the `private` keyword never reach the docs.
        assert!(!rendered.contains("private "), "{rendered}");
        assert!(!rendered.contains("sound"), "{rendered}");
        assert!(!rendered.contains("secret"), "{rendered}");
    }

    #[test]
    fn class_declarations_render_accessors_faithfully() {
        use crate::{AccessorSig, FieldSig, Param, Visibility};

        // Accessors are not methods on the public surface — they render as
        // properties (or explicit get/set when read/write types differ).
        let prop = |ty: Type, readonly| FieldSig {
            ty,
            visibility: Visibility::Public,
            readonly,
            optional: false,
            doc: None,
        };
        let mut fields = BTreeMap::new();
        fields.insert("area".to_string(), prop(Type::Number, true)); // get-only
        fields.insert("size".to_string(), prop(Type::Number, false)); // get+set, same type
        fields.insert("label".to_string(), prop(Type::String, false)); // get/set, diff types
        fields.insert("secret".to_string(), prop(Type::Number, false)); // set-only

        let setter = |name: &str, ty: Type| AccessorSig::Setter {
            name: name.to_string(),
            param: Param::new("v", ty),
        };
        let getter = |name: &str, ty: Type| AccessorSig::Getter {
            name: name.to_string(),
            ret_ty: ty,
        };
        let accessors = vec![
            getter("area", Type::Number),
            getter("size", Type::Number),
            setter("size", Type::Number),
            getter("label", Type::String),
            setter("label", Type::Number),
            setter("secret", Type::Number),
        ];

        let mut defs = PackageDeclaration::with_package("@acme/geo");
        defs.types.insert(
            "Shape".to_string(),
            TypeSymbol {
                name: "Shape".to_string(),
                mangled_name: crate::mangle::package_symbol("@acme/geo", "Shape"),
                declaration_span: Span::new(FileId(0), 0, 0).unwrap(),
                kind: TypeKind::Class {
                    generics: Vec::new(),
                    fields,
                    narrowing_checks: BTreeMap::new(),
                    methods: BTreeMap::new(),
                    method_visibility: BTreeMap::new(),
                    accessors,
                    constructor: Vec::new(),
                    statics: BTreeMap::new(),
                    static_visibility: BTreeMap::new(),
                    static_fields: BTreeMap::new(),
                    extends: None,
                    implements: Vec::new(),
                    doc: None,
                },
            },
        );

        let docs = render_declarations(&defs);
        let shape = &defs.types.get("Shape").unwrap().kind;
        let mut dts = String::new();
        render_ts_type(&mut dts, "Shape", shape, "", "export ", None);
        for rendered in [&docs, &dts] {
            assert!(rendered.contains("readonly area: number;"), "{rendered}"); // get-only
            assert!(rendered.contains("size: number;"), "{rendered}"); // get+set same type
            assert!(rendered.contains("get label(): string;"), "{rendered}"); // diff types
            assert!(rendered.contains("set label(v: number);"), "{rendered}");
            assert!(rendered.contains("set secret(v: number);"), "{rendered}"); // write-only
            // Never the synthetic `get x`/`set x` *method* form.
            assert!(!rendered.contains("get area"), "{rendered}");
            assert!(!rendered.contains("get size("), "{rendered}");
        }
    }

    #[test]
    fn class_declarations_render_public_statics_only() {
        use crate::{FieldSig, MethodSig, Param, Visibility};

        let method = |ret: Type| MethodSig {
            generics: Vec::new(),
            params: vec![Param::new("x", Type::Number)],
            ret,
            predicate: None,
            doc: None,
        };
        let mut statics = BTreeMap::new();
        statics.insert("make".to_string(), method(Type::Number));
        statics.insert("hidden".to_string(), method(Type::Number));
        let mut static_visibility = BTreeMap::new();
        static_visibility.insert("make".to_string(), Visibility::Public);
        static_visibility.insert("hidden".to_string(), Visibility::Private);
        let mut static_fields = BTreeMap::new();
        static_fields.insert(
            "MAX".to_string(),
            FieldSig {
                ty: Type::Number,
                visibility: Visibility::Public,
                readonly: true,
                optional: false,
                doc: None,
            },
        );
        static_fields.insert(
            "KEY".to_string(),
            FieldSig {
                ty: Type::Number,
                visibility: Visibility::Private,
                readonly: true,
                optional: false,
                doc: None,
            },
        );
        static_fields.insert(
            "count".to_string(),
            FieldSig {
                ty: Type::Number,
                visibility: Visibility::Public,
                readonly: false,
                optional: false,
                doc: None,
            },
        );
        static_fields.insert(
            "seed".to_string(),
            FieldSig {
                ty: Type::Number,
                visibility: Visibility::Private,
                readonly: false,
                optional: false,
                doc: None,
            },
        );

        let mut defs = PackageDeclaration::with_package("@acme/calc");
        defs.types.insert(
            "Calc".to_string(),
            TypeSymbol {
                name: "Calc".to_string(),
                mangled_name: crate::mangle::package_symbol("@acme/calc", "Calc"),
                declaration_span: Span::new(FileId(0), 0, 0).unwrap(),
                kind: TypeKind::Class {
                    generics: Vec::new(),
                    fields: BTreeMap::new(),
                    narrowing_checks: BTreeMap::new(),
                    methods: BTreeMap::new(),
                    method_visibility: BTreeMap::new(),
                    accessors: Vec::new(),
                    constructor: Vec::new(),
                    statics,
                    static_visibility,
                    static_fields,
                    extends: None,
                    implements: Vec::new(),
                    doc: None,
                },
            },
        );

        let docs = render_declarations(&defs);
        let calc = &defs.types.get("Calc").unwrap().kind;
        let mut dts = String::new();
        render_ts_type(&mut dts, "Calc", calc, "", "export ", None);
        for rendered in [&docs, &dts] {
            assert!(
                rendered.contains("static make(x: number): number;"),
                "{rendered}"
            );
            assert!(
                rendered.contains("static readonly MAX: number;"),
                "{rendered}"
            );
            assert!(rendered.contains("static count: number;"), "{rendered}");
            assert!(!rendered.contains("hidden"), "{rendered}");
            assert!(!rendered.contains("KEY"), "{rendered}");
            assert!(!rendered.contains("seed"), "{rendered}");
        }
    }
}
