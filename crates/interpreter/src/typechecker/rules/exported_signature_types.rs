use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Diagnostic, ExportEntry, MangledName, Package, PackageDeclaration, Severity, Type, TypeKind,
    TypeSymbol, ValueKind, ValueSymbol,
};

pub(crate) fn run(
    package_name: &str,
    level: &str,
    surface: &PackageDeclaration,
    exports: &[ExportEntry],
    diagnostics: &mut Vec<Diagnostic>,
) {
    run_symbols(
        package_name,
        level,
        surface.values.values(),
        surface.types.values(),
        exports,
        diagnostics,
    );
}

pub(in crate::typechecker) fn run_module(
    package_name: &str,
    level: &str,
    surface: &crate::typechecker::infer::module_symbols::ModuleSymbols,
    exports: &[ExportEntry],
    diagnostics: &mut Vec<Diagnostic>,
) {
    run_symbols(
        package_name,
        level,
        surface.exported_values(),
        surface.exported_types(),
        exports,
        diagnostics,
    );
}

fn run_symbols<'a>(
    package_name: &str,
    level: &str,
    values: impl Iterator<Item = &'a ValueSymbol> + Clone,
    types: impl Iterator<Item = &'a TypeSymbol> + Clone,
    exports: &[ExportEntry],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let exported_types = exported_type_mangles(types.clone(), exports);
    for value in values {
        let mut used = BTreeMap::new();
        let (noun, usage) = match &value.kind {
            ValueKind::Function {
                params,
                ret,
                type_predicate,
                ..
            } => {
                for param in params {
                    collect_signature_named_types(package_name, &param.ty, &mut used);
                }
                collect_signature_named_types(package_name, ret, &mut used);
                if let Some(predicate) = type_predicate {
                    collect_signature_named_types(
                        package_name,
                        &predicate.asserted_type,
                        &mut used,
                    );
                }
                ("function", "signature")
            }
            ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. } => {
                collect_signature_named_types(package_name, ty, &mut used);
                ("global", "type annotation")
            }
        };

        diagnose_private_types(
            &exported_types,
            used,
            ExportUse {
                noun,
                usage,
                exported_name: &value.name,
                span: value.declaration_span,
                level,
            },
            diagnostics,
        );
    }

    for ty in types {
        let mut used = BTreeMap::new();
        match &ty.kind {
            TypeKind::Interface {
                methods,
                properties,
                ..
            } => {
                for method in methods.values() {
                    for param in &method.params {
                        collect_signature_named_types(package_name, &param.ty, &mut used);
                    }
                    collect_signature_named_types(package_name, &method.ret, &mut used);
                    if let Some(predicate) = &method.predicate {
                        collect_signature_named_types(
                            package_name,
                            &predicate.asserted_type,
                            &mut used,
                        );
                    }
                }
                for property in properties.values() {
                    collect_signature_named_types(package_name, &property.ty, &mut used);
                }
            }
            TypeKind::Class {
                fields,
                methods,
                statics,
                static_fields,
                constructor,
                ..
            } => {
                for field in fields.values().chain(static_fields.values()) {
                    collect_signature_named_types(package_name, &field.ty, &mut used);
                }
                for method in methods.values().chain(statics.values()) {
                    for param in &method.params {
                        collect_signature_named_types(package_name, &param.ty, &mut used);
                    }
                    collect_signature_named_types(package_name, &method.ret, &mut used);
                }
                for param in constructor {
                    collect_signature_named_types(package_name, &param.ty, &mut used);
                }
            }
            TypeKind::Alias { ty, .. } => {
                collect_signature_named_types(package_name, ty, &mut used);
            }
            TypeKind::NumberEnum { .. } | TypeKind::StringEnum { .. } => {}
        }

        diagnose_private_types(
            &exported_types,
            used,
            ExportUse {
                noun: "type",
                usage: "definition",
                exported_name: &ty.name,
                span: ty.declaration_span,
                level,
            },
            diagnostics,
        );
    }
}

struct ExportUse<'a> {
    noun: &'a str,
    usage: &'a str,
    exported_name: &'a str,
    span: crate::Span,
    level: &'a str,
}

fn diagnose_private_types(
    exported_types: &BTreeSet<MangledName>,
    used: BTreeMap<MangledName, String>,
    export_use: ExportUse<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (mangled, name) in used {
        if exported_types.contains(&mangled) {
            continue;
        }
        diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span: export_use.span,
            message: format!(
                "exported {} `{}` uses private type `{name}` in its {}",
                export_use.noun, export_use.exported_name, export_use.usage,
            ),
            help: vec![format!(
                "export `{name}` from this {} so callers can name the {}'s {}",
                export_use.level, export_use.noun, export_use.usage,
            )],
            notes: Vec::new(),
        });
    }
}

fn exported_type_mangles<'a>(
    types: impl Iterator<Item = &'a TypeSymbol> + Clone,
    exports: &[ExportEntry],
) -> BTreeSet<MangledName> {
    let mut out: BTreeSet<MangledName> =
        types.clone().map(|sym| sym.mangled_name.clone()).collect();
    let public_type_mangles: BTreeSet<MangledName> =
        types.map(|sym| sym.mangled_name.clone()).collect();
    out.extend(
        exports
            .iter()
            .filter(|entry| public_type_mangles.contains(&entry.public_name))
            .map(|entry| entry.target.clone()),
    );
    out
}

fn collect_signature_named_types(
    package_name: &str,
    ty: &Type,
    out: &mut BTreeMap<MangledName, String>,
) {
    match ty {
        Type::InterfaceRef {
            mangled,
            package,
            name,
            args,
            ..
        }
        | Type::ClassRef {
            mangled,
            package,
            name,
            args,
            ..
        }
        | Type::AliasRef {
            mangled,
            package,
            name,
            args,
            ..
        } => {
            collect_if_package_type(package_name, mangled, package, name, out);
            for arg in args {
                collect_signature_named_types(package_name, arg, out);
            }
        }
        Type::NumberEnum {
            mangled,
            package,
            name,
            ..
        }
        | Type::StringEnum {
            mangled,
            package,
            name,
            ..
        } => {
            collect_if_package_type(package_name, mangled, package, name, out);
        }
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ty,
            ..
        } => {
            collect_if_package_type(package_name, mangled, package, name, out);
            for arg in args {
                collect_signature_named_types(package_name, arg, out);
            }
            collect_signature_named_types(package_name, ty, out);
        }
        Type::Function {
            params,
            ret,
            predicate,
            ..
        } => {
            for param in params {
                collect_signature_named_types(package_name, param, out);
            }
            collect_signature_named_types(package_name, ret, out);
            if let Some(predicate) = predicate {
                collect_signature_named_types(package_name, &predicate.asserted_type, out);
            }
        }
        Type::Object { fields } => {
            for field in fields.values() {
                collect_signature_named_types(package_name, &field.ty, out);
            }
        }
        Type::Refined { original, ty } => {
            collect_signature_named_types(package_name, original, out);
            collect_signature_named_types(package_name, ty, out);
        }
        Type::Array(elem) => collect_signature_named_types(package_name, elem, out),
        Type::Tuple(elements) | Type::Union(elements) => {
            for element in elements {
                collect_signature_named_types(package_name, element, out);
            }
        }
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::String
        | Type::StringLiteral(_)
        | Type::Uint8Array
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Void
        | Type::Unknown
        | Type::Error
        | Type::Never
        | Type::TypeVar(_)
        | Type::GenericParam { .. } => {}
    }
}

fn collect_if_package_type(
    package_name: &str,
    mangled: &MangledName,
    package: &Package,
    name: &str,
    out: &mut BTreeMap<MangledName, String>,
) {
    if package.as_str() == package_name {
        out.entry(mangled.clone())
            .or_insert_with(|| name.to_string());
    }
}
