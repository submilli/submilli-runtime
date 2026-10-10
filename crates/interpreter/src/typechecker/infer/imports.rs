//! Prelude population and explicit-import resolution.
//! Runs before signature inference so imported type names are in scope for function signatures.

use crate::compiler_error::CompilerFailure;

use crate::{
    Diagnostic, Ident, NamespaceSymbol, PackageDeclaration, Severity, StmtKind, Type, TypeKind,
    ValueKind,
};

use super::module_symbols::{ModuleSymbols, NamespaceMembers, NamespaceSymbolSet};
use super::type_namespace::TypeNamespace;
use super::type_registry::TypeRegistry;
use super::{Inferer, NamespaceBinding, ValueEntry};

/// Register every named type a package declares — top-level plus namespaced —
/// into the FQN registry under the package's name. Namespaced types use the same
/// dotted key [`flatten_namespace_types`] gives the import-scoped namespace, so a
/// `Temporal.Instant`-shaped `InterfaceRef` resolves identically through either.
pub(super) fn register_package_types<'a>(reg: &mut TypeRegistry<'a>, defs: &'a PackageDeclaration) {
    for (name, sym) in &defs.types {
        let _ = name;
        reg.insert_borrowed(sym);
    }
    for (ns_name, ns) in &defs.namespaces {
        register_namespace_types(reg, &defs.package_name, ns, ns_name);
    }
}

pub(super) fn register_module_types(reg: &mut TypeRegistry, symbols: &ModuleSymbols) {
    for sym in symbols.all_types() {
        reg.insert_owned(sym.clone());
    }
}

fn register_namespace_types<'a>(
    reg: &mut TypeRegistry<'a>,
    package: &str,
    ns: &'a NamespaceSymbol,
    path: &str,
) {
    for (name, sym) in &ns.types {
        let full = format!("{path}.{name}");
        let _ = (package, full);
        reg.insert_borrowed(sym);
    }
    for (sub_name, sub_ns) in &ns.namespaces {
        let sub_path = format!("{path}.{sub_name}");
        register_namespace_types(reg, package, sub_ns, &sub_path);
    }
}

/// Flat copy lets `find_method`/`lookup_named_type` resolve via a single dotted string key
/// without namespace awareness; nested entries stay in `NamespaceSymbol::types` for the dotted-walker.
fn flatten_namespace_types<'a>(
    types: &mut TypeNamespace<'a>,
    package: &'a str,
    ns: &'a NamespaceSymbol,
    path: &str,
) {
    for (name, sym) in &ns.types {
        let full = format!("{path}.{name}");
        types.insert_borrowed(full, package, sym);
    }
    for (sub_name, sub_ns) in &ns.namespaces {
        let sub_path = format!("{path}.{sub_name}");
        flatten_namespace_types(types, package, sub_ns, &sub_path);
    }
}

fn load_namespaces<'a>(
    types: &mut TypeNamespace<'a>,
    namespace_symbols: &mut std::collections::BTreeMap<String, NamespaceSymbolSet<'a>>,
    package: &'a str,
    defs_namespaces: &'a std::collections::BTreeMap<String, NamespaceSymbol>,
) {
    for (name, ns) in defs_namespaces {
        flatten_namespace_types(types, package, ns, name);
        namespace_symbols
            .entry(name.clone())
            .and_modify(|existing| existing.push(ns))
            .or_insert_with(|| NamespaceSymbolSet::new(ns));
    }
}

impl<'a> Inferer<'a> {
    pub(super) fn populate_prelude(&mut self) -> Result<(), CompilerFailure> {
        // Constructor bindings (`Const { ty: InterfaceRef }`) are public by
        // shape; everything else in `prelude.values` is an internal helper
        // (string_concat, …) except the bare globals named here.
        const PUBLIC_PRELUDE_VALUES: [&str; 8] = [
            "NaN",
            "Infinity",
            "isNaN",
            "isFinite",
            "encodeURIComponent",
            "encodeURI",
            "decodeURIComponent",
            "decodeURI",
        ];
        let prelude = self
            .packages_by_name
            .get(crate::mangle::PRELUDE_PACKAGE)
            .copied()
            .ok_or_else(|| {
                super::inference_failure("typechecker requires prelude package declaration")
            })?;
        for (name, sym) in &prelude.types {
            if matches!(
                sym.kind,
                TypeKind::Interface { .. } | TypeKind::Alias { .. } | TypeKind::Class { .. },
            ) {
                self.types
                    .insert_borrowed(name.clone(), crate::mangle::PRELUDE_PACKAGE, sym);
            }
        }
        for (name, sym) in &prelude.values {
            let is_constructor_binding = matches!(
                &sym.kind,
                ValueKind::Const {
                    ty: Type::InterfaceRef { .. },
                    ..
                }
            );
            if is_constructor_binding || PUBLIC_PRELUDE_VALUES.contains(&name.as_str()) {
                self.top_symbols.insert(
                    name.clone(),
                    ValueEntry {
                        declaration_span: sym.declaration_span,
                        package_name: crate::mangle::PRELUDE_PACKAGE.to_string(),
                        symbol_name: sym.name.clone(),
                        kind: sym.kind.clone(),
                        mangled_name: sym.mangled_name.clone(),
                    },
                );
            }
        }
        load_namespaces(
            &mut self.types,
            &mut self.namespace_symbols,
            crate::mangle::PRELUDE_PACKAGE,
            &prelude.namespaces,
        );
        // Host-declared values key their map entries by dispatch key
        // (`submilli:prelude#isNaN`), so the name-keyed pass above misses
        // them — re-scan by symbol name for the public bindings.
        for (name, sym) in &prelude.values {
            if name != &sym.name && PUBLIC_PRELUDE_VALUES.contains(&sym.name.as_str()) {
                self.top_symbols.insert(
                    sym.name.clone(),
                    ValueEntry {
                        declaration_span: sym.declaration_span,
                        package_name: prelude.package_name.clone(),
                        symbol_name: name.clone(),
                        kind: sym.kind.clone(),
                        mangled_name: sym.mangled_name.clone(),
                    },
                );
            }
        }
        for host_name in [
            crate::runtime::NUMBER_MODULE_NAME,
            crate::runtime::TEMPORAL_MODULE_NAME,
        ] {
            let Some(defs) = self.packages_by_name.get(host_name).copied() else {
                continue;
            };
            for (name, sym) in &defs.values {
                if matches!(sym.kind, ValueKind::Function { .. }) {
                    self.top_symbols.insert(
                        name.clone(),
                        ValueEntry {
                            declaration_span: sym.declaration_span,
                            package_name: defs.package_name.clone(),
                            symbol_name: sym.name.clone(),
                            kind: sym.kind.clone(),
                            mangled_name: sym.mangled_name.clone(),
                        },
                    );
                }
            }
            load_namespaces(
                &mut self.types,
                &mut self.namespace_symbols,
                defs.package_name.as_str(),
                &defs.namespaces,
            );
        }
        Ok(())
    }

    /// Build the import-independent FQN type registry: every prelude, host, and
    /// loaded-package type. Runs after [`populate_prelude`](Self::populate_prelude)
    /// so the package set is final; user-declared types are added later, as
    /// [`signatures`](Self::signatures) registers them.
    pub(super) fn populate_type_registry(&mut self) {
        // Transitive dependencies register here and nowhere else — see
        // `compile_package_with_transitive` for why they must resolve without
        // becoming importable.
        let packages: Vec<&crate::PackageDeclaration> = self
            .packages_by_name
            .values()
            .chain(self.type_only_packages.values())
            .copied()
            .collect();
        for defs in packages {
            register_package_types(&mut self.type_registry, defs);
        }
    }

    pub(super) fn populate_imports(&mut self) -> Result<(), CompilerFailure> {
        let top_level: Vec<crate::StmtId> = self.ast.top_level.clone();
        for stmt_id in top_level {
            let stmt = self
                .ast
                .try_stmt(stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            let StmtKind::Import {
                module,
                module_span,
                kind,
                doc: _,
            } = stmt.kind
            else {
                continue;
            };

            if crate::source::is_relative_specifier(&module) {
                self.populate_relative_import(&module, module_span, &kind);
                continue;
            }

            let Some(&pkg) = self.packages_by_name.get(module.as_str()) else {
                if let Some(server) = module.strip_prefix("@mcp/") {
                    let help = self.available_mcp_servers_help();
                    self.error_with_help(
                        module_span,
                        format!(
                            "MCP server `{server}` is unavailable — `{module}` is absent from the discovered catalog; check the blueprint's `mcp:` block and discovery warnings"
                        ),
                        help,
                    );
                    continue;
                }
                if let Some(ns) = self.prelude_namespace_import(&module, &kind) {
                    if self.prelude_import_is_noop(&kind) {
                        continue;
                    }
                    self.error_with_help(
                        module_span,
                        format!("`{ns}` is built in — remove the import and use it directly"),
                        vec![format!(
                            "prelude namespaces ({}) are auto-imported into every module; never `import` them",
                            self.prelude_namespace_help_list()
                        )],
                    );
                    continue;
                }
                if let Some((message, help)) = crate::stdlib::unavailable_import(&module) {
                    self.error_with_help(module_span, message, help);
                    continue;
                }
                // In the closure but not declared here: the types resolve, the
                // import doesn't. Say which, or the reader sees "not found" for
                // a package they can watch being compiled beside this one.
                if self.type_only_packages.contains_key(module.as_str()) {
                    self.error_with_help(
                        module_span,
                        format!(
                            "package `{module}` is not a dependency of `{}`",
                            self.package_name
                        ),
                        vec![format!(
                            "`{module}` is in this package's dependency closure — a dependency \
                             depends on it — so its types resolve, but importing from it needs \
                             `{module}` in this package's own `dependencies`"
                        )],
                    );
                    continue;
                }
                let available = self.available_packages_help();
                self.error_with_help(
                    module_span,
                    format!("package `{module}` not found"),
                    available,
                );
                continue;
            };
            self.typed_ast
                .imported_packages
                .insert(pkg.package_name.clone());

            match kind {
                crate::ImportKind::Named(specs) => {
                    for spec in specs {
                        self.bind_named_import(pkg, &spec);
                    }
                }
                crate::ImportKind::Namespace { local_name } => {
                    self.bind_namespace_import(NamespaceMembers::from_package(pkg), &local_name);
                }
            }
        }

        Ok(())
    }

    fn populate_relative_import(
        &mut self,
        module: &str,
        module_span: crate::Span,
        kind: &crate::ImportKind,
    ) {
        if !self.package_inference {
            self.reject_relative_import(module, module_span);
            return;
        }
        let resolved = match self.module.resolve_relative(module) {
            Ok(path) => path,
            Err(crate::source::RelativeImportError::EscapesRoot) => {
                self.error_with_help(
                    module_span,
                    format!("import escapes package root: `{module}` climbs above the package root"),
                    vec![
                        "a relative import may not use `..` to leave the package; import a package specifier or a module at or below the root instead"
                            .to_string(),
                    ],
                );
                return;
            }
        };
        let Some(imported_module) = self.inferred_modules.get(&resolved).cloned() else {
            self.error_with_help(
                module_span,
                format!("no such module `{module}`"),
                self.available_modules_help(),
            );
            return;
        };
        match kind {
            crate::ImportKind::Named(specs) => {
                for spec in specs {
                    self.bind_named_relative_import(module, &imported_module, &resolved, spec);
                }
            }
            crate::ImportKind::Namespace { local_name } => {
                self.bind_namespace_import(
                    NamespaceMembers::from_module(self.package_name, &imported_module),
                    local_name,
                );
            }
        }
    }

    fn bind_named_relative_import(
        &mut self,
        specifier: &str,
        imported_module: &ModuleSymbols,
        resolved: &crate::ModulePath,
        spec: &crate::ImportSpecifier,
    ) {
        let imported = spec.imported_name.name.as_str();
        let local = &spec.local_name;
        let mut found = false;

        if let Some(val_sym) = imported_module.exported_value(imported) {
            found = true;
            if let Some(existing) = self.top_symbols.get(&local.name) {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: local.span,
                    message: format!(
                        "duplicate declaration of `{}` (import collides with an existing binding)",
                        local.name,
                    ),
                    help: vec![],
                    notes: vec![(
                        existing.declaration_span,
                        "previously declared here".to_string(),
                    )],
                });
            } else {
                self.top_symbols.insert(
                    local.name.clone(),
                    ValueEntry {
                        declaration_span: local.span,
                        package_name: self.package_name.to_string(),
                        symbol_name: imported.to_string(),
                        kind: val_sym.kind.clone(),
                        mangled_name: val_sym.mangled_name.clone(),
                    },
                );
            }
        }

        if let Some(type_sym) = imported_module.exported_type(imported) {
            found = true;
            if self.types.contains(&local.name) {
                self.error(
                    local.span,
                    format!(
                        "duplicate declaration of type `{}` (import collides with an existing type)",
                        local.name,
                    ),
                );
            } else {
                let mut sym = type_sym.clone();
                sym.name = local.name.clone();
                self.types
                    .insert(local.name.clone(), self.package_name.to_string(), sym);
            }
        }

        if found {
            return;
        }

        let is_private = self
            .inferred_modules
            .get(resolved)
            .is_some_and(|module| module.contains(imported));
        if is_private {
            self.error_with_help(
                spec.imported_name.span,
                format!("`{imported}` is private to `{specifier}`"),
                vec![format!(
                    "add `export` to `{imported}` in module `{}` to make it importable",
                    resolved.as_str(),
                )],
            );
            return;
        }

        self.error_with_help(
            spec.imported_name.span,
            format!("module `{specifier}` does not export `{imported}`"),
            imported_module.exports_help(),
        );
    }

    fn available_modules_help(&self) -> Vec<String> {
        let names: Vec<String> = self
            .inferred_modules
            .keys()
            .map(|p| format!("`{}`", p.as_str()))
            .collect();
        if names.is_empty() {
            Vec::new()
        } else {
            vec![format!("modules in this package: {}", names.join(", "))]
        }
    }

    /// Reject a relative-path import (`./util`, `../math`). Cross-module
    /// resolution doesn't exist yet, so a single-file program has no siblings to
    /// resolve against — but a `..` that climbs above the package root is a
    /// distinct structural error that wins over the no-siblings message. The
    /// importer is the root module (its directory is the package root, empty).
    fn reject_relative_import(&mut self, specifier: &str, span: crate::Span) {
        let importer = crate::source::ModulePath::from("");
        match importer.resolve_relative(specifier) {
            Err(crate::source::RelativeImportError::EscapesRoot) => self.error_with_help(
                span,
                format!(
                    "import escapes package root: `{specifier}` climbs above the package's top-level module"
                ),
                vec![
                    "a relative import may not use `..` to leave the package; import a \
                     package specifier (e.g. `submilli:http`) or a module at or below the \
                     root instead"
                        .to_string(),
                ],
            ),
            Ok(_canonical) => self.error_with_help(
                span,
                format!(
                    "this package has no sibling modules: `{specifier}` refers to another file, \
                     but this is a single-file program"
                ),
                vec![
                    "inline the code you need, or import from a package specifier \
                     (e.g. `submilli:http`); relative-path imports require a multi-file package"
                        .to_string(),
                ],
            ),
        }
    }

    /// The auto-imported prelude namespaces: those `populate_prelude` loaded into
    /// `namespace_symbols` (`Math`, `Temporal`, …) plus the `JSON` compiler
    /// intrinsic, which is recognized syntactically rather than as a namespace value.
    fn prelude_namespace_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.namespace_symbols.keys().map(String::as_str).collect();
        names.push("JSON");
        names
    }

    /// If an `import … from '<module>'` targets one of the built-in prelude
    /// namespaces, return its canonical name. Matches the module name
    /// case-insensitively (`'temporal'` → `Temporal`) and any named specifier
    /// exactly (`import { Temporal } from '…'`).
    fn prelude_namespace_import(&self, module: &str, kind: &crate::ImportKind) -> Option<String> {
        let names = self.prelude_namespace_names();
        if let Some(found) = names.iter().find(|n| n.eq_ignore_ascii_case(module)) {
            return Some((*found).to_string());
        }
        if let crate::ImportKind::Named(specs) = kind {
            for spec in specs {
                let imported = spec.imported_name.name.as_str();
                if let Some(found) = names.iter().find(|n| **n == imported) {
                    return Some((*found).to_string());
                }
            }
        }
        None
    }

    /// A prelude-namespace import is forgiven when every binding it introduces
    /// already exists under the same name — unaliased named specifiers, or a
    /// `* as` form whose local name is the canonical namespace. Aliases would
    /// need real binding machinery, so they still error.
    fn prelude_import_is_noop(&self, kind: &crate::ImportKind) -> bool {
        let names = self.prelude_namespace_names();
        match kind {
            crate::ImportKind::Named(specs) => specs.iter().all(|spec| {
                spec.local_name.name == spec.imported_name.name
                    && names.contains(&spec.imported_name.name.as_str())
            }),
            crate::ImportKind::Namespace { local_name } => {
                names.contains(&local_name.name.as_str())
            }
        }
    }

    fn prelude_namespace_help_list(&self) -> String {
        let mut names = self.prelude_namespace_names();
        names.sort_unstable();
        names
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn bind_named_import(&mut self, pkg: &'a PackageDeclaration, spec: &crate::ImportSpecifier) {
        let imported = spec.imported_name.name.as_str();
        let local = &spec.local_name;
        let mut found = false;

        if let Some(val_sym) = pkg.values.get(imported) {
            found = true;
            if let Some(existing) = self.top_symbols.get(&local.name) {
                let prev_span = existing.declaration_span;
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: local.span,
                    message: format!(
                        "duplicate declaration of `{}` (import collides with an existing binding)",
                        local.name,
                    ),
                    help: vec![],
                    notes: vec![(prev_span, "previously declared here".to_string())],
                });
            } else {
                // Another package's `let` can be rebound by its own functions.
                if matches!(val_sym.kind, ValueKind::Let { .. }) {
                    self.typed_ast
                        .rebindable_globals
                        .insert(val_sym.mangled_name.clone(), imported.to_string());
                }
                self.top_symbols.insert(
                    local.name.clone(),
                    ValueEntry {
                        declaration_span: local.span,
                        // `imported` is the real export name (pre-alias); `local` may
                        // rename it. Store the unaliased name + origin package.
                        package_name: pkg.package_name.clone(),
                        symbol_name: imported.to_string(),
                        kind: val_sym.kind.clone(),
                        mangled_name: val_sym.mangled_name.clone(),
                    },
                );
            }
        }

        if let Some(type_sym) = pkg.types.get(imported) {
            found = true;
            if self.types.contains(&local.name) {
                self.error(
                    local.span,
                    format!(
                        "duplicate declaration of type `{}` (import collides with an existing type)",
                        local.name,
                    ),
                );
            } else if local.name == imported {
                self.types
                    .insert_borrowed(local.name.clone(), pkg.package_name.as_str(), type_sym);
            } else {
                let mut sym = type_sym.clone();
                sym.name = local.name.clone();
                self.types
                    .insert(local.name.clone(), pkg.package_name.clone(), sym);
            }
        }

        if !found {
            let exports = self.package_exports_help(pkg);
            self.error_with_help(
                spec.imported_name.span,
                format!(
                    "package `{}` does not export `{}`",
                    pkg.package_name, imported,
                ),
                exports,
            );
        }
    }

    fn bind_namespace_import(&mut self, members: NamespaceMembers<'a>, local_name: &Ident) {
        if let Some(existing) = self.top_symbols.get(&local_name.name) {
            let prev_span = existing.declaration_span;
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: local_name.span,
                message: format!(
                    "duplicate declaration of `{}` (namespace import collides with an existing binding)",
                    local_name.name,
                ),
                help: vec![],
                notes: vec![(prev_span, "previously declared here".to_string())],
            });
            return;
        }
        if let Some(existing) = self.namespace_bindings.get(&local_name.name) {
            let prev_span = existing.declaration_span;
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: local_name.span,
                message: format!("duplicate namespace import `{}`", local_name.name,),
                help: vec![],
                notes: vec![(prev_span, "previously imported here".to_string())],
            });
            return;
        }
        self.namespace_bindings.insert(
            local_name.name.clone(),
            NamespaceBinding {
                members,
                declaration_span: local_name.span,
            },
        );
    }

    fn available_packages_help(&self) -> Vec<String> {
        if self.packages_by_name.is_empty() {
            return Vec::new();
        }
        let mut names: Vec<&str> = self.packages_by_name.keys().copied().collect();
        names.sort();
        vec![format!(
            "packages loaded for this compilation (not the full catalog): {}",
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
        ), "use your package-discovery tool to list available packages, or check the blueprint configuration".to_string()]
    }

    /// The compiler sees discovered packages, not the blueprint's declarations.
    fn available_mcp_servers_help(&self) -> Vec<String> {
        let mut servers: Vec<&str> = self
            .packages_by_name
            .keys()
            .filter_map(|n| n.strip_prefix("@mcp/"))
            .collect();
        servers.sort();
        if servers.is_empty() {
            return vec!["no MCP servers are currently available; declared servers may need authentication or may have failed discovery".to_string()];
        }
        vec![format!(
            "available MCP servers: {}",
            servers
                .iter()
                .map(|n| format!("`@mcp/{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
        )]
    }

    pub(super) fn package_exports_help(&self, pkg: &PackageDeclaration) -> Vec<String> {
        let mut names: Vec<&str> = pkg
            .values
            .keys()
            .chain(pkg.types.keys())
            .map(String::as_str)
            .collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            return Vec::new();
        }
        vec![format!(
            "exports: {}",
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::run;

    #[test]
    fn relative_import_in_single_file_is_rejected() {
        let (_ta, diags) = run("import { x } from \"./util\";\nfunction main(): void {}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("this package has no sibling modules")),
            "expected no-siblings diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn relative_import_escaping_root_is_rejected() {
        let (_ta, diags) = run("import { x } from \"../x\";\nfunction main(): void {}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("import escapes package root")),
            "expected escapes-root diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn parent_climb_escapes_before_sibling_check() {
        let (_ta, diags) = run("import { x } from \"../../x\";\nfunction main(): void {}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("escapes package root")),
            "expected escapes-root diagnostic, got: {diags:?}",
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("sibling")),
            "escape must win over the no-siblings message: {diags:?}",
        );
    }
}
