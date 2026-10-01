use crate::{
    Asi, Diagnostic, PackageDeclaration, Token, TokenKind, Type, TypedAst, TypedStmtKind, parse,
};

use super::infer;

pub(super) fn run(source: &str) -> (TypedAst, Vec<Diagnostic>) {
    run_with_packages(source, &[])
}

/// Like [`run`], but threads a set of imported packages so tests can exercise
/// `import ns from "<pkg>"` resolution against a synthetic [`PackageDeclaration`].
pub(super) fn run_with_packages(
    source: &str,
    packages: &[&PackageDeclaration],
) -> (TypedAst, Vec<Diagnostic>) {
    let mut asi = Asi::new(source, crate::FileId(0));
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
    let (ast, parse_diags) = parse(source, tokens, crate::FileId(0));
    assert!(
        parse_diags.is_empty(),
        "unexpected parser diags: {parse_diags:?}"
    );
    let packages = runtime_packages(packages);
    infer(source, "main", &ast, &packages)
}

fn runtime_packages<'a>(packages: &[&'a PackageDeclaration]) -> Vec<&'a PackageDeclaration> {
    let (prelude_defs, host_defs, _) =
        crate::runtime::prelude::cached_runtime_package_declarations();
    let mut out = Vec::with_capacity(1 + host_defs.len() + packages.len());
    out.extend(prelude_defs.iter());
    out.extend(host_defs.iter());
    out.extend(packages.iter().copied());
    out
}

pub(super) fn run_clean(source: &str) -> TypedAst {
    let (ta, diags) = run(source);
    assert!(diags.is_empty(), "unexpected infer diags: {diags:?}");
    ta
}

pub(super) fn nth_decl_value_ty(ta: &TypedAst, n: usize) -> Type {
    let stmt_id = ta.top_level_statements[n];
    let value_id = match &ta.try_stmt(stmt_id).unwrap().kind {
        TypedStmtKind::AssignGlobal { value, .. } => *value,
        other => panic!("expected AssignGlobal, got {other:?}"),
    };
    ta.try_expr(value_id).unwrap().ty.clone()
}

pub(super) fn nth_expr_stmt_ty(ta: &TypedAst, n: usize) -> Type {
    let stmt_id = ta.top_level_statements[n];
    let expr_id = match &ta.try_stmt(stmt_id).unwrap().kind {
        TypedStmtKind::Expr(e) => *e,
        other => panic!("expected expr stmt, got {other:?}"),
    };
    ta.try_expr(expr_id).unwrap().ty.clone()
}

/// A fresh inference context for invalid-state tests; mutations stay inside the closure.
pub(super) fn with_inferer(test: impl FnOnce(&mut super::Inferer<'_>)) {
    with_source_inferer("", test);
}

pub(super) fn with_source_inferer(source: &str, test: impl FnOnce(&mut super::Inferer<'_>)) {
    use std::collections::BTreeMap;
    let package_name = "test";
    let mut asi = Asi::new(source, crate::FileId(0));
    let mut tokens = Vec::new();
    loop {
        let token = asi.next_token();
        let eof = matches!(token.kind, TokenKind::Eof);
        tokens.push(token);
        if eof {
            break;
        }
    }
    assert!(asi.into_diagnostics().is_empty());
    let (ast, diagnostics) = parse(source, tokens, crate::FileId(0));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let ast = crate::lower_patterns::lower(ast).unwrap();
    let packages = runtime_packages(&[]);
    let transitive: &[&PackageDeclaration] = &[];
    let packages_by_name = packages
        .iter()
        .map(|declaration| (declaration.package_name.as_str(), *declaration))
        .collect();
    let bindings = super::binding_analysis::analyze(&ast).unwrap();
    let mut tc = super::Inferer {
        source,
        package_name,
        ast: &ast,
        typed_ast: crate::TypedAst::with_package(package_name),
        diagnostics: bindings.diagnostics,
        top_symbols: BTreeMap::new(),
        types: super::TypeNamespace::new(),
        type_registry: super::TypeRegistry::new(),
        scopes: super::Scopes::default(),
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
        read_before_super: false,
        in_nested_function: false,
        super_call_is_statement: false,
        in_super_arguments: false,
        in_super_handler: false,
        local_class_mangles: std::collections::BTreeSet::new(),
        pending_implements: Vec::new(),
        unresolved_parents: std::collections::BTreeSet::new(),
        rejected_class_names: Default::default(),
        invalid_class_hierarchies: std::collections::BTreeSet::new(),
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
        module: crate::ModulePath::from(""),
        root_module: crate::ModulePath::from(""),
        package_inference: false,
        current_module_symbols: super::ModuleSymbols::default(),
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
    tc.populate_prelude().unwrap();
    test(&mut tc);
}
