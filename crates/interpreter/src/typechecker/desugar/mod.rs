//! Desugar pass — transforms surface-level Typed AST forms into a smaller
//! canonical shape that codegen has to handle.

use crate::{
    ExprId, FileId, Ident, Span, StmtId, Type, TypedAst, TypedExpr, TypedExprKind, TypedStmt,
};

mod do_while;
mod for_loop;
mod for_of;
mod postfix_incdec;

pub(crate) struct DesugarCtx<'a> {
    pub(crate) ta: &'a mut TypedAst,
    /// The module whose AST is being desugared; generated temporaries are
    /// attributed to it (they have no source range of their own).
    file: FileId,
    next_temp: u32,
}

impl DesugarCtx<'_> {
    /// Placeholder span for a generated node, anchored to the module being desugared.
    pub(crate) fn gen_span(&self) -> Span {
        Span::at(self.file)
    }

    pub(crate) fn fresh_name(&mut self, prefix: &str) -> Ident {
        let n = self.next_temp;
        self.next_temp += 1;
        Ident {
            // `#` cannot occur in a source identifier.
            name: format!("#desugar_{prefix}_{n}"),
            span: self.gen_span(),
        }
    }

    pub(crate) fn bool_true(&mut self) -> ExprId {
        let span = self.gen_span();
        self.ta.push_expr(TypedExpr {
            kind: TypedExprKind::Boolean(true),
            span,
            ty: Type::Boolean,
        })
    }

    pub(crate) fn bool_false(&mut self) -> ExprId {
        let span = self.gen_span();
        self.ta.push_expr(TypedExpr {
            kind: TypedExprKind::Boolean(false),
            span,
            ty: Type::Boolean,
        })
    }

    pub(crate) fn number_lit(&mut self, value: f64) -> ExprId {
        let span = self.gen_span();
        self.ta.push_expr(TypedExpr {
            kind: TypedExprKind::Number(value),
            span,
            ty: Type::Number,
        })
    }

    pub(crate) fn bigint_lit(&mut self, digits: &str) -> ExprId {
        let span = self.gen_span();
        self.ta.push_expr(TypedExpr {
            kind: TypedExprKind::BigInt(digits.to_string()),
            span,
            ty: Type::BigInt,
        })
    }

    /// `boxed` is always false — capture pass already ran before desugar.
    pub(crate) fn local_ref(&mut self, ident: Ident, ty: Type) -> ExprId {
        let span = self.gen_span();
        self.ta.push_expr(TypedExpr {
            kind: TypedExprKind::LocalRef {
                ident,
                boxed: false,
            },
            span,
            ty,
        })
    }

    pub(crate) fn push_stmt(&mut self, kind: crate::TypedStmtKind, span: Span) -> StmtId {
        self.ta.push_stmt(TypedStmt { kind, span })
    }

    /// `let #desugar_<prefix>_N = true;` — the flag that tells a `while`-true's head
    /// which pass it is on. Returns the flag's name and its declaration.
    pub(crate) fn first_pass_flag(&mut self, prefix: &str, span: Span) -> (Ident, StmtId) {
        let flag = self.fresh_name(prefix);
        let true_lit = self.bool_true();
        let decl = self.push_stmt(
            crate::TypedStmtKind::Let {
                name: flag.clone(),
                ty: Type::Boolean,
                value: true_lit,
                boxed: false,
                doc: None,
            },
            span,
        );
        (flag, decl)
    }

    /// `if (__first) { __first = false; } else { steps… }` — runs `steps` at the
    /// top of every iteration but the first.
    ///
    /// The loop head is the only position that a bare `continue` — including one
    /// that unwinds a `try`/`finally` — is guaranteed to reach, so anything a
    /// loop must do once per iteration goes here rather than after the body:
    /// `for`'s update clause (ECMA-262 §14.7.4.7) and `do`/`while`'s condition
    /// test (§14.7.2) both.
    pub(crate) fn skip_on_first_pass(
        &mut self,
        flag: &Ident,
        steps: Vec<StmtId>,
        span: Span,
    ) -> StmtId {
        let false_lit = self.bool_false();
        let clear = self.push_stmt(
            crate::TypedStmtKind::AssignLocal {
                ident: flag.clone(),
                target_ty: Type::Boolean,
                value: false_lit,
                boxed: false,
                narrowed_shadow_ty: None,
            },
            span,
        );
        let then_block = self.push_stmt(crate::TypedStmtKind::Block(vec![clear]), span);
        let else_block = self.push_stmt(crate::TypedStmtKind::Block(steps), span);
        let flag_ref = self.local_ref(flag.clone(), Type::Boolean);
        self.push_stmt(
            crate::TypedStmtKind::If {
                condition: flag_ref,
                then_block,
                else_block: Some(else_block),
            },
            span,
        )
    }

    /// Flattens a loop body for splicing into a block the caller is building.
    /// The parser always makes a body a Block, but this pass runs on the *typed*
    /// AST, where the narrowing fixed point may have wrapped it in `NarrowRegion`s
    /// (`wrap_narrow_regions`). A wrapper nests whole: its shadow binding scopes
    /// over the statements inside it, so splicing those out would orphan them.
    pub(crate) fn body_as_stmts(&self, body: StmtId) -> Vec<StmtId> {
        match &self.ta.stmt(body).kind {
            crate::TypedStmtKind::Block(stmts) => stmts.clone(),
            _ => vec![body],
        }
    }
}

pub fn desugar(ta: &mut TypedAst, file: FileId) {
    let mut ctx = DesugarCtx {
        ta,
        file,
        next_temp: 0,
    };
    // for_of must run first — its expansion produces a While that the other passes must not rewrite.
    for_of::run(&mut ctx);
    for_loop::run(&mut ctx);
    do_while::run(&mut ctx);
    // Runs after loop desugarers so i++ in for-loop update slots reaches this as Stmt::Expr(PostfixUnary).
    postfix_incdec::run(&mut ctx);
}

#[cfg(test)]
mod tests {
    use super::desugar;
    use crate::{Asi, Token, TokenKind, TypedAst, capture, infer, parse};

    fn pipeline(source: &str) -> TypedAst {
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
        let (mut ta, infer_diags) = infer(source, "main", &ast, &packages);
        assert!(
            infer_diags.is_empty(),
            "unexpected infer diags: {infer_diags:?}"
        );
        capture(&mut ta);
        desugar(&mut ta, crate::FileId(0));
        ta
    }

    #[test]
    fn desugar_runs_on_empty_program() {
        let ta = pipeline("");
        assert!(ta.globals.is_empty());
        assert!(ta.functions.is_empty());
        assert!(ta.top_level_statements.is_empty());
    }

    #[test]
    fn desugar_preserves_mvp_program_shape() {
        let src = "function f(n: number): number { let x: number = n + 1; return x; } let r: number = f(2); function main(): void { }";
        let before = {
            let mut asi = Asi::new(src, crate::FileId(0));
            let mut tokens: Vec<Token> = Vec::new();
            loop {
                let tok = asi.next_token();
                let is_eof = matches!(tok.kind, TokenKind::Eof);
                tokens.push(tok);
                if is_eof {
                    break;
                }
            }
            let _ = asi.into_diagnostics();
            let (ast, _) = parse(src, tokens, crate::FileId(0));
            let (prelude_defs, host_defs, _) =
                crate::runtime::prelude::cached_runtime_package_declarations();
            let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
            packages.extend(prelude_defs.iter());
            packages.extend(host_defs.iter());
            let (mut ta, _) = infer(src, "main", &ast, &packages);
            capture(&mut ta);
            ta
        };
        let mut after = before.clone();
        desugar(&mut after, crate::FileId(0));
        assert_eq!(format!("{before:?}"), format!("{after:?}"));
    }

    #[test]
    fn desugar_is_idempotent() {
        let mut ta = pipeline("function main(): number { return 1; }");
        let snapshot = ta.clone();
        desugar(&mut ta, crate::FileId(0));
        desugar(&mut ta, crate::FileId(0));
        assert_eq!(format!("{ta:?}"), format!("{snapshot:?}"));
    }

    #[test]
    fn desugar_pipeline_composes() {
        let ta = pipeline(
            "function add(a: number, b: number): number { return a + b; } let r: number = add(1, 2); function main(): void { if (r < 10) { } }",
        );
        assert_eq!(ta.functions.len(), 2);
        assert_eq!(ta.globals.len(), 1);
    }
}
