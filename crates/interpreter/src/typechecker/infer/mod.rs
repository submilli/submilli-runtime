//! Inference pass — produces the Typed AST.

mod assign_expr;
pub(crate) mod assignable;
mod binding_analysis;
mod classes;
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
    #[cfg(debug_assertions)]
    debug_assert_no_patterns(ast);
    let packages_by_name: BTreeMap<&'a str, &'a PackageDeclaration> = packages
        .iter()
        .map(|d| (d.package_name.as_str(), *d))
        .collect();
    let bindings = binding_analysis::analyze(ast);
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
        this_before_super: false,
        local_class_mangles: std::collections::BTreeSet::new(),
        pending_implements: Vec::new(),
        unresolved_parents: std::collections::BTreeSet::new(),
        current_type_predicate: None,
        inferred_returns: None,
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
    };
    tc.populate_prelude();
    tc.populate_type_registry();
    tc.populate_imports();
    if !tc.signatures() {
        return (tc.typed_ast, tc.diagnostics);
    }
    tc.infer_global_variables();
    tc.infer_functions();
    tc.infer_classes();
    tc.collect_exports();
    // The `&tc` borrow has to end before the `&mut tc.typed_ast` assignment.
    let shapes = shapes::collect(&tc.typed_ast, tc.resolver());
    tc.typed_ast.shapes = shapes;
    (tc.typed_ast, tc.diagnostics)
}

pub fn infer_package<'a>(
    package_name: &'a str,
    root_module: ModulePath,
    modules: Vec<(ModulePath, crate::FileId, &'a Ast)>,
    sources: &'a crate::Sources,
    external_packages: BTreeMap<String, PackageDeclaration>,
    transitive_packages: BTreeMap<String, PackageDeclaration>,
) -> (TypedAst, PackageDeclaration, Vec<Diagnostic>) {
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
        return (
            TypedAst::with_package(package_name),
            PackageDeclaration::with_package(package_name),
            diagnostics,
        );
    }

    let Some(order) = import_graph::topo_order(&module_map, &mut diagnostics) else {
        return (
            TypedAst::with_package(package_name),
            PackageDeclaration::with_package(package_name),
            diagnostics,
        );
    };

    let mut inferred_modules: BTreeMap<ModulePath, ModuleSymbols> = BTreeMap::new();
    let mut module_exports: BTreeMap<ModulePath, Vec<crate::ExportEntry>> = BTreeMap::new();
    let mut root_public_exports = Vec::new();
    let mut package_declaration = PackageDeclaration::with_package(package_name);
    let first_module = order
        .first()
        .expect("root existence implies at least one module");
    let (first_file, first_ast) = module_map
        .get(first_module)
        .copied()
        .expect("topo module exists");
    let first_source = sources
        .get(first_file)
        .map(|f| f.text.as_str())
        .unwrap_or_default();
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
        this_before_super: false,
        local_class_mangles: BTreeSet::new(),
        pending_implements: Vec::new(),
        unresolved_parents: BTreeSet::new(),
        current_type_predicate: None,
        inferred_returns: None,
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
    };

    for module in order {
        let (file, ast) = module_map
            .get(&module)
            .copied()
            .expect("topo module exists");
        let source = sources
            .get(file)
            .map(|f| f.text.as_str())
            .unwrap_or_default();
        let starts = ModuleTypedAstStarts::new(&tc.typed_ast);
        tc.reset_for_package_module(source, ast, module.clone(), &inferred_modules);
        tc.populate_prelude();
        tc.populate_type_registry();
        tc.populate_module_type_registry();
        tc.populate_imports();
        if !tc.signatures() {
            diagnostics.extend(tc.diagnostics);
            return (tc.typed_ast, package_declaration, diagnostics);
        }
        tc.infer_global_variables();
        tc.infer_functions();
        tc.infer_classes();
        for f in &tc.typed_ast.functions {
            if !f.generics.is_empty() {
                package_declaration
                    .runtime_generics
                    .insert(f.mangled_name.clone());
            }
        }
        diagnose_package_main_since(&tc.typed_ast, starts.functions, &mut tc.diagnostics);
        tc.resolve_package_export_statements();
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

    // The `&tc` borrow has to end before the `&mut tc.typed_ast` assignment.
    let shapes = shapes::collect(&tc.typed_ast, tc.resolver());
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
    tc.diagnostics
        .extend(crate::typechecker::rules::check_package(
            package_name,
            &tc.typed_ast,
            &package_declaration,
            &tc.typed_ast.exports,
            module_surfaces,
        ));
    diagnostics.extend(tc.diagnostics);
    (tc.typed_ast, package_declaration, diagnostics)
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
    for function in &ast.functions[functions_start..] {
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
    /// Set when `this` is accessed inside a subclass constructor *before*
    /// `super(...)` — flagged at the next `super(...)` call (and harmless once
    /// `super_seen`, since this-before-super is the only window that matters).
    pub(super) this_before_super: bool,
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
    pub(super) current_type_predicate: Option<(crate::TypePredicate, String)>,
    pub(super) inferred_returns: Option<Vec<(Type, Span)>>,
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
}

impl<'a> Inferer<'a> {
    pub(super) fn reset_for_package_module(
        &mut self,
        source: &'a str,
        ast: &'a Ast,
        module: ModulePath,
        inferred_modules: &BTreeMap<ModulePath, ModuleSymbols>,
    ) {
        #[cfg(debug_assertions)]
        debug_assert_no_patterns(ast);
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
        debug_assert!(
            self.pending_implements.is_empty(),
            "the signature pass must drain `implements` checks before the next module",
        );
        let bindings = binding_analysis::analyze(ast);
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
    }

    pub(super) fn populate_module_type_registry(&mut self) {
        for module in self.inferred_modules.values() {
            imports::register_module_types(&mut self.type_registry, module);
        }
    }

    pub(super) fn mangle_top_symbol(&self, name: &str) -> crate::MangledName {
        if !self.package_inference {
            return crate::mangle::package_symbol(self.package_name, name);
        }
        if self.module == self.root_module && self.is_exported_top_symbol(name) {
            crate::mangle::package_symbol(self.package_name, name)
        } else {
            crate::mangle::package_module_symbol(self.package_name, self.module.as_str(), name)
        }
    }

    fn is_exported_top_symbol(&self, name: &str) -> bool {
        self.ast.exported_decls.iter().any(|ed| {
            exports::exported_decl_name(&self.ast.stmt(ed.stmt).kind)
                .is_some_and(|exported| exported.name == name)
        })
    }

    pub(super) fn add_typed_global(&mut self, global: crate::TypedGlobal) {
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
            self.mark_direct_package_export(&global.name.name);
        }
        self.typed_ast.globals.push(global);
    }

    pub(super) fn add_typed_function(&mut self, function: crate::TypedFunction) {
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
            self.mark_direct_package_export(&function.name.name);
        }
        self.typed_ast.functions.push(function);
    }

    pub(super) fn add_typed_type_decl(
        &mut self,
        decl: crate::TypedTypeDecl,
        symbol: crate::TypeSymbol,
    ) {
        if self.package_inference {
            let name = symbol.name.clone();
            self.current_module_symbols
                .types
                .entry(name.clone())
                .or_insert((false, symbol));
            self.mark_direct_package_export(&name);
        }
        self.typed_ast.types.push(decl);
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

#[cfg(debug_assertions)]
fn debug_assert_no_patterns(ast: &Ast) {
    use crate::{ExprKind, StmtKind};
    for i in 0..ast.stmts_len() {
        let s = ast.stmt(crate::StmtId(i as u32));
        debug_assert!(
            !matches!(
                s.kind,
                StmtKind::LetPattern { .. }
                    | StmtKind::ConstPattern { .. }
                    | StmtKind::ForOfPattern { .. }
            ),
            "lower_patterns did not eliminate StmtKind::{} at stmt #{i}",
            match s.kind {
                StmtKind::LetPattern { .. } => "LetPattern",
                StmtKind::ConstPattern { .. } => "ConstPattern",
                StmtKind::ForOfPattern { .. } => "ForOfPattern",
                _ => "<unreachable>",
            },
        );
        if let StmtKind::Function { params, .. } = &s.kind {
            for (pi, p) in params.iter().enumerate() {
                debug_assert!(
                    p.pattern.is_none(),
                    "lower_patterns left a pattern on function param #{pi} of stmt #{i}",
                );
            }
        }
    }
    for i in 0..ast.exprs_len() {
        let e = ast.expr(crate::ExprId(i as u32));
        if let ExprKind::Arrow { params, .. } = &e.kind {
            for (pi, p) in params.iter().enumerate() {
                debug_assert!(
                    p.pattern.is_none(),
                    "lower_patterns left a pattern on arrow param #{pi} of expr #{i}",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{Asi, Sources, Token, TokenKind, lower_patterns, parse};

    use super::*;

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
                "/** Shared utility. */\nexport function shared(): number { return 1; }",
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
                "/** Shared utility. */\nexport function shared(): number { return 1; }",
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
                /** Public API. */
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
        let file = sources.add(module.to_string(), source.to_string());
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
        lower_patterns(&mut ast);
        (ModulePath::from(module), file, ast)
    }
}

#[cfg(test)]
mod test_support;
