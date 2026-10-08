//! Checked `.d.ts`-style diagnostic help. All sections share one output budget.
use super::format_signature::{substituted, write_generic_list, write_named_param};
use super::{type_namespace::TypeNamespace, type_registry::TypeRegistry};
#[cfg(test)]
use crate::Param;
use crate::rendering::{RenderError, RenderLimits, Writer};
use crate::type_rendering::CopyBudget;
use crate::type_rendering::write_type;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{MangledName, MethodSig, Type, TypeKind, TypeSymbol};

#[cfg(test)]
thread_local! { static FAIL_RENDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

pub(super) fn format_definition(
    ty: &Type,
    types: &TypeNamespace,
    registry: &TypeRegistry,
) -> Result<String, RenderError> {
    #[cfg(test)]
    if FAIL_RENDER.with(|fail| fail.replace(false)) {
        return Err(RenderError::Allocation);
    }
    Writer::render(RenderLimits::default(), |out| {
        let limits = CopyBudget::default();
        let mut peeled = ty;
        loop {
            out.step()?;
            peeled = match peeled {
                Type::Alias { ty, .. } | Type::Refined { ty, .. } | Type::Readonly(ty) => ty,
                _ => break,
            };
        }
        match peeled {
            Type::Number | Type::NumberLiteral(_) => write_interface(
                out,
                types,
                registry,
                &limits,
                &crate::mangle::prelude("Number"),
                "Number",
                &[],
            ),
            Type::BigInt | Type::BigIntLiteral(_) => write_interface(
                out,
                types,
                registry,
                &limits,
                &crate::mangle::prelude("BigInt"),
                "BigInt",
                &[],
            ),
            Type::Boolean | Type::BooleanLiteral(_) => write_interface(
                out,
                types,
                registry,
                &limits,
                &crate::mangle::prelude("Boolean"),
                "Boolean",
                &[],
            ),
            Type::String | Type::StringLiteral(_) => write_interface(
                out,
                types,
                registry,
                &limits,
                &crate::mangle::prelude("String"),
                "String",
                &[],
            ),
            Type::Array(inner) => write_interface(
                out,
                types,
                registry,
                &limits,
                &crate::mangle::prelude("Array"),
                "Array",
                std::slice::from_ref(inner.as_ref()),
            ),
            Type::Uint8Array => write_interface(
                out,
                types,
                registry,
                &limits,
                &crate::mangle::prelude("Uint8Array"),
                "Uint8Array",
                &[],
            ),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => write_interface(out, types, registry, &limits, mangled, name, args),
            Type::ClassRef {
                mangled,
                name,
                args,
                ..
            } => write_class(out, types, registry, &limits, mangled, name, args),
            Type::Object {
                fields,
                index: None,
            } if !fields.is_empty() => {
                out.push("{\n")?;
                for (name, field) in fields {
                    out.format(format_args!(
                        "  {name}{}: ",
                        if field.optional { "?" } else { "" }
                    ))?;
                    write_type(out, &field.ty)?;
                    out.push(";\n")?;
                }
                out.push("}")
            }
            Type::NumberEnum { mangled, name, .. } | Type::StringEnum { mangled, name, .. } => {
                write_enum(out, types, registry, mangled, name)
            }
            ty => write_type(out, ty),
        }
    })
    .map(|rendered| rendered.text)
}

fn resolve<'a>(
    types: &'a TypeNamespace,
    registry: &'a TypeRegistry,
    mangled: &MangledName,
    name: &str,
) -> Option<&'a TypeSymbol> {
    registry.lookup(mangled).or_else(|| types.lookup(name))
}

fn write_name(out: &mut Writer, kind: &str, name: &str, args: &[Type]) -> Result<(), RenderError> {
    out.format(format_args!("{kind} {name}"))?;
    if !args.is_empty() {
        out.push("<")?;
        for (i, arg) in args.iter().enumerate() {
            if i != 0 {
                out.push(", ")?;
            }
            write_type(out, arg)?;
        }
        out.push(">")?;
    }
    Ok(())
}

fn write_enum(
    out: &mut Writer,
    types: &TypeNamespace,
    registry: &TypeRegistry,
    mangled: &MangledName,
    name: &str,
) -> Result<(), RenderError> {
    let symbol = resolve(types, registry, mangled, name)
        .ok_or(RenderError::InvalidMetadata("enum definition is absent"))?;
    out.format(format_args!("enum {name} {{"))?;
    match &symbol.kind {
        TypeKind::NumberEnum { variants, .. } => {
            if !variants.is_empty() {
                out.push("\n")?;
            }
            for (name, value) in variants {
                out.format(format_args!("  {name} = {value};\n"))?;
            }
        }
        TypeKind::StringEnum { variants, .. } => {
            if !variants.is_empty() {
                out.push("\n")?;
            }
            for (name, value) in variants {
                out.format(format_args!("  {name} = \"{value}\";\n"))?;
            }
        }
        _ => {
            return Err(RenderError::InvalidMetadata(
                "enum definition has wrong kind",
            ));
        }
    }
    out.push("}")
}

fn bindings(
    generics: &[String],
    args: &[Type],
    limits: &CopyBudget,
) -> Result<TypeParamSubstitution, RenderError> {
    if generics.len() != args.len() {
        return Err(RenderError::InvalidMetadata(
            "definition generic arity mismatch",
        ));
    }
    for (name, arg) in generics.iter().zip(args) {
        limits.charge(name.len())?;
        limits.check(arg)?;
    }
    Ok(TypeParamSubstitution::from_pairs(generics, args))
}

#[allow(clippy::too_many_arguments)]
fn write_interface(
    out: &mut Writer,
    types: &TypeNamespace,
    registry: &TypeRegistry,
    limits: &CopyBudget,
    mangled: &MangledName,
    name: &str,
    args: &[Type],
) -> Result<(), RenderError> {
    let symbol = resolve(types, registry, mangled, name).ok_or(RenderError::InvalidMetadata(
        "interface definition is absent",
    ))?;
    let TypeKind::Interface {
        generics,
        methods,
        properties,
        index,
        doc,
        ..
    } = &symbol.kind
    else {
        return Err(RenderError::InvalidMetadata(
            "interface definition has wrong kind",
        ));
    };
    let sub = bindings(generics, args, limits)?;
    if let Some(doc) = doc {
        write_doc_block(out, "", doc)?;
    }
    write_name(out, "interface", name, args)?;
    if methods.is_empty() && properties.is_empty() && index.is_none() {
        return out.push(" {}");
    }
    out.push(" {\n")?;
    if let Some(index) = index {
        out.format(format_args!(
            "  {}[key: string]: ",
            if index.readonly { "readonly " } else { "" }
        ))?;
        write_type(out, &substituted(&index.value, &sub, limits)?)?;
        out.push(";\n")?;
    }
    for (name, prop) in properties {
        out.step()?;
        if let Some(doc) = &prop.doc {
            write_doc_block(out, "  ", doc)?;
        }
        out.format(format_args!(
            "  {name}{}: ",
            if prop.optional { "?" } else { "" }
        ))?;
        write_type(out, &substituted(&prop.ty, &sub, limits)?)?;
        out.push(";\n")?;
    }
    for (name, sig) in methods {
        out.step()?;
        if let Some(doc) = &sig.doc {
            write_doc_block(out, "  ", doc)?;
        }
        out.push("  ")?;
        write_method(out, name, sig, &sub, limits)?;
        out.push("\n")?;
    }
    out.push("}")
}

pub(super) fn method_bindings(
    types: &TypeNamespace,
    registry: &TypeRegistry,
    receiver: &Type,
    name: &str,
) -> Result<TypeParamSubstitution, RenderError> {
    let limits = CopyBudget::default();
    limits.check(receiver)?;
    let Some((mangled, _, interface_name, args)) = receiver.interface_routing() else {
        return Ok(TypeParamSubstitution::new());
    };
    let symbol = resolve(types, registry, &mangled, interface_name).ok_or(
        RenderError::InvalidMetadata("method receiver definition is absent"),
    )?;
    match &symbol.kind {
        TypeKind::Interface { generics, .. } => bindings(generics, &args, &limits),
        TypeKind::Class { .. } => {
            let mut found = None;
            let rendered = Writer::render(RenderLimits::default(), |out| {
                let chain = class_chain(out, types, registry, &mangled, &args, &limits)?;
                for link in chain {
                    let TypeKind::Class { methods, .. } = &link.sym.kind else {
                        return Err(RenderError::InvalidMetadata(
                            "method declaring class has wrong kind",
                        ));
                    };
                    if methods.contains_key(name) {
                        found = Some(link.sub);
                        break;
                    }
                }
                Ok(())
            })?;
            if rendered.truncated {
                return Err(RenderError::Truncated);
            }
            // Universal vtable methods have no declaration-owned generics.
            found
                .or_else(|| matches!(name, "toString" | "toJson").then(TypeParamSubstitution::new))
                .ok_or(RenderError::InvalidMetadata("method declaration is absent"))
        }
        _ => Err(RenderError::InvalidMetadata(
            "method receiver definition has wrong kind",
        )),
    }
}

struct ClassLink<'a> {
    sym: &'a TypeSymbol,
    sub: TypeParamSubstitution,
}

fn class_chain<'a>(
    out: &mut Writer,
    types: &'a TypeNamespace,
    registry: &'a TypeRegistry,
    mangled: &MangledName,
    args: &[Type],
    limits: &CopyBudget,
) -> Result<Vec<ClassLink<'a>>, RenderError> {
    let mut chain: Vec<ClassLink<'a>> = Vec::new();
    let mut symbol = registry
        .lookup(mangled)
        .or_else(|| types.lookup_by_mangled(mangled))
        .ok_or(RenderError::InvalidMetadata("class definition is absent"))?;
    let TypeKind::Class { generics, .. } = &symbol.kind else {
        return Err(RenderError::InvalidMetadata(
            "class definition has wrong kind",
        ));
    };
    let mut sub = bindings(generics, args, limits)?;
    loop {
        out.step()?;
        if chain.len() >= crate::compiler_limits::MAX_CLASS_CHAIN_LEN {
            return Err(RenderError::InvalidMetadata(
                "class chain exceeds its bound",
            ));
        }
        if chain
            .iter()
            .any(|link| link.sym.mangled_name == symbol.mangled_name)
        {
            return Err(RenderError::InvalidMetadata("cyclic class definition"));
        }
        let TypeKind::Class { extends, .. } = &symbol.kind else {
            return Err(RenderError::InvalidMetadata("class parent has wrong kind"));
        };
        let next = if let Some(parent) = extends {
            let parent_symbol = registry
                .lookup(&parent.parent)
                .or_else(|| types.lookup_by_mangled(&parent.parent))
                .ok_or(RenderError::InvalidMetadata("class parent is absent"))?;
            let TypeKind::Class { generics, .. } = &parent_symbol.kind else {
                return Err(RenderError::InvalidMetadata("class parent has wrong kind"));
            };
            let mut args = Vec::new();
            for arg in &parent.args {
                out.step()?;
                args.try_reserve(1).map_err(|_| RenderError::Allocation)?;
                args.push(substituted(arg, &sub, limits)?);
            }
            Some((parent_symbol, bindings(generics, &args, limits)?))
        } else {
            None
        };
        chain.try_reserve(1).map_err(|_| RenderError::Allocation)?;
        chain.push(ClassLink { sym: symbol, sub });
        let Some((parent, parent_sub)) = next else {
            break;
        };
        symbol = parent;
        sub = parent_sub;
    }
    Ok(chain)
}

#[allow(clippy::too_many_arguments)]
fn write_class(
    out: &mut Writer,
    types: &TypeNamespace,
    registry: &TypeRegistry,
    limits: &CopyBudget,
    mangled: &MangledName,
    name: &str,
    args: &[Type],
) -> Result<(), RenderError> {
    let chain = class_chain(out, types, registry, mangled, args, limits)?;
    write_name(out, "class", name, args)?;
    if let Some(parent) = chain.get(1) {
        out.format(format_args!(" extends {}", parent.sym.name))?;
        let TypeKind::Class { generics, .. } = &parent.sym.kind else {
            return Err(RenderError::InvalidMetadata("class parent has wrong kind"));
        };
        if !generics.is_empty() {
            out.push("<")?;
            for (i, generic) in generics.iter().enumerate() {
                if i != 0 {
                    out.push(", ")?;
                }
                write_type(
                    out,
                    parent
                        .sub
                        .get(generic)
                        .ok_or(RenderError::InvalidMetadata("parent binding is absent"))?,
                )?;
            }
            out.push(">")?;
        }
    }
    let has_members = chain.iter().any(|link| match &link.sym.kind {
        TypeKind::Class {
            fields,
            methods,
            statics,
            static_fields,
            constructor,
            ..
        } => {
            !fields.is_empty()
                || !methods.is_empty()
                || !statics.is_empty()
                || !static_fields.is_empty()
                || !constructor.is_empty()
        }
        _ => false,
    });
    if !has_members {
        return out.push(" {}");
    }
    out.push(" {\n")?;
    let mut seen = Vec::new();
    write_class_fields(out, &chain, &mut seen, limits, true)?;
    write_class_methods(out, &chain, &mut seen, limits, true)?;
    seen.clear();
    write_class_fields(out, &chain, &mut seen, limits, false)?;
    write_constructor(out, &chain, limits)?;
    write_class_methods(out, &chain, &mut seen, limits, false)?;
    out.push("}")
}

fn first_member<'a>(
    out: &mut Writer,
    seen: &mut Vec<&'a str>,
    name: &'a str,
) -> Result<bool, RenderError> {
    for prior in seen.iter() {
        out.step()?;
        if *prior == name {
            return Ok(false);
        }
    }
    seen.try_reserve(1).map_err(|_| RenderError::Allocation)?;
    seen.push(name);
    Ok(true)
}

fn inherited(out: &mut Writer, link: &ClassLink<'_>, index: usize) -> Result<(), RenderError> {
    if index != 0 {
        out.format(format_args!("  // from {}", link.sym.name))?;
    }
    out.push("\n")
}

fn write_class_fields<'a>(
    out: &mut Writer,
    chain: &[ClassLink<'a>],
    seen: &mut Vec<&'a str>,
    limits: &CopyBudget,
    is_static: bool,
) -> Result<(), RenderError> {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class {
            fields,
            static_fields,
            ..
        } = &link.sym.kind
        else {
            return Err(RenderError::InvalidMetadata("class has wrong kind"));
        };
        let fields = if is_static { static_fields } else { fields };
        for (name, field) in fields {
            if !first_member(out, seen, name)? {
                continue;
            }
            out.format(format_args!(
                "  {}{}{}{}{}: ",
                visibility_prefix(field.visibility),
                if is_static { "static " } else { "" },
                if field.readonly { "readonly " } else { "" },
                name,
                if !is_static && field.optional {
                    "?"
                } else {
                    ""
                }
            ))?;
            if is_static {
                write_type(out, &field.ty)?;
            } else {
                write_type(out, &substituted(&field.ty, &link.sub, limits)?)?;
            }
            out.push(";")?;
            inherited(out, link, i)?;
        }
    }
    Ok(())
}

fn write_class_methods<'a>(
    out: &mut Writer,
    chain: &[ClassLink<'a>],
    seen: &mut Vec<&'a str>,
    limits: &CopyBudget,
    is_static: bool,
) -> Result<(), RenderError> {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class {
            methods,
            statics,
            method_visibility,
            static_visibility,
            ..
        } = &link.sym.kind
        else {
            return Err(RenderError::InvalidMetadata("class has wrong kind"));
        };
        let (methods, visibility) = if is_static {
            (statics, static_visibility)
        } else {
            (methods, method_visibility)
        };
        let empty = TypeParamSubstitution::new();
        for (name, sig) in methods {
            if !first_member(out, seen, name)? {
                continue;
            }
            out.format(format_args!(
                "  {}{}",
                visibility_prefix(
                    visibility
                        .get(name)
                        .copied()
                        .unwrap_or(crate::Visibility::Public)
                ),
                if is_static { "static " } else { "" }
            ))?;
            write_method(
                out,
                name,
                sig,
                if is_static { &empty } else { &link.sub },
                limits,
            )?;
            inherited(out, link, i)?;
        }
    }
    Ok(())
}

fn write_constructor(
    out: &mut Writer,
    chain: &[ClassLink<'_>],
    limits: &CopyBudget,
) -> Result<(), RenderError> {
    for (i, link) in chain.iter().enumerate() {
        let TypeKind::Class {
            constructor,
            constructor_visibility,
            ..
        } = &link.sym.kind
        else {
            return Err(RenderError::InvalidMetadata("class has wrong kind"));
        };
        if constructor.is_empty() {
            continue;
        }
        out.format(format_args!(
            "  {}constructor(",
            visibility_prefix(*constructor_visibility)
        ))?;
        super::format_signature::write_params(out, constructor, &link.sub, limits)?;
        out.push(");")?;
        return inherited(out, link, i);
    }
    Ok(())
}

fn visibility_prefix(visibility: crate::Visibility) -> &'static str {
    if visibility == crate::Visibility::Private {
        "private "
    } else {
        ""
    }
}

pub(crate) fn format_interface_header(
    name: &str,
    generics: &[String],
) -> Result<String, RenderError> {
    header("interface", name, generics)
}
pub(crate) fn format_class_header(name: &str, generics: &[String]) -> Result<String, RenderError> {
    header("class", name, generics)
}
fn header(kind: &str, name: &str, generics: &[String]) -> Result<String, RenderError> {
    Writer::render(RenderLimits::default(), |out| {
        out.format(format_args!("{kind} {name}"))?;
        write_generic_list(out, generics)
    })
    .map(|rendered| rendered.text)
}

fn write_method(
    out: &mut Writer,
    name: &str,
    sig: &MethodSig,
    sub: &TypeParamSubstitution,
    limits: &CopyBudget,
) -> Result<(), RenderError> {
    out.push(name)?;
    write_generic_list(out, &sig.generics)?;
    out.push("(")?;
    for (i, param) in sig.params.iter().enumerate() {
        out.step()?;
        if i != 0 {
            out.push(", ")?;
        }
        let ty = substituted(&param.ty, sub, limits)?;
        // Definition snapshots intentionally omit the rest marker here.
        write_named_param(out, &param.name, &ty, param.default.as_ref(), false)?;
    }
    out.push("): ")?;
    write_type(out, &substituted(&sig.ret, sub, limits)?)?;
    out.push(";")
}

macro_rules! write_doc_line {
    ($out:expr, $format:literal $(, $arg:expr)* $(,)?) => { $out.format(format_args!(concat!($format, "\n") $(, $arg)*)) };
}
pub(super) fn write_doc_block(
    out: &mut Writer,
    indent: &str,
    doc: &crate::DocComment,
) -> Result<(), RenderError> {
    let has_tags = doc.returns.is_some()
        || !doc.params.is_empty()
        || !doc.throws.is_empty()
        || doc.deprecated.is_some()
        || !doc.examples.is_empty()
        || !doc.unknown_tags.is_empty();
    let has_summary = !doc.summary.is_empty();
    if !has_tags && has_summary {
        write_doc_line!(out, "{}/** {} */", indent, doc.summary)?;
        return Ok(());
    }
    if !has_tags && !has_summary {
        return Ok(());
    }
    write_doc_line!(out, "{}/**", indent)?;
    if has_summary {
        write_doc_line!(out, "{} * {}", indent, doc.summary)?;
    }
    for p in &doc.params {
        if p.description.is_empty() {
            write_doc_line!(out, "{} * @param {}", indent, p.name)?;
        } else {
            write_doc_line!(out, "{} * @param {} {}", indent, p.name, p.description)?;
        }
    }
    if let Some(r) = &doc.returns {
        if r.description.is_empty() {
            write_doc_line!(out, "{} * @returns", indent)?;
        } else {
            write_doc_line!(out, "{} * @returns {}", indent, r.description)?;
        }
    }
    for t in &doc.throws {
        write_doc_line!(out, "{} * @throws {}", indent, t.text)?;
    }
    if let Some(d) = &doc.deprecated {
        write_doc_line!(out, "{} * @deprecated {}", indent, d.text)?;
    }
    for ex in &doc.examples {
        write_doc_line!(out, "{} * @example {}", indent, ex.text)?;
    }
    for u in &doc.unknown_tags {
        write_doc_line!(out, "{} * @{} {}", indent, u.name, u.text)?;
    }
    write_doc_line!(out, "{} */", indent)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::package_declaration::TypeSymbol;
    use crate::{Span, TypeKind};

    /// Renders without a type limit being reached; shadows the 4-argument form.
    fn format_definition(ty: &Type, types: &TypeNamespace, registry: &TypeRegistry) -> String {
        super::format_definition(ty, types, registry).unwrap()
    }

    fn empty_ns() -> TypeNamespace<'static> {
        TypeNamespace::new()
    }

    fn ns_with(
        iface: &str,
        generics: Vec<String>,
        methods: Vec<(&'static str, MethodSig)>,
    ) -> TypeNamespace<'static> {
        ns_with_props(iface, generics, methods, vec![])
    }

    fn ns_with_props(
        iface: &str,
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
    fn oversized_interface_name_does_not_hide_wrong_generic_arity() {
        let name = "Interface".repeat(20_000);
        let types = ns_with(&name, vec!["T".into()], vec![]);
        let ty = Type::interface_ref(
            crate::Package::user(),
            &name,
            crate::mangle::package_symbol("test", &name),
            vec![],
        );
        let result = super::format_definition(&ty, &types, &TypeRegistry::new());
        assert!(matches!(
            result,
            Err(RenderError::InvalidMetadata(
                "definition generic arity mismatch"
            ))
        ));
    }

    #[test]
    fn rendering_failure_discards_compilation_and_preserves_prior_diagnostics() {
        crate::type_size::tests::on_compiler_stack(|| {
            let source =
                "function main(): void { const p: number = absent; const n = 1; n.missing; }";
            FAIL_RENDER.with(|fail| fail.set(true));
            let error = crate::compile::compile_script_checked(
                source,
                "broken.ts",
                crate::FileId(0),
                &[],
                &[],
            )
            .unwrap_err();
            assert!(
                matches!(
                    error.fatal,
                    Some(crate::compiler_error::CompilerFailure::Internal { .. })
                ),
                "{error:?}"
            );
            assert!(
                error
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("absent")),
                "{error:?}"
            );
            let good = crate::compile::compile_script_checked(
                "function main(): number { return 42; }",
                "good.ts",
                crate::FileId(0),
                &[],
                &[],
            )
            .unwrap();
            assert!(!good.wasm.is_empty());
        });
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
    fn unknown_interface_returns_internal_metadata_failure() {
        let out = super::format_definition(
            &Type::InterfaceRef {
                mangled: crate::mangle::prelude("Unknown"),
                package: crate::Package::prelude(),
                name: "Unknown".to_string(),
                args: vec![],
            },
            &empty_ns(),
            &TypeRegistry::new(),
        );
        assert!(matches!(out, Err(RenderError::InvalidMetadata(_))));
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
