//! Check (Rules) pass — semantic rules over the Typed AST.
//!
//! Read-only: produces additional diagnostics, never modifies the tree.

mod capability_consistency;
mod control_flow;
mod definite_assignment;
mod doc_consistency;
mod exported_docs;
pub(crate) mod exported_signature_types;
mod fallthrough;
mod main_required;
mod missing_return;
mod return_outside_function;
mod unreachable;

use crate::{Diagnostic, ExportEntry, PackageDeclaration, TypedAst};

use super::infer::module_symbols::ModuleSymbols;

pub fn check(ta: &TypedAst) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    missing_return::run(ta, &mut diags);
    unreachable::run(ta, &mut diags);
    return_outside_function::run(ta, &mut diags);
    fallthrough::run(ta, &mut diags);
    definite_assignment::run(ta, &mut diags);
    main_required::run(ta, &mut diags);
    doc_consistency::run(ta, &mut diags);
    capability_consistency::run(ta, &mut diags);
    diags
}

pub(in crate::typechecker) struct PackageModuleSurface<'a> {
    pub(in crate::typechecker) symbols: &'a ModuleSymbols,
    pub(in crate::typechecker) exports: &'a [ExportEntry],
}

pub(in crate::typechecker) fn check_package<'a>(
    package_name: &str,
    typed_ast: &TypedAst,
    package: &PackageDeclaration,
    exports: &[ExportEntry],
    modules: impl IntoIterator<Item = PackageModuleSurface<'a>>,
) -> Vec<Diagnostic> {
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
    diags
}

#[cfg(test)]
mod test_util {
    use super::check;
    use crate::{Asi, Diagnostic, Token, TokenKind, infer, parse};

    fn pipeline(source: &str) -> Vec<Diagnostic> {
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
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
        packages.extend(prelude_defs.iter());
        packages.extend(host_defs.iter());
        let (ta, mut diags) = infer(source, "main", &ast, &packages);
        diags.extend(check(&ta));
        diags
    }

    /// Most rule tests aren't about `main` — prefix the source with a no-op
    /// `main` so the missing-main diagnostic doesn't pollute their counts.
    pub fn run(source: &str) -> Vec<Diagnostic> {
        let with_main = format!("function main(): void {{ }}\n{source}");
        pipeline(&with_main)
    }

    pub fn run_raw(source: &str) -> Vec<Diagnostic> {
        pipeline(source)
    }
}
