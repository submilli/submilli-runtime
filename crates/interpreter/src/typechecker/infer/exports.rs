//! Export handling for single-file inference and package module surfaces.
//!
//! Runs after the three inference passes so every exported symbol already has a
//! `TypedFunction`/`TypedGlobal`/type entry with its mangled name. In a
//! single-file package the public and internal names coincide, so each entry's
//! `public_name` and `target` are the same `pkg#name`.

use crate::compiler_error::CompilerFailure;

use crate::{ExportEntry, ExportKind, Ident, StmtKind};

use super::Inferer;
use super::module_symbols::ModuleSymbols;

impl Inferer<'_> {
    pub(super) fn collect_exports(&mut self) -> Result<(), CompilerFailure> {
        // Form 1: every export-marked top-level declaration becomes a public entry.
        for ed in self.ast.exported_decls.clone() {
            let stmt = self.ast.try_stmt(ed.stmt).map_err(super::arena_failure)?;
            let Some(name) = exported_decl_name(&stmt.kind) else {
                continue;
            };
            let mangled = self.mangle_top_symbol(&name.name)?;
            if matches!(stmt.kind, StmtKind::ClassDecl { .. })
                && let Some(sym) = self.types.lookup(&name.name)
            {
                let statics = static_export_entries(&sym.kind, &sym.mangled_name, ed.export_span);
                self.typed_ast.exports.extend(statics);
            }
            self.typed_ast.exports.push(ExportEntry {
                public_name: mangled.clone(),
                target: mangled,
                kind: export_kind(&stmt.kind)?,
                span: ed.export_span,
            });
        }

        if self.package_inference {
            return Ok(());
        }

        // Form 2: re-exports are only meaningful across modules. Gate them.
        for stmt_id in self.ast.top_level.clone() {
            let stmt = self.ast.try_stmt(stmt_id).map_err(super::arena_failure)?;
            if matches!(stmt.kind, StmtKind::ExportFrom { .. }) {
                let span = stmt.span;
                self.error_with_help(
                    span,
                    "`export { … }` re-exports are only valid in multi-file packages".to_string(),
                    vec![
                        "this package has a single module; mark the declaration itself \
                         (`export function`, `export const`, …) instead"
                            .to_string(),
                    ],
                );
            }
        }

        Ok(())
    }

    pub(super) fn mark_direct_package_export(
        &mut self,
        name: &str,
        namespace: ExportNamespace,
    ) -> Result<(), CompilerFailure> {
        if !self.package_inference {
            return Ok(());
        }
        let Some(span) = self.direct_export_span(name, namespace)? else {
            return Ok(());
        };
        let declarations = self.current_module_symbols.clone();
        let source_label = self.module.as_str().to_string();
        self.export_symbol_from(
            &declarations,
            false,
            name,
            name,
            span,
            &source_label,
            ExportOrigin::Direct(namespace),
        );

        Ok(())
    }

    pub(super) fn resolve_package_export_statements(&mut self) -> Result<(), CompilerFailure> {
        for stmt_id in self.ast.top_level.clone() {
            let stmt = self.ast.try_stmt(stmt_id).map_err(super::arena_failure)?;
            let StmtKind::ExportFrom { specs, source, .. } = &stmt.kind else {
                continue;
            };

            let Some((specifier, _span)) = source else {
                for spec in specs {
                    let declarations = self.current_module_symbols.clone();
                    let source_label = self.module.as_str().to_string();
                    self.export_symbol_from(
                        &declarations,
                        false,
                        &spec.imported_name.name,
                        &spec.local_name.name,
                        spec.imported_name.span,
                        &source_label,
                        ExportOrigin::Specifier,
                    );
                }
                continue;
            };

            if !crate::source::is_relative_specifier(specifier) {
                continue;
            }
            let Ok(resolved) = self.module.resolve_relative(specifier) else {
                continue;
            };
            let Some(source_module) = self.inferred_modules.get(&resolved).cloned() else {
                continue;
            };
            for spec in specs {
                let imported = spec.imported_name.name.as_str();
                let public = spec.local_name.name.as_str();
                if !source_module.contains_exported(imported) {
                    if source_module.contains(imported) {
                        self.diagnostics.push(crate::Diagnostic {
                            severity: crate::Severity::Error,
                            span: spec.imported_name.span,
                            message: format!("`{imported}` is private to `{specifier}`"),
                            help: vec![format!(
                                "add `export` to `{imported}` in module `{}` to make it re-exportable",
                                resolved.as_str(),
                            )],
                            notes: Vec::new(),
                        });
                        continue;
                    }
                    self.diagnostics.push(crate::Diagnostic {
                        severity: crate::Severity::Error,
                        span: spec.imported_name.span,
                        message: format!("module `{specifier}` does not export `{imported}`"),
                        help: source_module.exports_help(),
                        notes: Vec::new(),
                    });
                    continue;
                }
                self.export_symbol_from(
                    &source_module,
                    true,
                    imported,
                    public,
                    spec.imported_name.span,
                    resolved.as_str(),
                    ExportOrigin::Specifier,
                );
            }
        }

        Ok(())
    }

    fn direct_export_span(
        &self,
        name: &str,
        namespace: ExportNamespace,
    ) -> Result<Option<crate::Span>, CompilerFailure> {
        self.ast
            .exported_decls
            .iter()
            .map(|ed| {
                let stmt = self.ast.try_stmt(ed.stmt).map_err(super::arena_failure)?;
                Ok::<_, CompilerFailure>(
                    (exported_decl_namespace(&stmt.kind) == Some(namespace)
                        && exported_decl_name(&stmt.kind)
                            .is_some_and(|exported| exported.name == name))
                    .then_some(ed.export_span),
                )
            })
            .find_map(Result::transpose)
            .transpose()
    }

    /// Exports what `source_name` names in the namespaces `origin` allows. A
    /// specifier (`export { A }`) carries both a value and a type of one name,
    /// as in TypeScript, where the two live in separate namespaces. The same
    /// declaration exported both directly and by a specifier is one export.
    #[allow(clippy::too_many_arguments)]
    fn export_symbol_from(
        &mut self,
        declarations: &ModuleSymbols,
        exported_only: bool,
        source_name: &str,
        public_name: &str,
        span: crate::Span,
        source_label: &str,
        origin: ExportOrigin,
    ) {
        let value = origin
            .allows(ExportNamespace::Value)
            .then(|| {
                if exported_only {
                    declarations.exported_value(source_name)
                } else {
                    declarations.value(source_name)
                }
            })
            .flatten();
        let ty = origin
            .allows(ExportNamespace::Type)
            .then(|| {
                if exported_only {
                    declarations.exported_type(source_name)
                } else {
                    declarations.type_symbol(source_name)
                }
            })
            .flatten();
        if value.is_none() && ty.is_none() {
            self.diagnostics.push(crate::Diagnostic {
                severity: crate::Severity::Error,
                span,
                message: format!("module `{source_label}` does not export `{source_name}`"),
                help: declarations.exports_help(),
                notes: Vec::new(),
            });
            return;
        }

        let exported = ExportedName {
            span,
            source: (source_label.to_string(), source_name.to_string()),
            direct: matches!(origin, ExportOrigin::Direct(_)),
        };
        let namespaces = [
            (ExportNamespace::Value, value.is_some()),
            (ExportNamespace::Type, ty.is_some()),
        ];
        let mut fresh = Vec::new();
        for (namespace, present) in namespaces {
            if !present {
                continue;
            }
            match self
                .current_export_seen
                .get(&(namespace, public_name.to_string()))
            {
                Some(prev) if prev.is_same_export(&exported) => {}
                Some(prev) => {
                    self.diagnostics.push(crate::Diagnostic {
                        severity: crate::Severity::Error,
                        span,
                        message: format!("duplicate public export `{public_name}`"),
                        help: Vec::new(),
                        notes: vec![(prev.span, "previously exported here".to_string())],
                    });
                    return;
                }
                None => fresh.push(namespace),
            }
        }

        for namespace in fresh {
            self.current_export_seen
                .insert((namespace, public_name.to_string()), exported.clone());
            match namespace {
                ExportNamespace::Value => {
                    if let Some(sym) = value {
                        self.export_value(sym, public_name, span);
                    }
                }
                ExportNamespace::Type => {
                    if let Some(sym) = ty {
                        self.export_type(sym, public_name, span);
                    }
                }
            }
        }
    }

    fn export_value(&mut self, sym: &crate::ValueSymbol, public_name: &str, span: crate::Span) {
        let mut sym = sym.clone();
        let target_mangled = sym.mangled_name.clone();
        sym.name = public_name.to_string();
        let is_root = self.module == self.root_module;
        let public_mangled = crate::mangle::package_symbol(self.package_name, public_name);
        if is_root {
            sym.mangled_name = public_mangled.clone();
        }
        let kind = match sym.kind {
            crate::ValueKind::Function { .. } => ExportKind::Function,
            crate::ValueKind::Let { .. } | crate::ValueKind::Const { .. } => ExportKind::Global,
        };
        self.current_module_symbols
            .values
            .insert(public_name.to_string(), (true, sym));
        if is_root {
            self.current_module_exports.push(ExportEntry {
                public_name: public_mangled,
                target: target_mangled,
                kind,
                span,
            });
        }
    }

    fn export_type(&mut self, sym: &crate::TypeSymbol, public_name: &str, span: crate::Span) {
        let mut sym = sym.clone();
        let target_mangled = sym.mangled_name.clone();
        sym.name = public_name.to_string();
        let is_root = self.module == self.root_module;
        let public_mangled = crate::mangle::package_symbol(self.package_name, public_name);
        // A class's mangled name is its nominal identity — the same name its
        // `extends`/`implements` references and its WasmGC type/constructor
        // exports use. Re-exporting must not rebrand it to the public package
        // name, or a cross-package consumer would see the parent under one
        // name and the subclass's `extends` under another (SUB-488). Only the
        // public *name* (the lookup key) changes; other types still adopt the
        // package-qualified mangle.
        let is_class = matches!(sym.kind, crate::TypeKind::Class { .. });
        if is_root && !is_class {
            sym.mangled_name = public_mangled.clone();
        }
        let statics = static_export_entries(&sym.kind, &target_mangled, span);
        self.current_module_symbols
            .types
            .insert(public_name.to_string(), (true, sym));
        if !is_root {
            return;
        }
        // Statics ride the class export: one function/global entry per
        // public static, keyed by the defining class's `Class#static#name`
        // (stable across re-exports — statics derive from the class
        // mangle, which never rebrands).
        self.current_module_exports.extend(statics);
        self.current_module_exports.push(ExportEntry {
            public_name: if is_class {
                target_mangled.clone()
            } else {
                public_mangled
            },
            target: target_mangled,
            kind: ExportKind::Type,
            span,
        });
    }
}

/// The namespace an exported name occupies. TypeScript keeps types and values
/// apart, so one name may export a type and a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::typechecker) enum ExportNamespace {
    Value,
    Type,
}

/// How a name came to be exported: by an `export` on its declaration, which
/// exports only that declaration's namespace, or by an `export { … }`
/// specifier, which exports whatever the name means.
#[derive(Clone, Copy, Debug)]
enum ExportOrigin {
    Direct(ExportNamespace),
    Specifier,
}

impl ExportOrigin {
    fn allows(self, namespace: ExportNamespace) -> bool {
        match self {
            Self::Direct(own) => own == namespace,
            Self::Specifier => true,
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::typechecker) struct ExportedName {
    span: crate::Span,
    /// The exporting module and the name it exports from.
    source: (String, String),
    direct: bool,
}

impl ExportedName {
    /// `export type A = …; export { A };` exports the alias twice, which tsc
    /// accepts; two specifiers of one name are a duplicate, as in tsc.
    fn is_same_export(&self, other: &Self) -> bool {
        self.source == other.source && (self.direct || other.direct)
    }
}

/// The declared name of an exportable top-level declaration, or `None` for any
/// statement kind that cannot carry a leading `export` (the parser only records
/// the six below in `exported_decls`).
pub(super) fn exported_decl_name(kind: &StmtKind) -> Option<&Ident> {
    match kind {
        StmtKind::Function { name, .. }
        | StmtKind::Let { name, .. }
        | StmtKind::Const { name, .. }
        | StmtKind::InterfaceDecl { name, .. }
        | StmtKind::ClassDecl { name, .. }
        | StmtKind::EnumDecl { name, .. }
        | StmtKind::TypeAliasDecl { name, .. } => Some(name),
        _ => None,
    }
}

/// The namespace an exportable top-level declaration exports into. Classes
/// and enums are recorded as types; their constructor and members ride on the
/// type's export.
fn exported_decl_namespace(kind: &StmtKind) -> Option<ExportNamespace> {
    match kind {
        StmtKind::Function { .. } | StmtKind::Let { .. } | StmtKind::Const { .. } => {
            Some(ExportNamespace::Value)
        }
        StmtKind::InterfaceDecl { .. }
        | StmtKind::ClassDecl { .. }
        | StmtKind::EnumDecl { .. }
        | StmtKind::TypeAliasDecl { .. } => Some(ExportNamespace::Type),
        _ => None,
    }
}

/// One `Function`/`Global` export entry per public static of a class, keyed by
/// the defining class's `Class#static#name`. Private statics stay
/// module-internal (not exported, not rendered).
fn static_export_entries(
    kind: &crate::TypeKind,
    class_mangled: &crate::MangledName,
    span: crate::Span,
) -> Vec<ExportEntry> {
    let crate::TypeKind::Class {
        statics,
        static_visibility,
        static_fields,
        ..
    } = kind
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in statics.keys() {
        if static_visibility.get(name) == Some(&crate::Visibility::Private) {
            continue;
        }
        let key = crate::mangle::static_member(class_mangled, name);
        out.push(ExportEntry {
            public_name: key.clone(),
            target: key,
            kind: ExportKind::Function,
            span,
        });
    }
    for (name, field) in static_fields {
        if field.visibility == crate::Visibility::Private {
            continue;
        }
        let key = crate::mangle::static_member(class_mangled, name);
        out.push(ExportEntry {
            public_name: key.clone(),
            target: key,
            kind: ExportKind::Global,
            span,
        });
    }
    out
}

fn export_kind(kind: &StmtKind) -> Result<ExportKind, CompilerFailure> {
    Ok(match kind {
        StmtKind::Function { .. } => ExportKind::Function,
        StmtKind::Let { .. } | StmtKind::Const { .. } => ExportKind::Global,
        StmtKind::InterfaceDecl { .. }
        | StmtKind::ClassDecl { .. }
        | StmtKind::EnumDecl { .. }
        | StmtKind::TypeAliasDecl { .. } => ExportKind::Type,
        _ => {
            return Err(super::inference_failure(
                "unexpected exported declaration kind",
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{run, run_clean};
    use crate::mangle::package_symbol;

    #[test]
    fn form1_exports_only_marked_declarations() {
        let ta = run_clean(
            "export function foo(): void {} export const X: number = 1; function helper(): void {}",
        );
        assert_eq!(
            ta.exports.len(),
            2,
            "only the two marked decls: {:?}",
            ta.exports
        );
        for entry in &ta.exports {
            assert_eq!(
                entry.public_name, entry.target,
                "single-file: public == target"
            );
        }
        let names: Vec<_> = ta.exports.iter().map(|e| e.target.clone()).collect();
        assert!(names.contains(&package_symbol("main", "foo")));
        assert!(names.contains(&package_symbol("main", "X")));
        assert!(
            !names.contains(&package_symbol("main", "helper")),
            "unmarked declaration must not be exported",
        );
    }

    #[test]
    fn exported_interface_is_recorded() {
        let ta = run_clean("export interface Result { x: number; } function main(): void {}");
        let names: Vec<_> = ta.exports.iter().map(|e| e.target.clone()).collect();
        assert!(names.contains(&package_symbol("main", "Result")));
    }

    #[test]
    fn reexport_is_gated_in_single_file() {
        let (_ta, diags) = run("function main(): void {}\nexport { main };");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("only valid in multi-file packages")),
            "expected re-export gating diagnostic, got: {diags:?}",
        );
    }
}
