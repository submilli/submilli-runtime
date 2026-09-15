use crate::{
    Diagnostic, ExportEntry, ExportKind, PackageDeclaration, Severity, Span, TypeKind, TypeSymbol,
    TypedAst, TypedInterfaceDecl, TypedInterfaceMember, TypedTypeDecl, ValueKind, ValueSymbol,
};

pub(super) fn run(
    typed_ast: &TypedAst,
    package: &PackageDeclaration,
    exports: &[ExportEntry],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for export in exports {
        match export.kind {
            ExportKind::Function | ExportKind::Global => {
                let Some(symbol) = exported_value(package, export) else {
                    continue;
                };
                if !value_has_doc(&symbol.kind) {
                    diagnostics.push(missing_export_doc(export.span, &symbol.name));
                }
            }
            ExportKind::Type => {
                let Some(symbol) = exported_type(package, export) else {
                    continue;
                };
                if !type_has_doc(&symbol.kind) {
                    diagnostics.push(missing_export_doc(export.span, &symbol.name));
                }
                validate_interface_members(typed_ast, symbol, diagnostics);
            }
        }
    }
}

fn exported_value<'a>(
    package: &'a PackageDeclaration,
    export: &ExportEntry,
) -> Option<&'a ValueSymbol> {
    package
        .values
        .values()
        .find(|symbol| symbol.mangled_name == export.public_name)
}

fn exported_type<'a>(
    package: &'a PackageDeclaration,
    export: &ExportEntry,
) -> Option<&'a TypeSymbol> {
    package
        .types
        .values()
        .find(|symbol| symbol.mangled_name == export.public_name)
}

fn value_has_doc(kind: &ValueKind) -> bool {
    match kind {
        ValueKind::Function { doc, .. }
        | ValueKind::Let { doc, .. }
        | ValueKind::Const { doc, .. } => doc.is_some(),
    }
}

fn type_has_doc(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Interface { doc, .. }
        | TypeKind::Class { doc, .. }
        | TypeKind::NumberEnum { doc, .. }
        | TypeKind::StringEnum { doc, .. }
        | TypeKind::Alias { doc, .. } => doc.is_some(),
    }
}

fn validate_interface_members(
    typed_ast: &TypedAst,
    symbol: &TypeSymbol,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let TypeKind::Interface { .. } = &symbol.kind else {
        return;
    };
    let Some(interface) = typed_interface(typed_ast, symbol.declaration_span) else {
        return;
    };
    for member in &interface.members {
        match member {
            TypedInterfaceMember::Method {
                name, doc: None, ..
            }
            | TypedInterfaceMember::Property {
                name, doc: None, ..
            } => diagnostics.push(warning(
                name.span,
                format!(
                    "exported interface `{}` member `{}` has no doc comment",
                    symbol.name, name.name,
                ),
            )),
            TypedInterfaceMember::Method { .. } | TypedInterfaceMember::Property { .. } => {}
        }
    }
}

fn typed_interface(typed_ast: &TypedAst, declaration_span: Span) -> Option<&TypedInterfaceDecl> {
    typed_ast.types.iter().find_map(|decl| match decl {
        TypedTypeDecl::Interface(interface) if interface.name.span == declaration_span => {
            Some(interface)
        }
        _ => None,
    })
}

fn missing_export_doc(span: Span, name: &str) -> Diagnostic {
    warning(span, format!("exported symbol `{name}` has no doc comment"))
}

fn warning(span: Span, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        span,
        message,
        help: Vec::new(),
        notes: Vec::new(),
    }
}
