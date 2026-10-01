//! Check (Rules) pass — semantic rules over the Typed AST.
//!
//! Read-only: produces additional diagnostics, never modifies the tree.

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
mod main_required;
mod missing_return;
mod return_outside_function;
mod super_call;
mod unreachable;

use crate::compiler_error::{CompileError, CompilerFailure, CompilerStage};
use crate::{Diagnostic, ExportEntry, PackageDeclaration, TypedAst, tree_height};

use super::infer::module_symbols::ModuleSymbols;
use declarations::TypeDeclarations;

/// [`check_script`] for a script whose types are all its own: one imported
/// from a package cannot be resolved here.
pub fn check(ta: &TypedAst) -> Result<Vec<Diagnostic>, CompileError> {
    check_script(ta, &[])
}

/// Checks a script. `dependencies` are the packages it was inferred against,
/// which declare the types it imports.
pub fn check_script(
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<Diagnostic>, CompileError> {
    tree_height::check_typed(ta, CompilerStage::Infer)?;
    let declarations = TypeDeclarations::for_script(ta, dependencies);
    let mut diags = Vec::new();
    run_rules(ta, &declarations, Source::Script, &mut diags).map_err(|fatal| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(fatal),
    })?;
    Ok(diags)
}

pub(in crate::typechecker) struct PackageModuleSurface<'a> {
    pub(in crate::typechecker) symbols: &'a ModuleSymbols,
    pub(in crate::typechecker) exports: &'a [ExportEntry],
}

/// Checks a package. `dependencies` are the packages it was inferred against,
/// the ones it cannot import from included.
pub(in crate::typechecker) fn check_package<'a>(
    package_name: &str,
    typed_ast: &TypedAst,
    package: &PackageDeclaration,
    exports: &[ExportEntry],
    modules: impl IntoIterator<Item = PackageModuleSurface<'a>>,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<Diagnostic>, CompileError> {
    tree_height::check_typed(typed_ast, CompilerStage::Infer)?;
    let mut diags = Vec::new();
    for module in modules {
        exported_signature_types::run_module(
            package_name,
            "module",
            module.symbols,
            module.exports,
            &mut diags,
        );
    }
    exported_signature_types::run(package_name, "package", package, exports, &mut diags);
    exported_docs::run(typed_ast, package, exports, &mut diags);
    let declarations = TypeDeclarations::for_package(package, dependencies);
    run_rules(typed_ast, &declarations, Source::Package, &mut diags)
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
    diags: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    missing_return::run(ta, declarations, diags)?;
    unreachable::run(ta, declarations, diags)?;
    return_outside_function::run(ta, diags)?;
    fallthrough::run(ta, declarations, diags)?;
    definite_assignment::run(ta, diags)?;
    super_call::run(ta, diags)?;
    if source == Source::Script {
        // Inference reports a package that declares `main`.
        main_required::run(ta, diags);
        // First-party packages leave parameters and results undocumented, so
        // packages are not held to this yet.
        doc_consistency::run(ta, diags);
    }
    capability_consistency::run(ta, declarations, diags)
}

#[cfg(test)]
mod test_util {
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
