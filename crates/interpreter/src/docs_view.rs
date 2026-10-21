//! Documentation as one reader can use it: the callables whose capabilities
//! the reader may hold, the types those callables still reach, and the
//! Markdown regions whose conditions hold.
//!
//! The engine does not know the policy. An embedder answers through
//! [`DocAvailability`], and [`Unscoped`] answers for an author reading
//! everything. A projection is a copy for documentation: the declaration a
//! program compiles against is never changed.

use std::collections::{BTreeMap, BTreeSet};

use crate::doc_directives::{DirectiveError, parse_doc_directives};
use crate::stdlib::capabilities;
use crate::{
    DocCapability, DocCapabilityBindingKind, DocCapabilityLiteral, DocComment, MethodSig,
    NamespaceSymbol, PackageDeclaration, TypeKind, TypeSymbol, ValueKind, ValueSymbol,
};

/// Whether a reader could use a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// No call can proceed.
    Denied,
    /// Some call could proceed. `notes` say what limits it, such as a filter
    /// or a person's approval, as sentences documentation shows the reader.
    Available { notes: Vec<String> },
}

impl Availability {
    /// Available with nothing to note.
    pub const OPEN: Self = Availability::Available { notes: Vec::new() };

    pub fn is_available(&self) -> bool {
        matches!(self, Availability::Available { .. })
    }
}

/// The reader's side of documentation: which capabilities it could use.
pub trait DocAvailability {
    /// A capability a callable's `@capability` tag names. [`known_context`]
    /// gives the context the tag's literal bindings fix ahead of the call.
    fn tag(&self, tag: &DocCapability) -> Availability;

    /// A capability a Markdown region names, with nothing known of the call.
    fn capability(&self, name: &str) -> Availability;
}

/// Everything available: documentation as its author reads it.
pub struct Unscoped;

impl DocAvailability for Unscoped {
    fn tag(&self, _tag: &DocCapability) -> Availability {
        Availability::OPEN
    }

    fn capability(&self, _name: &str) -> Availability {
        Availability::OPEN
    }
}

/// The context fields a tag's literal bindings fix, in the form the runtime
/// checks them. A literal the runtime would refuse, a relative path, which
/// the runtime resolves against the session's working directory, or a number
/// that does not parse, is left out, which leaves the field unknown.
pub fn known_context(tag: &DocCapability) -> serde_json::Value {
    let catalogued = capabilities::find_any(&tag.capability);
    let mut fields = serde_json::Map::new();
    for binding in &tag.bindings {
        let DocCapabilityBindingKind::Literal { value, .. } = &binding.kind else {
            continue;
        };
        let value = match value {
            DocCapabilityLiteral::String(text) => {
                let normalization = catalogued
                    .and_then(|capability| {
                        capability
                            .filter_fields
                            .iter()
                            .find(|field| field.name == binding.field)
                    })
                    .map_or(capabilities::FieldNormalization::Verbatim, |field| {
                        field.normalization
                    });
                if normalization == capabilities::FieldNormalization::VfsPath
                    && !text.starts_with('/')
                {
                    continue;
                }
                match normalization.apply(text) {
                    Ok(text) => serde_json::Value::String(text),
                    Err(_) => continue,
                }
            }
            DocCapabilityLiteral::Number(number) => {
                let Some(number) = number
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                else {
                    continue;
                };
                serde_json::Value::Number(number)
            }
            DocCapabilityLiteral::Boolean(value) => serde_json::Value::Bool(*value),
            DocCapabilityLiteral::Null => serde_json::Value::Null,
        };
        fields.insert(binding.field.clone(), value);
    }
    serde_json::Value::Object(fields)
}

/// A declaration narrowed to what a reader can use.
#[derive(Debug, Clone)]
pub struct Projection {
    /// The callables the reader could call, each tagged one's documentation
    /// noting what limits it, and the types they reach.
    pub declaration: PackageDeclaration,
    /// Whether the package is worth showing: some callable that needs a
    /// capability is left, or none ever needed one, as in a package of pure
    /// helpers or only types. The ungated helpers of a package whose every
    /// gated callable was removed, such as its working directory, do not count.
    pub usable: bool,
}

/// Narrow `defs` to what `availability` allows. A callable stays when every
/// capability its tags name is available, since a call checks each of them.
/// A type stays when a remaining callable reaches it, or when no callable
/// ever did, as with a type a package exports on its own.
pub fn project_declaration(
    defs: &PackageDeclaration,
    availability: &dyn DocAvailability,
) -> Projection {
    let mut declaration = defs.clone();
    let mut gated = Gated::default();
    retain_values(&mut declaration.values, availability, &mut gated);
    for symbol in declaration.types.values_mut() {
        retain_members(symbol, availability, &mut gated);
    }
    let mut pending: Vec<&mut NamespaceSymbol> = declaration.namespaces.values_mut().collect();
    while let Some(namespace) = pending.pop() {
        retain_values(&mut namespace.values, availability, &mut gated);
        for symbol in namespace.types.values_mut() {
            retain_members(symbol, availability, &mut gated);
        }
        pending.extend(namespace.namespaces.values_mut());
    }
    if gated.removed > 0 {
        drop_unreached_types(defs, &mut declaration);
    }
    Projection {
        declaration,
        usable: gated.kept > 0 || gated.removed == 0,
    }
}

/// `markdown` with the regions whose conditions fail removed, or the
/// directives that make it malformed.
pub fn scoped_documentation(
    markdown: &str,
    availability: &dyn DocAvailability,
) -> Result<String, Vec<DirectiveError>> {
    let document = parse_doc_directives(markdown)?;
    Ok(document
        .render(&|condition| condition.holds(|name| availability.capability(name).is_available())))
}

/// How many callables that need a capability a projection kept and removed.
#[derive(Default)]
struct Gated {
    kept: usize,
    removed: usize,
}

fn retain_values(
    values: &mut BTreeMap<String, ValueSymbol>,
    availability: &dyn DocAvailability,
    gated: &mut Gated,
) {
    values.retain(|_, symbol| match &mut symbol.kind {
        ValueKind::Function { doc, .. } => keep_callable(doc, availability, gated),
        ValueKind::Let { .. } | ValueKind::Const { .. } => true,
    });
}

fn retain_members(symbol: &mut TypeSymbol, availability: &dyn DocAvailability, gated: &mut Gated) {
    match &mut symbol.kind {
        TypeKind::Interface { methods, .. } => retain_methods(methods, availability, gated),
        TypeKind::Class {
            methods,
            method_visibility,
            statics,
            static_visibility,
            ..
        } => {
            retain_methods(methods, availability, gated);
            method_visibility.retain(|name, _| methods.contains_key(name));
            retain_methods(statics, availability, gated);
            static_visibility.retain(|name, _| statics.contains_key(name));
        }
        TypeKind::NumberEnum { .. } | TypeKind::StringEnum { .. } | TypeKind::Alias { .. } => {}
    }
}

fn retain_methods(
    methods: &mut BTreeMap<String, MethodSig>,
    availability: &dyn DocAvailability,
    gated: &mut Gated,
) {
    methods.retain(|_, method| keep_callable(&mut method.doc, availability, gated));
}

/// Whether a callable with `doc` stays, adding to its summary what limits it.
fn keep_callable(
    doc: &mut Option<DocComment>,
    availability: &dyn DocAvailability,
    gated: &mut Gated,
) -> bool {
    let Some(doc) = doc.as_mut().filter(|doc| !doc.capabilities.is_empty()) else {
        return true;
    };
    let mut notes: Vec<String> = Vec::new();
    for tag in &doc.capabilities {
        match availability.tag(tag) {
            Availability::Denied => {
                gated.removed = gated.removed.saturating_add(1);
                return false;
            }
            Availability::Available { notes: tag_notes } => {
                for note in tag_notes {
                    if !notes.contains(&note) {
                        notes.push(note);
                    }
                }
            }
        }
    }
    for note in notes {
        if !doc.summary.is_empty() {
            doc.summary.push('\n');
        }
        doc.summary.push_str(&note);
    }
    gated.kept = gated.kept.saturating_add(1);
    true
}

/// Remove the types only removed callables reached. Reach starts from the
/// values, from the types neither a value nor another type mentions, which a
/// package exports on their own, and, after projection, from a class or interface that kept a
/// callable needing a capability, which a reader can still use whatever
/// returned it. A type `original` never reached stays.
fn drop_unreached_types(original: &PackageDeclaration, projected: &mut PackageDeclaration) {
    let mentioned: BTreeSet<String> = original
        .types
        .values()
        .flat_map(crate::packages::type_symbol_references)
        .collect();
    let reached_by_values = reached_types(original, Vec::new());
    let exported_alone: Vec<String> = original
        .types
        .values()
        .map(|symbol| symbol.mangled_name.as_str().to_string())
        .filter(|mangled| !mentioned.contains(mangled) && !reached_by_values.contains(mangled))
        .collect();
    let reached_before = reached_types(original, exported_alone.clone());
    let mut roots = exported_alone;
    roots.extend(
        projected
            .types
            .values()
            .filter(|symbol| keeps_gated_member(symbol))
            .map(|symbol| symbol.mangled_name.as_str().to_string()),
    );
    let reached_after = reached_types(projected, roots);
    let stays = |symbol: &TypeSymbol| {
        let mangled = symbol.mangled_name.as_str();
        !reached_before.contains(mangled) || reached_after.contains(mangled)
    };
    projected.types.retain(|_, symbol| stays(symbol));
    let mut pending: Vec<&mut NamespaceSymbol> = projected.namespaces.values_mut().collect();
    while let Some(namespace) = pending.pop() {
        namespace.types.retain(|_, symbol| stays(symbol));
        pending.extend(namespace.namespaces.values_mut());
    }
}

fn keeps_gated_member(symbol: &TypeSymbol) -> bool {
    let gated = |methods: &BTreeMap<String, MethodSig>| {
        methods.values().any(|method| {
            method
                .doc
                .as_ref()
                .is_some_and(|doc| !doc.capabilities.is_empty())
        })
    };
    match &symbol.kind {
        TypeKind::Interface { methods, .. } => gated(methods),
        TypeKind::Class {
            methods, statics, ..
        } => gated(methods) || gated(statics),
        TypeKind::NumberEnum { .. } | TypeKind::StringEnum { .. } | TypeKind::Alias { .. } => false,
    }
}

/// The mangled names of the types reached from `declaration`'s values, in
/// every namespace, and from `roots`, through the types they mention.
/// Namespace types are mirrored into `declaration.types`, so they are found
/// there.
fn reached_types(declaration: &PackageDeclaration, roots: Vec<String>) -> BTreeSet<String> {
    let by_mangled: BTreeMap<&str, &TypeSymbol> = declaration
        .types
        .values()
        .map(|symbol| (symbol.mangled_name.as_str(), symbol))
        .collect();
    let mut values = vec![serde_json::to_value(&declaration.values)];
    let mut namespaces: Vec<&NamespaceSymbol> = declaration.namespaces.values().collect();
    while let Some(namespace) = namespaces.pop() {
        values.push(serde_json::to_value(&namespace.values));
        namespaces.extend(namespace.namespaces.values());
    }
    let mut pending = roots;
    pending.extend(crate::packages::type_references(&values));
    let mut reached = BTreeSet::new();
    while let Some(mangled) = pending.pop() {
        if !reached.insert(mangled.clone()) {
            continue;
        }
        let Some(symbol) = by_mangled.get(mangled.as_str()) else {
            continue;
        };
        pending.extend(crate::packages::type_symbol_references(symbol));
    }
    reached
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Capabilities in `denied` are unavailable, ones in `noted` carry a note.
    struct Fixed {
        denied: &'static [&'static str],
        noted: &'static [&'static str],
    }

    impl Fixed {
        fn answer(&self, name: &str) -> Availability {
            if self.denied.contains(&name) {
                Availability::Denied
            } else if self.noted.contains(&name) {
                Availability::Available {
                    notes: vec![format!("`{name}` is restricted.")],
                }
            } else {
                Availability::OPEN
            }
        }
    }

    impl DocAvailability for Fixed {
        fn tag(&self, tag: &DocCapability) -> Availability {
            self.answer(&tag.capability)
        }

        fn capability(&self, name: &str) -> Availability {
            self.answer(name)
        }
    }

    fn declaration(source: &str) -> PackageDeclaration {
        crate::compile_package(
            "@acme/pkg",
            crate::ModulePath::from("lib"),
            &[crate::PackageSourceModule {
                path: crate::ModulePath::from("lib"),
                source,
            }],
            &[],
        )
        .unwrap_or_else(|diagnostics| panic!("compile failed: {diagnostics:#?}"))
        .declaration
    }

    const SOURCE: &str = r#"
        export interface Issue { id: string; title: string; }
        export interface Draft { title: string; }
        export interface Team { name: string; }
        export interface Standalone { note: string; }

        /** List issues.
         * @capability acme.test/read */
        export function listIssues(team: Team): Issue[] { return []; }

        /** Create an issue.
         * @capability acme.test/write */
        export function createIssue(draft: Draft): Issue { return { id: "1", title: draft.title }; }

        /** Download an issue.
         * @capability acme.test/read
         * @capability acme.test/write */
        export function exportIssue(id: string): void {}

        export function helper(): string { return "x"; }

        export class Client {
            /** @capability acme.test/read */
            get(id: string): Issue { return { id, title: "" }; }
            /** @capability acme.test/write */
            update(draft: Draft): void {}
        }
    "#;

    #[test]
    fn denied_callables_and_the_types_only_they_reach_are_removed() {
        let defs = declaration(SOURCE);
        let projection = project_declaration(
            &defs,
            &Fixed {
                denied: &["acme.test/write"],
                noted: &[],
            },
        );
        let declaration = &projection.declaration;
        assert!(projection.usable);
        assert!(declaration.values.contains_key("listIssues"));
        assert!(declaration.values.contains_key("helper"));
        assert!(!declaration.values.contains_key("createIssue"));
        // Every tag counts: one denied capability removes the callable.
        assert!(!declaration.values.contains_key("exportIssue"));
        assert!(declaration.types.contains_key("Issue"));
        assert!(declaration.types.contains_key("Team"));
        assert!(declaration.types.contains_key("Standalone"));
        assert!(!declaration.types.contains_key("Draft"));
        let Some(TypeKind::Class {
            methods,
            method_visibility,
            ..
        }) = declaration.types.get("Client").map(|symbol| &symbol.kind)
        else {
            panic!("Client stays");
        };
        assert!(methods.contains_key("get"));
        assert!(!methods.contains_key("update"));
        assert!(!method_visibility.contains_key("update"));
        // The compiled declaration is untouched.
        assert!(defs.values.contains_key("createIssue"));
    }

    #[test]
    fn a_class_keeps_its_place_by_what_it_still_offers() {
        let defs = declaration(
            r#"
            export interface Secret { value: string; }
            export interface Page { text: string; }
            export class Session {
                /** @capability acme.test/read */
                read(): Page { return { text: "" }; }
                /** @capability acme.test/write */
                rotate(): Secret { return { value: "" }; }
            }
            /** @capability acme.test/connect */
            export function connect(): Session { return new Session(); }
            "#,
        );
        let projection = project_declaration(
            &defs,
            &Fixed {
                denied: &["acme.test/connect", "acme.test/write"],
                noted: &[],
            },
        );
        let types = &projection.declaration.types;
        // Only the removed `connect` reached `Session`, but `read` still works.
        assert!(types.contains_key("Session"), "{types:?}");
        assert!(types.contains_key("Page"));
        // Only the removed `rotate` reached `Secret`.
        assert!(!types.contains_key("Secret"));
    }

    #[test]
    fn unscoped_projection_keeps_everything() {
        let defs = declaration(SOURCE);
        let projection = project_declaration(&defs, &Unscoped);
        assert_eq!(projection.declaration, defs);
        assert!(projection.usable);
    }

    #[test]
    fn notes_follow_the_summary() {
        let defs = declaration(SOURCE);
        let projection = project_declaration(
            &defs,
            &Fixed {
                denied: &[],
                noted: &["acme.test/read"],
            },
        );
        let rendered = crate::packages::render_declarations(&projection.declaration);
        assert!(
            rendered.contains(" * List issues.\n * `acme.test/read` is restricted.\n"),
            "{rendered}"
        );
        // One note per distinct sentence, however many tags repeat it.
        assert_eq!(rendered.matches("is restricted.").count(), 3, "{rendered}");
    }

    #[test]
    fn a_package_with_nothing_left_to_call_is_not_usable() {
        let defs = declaration(SOURCE);
        let projection = project_declaration(
            &defs,
            &Fixed {
                denied: &["acme.test/read", "acme.test/write"],
                noted: &[],
            },
        );
        // `helper` is left, but it needs no capability.
        assert!(!projection.usable);
        let gated_only = declaration(
            r#"
                export interface Issue { id: string; }
                /** @capability acme.test/read */
                export function listIssues(): Issue[] { return []; }
            "#,
        );
        let projection = project_declaration(
            &gated_only,
            &Fixed {
                denied: &["acme.test/read"],
                noted: &[],
            },
        );
        assert!(!projection.usable);
        assert!(projection.declaration.types.is_empty());
    }

    #[test]
    fn recursive_and_generic_types_terminate_and_stay_whole() {
        let defs = declaration(
            r#"
                export interface Node { children: Node[]; parent: Box<Node> | undefined; }
                export interface Box<T> { value: T; }
                /** @capability acme.test/read */
                export function tree(): Node { return { children: [], parent: undefined }; }
                /** @capability acme.test/write */
                export function boxed(): Box<string> { return { value: "" }; }
            "#,
        );
        let projection = project_declaration(
            &defs,
            &Fixed {
                denied: &["acme.test/write"],
                noted: &[],
            },
        );
        assert!(projection.declaration.types.contains_key("Node"));
        assert!(projection.declaration.types.contains_key("Box"));
    }

    #[test]
    fn known_context_normalizes_literals() {
        let defs = declaration(
            r#"
                /** @capability fs.read { path: "/a/../b.txt", recursive: true } */
                export function read(): void {}
                /** @capability fs.read { path: "notes.txt" } */
                export function readRelative(): void {}
            "#,
        );
        let Some(ValueKind::Function { doc: Some(doc), .. }) =
            defs.values.get("read").map(|symbol| &symbol.kind)
        else {
            panic!("read is documented");
        };
        let context = known_context(&doc.capabilities[0]);
        assert_eq!(
            context,
            serde_json::json!({ "path": "/b.txt", "recursive": true })
        );
        let Some(ValueKind::Function { doc: Some(doc), .. }) =
            defs.values.get("readRelative").map(|symbol| &symbol.kind)
        else {
            panic!("readRelative is documented");
        };
        // The runtime resolves a relative path against the working directory.
        assert_eq!(known_context(&doc.capabilities[0]), serde_json::json!({}));
    }

    #[test]
    fn regions_follow_availability() {
        let markdown = "Intro.\n\n<!-- submilli:if capability=\"acme.test/write\" -->\n\nWrite.\n\n<!-- submilli:endif -->\n\nEnd.\n";
        let denied = Fixed {
            denied: &["acme.test/write"],
            noted: &[],
        };
        assert_eq!(
            scoped_documentation(markdown, &denied).unwrap(),
            "Intro.\n\nEnd.\n"
        );
        assert_eq!(
            scoped_documentation(markdown, &Unscoped).unwrap(),
            "Intro.\n\nWrite.\n\nEnd.\n"
        );
        assert!(scoped_documentation("<!-- submilli:endif -->\n", &Unscoped).is_err());
    }
}
