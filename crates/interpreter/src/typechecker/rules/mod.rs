//! Check (Rules) pass — semantic rules over the Typed AST.
//!
//! Read-only: produces additional diagnostics, never modifies the tree.

pub(crate) mod body_walk;
mod capability_consistency;
mod check_calls;
mod check_discipline;
mod control_flow;
mod declarations;
mod definite_assignment;
mod doc_consistency;
mod exported_docs;
pub(crate) mod exported_signature_types;
mod fallthrough;
pub(crate) mod finalizers;
mod main_required;
mod missing_return;
mod return_outside_function;
mod super_call;
mod unreachable;

use crate::compiler_error::{CompileError, CompilerFailure, CompilerStage};
use crate::{Diagnostic, PackageDeclaration, Type, TypedAst, tree_height};

use declarations::TypeDeclarations;

/// [`check_script`] for a script whose types are all its own: one imported
/// from a package cannot be resolved here.
pub fn check(ta: &TypedAst) -> Result<Vec<Diagnostic>, CompileError> {
    check_script(ta, &[])
}

/// Checks a script. `dependencies` are the packages it was inferred against,
/// which declare the types it imports.
///
/// The tree must satisfy [`TypedAst`]'s phase input contract. Dependency type
/// limits are checked again because callers may have modified declarations
/// since inference.
pub fn check_script(
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<Diagnostic>, CompileError> {
    tree_height::check_typed(ta, CompilerStage::Infer)?;
    super::infer::check_declaration_types(dependencies.iter().copied())?;
    let declarations = TypeDeclarations::for_script(ta, dependencies);
    let mut diags = Vec::new();
    run_rules(ta, &declarations, Source::Script, true, &mut diags).map_err(|fatal| {
        CompileError {
            diagnostics: diags.clone(),
            fatal: Some(fatal),
        }
    })?;
    Ok(diags)
}

/// Checks only the rules specific to Submilli on a script typed by either front end.
pub fn check_script_submilli(
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<Diagnostic>, CompileError> {
    tree_height::check_typed(ta, CompilerStage::Infer)?;
    super::infer::check_declaration_types(dependencies.iter().copied())?;
    let declarations = TypeDeclarations::for_script(ta, dependencies);
    let mut diags = Vec::new();
    run_rules(ta, &declarations, Source::Script, false, &mut diags).map_err(|fatal| {
        CompileError {
            diagnostics: diags.clone(),
            fatal: Some(fatal),
        }
    })?;
    Ok(diags)
}

/// The type a `@capability` binding path reads from a parameter of type `ty`
/// in `package`, resolved as the capability-consistency rule resolves it.
/// `None` when a segment names no field or reaches an unknown declaration.
/// `ty` must satisfy [`crate::type_size::check`], and all declarations must pass
/// [`PackageDeclaration::check_type_limits`] since their last mutation; `None` is
/// not a validation failure.
pub fn capability_binding_type(
    package: &PackageDeclaration,
    dependencies: &[&PackageDeclaration],
    ty: &Type,
    path: &[String],
) -> Option<Type> {
    let declarations = TypeDeclarations::for_package(package, dependencies);
    match capability_consistency::binding_target(&declarations, ty, path) {
        capability_consistency::BindingTarget::Found(ty) => Some(ty),
        capability_consistency::BindingTarget::Missing(_)
        | capability_consistency::BindingTarget::Unresolved => None,
    }
}

/// Checks a package. `dependencies` are the packages it was inferred against,
/// the ones it cannot import from included.
pub fn check_package(
    typed_ast: &TypedAst,
    package: &PackageDeclaration,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<Diagnostic>, CompileError> {
    check_package_rules(
        typed_ast,
        package,
        dependencies,
        true,
        &crate::type_size::TypeLimits::default(),
    )
}

/// Checks only the rules specific to Submilli on a package typed by either front end.
pub fn check_package_submilli(
    typed_ast: &TypedAst,
    package: &PackageDeclaration,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<Diagnostic>, CompileError> {
    check_package_rules(
        typed_ast,
        package,
        dependencies,
        false,
        &crate::type_size::TypeLimits::default(),
    )
}

pub(in crate::typechecker) fn check_package_rules(
    typed_ast: &TypedAst,
    package: &PackageDeclaration,
    dependencies: &[&PackageDeclaration],
    include_typescript: bool,
    limits: &crate::type_size::TypeLimits,
) -> Result<Vec<Diagnostic>, CompileError> {
    tree_height::check_typed(typed_ast, CompilerStage::Infer)?;
    let root_module = typed_ast.package_root_module.as_ref().ok_or_else(|| {
        CompileError::from(super::invariant_failure(
            "package TypedAst lacks its root module path",
        ))
    })?;
    super::infer::check_declaration_types(
        std::iter::once(package).chain(dependencies.iter().copied()),
    )?;
    let mut registry = super::infer::type_registry::TypeRegistry::new();
    for declaration in std::iter::once(package).chain(dependencies.iter().copied()) {
        for symbol in declaration
            .runtime_types
            .values()
            .chain(declaration.types.values())
        {
            registry.insert_borrowed(symbol);
        }
    }
    let namespace = super::infer::type_namespace::TypeNamespace::new();
    let constraints = Default::default();
    let resolver = super::infer::assignable::TypeResolver {
        types: &namespace,
        registry: &registry,
        limits,
        strict_subtype: false,
        generic_constraints: &constraints,
    };
    let mut diags = Vec::new();
    finalizers::run(typed_ast, package, resolver, &mut diags).map_err(|fatal| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(fatal),
    })?;
    let exports = &typed_ast.exports;
    exported_signature_types::run_modules(
        &package.package_name,
        "module",
        typed_ast,
        package,
        root_module,
        &mut diags,
    );
    exported_signature_types::run(
        &package.package_name,
        "package",
        package,
        exports,
        &mut diags,
    );
    exported_docs::run(typed_ast, package, exports, &mut diags);
    let declarations = TypeDeclarations::for_package(package, dependencies);
    run_rules(
        typed_ast,
        &declarations,
        Source::Package,
        include_typescript,
        &mut diags,
    )
    .and_then(|()| check_discipline::run(typed_ast, &mut diags))
    .map_err(|fatal| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(fatal),
    })?;
    Ok(diags)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Script,
    Package,
}

/// The rules a script and a package share, in the order they report.
fn run_rules(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    source: Source,
    include_typescript: bool,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    if include_typescript {
        missing_return::run(ta, declarations, diags)?;
        unreachable::run(ta, declarations, diags)?;
        return_outside_function::run(ta, diags)?;
        fallthrough::run(ta, declarations, diags)?;
        definite_assignment::run(ta, diags)?;
    }
    super_call::run(ta, diags)?;
    if source == Source::Script {
        // Inference reports a package that declares `main`.
        main_required::run(ta, diags);
    }
    doc_consistency::run(ta, diags)?;
    capability_consistency::run(ta, declarations, diags)
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::collections::BTreeMap;

    use super::check_script;
    use crate::{
        Asi, Ast, Diagnostic, FileId, ModulePath, PackageDeclaration, Sources, Token, TokenKind,
        TypedAst, infer, infer_package, lower_patterns, parse,
    };

    /// Most rule tests aren't about `main` — prefix the source with a no-op
    /// `main` so the missing-main diagnostic doesn't pollute their counts.
    pub fn run(source: &str) -> Vec<Diagnostic> {
        let with_main = format!("function main(): void {{ }}\n{source}");
        pipeline(&with_main)
    }

    pub fn run_raw(source: &str) -> Vec<Diagnostic> {
        pipeline(source)
    }

    /// Every diagnostic the whole compile pipeline reports for `source`. Unlike
    /// [`run_raw`], parse errors are included and later phases still run.
    pub fn compile_messages(source: &str) -> Vec<String> {
        crate::typecheck(source, FileId(0))
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|diag| diag.message)
            .collect()
    }

    /// Each diagnostic of [`run_raw`] as its message and 1-based line, so a
    /// test can tell apart several reports that share a message.
    pub fn run_lines(source: &str) -> Vec<(String, usize)> {
        run_raw(source)
            .into_iter()
            .map(|diag| {
                let line = source[..diag.span.start as usize].lines().count().max(1);
                (diag.message, line)
            })
            .collect()
    }

    /// Infers a script against the runtime and the standard library, without
    /// checking it.
    pub fn infer_script(source: &str) -> (TypedAst, Vec<Diagnostic>) {
        infer_script_with(source, &[])
    }

    /// [`infer_script`] with `packages` to import from as well.
    pub fn infer_script_with(
        source: &str,
        packages: &[PackageDeclaration],
    ) -> (TypedAst, Vec<Diagnostic>) {
        let ast = parse_source(source, FileId(0));
        let dependencies = runtime_declarations();
        let dependencies: Vec<_> = dependencies.iter().chain(packages).collect();
        infer(source, "main", &ast, &dependencies)
    }

    /// Infers and checks the package `modules` make up, rooted at `lib`.
    pub fn run_package(
        name: &str,
        modules: &[(&str, &str)],
        dependencies: &[PackageDeclaration],
    ) -> (TypedAst, PackageDeclaration, Vec<Diagnostic>) {
        let mut sources = Sources::new();
        let parsed: Vec<(ModulePath, FileId, Ast)> = modules
            .iter()
            .map(|(module, source)| {
                let file = sources.add((*module).to_string(), *source).unwrap();
                let ast = lower_patterns(parse_source(source, file)).unwrap();
                (ModulePath::from(*module), file, ast)
            })
            .collect();
        let external: BTreeMap<String, PackageDeclaration> = runtime_declarations()
            .into_iter()
            .chain(dependencies.iter().cloned())
            .map(|declaration| (declaration.package_name.clone(), declaration))
            .collect();
        infer_package(
            name,
            ModulePath::from("lib"),
            parsed
                .iter()
                .map(|(module, file, ast)| (module.clone(), *file, ast))
                .collect(),
            &sources,
            external,
            BTreeMap::new(),
        )
    }

    fn pipeline(source: &str) -> Vec<Diagnostic> {
        let ast = parse_source(source, FileId(0));
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
        packages.extend(prelude_defs.iter());
        packages.extend(host_defs.iter());
        let (ta, mut diags) = infer(source, "main", &ast, &packages);
        diags.extend(check_script(&ta, &packages).unwrap());
        diags
    }

    fn parse_source(source: &str, file: FileId) -> Ast {
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
        let (ast, parse_diags) = parse(source, tokens, file);
        assert!(
            parse_diags.is_empty(),
            "unexpected parser diags: {parse_diags:?}"
        );
        ast
    }

    fn runtime_declarations() -> Vec<PackageDeclaration> {
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        prelude_defs
            .iter()
            .chain(host_defs)
            .cloned()
            .chain(crate::stdlib::stdlib_package_declarations())
            .collect()
    }
}

#[cfg(test)]
mod public_entry_tests {
    use super::test_util::{infer_script, run_package};
    use super::{check_package, check_package_submilli, check_script, check_script_submilli};

    #[test]
    fn submilli_script_check_omits_typescript_rules() {
        let (typed_ast, inference) =
            infer_script("function main(): void { return; const value = 1; }");
        assert!(
            !inference
                .iter()
                .any(|diagnostic| diagnostic.severity == crate::Severity::Error),
            "{inference:?}"
        );
        let full = check_script(&typed_ast, &[]).unwrap();
        assert!(
            full.iter()
                .any(|diagnostic| diagnostic.message.contains("unreachable"))
        );
        assert!(check_script_submilli(&typed_ast, &[]).unwrap().is_empty());
        let (without_main, _) = infer_script("function helper(): void {}");
        assert!(
            check_script_submilli(&without_main, &[])
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("main"))
        );
    }

    #[test]
    fn package_entry_needs_only_tree_and_declarations() {
        let (typed_ast, package, inference) = run_package(
            "@test/public-rules",
            &[
                ("lib", "export function entry(): void {}"),
                ("other", "export function helper(): void {}"),
            ],
            &[],
        );
        assert!(
            !inference
                .iter()
                .any(|diagnostic| diagnostic.severity == crate::Severity::Error),
            "{inference:?}"
        );
        assert_eq!(
            check_package(&typed_ast, &package, &[]).unwrap(),
            check_package_submilli(&typed_ast, &package, &[]).unwrap()
        );
        assert_eq!(
            typed_ast.package_root_module,
            Some(crate::ModulePath::from("lib"))
        );
        let mut without_root = typed_ast.clone();
        without_root.package_root_module = None;
        assert!(check_package(&without_root, &package, &[]).is_err());
    }

    #[test]
    fn package_entry_checks_private_module_signature_types() {
        let (typed_ast, package, _) = run_package(
            "@test/private-type",
            &[
                ("lib", "export function entry(): void {}"),
                (
                    "other",
                    "interface Hidden { value: string } export function expose(value: Hidden): void {}",
                ),
            ],
            &[],
        );
        let diagnostics = check_package_submilli(&typed_ast, &package, &[]).unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("private type `Hidden`")),
            "{diagnostics:?}"
        );
    }
}
