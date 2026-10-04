//! `.d.ts`-style type lifting for compile diagnostics.

use std::fmt::Write;

use super::type_namespace::TypeNamespace;
use super::type_registry::TypeRegistry;
#[cfg(test)]
use crate::Param;
use crate::type_size::TypeLimits;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{MangledName, MethodSig, Type, TypeKind, TypeSymbol};

/// Resolve a type symbol for diagnostic lifting the same way structural access does:
/// FQN registry first (so library types resolve without an import), import-scoped
/// namespace as fallback (the current module's own types).
fn resolve<'a>(
    types: &'a TypeNamespace,
    registry: &'a TypeRegistry,
    mangled: &MangledName,
    name: &str,
) -> Option<&'a TypeSymbol> {
    registry.lookup(mangled).or_else(|| types.lookup(name))
}

/// Substitutions that pass a type limit render as `Type::Error` and are
/// recorded in `limits`, so the compile still fails at the inferer's next
/// checkpoint.
pub(super) fn format_definition(
    ty: &Type,
    types: &TypeNamespace,
    registry: &TypeRegistry,
    limits: &TypeLimits,
) -> String {
    let ty = ty.peel();
    match ty {
        Type::Number | Type::NumberLiteral(_) => format_named_interface(
            types,
            registry,
            limits,
            &crate::mangle::prelude("Number"),
            "Number",
            &[],
        ),
        Type::BigInt => format_named_interface(
            types,
            registry,
            limits,
            &crate::mangle::prelude("BigInt"),
            "BigInt",
            &[],
        ),
        Type::Boolean | Type::BooleanLiteral(_) => format_named_interface(
            types,
            registry,
            limits,
            &crate::mangle::prelude("Boolean"),
            "Boolean",
            &[],
        ),
        Type::String | Type::StringLiteral(_) => format_named_interface(
            types,
            registry,
            limits,
            &crate::mangle::prelude("String"),
            "String",
            &[],
        ),
        Type::Array(elem) => format_named_interface(
            types,
            registry,
            limits,
            &crate::mangle::prelude("Array"),
            "Array",
            &[(**elem).clone()],
        ),
        Type::Uint8Array => format_named_interface(
            types,
            registry,
            limits,
            &crate::mangle::prelude("Uint8Array"),
            "Uint8Array",
            &[],
        ),
        Type::InterfaceRef {
            mangled,
            name,
            args,
            ..
        } => format_named_interface(types, registry, limits, mangled, name, args),
        Type::ClassRef {
            mangled,
            name,
            args,
            ..
        } => format_named_class(types, registry, limits, mangled, name, args),
        Type::Object { fields, index } => {
            if index.is_some() {
                return ty.to_string();
            }
            if fields.is_empty() {
                return "{}".to_string();
            }
            let mut out = String::from("{\n");
            for (field, of) in fields {
                let marker = if of.optional { "?" } else { "" };
                writeln!(out, "  {}{}: {};", field, marker, of.ty).unwrap();
            }
            out.push('}');
            out
        }
        // Must render exactly as `Display` does: `definition_help` suppresses the
        // whole block when the lift equals the type's own rendering, and a
        // function type has nothing to add beyond that rendering. Diverging here
        // defeats the guard and prints a second, stale copy as advice.
        Type::Function {
            params,
            ret,
            has_rest,
            ..
        } => {
            let mut out = String::new();
            crate::types::write_synthetic_params(&mut out, params, *has_rest).unwrap();
            write!(out, " => {ret}").unwrap();
            out
        }
        Type::Null
        | Type::Void
        | Type::Never
        | Type::Error
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::Unknown
        | Type::Tuple(_)
        | Type::AliasRef { .. }
        | Type::Union(_) => format!("{ty}"),
        Type::NumberEnum { mangled, name, .. } => {
            format_number_enum_definition(types, registry, mangled, name)
        }
        Type::StringEnum { mangled, name, .. } => {
            format_string_enum_definition(types, registry, mangled, name)
        }
        Type::Alias { .. } | Type::Refined { .. } | Type::Readonly(_) => {
            unreachable!("peel guarantees no alias here")
        }
    }
}

fn format_number_enum_definition(
    types: &TypeNamespace,
    registry: &TypeRegistry,
    mangled: &MangledName,
    name: &str,
) -> String {
    let sym = resolve(types, registry, mangled, name);
    let variants = sym.and_then(|s| match &s.kind {
        TypeKind::NumberEnum { variants, .. } => Some(variants),
        _ => None,
    });
    let mut out = format!("enum {name} {{");
    let Some(variants) = variants else {
        out.push('}');
        return out;
    };
    if variants.is_empty() {
        out.push('}');
        return out;
    }
    out.push('\n');
    for (vname, value) in variants {
        writeln!(out, "  {vname} = {value};").unwrap();
    }
    out.push('}');
    out
}

fn format_string_enum_definition(
    types: &TypeNamespace,
    registry: &TypeRegistry,
    mangled: &MangledName,
    name: &str,
) -> String {
    let sym = resolve(types, registry, mangled, name);
    let variants = sym.and_then(|s| match &s.kind {
        TypeKind::StringEnum { variants, .. } => Some(variants),
        _ => None,
    });
    let mut out = format!("enum {name} {{");
    let Some(variants) = variants else {
        out.push('}');
        return out;
    };
    if variants.is_empty() {
        out.push('}');
        return out;
    }
    out.push('\n');
    for (vname, value) in variants {
        writeln!(out, "  {vname} = \"{value}\";").unwrap();
    }
    out.push('}');
    out
}

/// One class on an `extends` chain, with its own type parameters bound to the
/// arguments they take at the instantiation being dumped.
struct ClassLink {
    sym: TypeSymbol,
    sub: TypeParamSubstitution,
}

/// The class and every ancestor, nearest first. A chain that stops resolving
/// partway keeps the prefix it did resolve — a partial shape still beats none.
fn class_chain(
    types: &TypeNamespace,
    registry: &TypeRegistry,
    limits: &TypeLimits,
    mangled: &MangledName,
    args: &[Type],
) -> Vec<ClassLink> {
    let mut chain = Vec::new();
    let _ = super::classes::for_each_class_in_chain(
        |m| {
            registry
                .lookup(m)
                .or_else(|| types.lookup_by_mangled(m))
                .cloned()
        },
        limits,
        mangled,
        args,
        |sym, bindings| {
            let mut sub = TypeParamSubstitution::new();
            for (param, ty) in bindings {
                sub.insert(param.clone(), ty.clone());
            }
            chain.push(ClassLink {
                sym: sym.clone(),
                sub,
            });
        },
    );
    chain
}

/// The dumped class's `extends` clause, carrying the parent's type arguments as this
/// instantiation binds them — a bare `extends Base` would hide which `Base` the
/// inherited members below were substituted against.
fn extends_clause(chain: &[ClassLink]) -> String {
    let Some(parent) = chain.get(1) else {
        return String::new();
    };
    let TypeKind::Class { generics, .. } = &parent.sym.kind else {
        return String::new();
    };
    if generics.is_empty() {
        return format!(" extends {}", parent.sym.name);
    }
    let args: Vec<String> = generics
        .iter()
        .map(|g| match parent.sub.get(g) {
            Some(ty) => ty.to_string(),
            None => g.clone(),
        })
        .collect();
    format!(" extends {}<{}>", parent.sym.name, args.join(", "))
}

/// Trailing note naming the class a member is inherited from, empty for the dumped
/// class's own members (`chain[0]`). A reader matching the dump against their source
/// needs to know which members they will not find in the class they are looking at.
fn inherited_from_note(chain: &[ClassLink], index: usize) -> String {
    if index == 0 {
        String::new()
    } else {
        format!("  // from {}", chain[index].sym.name)
    }
}

/// Members already emitted for this group, nearest declaration winning — a child's
/// member shadows the parent's, and the chain is walked nearest-first.
type Shadowed = std::collections::BTreeSet<String>;

fn write_static_fields(out: &mut String, chain: &[ClassLink], seen: &mut Shadowed) {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class { static_fields, .. } = &link.sym.kind else {
            continue;
        };
        for (fname, field) in static_fields {
            if !seen.insert(fname.clone()) {
                continue;
            }
            let vis = visibility_prefix(field.visibility);
            let ro = if field.readonly { "readonly " } else { "" };
            writeln!(
                out,
                "  {vis}static {ro}{fname}: {};{}",
                field.ty,
                inherited_from_note(chain, i)
            )
            .unwrap();
        }
    }
}

fn write_static_methods(
    out: &mut String,
    chain: &[ClassLink],
    seen: &mut Shadowed,
    limits: &TypeLimits,
) {
    // Statics never see the class's type parameters, so no substitution applies.
    let static_sub = TypeParamSubstitution::new();
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class {
            statics,
            static_visibility,
            ..
        } = &link.sym.kind
        else {
            continue;
        };
        for (mname, sig) in statics {
            if !seen.insert(mname.clone()) {
                continue;
            }
            write!(out, "  {}static ", visibility_of(static_visibility, mname)).unwrap();
            format_method_sig(out, mname, sig, &static_sub, limits);
            writeln!(out, "{}", inherited_from_note(chain, i)).unwrap();
        }
    }
}

fn write_instance_fields(
    out: &mut String,
    chain: &[ClassLink],
    seen: &mut Shadowed,
    limits: &TypeLimits,
) {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class { fields, .. } = &link.sym.kind else {
            continue;
        };
        for (fname, field) in fields {
            if !seen.insert(fname.clone()) {
                continue;
            }
            let vis = visibility_prefix(field.visibility);
            let ro = if field.readonly { "readonly " } else { "" };
            let marker = if field.optional { "?" } else { "" };
            let ty = link.sub.apply_or_record(&field.ty, limits);
            let note = inherited_from_note(chain, i);
            out.push_str(&format!("  {vis}{ro}{fname}{marker}: {ty};{note}\n"));
        }
    }
}

/// The one constructor an instantiation actually runs. `resolve_implicit_constructors`
/// normally copies an inherited signature onto the subclass itself, so this stops at
/// `chain[0]`; the walk is the fallback for a symbol that never went through it.
fn write_constructor(out: &mut String, chain: &[ClassLink], limits: &TypeLimits) {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class {
            constructor,
            constructor_visibility,
            ..
        } = &link.sym.kind
        else {
            continue;
        };
        if constructor.is_empty() {
            continue;
        }
        out.push_str("  ");
        out.push_str(visibility_prefix(*constructor_visibility));
        out.push_str("constructor(");
        for (n, p) in constructor.iter().enumerate() {
            if n > 0 {
                out.push_str(", ");
            }
            super::format_signature::write_named_param(
                out,
                &p.name,
                &link.sub.apply_or_record(&p.ty, limits),
                p.default.as_ref(),
                p.rest,
            );
        }
        writeln!(out, ");{}", inherited_from_note(chain, i)).unwrap();
        return;
    }
}

fn write_instance_methods(
    out: &mut String,
    chain: &[ClassLink],
    seen: &mut Shadowed,
    limits: &TypeLimits,
) {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class {
            methods,
            method_visibility,
            ..
        } = &link.sym.kind
        else {
            continue;
        };
        for (mname, sig) in methods {
            if !seen.insert(mname.clone()) {
                continue;
            }
            write!(out, "  {}", visibility_of(method_visibility, mname)).unwrap();
            format_method_sig(out, mname, sig, &link.sub, limits);
            writeln!(out, "{}", inherited_from_note(chain, i)).unwrap();
        }
    }
}

fn visibility_of(
    visibility: &std::collections::BTreeMap<String, crate::Visibility>,
    member: &str,
) -> &'static str {
    visibility_prefix(
        visibility
            .get(member)
            .copied()
            .unwrap_or(crate::Visibility::Public),
    )
}

fn visibility_prefix(visibility: crate::Visibility) -> &'static str {
    if visibility == crate::Visibility::Private {
        "private "
    } else {
        ""
    }
}

/// Lift a class's `.d.ts`-style shape for member-miss diagnostics. Renders fields (with
/// `private`/`readonly` markers), the constructor signature, and methods, walking the
/// `extends` chain: a subclass that declares nothing of its own still has the members it
/// inherits, and dumping an empty body tells the reader the opposite of the truth.
/// Falls back to a header-only block when lookup misses.
fn format_named_class(
    types: &TypeNamespace,
    registry: &TypeRegistry,
    limits: &TypeLimits,
    mangled: &MangledName,
    name: &str,
    args: &[Type],
) -> String {
    let mut out = String::new();
    write!(out, "class {name}").unwrap();
    if !args.is_empty() {
        out.push('<');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write!(out, "{a}").unwrap();
        }
        out.push('>');
    }

    let chain = class_chain(types, registry, limits, mangled, args);
    if chain.is_empty() {
        out.push_str(" {}");
        return out;
    }
    out.push_str(&extends_clause(&chain));

    // Statics and instance members live in disjoint namespaces, so each half tracks
    // its own shadowing; within a half, fields and methods cannot share a name.
    let mut body = String::new();
    let mut statics = Shadowed::new();
    write_static_fields(&mut body, &chain, &mut statics);
    write_static_methods(&mut body, &chain, &mut statics, limits);
    let mut instance = Shadowed::new();
    write_instance_fields(&mut body, &chain, &mut instance, limits);
    write_constructor(&mut body, &chain, limits);
    write_instance_methods(&mut body, &chain, &mut instance, limits);

    if body.is_empty() {
        out.push_str(" {}");
        return out;
    }
    out.push_str(" {\n");
    out.push_str(&body);
    out.push('}');
    out
}

pub(crate) fn format_interface_header(name: &str, generics: &[String]) -> String {
    if generics.is_empty() {
        format!("interface {name}")
    } else {
        format!("interface {}<{}>", name, generics.join(", "))
    }
}

pub(crate) fn format_class_header(name: &str, generics: &[String]) -> String {
    if generics.is_empty() {
        format!("class {name}")
    } else {
        format!("class {}<{}>", name, generics.join(", "))
    }
}

/// Falls back to a header-only block when lookup misses — receiver type is already in the diagnostic.
fn format_named_interface(
    types: &TypeNamespace,
    registry: &TypeRegistry,
    limits: &TypeLimits,
    mangled: &MangledName,
    name: &str,
    args: &[Type],
) -> String {
    let mut out = String::new();
    let sym = resolve(types, registry, mangled, name);
    let interface = sym.and_then(|s| match &s.kind {
        TypeKind::Interface {
            generics,
            methods,
            properties,
            index,
            doc,
            ..
        } => Some((generics, methods, properties, index, doc)),
        _ => None,
    });
    if let Some((_, _, _, _, Some(doc))) = interface {
        write_doc_block(&mut out, "", doc);
    }
    write!(out, "interface {name}").unwrap();
    if !args.is_empty() {
        out.push('<');
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write!(out, "{a}").unwrap();
        }
        out.push('>');
    }

    let Some((generics, methods, properties, index, _doc)) = interface else {
        out.push_str(" {}");
        return out;
    };

    if methods.is_empty() && properties.is_empty() && index.is_none() {
        out.push_str(" {}");
        return out;
    }

    // Method-own generics (e.g. `map<U>`) stay as TypeVar — from_pairs only seeds interface-level params.
    let sub = TypeParamSubstitution::from_pairs(generics, args);

    out.push_str(" {\n");
    if let Some(index) = index {
        let ro = if index.readonly { "readonly " } else { "" };
        out.push_str(&format!(
            "  {ro}[key: string]: {};\n",
            sub.apply_or_record(&index.value, limits)
        ));
    }
    for (pname, prop) in properties {
        if let Some(doc) = &prop.doc {
            write_doc_block(&mut out, "  ", doc);
        }
        let marker = if prop.optional { "?" } else { "" };
        let ty = sub.apply_or_record(&prop.ty, limits);
        out.push_str(&format!("  {pname}{marker}: {ty};\n"));
    }
    for (mname, sig) in methods {
        if let Some(doc) = &sig.doc {
            write_doc_block(&mut out, "  ", doc);
        }
        out.push_str("  ");
        format_method_sig(&mut out, mname, sig, &sub, limits);
        out.push('\n');
    }
    out.push('}');
    out
}

pub(super) fn write_doc_block(out: &mut String, indent: &str, doc: &crate::DocComment) {
    let has_tags = doc.returns.is_some()
        || !doc.params.is_empty()
        || !doc.throws.is_empty()
        || doc.deprecated.is_some()
        || !doc.examples.is_empty()
        || !doc.unknown_tags.is_empty();
    let has_summary = !doc.summary.is_empty();
    if !has_tags && has_summary {
        writeln!(out, "{}/** {} */", indent, doc.summary).unwrap();
        return;
    }
    if !has_tags && !has_summary {
        return;
    }
    writeln!(out, "{indent}/**").unwrap();
    if has_summary {
        writeln!(out, "{} * {}", indent, doc.summary).unwrap();
    }
    for p in &doc.params {
        if p.description.is_empty() {
            writeln!(out, "{} * @param {}", indent, p.name).unwrap();
        } else {
            writeln!(out, "{} * @param {} {}", indent, p.name, p.description).unwrap();
        }
    }
    if let Some(r) = &doc.returns {
        if r.description.is_empty() {
            writeln!(out, "{indent} * @returns").unwrap();
        } else {
            writeln!(out, "{} * @returns {}", indent, r.description).unwrap();
        }
    }
    for t in &doc.throws {
        writeln!(out, "{} * @throws {}", indent, t.text).unwrap();
    }
    if let Some(d) = &doc.deprecated {
        writeln!(out, "{} * @deprecated {}", indent, d.text).unwrap();
    }
    for ex in &doc.examples {
        writeln!(out, "{} * @example {}", indent, ex.text).unwrap();
    }
    for u in &doc.unknown_tags {
        writeln!(out, "{} * @{} {}", indent, u.name, u.text).unwrap();
    }
    writeln!(out, "{indent} */").unwrap();
}

fn format_method_sig(
    out: &mut String,
    name: &str,
    sig: &MethodSig,
    sub: &TypeParamSubstitution,
    limits: &TypeLimits,
) {
    out.push_str(name);
    if !sig.generics.is_empty() {
        out.push('<');
        for (i, g) in sig.generics.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(g);
        }
        out.push('>');
    }
    out.push('(');
    for (i, p) in sig.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let ty = sub.apply_or_record(&p.ty, limits);
        if p.name.is_empty() {
            write!(out, "{ty}").unwrap();
        } else {
            write!(out, "{}: {}", p.name, ty).unwrap();
        }
        if let Some(d) = &p.default {
            out.push_str(" = ");
            super::format_signature::write_default_value(out, d);
        }
    }
    out.push(')');
    let ret = sub.apply_or_record(&sig.ret, limits);
    out.push_str(&format!(": {ret};"));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::package_declaration::TypeSymbol;
    use crate::{Span, TypeKind};

    /// Renders without a type limit being reached; shadows the 4-argument form.
    fn format_definition(ty: &Type, types: &TypeNamespace, registry: &TypeRegistry) -> String {
        let limits = TypeLimits::default();
        let out = super::format_definition(ty, types, registry, &limits);
        assert_eq!(limits.take(), Ok(()));
        out
    }

    fn empty_ns() -> TypeNamespace<'static> {
        TypeNamespace::new()
    }

    fn ns_with(
        iface: &'static str,
        generics: Vec<String>,
        methods: Vec<(&'static str, MethodSig)>,
    ) -> TypeNamespace<'static> {
        ns_with_props(iface, generics, methods, vec![])
    }

    fn ns_with_props(
        iface: &'static str,
        generics: Vec<String>,
        methods: Vec<(&'static str, MethodSig)>,
        properties: Vec<(&'static str, crate::package_declaration::PropertySig)>,
    ) -> TypeNamespace<'static> {
        let mut ns = TypeNamespace::new();
        let mut m = BTreeMap::new();
        for (name, sig) in methods {
            m.insert(name.to_string(), sig);
        }
        let mut p = BTreeMap::new();
        for (name, sig) in properties {
            p.insert(name.to_string(), sig);
        }
        ns.insert(
            iface.to_string(),
            "test".to_string(),
            TypeSymbol {
                name: iface.to_string(),
                mangled_name: crate::mangle::package_symbol("test", iface),
                declaration_span: Span::at(crate::FileId(0)),
                kind: TypeKind::Interface {
                    index: None,
                    generics,
                    methods: m,
                    properties: p,
                    dispatch: crate::Dispatch::VTable,
                    doc: None,
                },
            },
        );
        ns
    }

    fn prop(ty: Type, optional: bool) -> crate::package_declaration::PropertySig {
        crate::package_declaration::PropertySig {
            ty,
            readonly: true,
            intrinsic: false,
            optional,
            doc: None,
        }
    }

    fn no_arg_string_ret() -> MethodSig {
        MethodSig {
            generics: vec![],
            params: vec![],
            ret: Type::String,
            predicate: None,
            doc: None,
        }
    }

    #[test]
    fn primitive_with_declared_methods() {
        let ns = ns_with("Number", vec![], vec![("toString", no_arg_string_ret())]);
        let out = format_definition(&Type::Number, &ns, &TypeRegistry::new());
        insta::assert_snapshot!(out);
    }

    #[test]
    fn array_substitutes_interface_generic() {
        let map_sig = MethodSig {
            generics: vec!["U".to_string()],
            params: vec![Param::new(
                "fn",
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    ret: Box::new(Type::TypeVar("U".to_string())),
                    predicate: None,
                    has_rest: false,
                },
            )],
            ret: Type::Array(Box::new(Type::TypeVar("U".to_string()))),
            predicate: None,
            doc: None,
        };
        let ns = ns_with(
            "Array",
            vec!["T".to_string()],
            vec![("toString", no_arg_string_ret()), ("map", map_sig)],
        );
        let out = format_definition(
            &Type::Array(Box::new(Type::Number)),
            &ns,
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn object_renders_structural_shape() {
        use crate::ObjectField;
        let mut fields = BTreeMap::new();
        fields.insert("x".to_string(), ObjectField::required(Type::Number));
        fields.insert("name".to_string(), ObjectField::required(Type::String));
        let out = format_definition(
            &Type::Object {
                index: None,
                fields,
            },
            &empty_ns(),
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn function_renders_arrow_form() {
        let out = format_definition(
            &Type::Function {
                params: vec![Type::Number, Type::String],
                ret: Box::new(Type::Boolean),
                predicate: None,
                has_rest: false,
            },
            &empty_ns(),
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn interface_ref_substitutes_multiple_args() {
        let get_sig = MethodSig {
            generics: vec![],
            params: vec![Param::new("key", Type::TypeVar("K".to_string()))],
            ret: Type::TypeVar("V".to_string()),
            predicate: None,
            doc: None,
        };
        let ns = ns_with(
            "Map",
            vec!["K".to_string(), "V".to_string()],
            vec![("get", get_sig)],
        );
        let out = format_definition(
            &Type::InterfaceRef {
                mangled: crate::mangle::prelude("Map"),
                package: crate::Package::prelude(),
                name: "Map".to_string(),
                args: vec![Type::String, Type::Number],
            },
            &ns,
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn interface_renders_properties() {
        let ns = ns_with_props(
            "Item",
            vec![],
            vec![],
            vec![
                ("key", prop(Type::Number, false)),
                ("tag", prop(Type::String, false)),
            ],
        );
        let out = format_definition(
            &Type::InterfaceRef {
                mangled: crate::mangle::prelude("Item"),
                package: crate::Package::prelude(),
                name: "Item".to_string(),
                args: vec![],
            },
            &ns,
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn interface_renders_optional_property() {
        let ns = ns_with_props(
            "Box",
            vec![],
            vec![],
            vec![("label", prop(Type::String, true))],
        );
        let out = format_definition(
            &Type::InterfaceRef {
                mangled: crate::mangle::prelude("Box"),
                package: crate::Package::prelude(),
                name: "Box".to_string(),
                args: vec![],
            },
            &ns,
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn interface_substitutes_property_generic() {
        let ns = ns_with_props(
            "Cell",
            vec!["T".to_string()],
            vec![],
            vec![("value", prop(Type::TypeVar("T".to_string()), false))],
        );
        let out = format_definition(
            &Type::InterfaceRef {
                mangled: crate::mangle::prelude("Cell"),
                package: crate::Package::prelude(),
                name: "Cell".to_string(),
                args: vec![Type::Number],
            },
            &ns,
            &TypeRegistry::new(),
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn unknown_interface_renders_empty_body() {
        let out = format_definition(
            &Type::InterfaceRef {
                mangled: crate::mangle::prelude("Unknown"),
                package: crate::Package::prelude(),
                name: "Unknown".to_string(),
                args: vec![],
            },
            &empty_ns(),
            &TypeRegistry::new(),
        );
        assert_eq!(out, "interface Unknown {}");
    }

    #[test]
    fn library_interface_lifts_via_registry_when_not_imported() {
        // SUB-386: the receiver type came from a library and was never imported,
        // so the import-scoped namespace misses — the FQN registry must still
        // yield the full shape, not the misleading empty `interface Foo {}`.
        let mut m = BTreeMap::new();
        m.insert("status".to_string(), prop(Type::Number, false));
        let sym = TypeSymbol {
            name: "Resp".to_string(),
            mangled_name: crate::mangle::package_symbol("submilli:lib", "Resp"),
            declaration_span: Span::at(crate::FileId(0)),
            kind: TypeKind::Interface {
                index: None,
                generics: vec![],
                methods: BTreeMap::new(),
                properties: m,
                dispatch: crate::Dispatch::Direct,
                doc: None,
            },
        };
        let mut registry = TypeRegistry::new();
        registry.insert_owned(sym);

        let out = format_definition(
            &Type::interface_ref(
                crate::Package("submilli:lib".to_string()),
                "Resp",
                crate::mangle::package_symbol("submilli:lib", "Resp"),
                vec![],
            ),
            &empty_ns(),
            &registry,
        );
        assert!(
            out.contains("status: number;"),
            "expected the registry-resolved property body, got: {out}"
        );
    }

    #[test]
    fn class_dump_marks_only_readonly_statics() {
        use crate::{FieldSig, Visibility};

        let field = |readonly: bool, visibility: Visibility| FieldSig {
            ty: Type::Number,
            visibility,
            readonly,
            optional: false,
            doc: None,
        };
        let mut static_fields = BTreeMap::new();
        static_fields.insert("MAX".to_string(), field(true, Visibility::Public));
        static_fields.insert("count".to_string(), field(false, Visibility::Public));
        static_fields.insert("seed".to_string(), field(false, Visibility::Private));

        let mangled = crate::mangle::package_symbol("test", "C");
        let mut registry = TypeRegistry::new();
        registry.insert_owned(TypeSymbol {
            name: "C".to_string(),
            mangled_name: mangled.clone(),
            declaration_span: Span::at(crate::FileId(0)),
            kind: TypeKind::Class {
                generics: Vec::new(),
                fields: BTreeMap::new(),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: Vec::new(),
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields,
                extends: None,
                implements: Vec::new(),
                doc: None,
            },
        });

        let out = format_definition(
            &Type::class_ref(crate::Package("test".to_string()), "C", mangled, vec![]),
            &empty_ns(),
            &registry,
        );
        assert!(out.contains("static readonly MAX: number;"), "{out}");
        assert!(out.contains("static count: number;"), "{out}");
        assert!(out.contains("private static seed: number;"), "{out}");
    }
}
