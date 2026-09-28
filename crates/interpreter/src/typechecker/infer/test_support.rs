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
