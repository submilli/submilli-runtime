//! Inference pass — produces the Typed AST.

mod assign_expr;
pub(crate) mod assignable;
mod binding_analysis;
mod classes;
mod closure_arity;
mod diagnostics;
mod enums;
mod exports;
pub mod expr;
mod format_definition;
mod format_signature;
pub mod generic;
mod generic_scopes;
mod globals;
mod import_graph;
mod imports;
mod inference_sources;
mod literal_freshness;
mod lookup;
pub(in crate::typechecker) mod module_symbols;
mod namespace_symbol;
mod narrow_scopes;
pub mod narrowing;
mod nested_functions;
mod predicate_envs;
mod records;
mod reserved;
mod resolve_type;
mod scopes;
mod shapes;
mod signatures;
pub mod stmt;
mod switch_stmt;
pub(crate) mod type_aliases;
mod type_diff;
mod type_namespace;
mod type_predicate;
mod type_registry;
mod void_type_arguments;
mod void_value;

use std::collections::{BTreeMap, BTreeSet};

use module_symbols::{ModuleSymbols, NamespaceMembers, NamespaceSymbolSet};
use scopes::Scopes;
use type_namespace::TypeNamespace;
use type_registry::TypeRegistry;

use crate::compiler_error::{CompileError, CompilerFailure, CompilerStage};
use crate::{Ast, Diagnostic, ModulePath, PackageDeclaration, Span, Type, TypedAst, ValueKind};

pub fn infer<'a>(
    source: &'a str,
    package_name: &'a str,
    ast: &'a Ast,
    packages: &'a [&'a PackageDeclaration],
) -> (TypedAst, Vec<Diagnostic>) {
    infer_with_transitive(source, package_name, ast, packages, &[])
}

pub fn infer_with_transitive<'a>(
    source: &'a str,
    package_name: &'a str,
    ast: &'a Ast,
    packages: &'a [&'a PackageDeclaration],
    transitive: &'a [&'a PackageDeclaration],
) -> (TypedAst, Vec<Diagnostic>) {
    match infer_with_transitive_checked(source, package_name, ast, packages, transitive) {
        Ok(result) => result,
        Err(error) => (
            TypedAst::with_package(package_name),
            error.into_diagnostics(crate::FileId(0)),
        ),
    }
}

pub fn infer_with_transitive_checked<'a>(
    source: &'a str,
    package_name: &'a str,
    ast: &'a Ast,
    packages: &'a [&'a PackageDeclaration],
    transitive: &'a [&'a PackageDeclaration],
) -> Result<(TypedAst, Vec<Diagnostic>), CompileError> {
    let file = ast
        .source_statements()
        .first()
        .map(|stmt| stmt.span.file)
        .or_else(|| ast.source_expressions().first().map(|expr| expr.span.file))
        .unwrap_or(crate::FileId(0));
    ast.validate_source(source, file)
        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
    // Callers may supply an AST that did not come from `parse_checked`.
    crate::tree_height::check_syntax(ast)
        .map_err(|failure| failure.with_stage(CompilerStage::Infer))?;
    validate_lowered_patterns(ast)?;
    check_declaration_types(packages.iter().chain(transitive).copied())?;
    let packages_by_name: BTreeMap<&'a str, &'a PackageDeclaration> = packages
        .iter()
        .map(|d| (d.package_name.as_str(), *d))
        .collect();
    let bindings = binding_analysis::analyze(ast)?;
    let mut tc = Inferer {
        source,
        package_name,
        ast,
        typed_ast: TypedAst::with_package(package_name),
        diagnostics: bindings.diagnostics,
        top_symbols: BTreeMap::new(),
        types: TypeNamespace::new(),
        type_registry: TypeRegistry::new(),
        scopes: Scopes::default(),
        narrow_scopes: Vec::new(),
        assigned_scopes: Vec::new(),
        clause_write_scopes: Vec::new(),
        tombstone_scopes: Vec::new(),
        last_write_spans: std::collections::BTreeMap::new(),
        suspended_narrow_scopes: Vec::new(),
        pending_post_if_materializations: Vec::new(),
        pattern_sources: BTreeMap::new(),
        literal_freshness: literal_freshness::LiteralFreshness::default(),
        keeps_literal_types: false,
        returns_keep_literals: false,
        function_keeps_returned_literals: false,
        captured_mutators: bindings.mutators,
        last_assignments: bindings.last_assignments,
        nested_function_creation_points: bindings.nested_function_creation_points,
        nested_functions: Vec::new(),
        nested_function_bodies: Vec::new(),
        reachable: true,
        next_narrow_counter: 0,
        current_return: None,
        current_class: None,
        function_this: None,
        object_this_hint: None,
        current_static: None,
        current_super: None,
        in_constructor: false,
        super_seen: false,
        read_before_super: false,
        in_nested_function: false,
        super_call_is_statement: false,
        in_super_arguments: false,
        in_super_handler: false,
        local_class_mangles: std::collections::BTreeSet::new(),
        pending_implements: Vec::new(),
        unresolved_parents: std::collections::BTreeSet::new(),
        hidden_parent_constructors: std::collections::BTreeSet::new(),
        rejected_class_names: Default::default(),
        invalid_class_hierarchies: std::collections::BTreeSet::new(),
        current_type_predicate: None,
        inferred_returns: None,
        inference_source_literals: BTreeSet::new(),
        arguments_hinted_by_expected_result: BTreeSet::new(),
        object_argument_inference: None,
        generics_in_scope: Vec::new(),
        body_instantiations: Vec::new(),
        next_generic_param_id: 0,
        packages_by_name,
        type_only_packages: transitive
            .iter()
            .filter(|decl| {
                !packages
                    .iter()
                    .any(|direct| direct.package_name == decl.package_name)
            })
            .map(|decl| (decl.package_name.as_str(), *decl))
            .collect(),
        module: ModulePath::from(""),
        root_module: ModulePath::from(""),
        package_inference: false,
        current_module_symbols: ModuleSymbols::default(),
        current_module_exports: Vec::new(),
        current_export_seen: BTreeMap::new(),
        inferred_modules: BTreeMap::new(),
        namespace_bindings: BTreeMap::new(),
        namespace_symbols: BTreeMap::new(),
        loop_depth: 0,
        loop_invalidations: Default::default(),
        switch_depth: 0,
        pending_joins: Vec::new(),
        pending_aliases: BTreeMap::new(),
        pending_index_checks: None,
        field_narrowing_checks: BTreeMap::new(),
        alias_resolution_stack: Vec::new(),
        type_resolution_depth: 0,
        type_limits: Default::default(),
    };
    tc.populate_prelude().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    tc.populate_type_registry();
    tc.populate_imports().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    if !tc.signatures().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })? {
        crate::tree_height::check_typed(&tc.typed_ast, CompilerStage::Infer)
            .and_then(|()| tc.type_size_checkpoint(None))
            .map_err(|fatal| CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            })?;
        return Ok((tc.typed_ast, tc.diagnostics));
    }
    tc.infer_global_variables().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    tc.infer_functions().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    tc.infer_classes().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    tc.collect_exports().map_err(|fatal| CompileError {
        diagnostics: tc.diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    crate::tree_height::check_typed(&tc.typed_ast, CompilerStage::Infer).map_err(|fatal| {
        CompileError {
            diagnostics: tc.diagnostics.clone(),
            fatal: Some(fatal),
        }
    })?;
    // The `&tc` borrow has to end before the `&mut tc.typed_ast` assignment.
    // Shape collection resolves types under the limits too, so the last checkpoint
    // follows it.
    let shapes = shapes::collect(&tc.typed_ast, tc.resolver())
        .map_err(|failure| tc.pending_limit_or(failure))
        .and_then(|shapes| tc.type_size_checkpoint(None).map(|()| shapes))
        .map_err(|fatal| CompileError {
            diagnostics: tc.diagnostics.clone(),
            fatal: Some(fatal),
        })?;
    tc.typed_ast.shapes = shapes;
    Ok((tc.typed_ast, tc.diagnostics))
}

pub fn infer_package<'a>(
    package_name: &'a str,
    root_module: ModulePath,
    modules: Vec<(ModulePath, crate::FileId, &'a Ast)>,
    sources: &'a crate::Sources,
    external_packages: BTreeMap<String, PackageDeclaration>,
    transitive_packages: BTreeMap<String, PackageDeclaration>,
) -> (TypedAst, PackageDeclaration, Vec<Diagnostic>) {
    match infer_package_checked(
        package_name,
        root_module,
        modules,
        sources,
        external_packages,
        transitive_packages,
    ) {
        Ok(result) => result,
        Err(error) => (
            TypedAst::with_package(package_name),
            PackageDeclaration::with_package(package_name),
            error.into_diagnostics(crate::FileId(0)),
        ),
    }
}

pub fn infer_package_checked<'a>(
    package_name: &'a str,
    root_module: ModulePath,
    modules: Vec<(ModulePath, crate::FileId, &'a Ast)>,
    sources: &'a crate::Sources,
    external_packages: BTreeMap<String, PackageDeclaration>,
    transitive_packages: BTreeMap<String, PackageDeclaration>,
) -> Result<(TypedAst, PackageDeclaration, Vec<Diagnostic>), CompileError> {
    check_declaration_types(
        external_packages
            .values()
            .chain(transitive_packages.values()),
    )?;
    let mut diagnostics = Vec::new();
    let module_map: BTreeMap<ModulePath, (crate::FileId, &'a Ast)> = modules
        .into_iter()
        .map(|(path, file, ast)| (path, (file, ast)))
        .collect();
    if !module_map.contains_key(&root_module) {
        diagnostics.push(Diagnostic {
            severity: crate::Severity::Error,
            span: Span::at(crate::FileId(0)),
            message: format!("no such root module `{}`", root_module.as_str()),
            help: import_graph::available_modules_help_from_paths(module_map.keys()),
            notes: Vec::new(),
        });
        return Ok((
            TypedAst::with_package(package_name),
            PackageDeclaration::with_package(package_name),
            diagnostics,
        ));
    }

    // Graph diagnostics also consume import/export spans. Validate available
    // modules before graph traversal; missing sources still fail in inference
    // order so earlier module diagnostics remain available.
    for (file, ast) in module_map.values() {
        if let Some(source) = sources.get(*file) {
            ast.validate_source(source.text(), *file)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
        }
    }
    let Some(order) =
        import_graph::topo_order(&module_map, &mut diagnostics).map_err(|fatal| CompileError {
            diagnostics: diagnostics.clone(),
            fatal: Some(fatal),
        })?
    else {
        if module_map
            .values()
            .any(|(file, _)| sources.get(*file).is_none())
        {
            return Err(
                CompileError::from(inference_failure("module source is missing"))
                    .with_prior_diagnostics(&diagnostics),
            );
        }
        return Ok((
            TypedAst::with_package(package_name),
            PackageDeclaration::with_package(package_name),
            diagnostics,
        ));
    };

    let mut inferred_modules: BTreeMap<ModulePath, ModuleSymbols> = BTreeMap::new();
    let mut module_exports: BTreeMap<ModulePath, Vec<crate::ExportEntry>> = BTreeMap::new();
    let mut root_public_exports = Vec::new();
    let mut package_declaration = PackageDeclaration::with_package(package_name);
    let first_module = order.first().ok_or_else(|| {
        CompileError::from(inference_failure("root module absent from inference order"))
            .with_prior_diagnostics(&diagnostics)
    })?;
    let (first_file, first_ast) = module_map.get(first_module).copied().ok_or_else(|| {
        CompileError::from(inference_failure("module absent from inference map"))
            .with_prior_diagnostics(&diagnostics)
    })?;
    let first_source = sources
        .get(first_file)
        .map(crate::source::SourceFile::text)
        .ok_or_else(|| {
            CompileError::from(inference_failure("module source is missing"))
                .with_prior_diagnostics(&diagnostics)
        })?;
    let packages_by_name: BTreeMap<&str, &PackageDeclaration> = external_packages
        .values()
        .map(|d| (d.package_name.as_str(), d))
        .collect();
    // Declared by a dependency, not by this package: its types have to resolve
    // (a direct dependency's public surface can name them), but importing from
    // it stays an error — `packages_by_name` is what `populate_imports` reads.
    let type_only_packages: BTreeMap<&str, &PackageDeclaration> = transitive_packages
        .values()
        .map(|d| (d.package_name.as_str(), d))
        .filter(|(name, _)| !packages_by_name.contains_key(name))
        .collect();
    let mut tc = Inferer {
        source: first_source,
        package_name,
        ast: first_ast,
        typed_ast: TypedAst::with_package(package_name),
        diagnostics: Vec::new(),
        top_symbols: BTreeMap::new(),
        types: TypeNamespace::new(),
        type_registry: TypeRegistry::new(),
        scopes: Scopes::default(),
        narrow_scopes: Vec::new(),
        assigned_scopes: Vec::new(),
        clause_write_scopes: Vec::new(),
        tombstone_scopes: Vec::new(),
        last_write_spans: std::collections::BTreeMap::new(),
        suspended_narrow_scopes: Vec::new(),
        pending_post_if_materializations: Vec::new(),
        pattern_sources: BTreeMap::new(),
        literal_freshness: literal_freshness::LiteralFreshness::default(),
        keeps_literal_types: false,
        returns_keep_literals: false,
        function_keeps_returned_literals: false,
        captured_mutators: Default::default(),
        last_assignments: Default::default(),
        nested_function_creation_points: Default::default(),
        nested_functions: Vec::new(),
        nested_function_bodies: Vec::new(),
        reachable: true,
        next_narrow_counter: 0,
        current_return: None,
        current_class: None,
        function_this: None,
        object_this_hint: None,
        current_static: None,
        current_super: None,
        in_constructor: false,
        super_seen: false,
        read_before_super: false,
        in_nested_function: false,
        super_call_is_statement: false,
        in_super_arguments: false,
        in_super_handler: false,
        local_class_mangles: BTreeSet::new(),
        pending_implements: Vec::new(),
        unresolved_parents: BTreeSet::new(),
        hidden_parent_constructors: BTreeSet::new(),
        rejected_class_names: Default::default(),
        invalid_class_hierarchies: BTreeSet::new(),
        current_type_predicate: None,
        inferred_returns: None,
        inference_source_literals: BTreeSet::new(),
        arguments_hinted_by_expected_result: BTreeSet::new(),
        object_argument_inference: None,
        generics_in_scope: Vec::new(),
        body_instantiations: Vec::new(),
        next_generic_param_id: 0,
        packages_by_name,
        type_only_packages,
        module: first_module.clone(),
        root_module: root_module.clone(),
        package_inference: true,
        current_module_symbols: ModuleSymbols::default(),
        current_module_exports: Vec::new(),
        current_export_seen: BTreeMap::new(),
        inferred_modules: BTreeMap::new(),
        namespace_bindings: BTreeMap::new(),
        namespace_symbols: BTreeMap::new(),
        loop_depth: 0,
        loop_invalidations: Default::default(),
        switch_depth: 0,
        pending_joins: Vec::new(),
        pending_aliases: BTreeMap::new(),
        pending_index_checks: None,
        field_narrowing_checks: BTreeMap::new(),
        alias_resolution_stack: Vec::new(),
        type_resolution_depth: 0,
        type_limits: Default::default(),
    };

    for module in order {
        let (file, ast) = module_map.get(&module).copied().ok_or_else(|| {
            CompileError::from(inference_failure("module absent from inference map"))
                .with_prior_diagnostics(&tc.diagnostics)
                .with_prior_diagnostics(&diagnostics)
        })?;
        let source = sources
            .get(file)
            .map(crate::source::SourceFile::text)
            .ok_or_else(|| {
                CompileError::from(inference_failure("module source is missing"))
                    .with_prior_diagnostics(&tc.diagnostics)
                    .with_prior_diagnostics(&diagnostics)
            })?;
        ast.validate_source(source, file).map_err(|error| {
            CompileError::from(error.into_compiler_failure(CompilerStage::Infer))
                .with_prior_diagnostics(&tc.diagnostics)
                .with_prior_diagnostics(&diagnostics)
        })?;
        let starts = ModuleTypedAstStarts::new(&tc.typed_ast);
        tc.reset_for_package_module(source, ast, module.clone(), &inferred_modules)
            .map_err(|error| {
                error
                    .with_prior_diagnostics(&tc.diagnostics)
                    .with_prior_diagnostics(&diagnostics)
            })?;
        tc.populate_prelude().map_err(|fatal| {
            CompileError::from(fatal)
                .with_prior_diagnostics(&tc.diagnostics)
                .with_prior_diagnostics(&diagnostics)
        })?;
        tc.populate_type_registry();
        tc.populate_module_type_registry();
        tc.populate_imports().map_err(|fatal| {
            CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            }
            .with_prior_diagnostics(&diagnostics)
        })?;
        if !tc.signatures().map_err(|fatal| {
            CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            }
            .with_prior_diagnostics(&diagnostics)
        })? {
            tc.type_size_checkpoint(None).map_err(|fatal| {
                CompileError {
                    diagnostics: tc.diagnostics.clone(),
                    fatal: Some(fatal),
                }
                .with_prior_diagnostics(&diagnostics)
            })?;
            diagnostics.extend(tc.diagnostics);
            return Ok((tc.typed_ast, package_declaration, diagnostics));
        }
        tc.infer_global_variables().map_err(|fatal| {
            CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            }
            .with_prior_diagnostics(&diagnostics)
        })?;
        tc.infer_functions().map_err(|fatal| {
            CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            }
            .with_prior_diagnostics(&diagnostics)
        })?;
        tc.infer_classes().map_err(|fatal| {
            CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            }
            .with_prior_diagnostics(&diagnostics)
        })?;
        for f in &tc.typed_ast.functions {
            if !f.generics.is_empty() {
                package_declaration
                    .runtime_generics
                    .insert(f.mangled_name.clone());
            }
        }
        diagnose_package_main_since(&tc.typed_ast, starts.functions, &mut tc.diagnostics);
        // A limit recorded in this module must not be reported against the next.
        tc.resolve_package_export_statements()
            .and_then(|()| tc.type_size_checkpoint(None))
            .map_err(|fatal| {
                CompileError {
                    diagnostics: tc.diagnostics.clone(),
                    fatal: Some(fatal),
                }
                .with_prior_diagnostics(&diagnostics)
            })?;
        let module_symbols = std::mem::take(&mut tc.current_module_symbols);
        for symbol in module_symbols.all_types() {
            let mut symbol = symbol.clone();
            if let crate::TypeKind::Class {
                statics,
                static_fields,
                ..
            } = &mut symbol.kind
            {
                // Static entry points are source API, not instance layout metadata.
                statics.clear();
                static_fields.clear();
            }
            package_declaration
                .runtime_types
                .insert(symbol.mangled_name.to_string(), symbol);
        }
        let exports = std::mem::take(&mut tc.current_module_exports);
        if module == root_module {
            package_declaration.values = module_symbols.exported_value_map();
            package_declaration.types = module_symbols.exported_type_map();
            root_public_exports = exports.clone();
        }
        module_exports.insert(module.clone(), exports);
        inferred_modules.insert(module, module_symbols);
    }

    crate::tree_height::check_typed(&tc.typed_ast, CompilerStage::Infer).map_err(|fatal| {
        CompileError {
            diagnostics: tc.diagnostics.clone(),
            fatal: Some(fatal),
        }
        .with_prior_diagnostics(&diagnostics)
    })?;
    // The `&tc` borrow has to end before the `&mut tc.typed_ast` assignment.
    // Shape collection resolves types under the limits too, so the last checkpoint
    // follows it.
    let shapes = shapes::collect(&tc.typed_ast, tc.resolver())
        .map_err(|failure| tc.pending_limit_or(failure))
        .and_then(|shapes| tc.type_size_checkpoint(None).map(|()| shapes))
        .map_err(|fatal| {
            CompileError {
                diagnostics: tc.diagnostics.clone(),
                fatal: Some(fatal),
            }
            .with_prior_diagnostics(&diagnostics)
        })?;
    tc.typed_ast.shapes = shapes;
    for entry in &root_public_exports {
        if package_declaration.runtime_generics.contains(&entry.target)
            || tc
                .packages_by_name
                .values()
                .any(|defs| defs.runtime_generics.contains(&entry.target))
        {
            package_declaration
                .runtime_generics
                .insert(entry.public_name.clone());
        }
    }
    tc.typed_ast.exports = root_public_exports;
    package_declaration.refresh_shapes();
    let module_surfaces = module_exports.iter().filter_map(|(module, exports)| {
        inferred_modules
            .get(module)
            .map(|symbols| crate::typechecker::rules::PackageModuleSurface { symbols, exports })
    });
    let dependencies: Vec<&PackageDeclaration> = tc
        .packages_by_name
        .values()
        .chain(tc.type_only_packages.values())
        .copied()
        .collect();
    let rule_diagnostics = crate::typechecker::rules::check_package(
        package_name,
        &tc.typed_ast,
        &package_declaration,
        &tc.typed_ast.exports,
        module_surfaces,
        &dependencies,
    )
    .map_err(|error| {
        error
            .with_prior_diagnostics(&tc.diagnostics)
            .with_prior_diagnostics(&diagnostics)
    })?;
    tc.diagnostics.extend(rule_diagnostics);
    diagnostics.extend(tc.diagnostics);
    Ok((tc.typed_ast, package_declaration, diagnostics))
}

#[derive(Clone, Copy)]
struct ModuleTypedAstStarts {
    functions: usize,
}

impl ModuleTypedAstStarts {
    fn new(ast: &TypedAst) -> Self {
        Self {
            functions: ast.functions.len(),
        }
    }
}

fn diagnose_package_main_since(
    ast: &TypedAst,
    functions_start: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for function in ast.functions.iter().skip(functions_start) {
        if function.name.name == "main" {
            diagnostics.push(Diagnostic {
                severity: crate::Severity::Error,
                span: function.name.span,
                message: "packages cannot declare main".to_string(),
                help: vec![
                    "remove `main`; packages are libraries and export named declarations instead"
                        .to_string(),
                ],
                notes: Vec::new(),
            });
        }
    }
}

pub(super) struct ValueEntry {
    pub(super) declaration_span: Span,
    pub(super) kind: ValueKind,
    pub(super) mangled_name: crate::MangledName,
    /// Origin package of the symbol (e.g. `@mcp/linear`), for identifying the
    /// package without parsing `mangled_name`.
    pub(super) package_name: String,
    /// The symbol's own name in its package, *before* any `import { x as y }`
    /// alias — i.e. the real export name (the `tool` for an `@mcp` call).
    pub(super) symbol_name: String,
}

#[allow(dead_code)] // `package` consumed in Phase 6 (namespace member dispatch).
pub(super) struct NamespaceBinding<'a> {
    pub(super) members: NamespaceMembers<'a>,
    pub(super) declaration_span: Span,
}

pub(super) struct Inferer<'a> {
    /// Source expressions before synthetic destructuring annotations widen them.
    pattern_sources: BTreeMap<String, crate::ExprId>,
    literal_freshness: literal_freshness::LiteralFreshness,
    /// The next expression `infer_expr` infers keeps the literal type of a
    /// literal it is, or passes its value through from, without a hint asking
    /// for one: an unannotated `const`'s initializer. Read and cleared on
    /// entry, so it reaches only the operands that carry the value.
    keeps_literal_types: bool,
    /// Whether the unannotated function literal being inferred keeps the
    /// literal types of the values it returns (see
    /// `function_keeps_returned_literals`).
    returns_keep_literals: bool,
    /// The next expression `infer_expr` infers, when it is a function literal,
    /// keeps the literal types of the values it returns: it is the sole
    /// argument for a type parameter that is the call's result, as in tsc
    /// (`id(() => 42)` is `() => 42`). Read and cleared on entry like
    /// `keeps_literal_types`, so it doesn't reach a conditional's branches,
    /// whose function types couldn't form one callable union.
    function_keeps_returned_literals: bool,
    pub(super) source: &'a str,
    pub(super) package_name: &'a str,
    pub(super) ast: &'a Ast,
    pub(super) typed_ast: TypedAst,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) top_symbols: BTreeMap<String, ValueEntry>,
    pub(super) types: TypeNamespace<'a>,
    /// Import-independent FQN registry for *structural* resolution of already-typed
    /// values. See [`TypeRegistry`].
    pub(super) type_registry: TypeRegistry<'a>,
    pub(super) scopes: Scopes,
    pub(super) narrow_scopes: Vec<narrowing::NarrowEnv>,
    /// Bindings reassigned inside closures; narrowings on these paths are dropped (a closure could invalidate the narrowing between check and use).
    pub(super) captured_mutators: std::collections::HashSet<(String, Span)>,
    pub(super) last_assignments: std::collections::HashMap<Span, u32>,
    /// From the binding analysis: nested functions that capture a local of
    /// their block, by name span, with the last declared of those locals. See
    /// [`nested_functions`].
    pub(super) nested_function_creation_points: std::collections::HashMap<Span, crate::Ident>,
    pub(super) nested_functions: Vec<nested_functions::NestedFunction>,
    /// The nested functions whose bodies are being inferred, outermost first.
    pub(super) nested_function_bodies: Vec<usize>,
    pub(super) reachable: bool,
    /// All clause writes, including terminating branches, for exceptional entry.
    pub(super) clause_write_scopes: Vec<std::collections::BTreeSet<narrowing::ReferencePath>>,
    /// Writes carried by normal flow and used when joining branch exits.
    pub(super) assigned_scopes: Vec<std::collections::BTreeSet<narrowing::ReferencePath>>,
    pub(super) tombstone_scopes:
        Vec<std::collections::BTreeMap<narrowing::ReferencePath, narrowing::InvalidationReason>>,
    /// Where each path was last written, so a tombstone raised at a *join* can
    /// still point at the assignment rather than at the `if`/loop it escaped.
    /// Not scoped: only the most recent write to a path is ever wanted, and a
    /// tombstone that outlives it has nothing better to name.
    pub(super) last_write_spans: std::collections::BTreeMap<narrowing::ReferencePath, Span>,
    /// Enclosing narrowing state parked while inferring a closure body; see
    /// [`Inferer::enter_closure_narrow_boundary`].
    pub(super) suspended_narrow_scopes: Vec<narrow_scopes::SuspendedNarrowing>,
    /// Always drained by the enclosing block.
    pub(super) pending_post_if_materializations: Vec<narrowing::PendingPostIfMaterialization>,
    pub(super) next_narrow_counter: u32,
    /// `None` = no enclosing function OR unannotated arrow (disambiguated by `inferred_returns`).
    pub(super) current_return: Option<Type>,
    /// The enclosing class's `Type::ClassRef`, set only while checking a class
    /// method or constructor body. `this` resolves to it; `None` everywhere else
    /// (bare `this` is rejected).
    pub(super) current_class: Option<Type>,
    pub(super) function_this: Option<Type>,
    pub(super) object_this_hint: Option<Type>,
    /// `(class name, member name)` while checking a static method body or a
    /// static field initializer — there is no instance, so `this` gets a
    /// tailored diagnostic instead of the generic rejection.
    pub(super) current_static: Option<(String, String)>,
    /// The enclosing class's resolved `extends` clause (parent + type args),
    /// for `super(...)` and `super.method()`. `None` when the class has no
    /// parent.
    pub(super) current_super: Option<crate::ClassExtends>,
    /// True only inside a constructor body — gates `readonly` field writes and
    /// legalizes `super(...)`.
    pub(super) in_constructor: bool,
    /// Set when a subclass constructor body has called `super(...)`. Used to
    /// require exactly one call and reject a second.
    pub(super) super_seen: bool,
    /// Set when `this` or a `super` member is read inside a subclass
    /// constructor *before* `super(...)` — flagged at the next `super(...)`
    /// call (and harmless once `super_seen`, since before the call is the only
    /// window that matters).
    pub(super) read_before_super: bool,
    /// True inside an arrow or function expression or declaration, and reset
    /// by `check_constructor`: with `in_constructor`, it means the code is a
    /// function nested in the constructor rather than the constructor's own
    /// body. A `super(...)` there can run late or never.
    pub(super) in_nested_function: bool,
    /// Set by an expression statement that is a bare `super(...)` call, for
    /// `infer_super_call` to take. A call inside an expression can be skipped
    /// (`c ? super(1) : f()`), which the super-call rule can't see.
    pub(super) super_call_is_statement: bool,
    /// True while a `super(...)` call's arguments are inferred, when the
    /// instance they would read through `this` isn't built yet.
    pub(super) in_super_arguments: bool,
    /// True in the `catch` and `finally` of a `try` whose body calls
    /// `super(...)`: they also run when that call throws, before the instance
    /// is built.
    pub(super) in_super_handler: bool,
    /// Mangled names of classes declared in the *current* module. A `private`
    /// member is visible only when its class is in this set (module-scoped
    /// privacy); imported classes are absent, so their privates are hidden.
    pub(super) local_class_mangles: std::collections::BTreeSet<crate::MangledName>,
    /// `implements` clauses awaiting [`Inferer::check_pending_implements`],
    /// which drains them at the end of the same module's signature pass — the
    /// spans inside carry that module's `FileId`, so an entry must never
    /// outlive it.
    pub(super) pending_implements: Vec<classes::PendingImplements>,
    /// Classes whose `extends` clause named something whose members we cannot
    /// know — an unresolved type name, an interface, a non-class. The clause
    /// itself is already diagnosed; the entry keeps every *dependent*
    /// diagnostic inside such a class (and its subclasses) silent, since the
    /// missing parent could have declared the member that appears to be
    /// missing. Never cleared between modules: an importer of a broken class
    /// must stay silent too.
    pub(super) unresolved_parents: std::collections::BTreeSet<crate::MangledName>,
    /// Classes whose `extends` names a class with a constructor private to
    /// another module. The clause is diagnosed and the parent link kept, so
    /// members and subtyping still check; only the hidden constructor is left
    /// out: `super(...)` and an implicit constructor take any arguments. Never
    /// cleared between modules, like `unresolved_parents`.
    pub(super) hidden_parent_constructors: std::collections::BTreeSet<crate::MangledName>,
    /// Names whose signatures were skipped after source errors in this module.
    pub(super) rejected_class_names: BTreeSet<String>,
    /// Source-declared cycles already diagnosed during signature validation.
    pub(super) invalid_class_hierarchies: std::collections::BTreeSet<crate::MangledName>,
    pub(super) current_type_predicate: Option<(crate::TypePredicate, String)>,
    pub(super) inferred_returns: Option<Vec<(Type, Span)>>,
    /// Object literals inferred for their own type rather than checked against
    /// a declared one; see [`inference_sources`].
    pub(super) inference_source_literals: BTreeSet<crate::ExprId>,
    /// Call arguments whose expected type comes partly from the call's own
    /// expected result, so it guides their inference without being a
    /// requirement: an argument that doesn't fit it decides the type parameter
    /// instead.
    pub(super) arguments_hinted_by_expected_result: BTreeSet<crate::ExprId>,
    /// The object literal argument whose fields a generic call is inferring
    /// one at a time; see [`generic::ObjectArgumentInference`].
    pub(super) object_argument_inference: Option<generic::ObjectArgumentInference>,
    pub(super) generics_in_scope: Vec<Vec<String>>,
    /// Empty during the signature pass; populated with fresh `GenericParam` ids at body entry.
    pub(super) body_instantiations: Vec<BTreeMap<String, Type>>,
    pub(super) packages_by_name: BTreeMap<&'a str, &'a PackageDeclaration>,
    /// Dependencies of this package's dependencies. Their types register in the
    /// FQN registry so a direct dependency's public surface resolves, but they
    /// are absent from `packages_by_name`, so `import`ing one is still the
    /// "unknown package" error it should be.
    pub(super) type_only_packages: BTreeMap<&'a str, &'a PackageDeclaration>,
    pub(super) namespace_bindings: BTreeMap<String, NamespaceBinding<'a>>,
    pub(super) namespace_symbols: BTreeMap<String, NamespaceSymbolSet<'a>>,
    pub(super) loop_depth: u32,
    loop_invalidations: BTreeMap<crate::StmtId, Vec<stmt::LoopInvalidation>>,
    /// `break` accepted when `loop_depth + switch_depth > 0`; `continue` still requires `loop_depth > 0` (switch is transparent to continue).
    pub(super) switch_depth: u32,
    pub(super) pending_joins: Vec<narrowing::PendingJoinFrame>,
    /// Fresh id per `<T>` per body open; recursive calls reuse the body's ids (no re-allocation).
    pub(super) next_generic_param_id: u32,
    pub(super) module: ModulePath,
    pub(super) root_module: ModulePath,
    pub(super) package_inference: bool,
    pub(super) current_module_symbols: ModuleSymbols,
    pub(super) current_module_exports: Vec<crate::ExportEntry>,
    pub(super) current_export_seen: BTreeMap<String, Span>,
    pub(super) inferred_modules: BTreeMap<ModulePath, ModuleSymbols>,
    /// Type aliases whose name is forward-declared (placeholder in
    /// `types`) but whose body hasn't been resolved yet. Resolved on
    /// demand the first time a reference is encountered (driving
    /// forward + mutual references), then removed. Any still pending
    /// after the signatures walk are drained so declared-but-unused
    /// aliases still validate.
    pub(super) pending_aliases: BTreeMap<String, PendingAlias>,
    /// Index compatibility waits until all nominal signature placeholders are filled.
    pending_index_checks: Option<Vec<records::PendingIndexCheck>>,
    /// Produced by `check_field_redeclaration` in the signature pass, consumed
    /// when `infer_classes` builds the typed field. See
    /// [`crate::FieldNarrowingCheck`].
    pub(super) field_narrowing_checks:
        BTreeMap<(crate::MangledName, String), crate::FieldNarrowingCheck>,
    /// Names of aliases currently mid-resolution. A reference to a name
    /// already on this stack is a recursion **back-edge** — it resolves
    /// to a lazy [`Type::AliasRef`] instead of inlining the (not-yet-
    /// finished, and for a true cycle infinite) body.
    pub(super) alias_resolution_stack: Vec<String>,
    /// Nested annotation and alias-body resolutions in progress. Alias chains
    /// resolve recursively, so their length is bounded before the stack is.
    pub(super) type_resolution_depth: u32,
    /// Oversized types met where the code could not return an error; see
    /// [`Inferer::type_size_checkpoint`].
    pub(super) type_limits: crate::type_size::TypeLimits,
}

impl<'a> Inferer<'a> {
    /// Fails with a type limit recorded since the last checkpoint, reported at
    /// `span`: the source being inferred when an oversized type was met where
    /// no error could be returned.
    pub(super) fn type_size_checkpoint(&self, span: Option<Span>) -> Result<(), CompilerFailure> {
        self.type_limits
            .take()
            .map_err(|exceeded| exceeded.into_failure(CompilerStage::Infer, span))
    }

    /// A recorded type limit in place of `internal`: once a limit is recorded,
    /// a walk that stopped early or a phase that failed may have met the
    /// `Type::Error` stand-in rather than a real inconsistency.
    pub(super) fn pending_limit_or(&self, internal: CompilerFailure) -> CompilerFailure {
        match self.type_size_checkpoint(None) {
            Err(limit) => limit,
            Ok(()) => internal,
        }
    }

    pub(super) fn reset_for_package_module(
        &mut self,
        source: &'a str,
        ast: &'a Ast,
        module: ModulePath,
        inferred_modules: &BTreeMap<ModulePath, ModuleSymbols>,
    ) -> Result<(), CompileError> {
        // Callers may supply ASTs that did not come from `parse_checked`.
        crate::tree_height::check_syntax(ast)
            .map_err(|failure| failure.with_stage(CompilerStage::Infer))?;
        validate_lowered_patterns(ast)?;
        self.source = source;
        self.ast = ast;
        self.module = module;
        self.current_module_symbols = ModuleSymbols::default();
        self.current_module_exports.clear();
        self.current_export_seen.clear();
        self.inferred_modules = inferred_modules.clone();
        self.top_symbols.clear();
        self.types = TypeNamespace::new();
        self.type_registry = TypeRegistry::new();
        self.scopes = Scopes::default();
        self.narrow_scopes.clear();
        self.loop_invalidations.clear();
        self.assigned_scopes.clear();
        self.clause_write_scopes.clear();
        self.tombstone_scopes.clear();
        self.last_write_spans.clear();
        self.suspended_narrow_scopes.clear();
        self.pending_post_if_materializations.clear();
        if !self.pending_implements.is_empty() {
            return Err(inference_failure("signature pass left pending implements checks").into());
        }
        let bindings = binding_analysis::analyze(ast)?;
        self.captured_mutators = bindings.mutators;
        self.last_assignments = bindings.last_assignments;
        self.nested_function_creation_points = bindings.nested_function_creation_points;
        self.nested_functions.clear();
        self.diagnostics.extend(bindings.diagnostics);
        self.reachable = true;
        self.next_narrow_counter = 0;
        self.current_return = None;
        self.current_class = None;
        self.current_super = None;
        self.in_constructor = false;
        self.local_class_mangles.clear();
        self.current_type_predicate = None;
        self.inferred_returns = None;
        self.generics_in_scope.clear();
        self.body_instantiations.clear();
        self.next_generic_param_id = 0;
        self.namespace_bindings.clear();
        self.namespace_symbols.clear();
        self.loop_depth = 0;
        self.switch_depth = 0;
        self.pending_joins.clear();
        self.pending_aliases.clear();
        self.alias_resolution_stack.clear();
        Ok(())
    }

    pub(super) fn populate_module_type_registry(&mut self) {
        for module in self.inferred_modules.values() {
            imports::register_module_types(&mut self.type_registry, module);
        }
    }

    pub(super) fn mangle_top_symbol(
        &self,
        name: &str,
    ) -> Result<crate::MangledName, crate::compiler_error::CompilerFailure> {
        if !self.package_inference {
            return Ok(crate::mangle::package_symbol(self.package_name, name));
        }
        Ok(
            if self.module == self.root_module && self.is_exported_top_symbol(name)? {
                crate::mangle::package_symbol(self.package_name, name)
            } else {
                crate::mangle::package_module_symbol(self.package_name, self.module.as_str(), name)
            },
        )
    }

    fn is_exported_top_symbol(
        &self,
        name: &str,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        for ed in &self.ast.exported_decls {
            let stmt = self.ast.try_stmt(ed.stmt).map_err(arena_failure)?;
            if exports::exported_decl_name(&stmt.kind).is_some_and(|exported| exported.name == name)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn add_typed_global(
        &mut self,
        global: crate::TypedGlobal,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if self.package_inference {
            let symbol = crate::ValueSymbol {
                name: global.name.name.clone(),
                mangled_name: global.mangled_name.clone(),
                declaration_span: global.name.span,
                kind: match global.kind {
                    crate::GlobalKind::Let => ValueKind::Let {
                        ty: global.ty.clone(),
                        doc: global.doc.clone(),
                    },
                    crate::GlobalKind::Const => ValueKind::Const {
                        ty: global.ty.clone(),
                        doc: global.doc.clone(),
                    },
                },
            };
            self.current_module_symbols
                .values
                .entry(symbol.name.clone())
                .or_insert((false, symbol));
            self.mark_direct_package_export(&global.name.name)?;
        }
        self.typed_ast.globals.push(global);

        Ok(())
    }

    pub(super) fn add_typed_function(
        &mut self,
        function: crate::TypedFunction,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if self.package_inference {
            let symbol = crate::ValueSymbol {
                name: function.name.name.clone(),
                mangled_name: function.mangled_name.clone(),
                declaration_span: function.name.span,
                kind: ValueKind::Function {
                    generics: function.generics.clone(),
                    params: function
                        .params
                        .iter()
                        .map(crate::package_declaration::param_from_typed)
                        .collect(),
                    ret: function.return_type.clone(),
                    type_predicate: function.type_predicate.clone(),
                    doc: function.doc.clone(),
                },
            };
            self.current_module_symbols
                .values
                .entry(symbol.name.clone())
                .or_insert((false, symbol));
            self.mark_direct_package_export(&function.name.name)?;
        }
        self.typed_ast.functions.push(function);

        Ok(())
    }

    pub(super) fn add_typed_type_decl(
        &mut self,
        decl: crate::TypedTypeDecl,
        symbol: crate::TypeSymbol,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if self.package_inference {
            let name = symbol.name.clone();
            self.current_module_symbols
                .types
                .entry(name.clone())
                .or_insert((false, symbol));
            self.mark_direct_package_export(&name)?;
        }
        self.typed_ast.types.push(decl);

        Ok(())
    }
}

/// A type alias whose name is registered but whose body resolution is
/// deferred until first reference. Holds everything
/// [`Inferer::resolve_alias_body`] needs to resolve it in isolation.
pub(super) struct PendingAlias {
    pub(super) name: crate::Ident,
    pub(super) generics: Vec<String>,
    pub(super) annotation: crate::TypeAnnotation,
    pub(super) doc: Option<crate::DocComment>,
}

// Without this, `super::assignable` names the module, not the function.
pub(in crate::typechecker::infer) use assignable::assignable;

fn inference_failure(message: &str) -> CompilerFailure {
    CompilerFailure::Internal {
        stage: CompilerStage::Infer,
        span: None,
        message: message.into(),
    }
}

fn validate_lowered_patterns(ast: &Ast) -> Result<(), CompileError> {
    use crate::{ExprKind, StmtKind};
    for stmt in ast.source_statements() {
        let unlowered = match &stmt.kind {
            StmtKind::LetPattern { .. }
            | StmtKind::ConstPattern { .. }
            | StmtKind::ForOfPattern { .. } => true,
            StmtKind::Function { params, .. } => params.iter().any(|param| param.pattern.is_some()),
            _ => false,
        };
        if unlowered {
            return Err(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                span: Some(stmt.span),
                message: "pattern lowering left an unlowered statement or parameter".into(),
            }
            .into());
        }
    }
    for expr in ast.source_expressions() {
        if let ExprKind::Arrow { params, .. } = &expr.kind
            && params.iter().any(|param| param.pattern.is_some())
        {
            return Err(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                span: Some(expr.span),
                message: "pattern lowering left an unlowered closure parameter".into(),
            }
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{Asi, Sources, Token, TokenKind, lower_patterns, parse};

    use super::*;

    #[test]
    fn missing_prelude_is_a_typed_inference_failure() {
        let ast = Ast::new();
        let error = infer_with_transitive_checked("", "main", &ast, &[], &[])
            .expect_err("prelude metadata is required");
        assert!(matches!(
            error.fatal,
            Some(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                ..
            })
        ));
        assert!(error.to_string().contains("prelude package declaration"));
    }

    #[test]
    fn later_module_failure_keeps_earlier_diagnostics() {
        let mut sources = Sources::new();
        let (path, file, ast) = parse_module(
            &mut sources,
            "a",
            "export function bad(): number { return false; }",
        );
        let (_, _, expected) = infer_package_checked(
            "test",
            path.clone(),
            vec![(path.clone(), file, &ast)],
            &sources,
            runtime_external_packages(),
            BTreeMap::new(),
        )
        .expect("ordinary diagnostics");
        assert!(
            expected
                .iter()
                .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
        );
        assert!(
            expected
                .iter()
                .any(|diagnostic| diagnostic.severity == crate::Severity::Warning)
        );

        let missing = Ast::new();
        let error = infer_package_checked(
            "test",
            path.clone(),
            vec![
                (path, file, &ast),
                (ModulePath::from("b"), crate::FileId(999), &missing),
            ],
            &sources,
            runtime_external_packages(),
            BTreeMap::new(),
        )
        .expect_err("later module has no source");
        // Export-documentation warnings are generated only after all modules
        // finish. The first module's already-emitted type error must survive.
        let expected: Vec<_> = expected
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == crate::Severity::Error)
            .collect();
        assert_eq!(error.diagnostics, expected);
        assert!(error.to_string().contains("module source is missing"));
    }

    fn runtime_external_packages() -> BTreeMap<String, PackageDeclaration> {
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut packages = BTreeMap::new();
        for defs in prelude_defs {
            packages.insert(defs.package_name.clone(), defs.clone());
        }
        for defs in host_defs {
            packages.insert(defs.package_name.clone(), defs.clone());
        }
        packages
    }

    #[test]
    fn package_declaration_is_root_lib_surface_only() {
        let mut sources = Sources::new();
        let modules = [
            parse_module(
                &mut sources,
                "a",
                r#"export function shared(): string { return "wrong"; }"#,
            ),
            parse_module(
                &mut sources,
                "lib",
                r#"export { shared as api } from "./util";"#,
            ),
            parse_module(
                &mut sources,
                "util",
                "/** Shared utility.\n * @returns One. */\nexport function shared(): number { return 1; }",
            ),
        ];
        let module_refs: Vec<_> = modules
            .iter()
            .map(|(module, file, ast)| (module.clone(), *file, ast))
            .collect();

        let (typed_ast, package, diagnostics) = infer_package(
            "main",
            ModulePath::from("lib"),
            module_refs,
            &sources,
            runtime_external_packages(),
            BTreeMap::new(),
        );

        assert!(diagnostics.is_empty(), "unexpected diags: {diagnostics:?}");
        assert_eq!(
            typed_ast.exports,
            vec![crate::ExportEntry {
                public_name: crate::mangle::package_symbol("main", "api"),
                target: crate::mangle::package_module_symbol("main", "util", "shared"),
                kind: crate::ExportKind::Function,
                span: typed_ast.exports[0].span,
            }],
        );
        assert_eq!(
            package.values.keys().collect::<Vec<_>>(),
            vec![&"api".to_string()],
        );
        let api = package.values.get("api").expect("api exported");
        let ValueKind::Function { ret, .. } = &api.kind else {
            panic!("api should be a function");
        };
        assert_eq!(ret, &Type::Number);
    }

    #[test]
    fn package_reexport_chain_targets_original_module_symbol() {
        let mut sources = Sources::new();
        let modules = [
            parse_module(
                &mut sources,
                "internal",
                "/** Shared utility.\n * @returns One. */\nexport function shared(): number { return 1; }",
            ),
            parse_module(&mut sources, "lib", r#"export { shared } from "./util";"#),
            parse_module(
                &mut sources,
                "util",
                r#"export { shared } from "./internal";"#,
            ),
        ];
        let module_refs: Vec<_> = modules
            .iter()
            .map(|(module, file, ast)| (module.clone(), *file, ast))
            .collect();

        let (typed_ast, _package, diagnostics) = infer_package(
            "main",
            ModulePath::from("lib"),
            module_refs,
            &sources,
            runtime_external_packages(),
            BTreeMap::new(),
        );

        assert!(diagnostics.is_empty(), "unexpected diags: {diagnostics:?}");
        assert_eq!(
            typed_ast.exports,
            vec![crate::ExportEntry {
                public_name: crate::mangle::package_symbol("main", "shared"),
                target: crate::mangle::package_module_symbol("main", "internal", "shared"),
                kind: crate::ExportKind::Function,
                span: typed_ast.exports[0].span,
            }],
        );
    }

    #[test]
    fn package_declaration_shapes_are_export_surface_only() {
        let mut sources = Sources::new();
        let modules = [
            parse_module(
                &mut sources,
                "lib",
                r#"
                /**
                 * Public API.
                 * @returns The public shape.
                 */
                export function api(): { public: number } {
                    return { public: 1 };
                }
                let privateLocal: { private: string } = { private: "x" };
                "#,
            ),
            parse_module(
                &mut sources,
                "util",
                r#"
                export function internal(): { hidden: boolean } {
                    return { hidden: true };
                }
                "#,
            ),
        ];
        let module_refs: Vec<_> = modules
            .iter()
            .map(|(module, file, ast)| (module.clone(), *file, ast))
            .collect();

        let (_typed_ast, package, diagnostics) = infer_package(
            "main",
            ModulePath::from("lib"),
            module_refs,
            &sources,
            runtime_external_packages(),
            BTreeMap::new(),
        );

        assert!(diagnostics.is_empty(), "unexpected diags: {diagnostics:?}");
        let object_shapes: Vec<_> = package
            .shapes
            .iter()
            .filter_map(|shape| match shape {
                crate::Shape::Object { fields, .. } => Some(fields),
                _ => None,
            })
            .collect();
        assert_eq!(object_shapes.len(), 1);
        assert!(object_shapes[0].contains_key("public"));
        assert!(!object_shapes[0].contains_key("private"));
        assert!(!object_shapes[0].contains_key("hidden"));
    }

    fn parse_module(
        sources: &mut Sources,
        module: &str,
        source: &str,
    ) -> (ModulePath, crate::FileId, Ast) {
        let file = sources.add(module.to_string(), source).unwrap();
        let mut asi = Asi::new(source, file);
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let lex_diags = asi.into_diagnostics();
        assert!(
            lex_diags.is_empty(),
            "unexpected lexer diags: {lex_diags:?}"
        );
        let (mut ast, parse_diags) = parse(source, tokens, file);
        assert!(
            parse_diags.is_empty(),
            "unexpected parser diags: {parse_diags:?}",
        );
        ast = lower_patterns(ast).unwrap();
        (ModulePath::from(module), file, ast)
    }
}

#[cfg(test)]
mod test_support;

fn arena_failure(error: crate::arena::ArenaError) -> CompilerFailure {
    error.into_compiler_failure(CompilerStage::Infer)
}

/// Rejects a dependency declaration holding a type beyond the type limits
/// before any recursive pass reads it.
fn check_declaration_types<'d>(
    declarations: impl IntoIterator<Item = &'d PackageDeclaration>,
) -> Result<(), CompilerFailure> {
    for declaration in declarations {
        declaration
            .check_type_limits()
            .map_err(|exceeded| CompilerFailure::Limit {
                stage: CompilerStage::Infer,
                span: None,
                message: format!(
                    "package `{}` declares a type beyond a compiler limit: {exceeded}",
                    declaration.package_name
                ),
                help: vec![
                    "rebuild the package with a compiler that enforces the same limits".into(),
                ],
            })?;
    }
    Ok(())
}

/// Converts a type limit met where no source position is at hand; an enclosing
/// caller adds one with `with_span`, such as `infer_expr` or the interface
/// validation around member comparisons.
pub(super) fn type_limit_unlocated(exceeded: crate::type_size::TypeTooLarge) -> CompilerFailure {
    exceeded.into_failure(CompilerStage::Infer, None)
}

/// Converts a type limit met while inferring the source at `span`.
pub(super) fn type_limit_at(
    span: Span,
) -> impl FnOnce(crate::type_size::TypeTooLarge) -> CompilerFailure {
    move |exceeded| exceeded.into_failure(CompilerStage::Infer, Some(span))
}
