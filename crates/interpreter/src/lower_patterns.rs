//! Pre-infer pass eliminating destructuring patterns from [`Ast`].
//! Runs between [`crate::parse`] and [`crate::infer`]; downstream sees only plain
//! `Let`/`Const`/`ConstRest`/`ForOf` and `ParamDecl` with `pattern: None`.
//! Synthesised array-pattern `IndexAccess` nodes are tagged in [`Ast::pattern_origins`]
//! so the typechecker can rephrase index errors as destructure-specific diagnostics.

use crate::{
    ArrowBody, Ast, Binding, BindingKind, Expr, ExprId, ExprKind, Ident, ParamDecl, PatternOrigin,
    Span, Stmt, StmtId, StmtKind,
};

pub fn lower(ast: &mut Ast) {
    let mut ctx = LowerCtx { next_tmp: 0 };
    ctx.lower_arrows(ast);
    ctx.lower_function_params(ast);
    ctx.lower_for_of_patterns(ast);
    ctx.lower_pattern_stmts(ast);
}

struct LowerCtx {
    next_tmp: u32,
}

impl LowerCtx {
    fn fresh(&mut self, prefix: &str, span: Span) -> Ident {
        let n = self.next_tmp;
        self.next_tmp += 1;
        Ident {
            name: format!("__{prefix}_{n}"),
            span,
        }
    }

    fn lower_arrows(&mut self, ast: &mut Ast) {
        let len = ast.exprs_len();
        for i in 0..len {
            let id = ExprId(i as u32);
            let has_pattern = match &ast.expr(id).kind {
                ExprKind::Arrow { params, .. } => params.iter().any(|p| p.pattern.is_some()),
                _ => false,
            };
            if has_pattern {
                self.lower_arrow(ast, id);
            }
        }
    }

    fn lower_arrow(&mut self, ast: &mut Ast, id: ExprId) {
        let arrow_span = ast.expr(id).span;
        // Clone the kind to release the read borrow before we start
        // mutating other arena slots.
        let kind = ast.expr(id).kind.clone();
        let ExprKind::Arrow {
            mut params,
            return_type,
            type_predicate,
            body,
        } = kind
        else {
            unreachable!("lower_arrow guarded by Arrow match")
        };

        let mut decompose: Vec<StmtId> = Vec::new();
        self.materialise_pattern_params(ast, &mut params, &mut decompose);

        let new_body = match body {
            ArrowBody::Block(block_id) => {
                self.prepend_to_block(ast, block_id, decompose);
                ArrowBody::Block(block_id)
            }
            ArrowBody::Expr(expr_id) => {
                let return_span = ast.expr(expr_id).span;
                let return_stmt = ast.push_stmt(Stmt {
                    kind: StmtKind::Return(Some(expr_id)),
                    span: return_span,
                });
                decompose.push(return_stmt);
                let block = ast.push_stmt(Stmt {
                    kind: StmtKind::Block(decompose),
                    span: arrow_span,
                });
                ArrowBody::Block(block)
            }
        };

        ast.expr_mut(id).kind = ExprKind::Arrow {
            params,
            return_type,
            type_predicate,
            body: new_body,
        };
    }

    fn lower_function_params(&mut self, ast: &mut Ast) {
        let len = ast.stmts_len();
        for i in 0..len {
            let id = StmtId(i as u32);
            let has_pattern = match &ast.stmt(id).kind {
                StmtKind::Function { params, .. } => params.iter().any(|p| p.pattern.is_some()),
                _ => false,
            };
            if has_pattern {
                self.lower_function(ast, id);
            }
        }
    }

    fn lower_function(&mut self, ast: &mut Ast, id: StmtId) {
        let stmt_span = ast.stmt(id).span;
        let kind = std::mem::replace(
            &mut ast.stmt_mut(id).kind,
            StmtKind::Block(Vec::new()), // sentinel, restored before return
        );
        let StmtKind::Function {
            name,
            generics,
            mut params,
            return_type,
            type_predicate,
            body,
            doc,
        } = kind
        else {
            unreachable!("lower_function guarded by Function match");
        };

        let mut decompose: Vec<StmtId> = Vec::new();
        self.materialise_pattern_params(ast, &mut params, &mut decompose);
        self.prepend_to_block(ast, body, decompose);

        ast.stmt_mut(id).kind = StmtKind::Function {
            name,
            generics,
            params,
            return_type,
            type_predicate,
            body,
            doc,
        };
        // Span is unchanged, but reassert for the linter.
        let _ = stmt_span;
    }

    fn lower_for_of_patterns(&mut self, ast: &mut Ast) {
        let len = ast.stmts_len();
        for i in 0..len {
            let id = StmtId(i as u32);
            if !matches!(ast.stmt(id).kind, StmtKind::ForOfPattern { .. }) {
                continue;
            }
            self.expand_for_of_pattern(ast, id);
        }
    }

    fn expand_for_of_pattern(&mut self, ast: &mut Ast, id: StmtId) {
        let stmt_span = ast.stmt(id).span;
        let kind = std::mem::replace(&mut ast.stmt_mut(id).kind, StmtKind::Block(Vec::new()));
        let StmtKind::ForOfPattern {
            binding_kind,
            binding,
            ty,
            iter,
            body,
        } = kind
        else {
            unreachable!("expand_for_of_pattern guarded by ForOfPattern match");
        };

        let pattern_span = binding.span();
        let dst = self.fresh("dst", pattern_span);
        let is_const = matches!(binding_kind, BindingKind::Const);
        let decompose = self.emit_decompose(ast, binding, dst.clone(), is_const, None);
        self.prepend_to_block(ast, body, decompose);

        ast.stmt_mut(id).kind = StmtKind::ForOf {
            binding_kind,
            name: dst,
            ty,
            iter,
            body,
        };
        let _ = stmt_span;
    }

    fn lower_pattern_stmts(&mut self, ast: &mut Ast) {
        let top = std::mem::take(&mut ast.top_level);
        ast.top_level = self.expand_list(ast, top);

        let len = ast.stmts_len();
        for i in 0..len {
            let id = StmtId(i as u32);
            let needs = matches!(ast.stmt(id).kind, StmtKind::Block(_));
            if !needs {
                continue;
            }
            let StmtKind::Block(taken) =
                std::mem::replace(&mut ast.stmt_mut(id).kind, StmtKind::Block(Vec::new()))
            else {
                unreachable!()
            };
            let expanded = self.expand_list(ast, taken);
            ast.stmt_mut(id).kind = StmtKind::Block(expanded);
        }
    }

    fn expand_list(&mut self, ast: &mut Ast, ids: Vec<StmtId>) -> Vec<StmtId> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let is_pattern = matches!(
                ast.stmt(id).kind,
                StmtKind::LetPattern { .. } | StmtKind::ConstPattern { .. }
            );
            if !is_pattern {
                out.push(id);
                continue;
            }
            let expanded = self.expand_pattern_stmt(ast, id);
            out.extend(expanded);
        }
        out
    }

    fn expand_pattern_stmt(&mut self, ast: &mut Ast, id: StmtId) -> Vec<StmtId> {
        let span = ast.stmt(id).span;
        let kind = std::mem::replace(
            &mut ast.stmt_mut(id).kind,
            StmtKind::Block(Vec::new()), // sentinel; the slot is unreachable post-lowering
        );
        let (is_const, binding, ty, value, doc) = match kind {
            StmtKind::LetPattern {
                binding,
                ty,
                value,
                doc,
            } => (false, binding, ty, value, doc),
            StmtKind::ConstPattern {
                binding,
                ty,
                value,
                doc,
            } => (true, binding, ty, value, doc),
            _ => unreachable!("expand_pattern_stmt guarded by LetPattern/ConstPattern"),
        };

        let pattern_span = binding.span();
        let dst = self.fresh("dst", pattern_span);

        // temp is always `const` even for `let` patterns — it's never re-assigned
        let dst_stmt = ast.push_stmt(Stmt {
            kind: StmtKind::Const {
                name: dst.clone(),
                ty,
                value,
                doc: None,
            },
            span,
        });

        let mut out = vec![dst_stmt];
        out.extend(self.emit_decompose(ast, binding, dst, is_const, doc));
        out
    }

    fn materialise_pattern_params(
        &mut self,
        ast: &mut Ast,
        params: &mut [ParamDecl],
        decompose: &mut Vec<StmtId>,
    ) {
        for param in params.iter_mut() {
            let Some(pattern) = param.pattern.take() else {
                continue;
            };
            let fresh = self.fresh("p", pattern.span());
            param.name = fresh.clone();
            decompose.extend(self.emit_decompose(ast, pattern, fresh, /*is_const=*/ true, None));
        }
    }

    /// `doc` attaches to the first emitted stmt only.
    fn emit_decompose(
        &mut self,
        ast: &mut Ast,
        binding: Binding,
        source: Ident,
        is_const: bool,
        mut doc: Option<crate::DocComment>,
    ) -> Vec<StmtId> {
        let mut out = Vec::new();
        match binding {
            Binding::Object {
                fields,
                rest,
                span: _,
            } => {
                let bound_names: Vec<Ident> = fields.iter().map(|f| f.source.clone()).collect();
                for field in fields {
                    let recv = ast.push_expr(Expr {
                        kind: ExprKind::Identifier(source.clone()),
                        span: field.span,
                    });
                    let access = ast.push_expr(Expr {
                        kind: ExprKind::FieldAccess {
                            receiver: recv,
                            name: field.source.clone(),
                        },
                        span: field.span,
                    });
                    let decl = ast.push_stmt(Stmt {
                        kind: make_decl(is_const, field.local, access, doc.take()),
                        span: field.span,
                    });
                    out.push(decl);
                }
                if let Some(rest_ident) = rest {
                    let src_expr = ast.push_expr(Expr {
                        kind: ExprKind::Identifier(source.clone()),
                        span: rest_ident.span,
                    });
                    let decl = ast.push_stmt(Stmt {
                        kind: StmtKind::ConstRest {
                            name: rest_ident.clone(),
                            source: src_expr,
                            exclude: bound_names,
                            ty: None,
                            doc: doc.take(),
                        },
                        span: rest_ident.span,
                    });
                    out.push(decl);
                }
            }
            Binding::Array {
                elems,
                rest,
                span: pattern_span,
            } => {
                let elems_len = elems.len();
                for (i, slot) in elems.into_iter().enumerate() {
                    let Some(local) = slot else { continue };
                    let recv = ast.push_expr(Expr {
                        kind: ExprKind::Identifier(source.clone()),
                        span: local.span,
                    });
                    let idx = ast.push_expr(Expr {
                        kind: ExprKind::Number(i as f64),
                        span: local.span,
                    });
                    let access = ast.push_expr(Expr {
                        kind: ExprKind::IndexAccess {
                            receiver: recv,
                            index: idx,
                        },
                        span: local.span,
                    });
                    // tag so typechecker rephrases index errors as destructure-specific
                    ast.pattern_origins.insert(
                        access,
                        PatternOrigin {
                            pattern_span,
                            slot_arity: elems_len,
                        },
                    );
                    let decl = ast.push_stmt(Stmt {
                        kind: make_decl(is_const, local.clone(), access, doc.take()),
                        span: local.span,
                    });
                    out.push(decl);
                }
                if let Some(rest_ident) = rest {
                    // prelude slice(start, end) — pass source.length as end to copy remaining
                    let from_lit = ast.push_expr(Expr {
                        kind: ExprKind::Number(elems_len as f64),
                        span: rest_ident.span,
                    });
                    let recv_for_slice = ast.push_expr(Expr {
                        kind: ExprKind::Identifier(source.clone()),
                        span: rest_ident.span,
                    });
                    let slice_callee = ast.push_expr(Expr {
                        kind: ExprKind::FieldAccess {
                            receiver: recv_for_slice,
                            name: Ident {
                                name: "slice".to_string(),
                                span: rest_ident.span,
                            },
                        },
                        span: rest_ident.span,
                    });
                    let recv_for_len = ast.push_expr(Expr {
                        kind: ExprKind::Identifier(source.clone()),
                        span: rest_ident.span,
                    });
                    let length_access = ast.push_expr(Expr {
                        kind: ExprKind::FieldAccess {
                            receiver: recv_for_len,
                            name: Ident {
                                name: "length".to_string(),
                                span: rest_ident.span,
                            },
                        },
                        span: rest_ident.span,
                    });
                    let slice_call = ast.push_expr(Expr {
                        kind: ExprKind::Call {
                            callee: slice_callee,
                            type_args: None,
                            args: vec![from_lit, length_access],
                        },
                        span: rest_ident.span,
                    });
                    let decl = ast.push_stmt(Stmt {
                        kind: make_decl(is_const, rest_ident.clone(), slice_call, doc.take()),
                        span: rest_ident.span,
                    });
                    out.push(decl);
                }
            }
        }
        out
    }

    fn prepend_to_block(&mut self, ast: &mut Ast, block_id: StmtId, prefix: Vec<StmtId>) {
        if prefix.is_empty() {
            return;
        }
        let original = match &ast.stmt(block_id).kind {
            StmtKind::Block(stmts) => stmts.clone(),
            _ => unreachable!("prepend_to_block expects a Block stmt"),
        };
        let combined: Vec<StmtId> = prefix.into_iter().chain(original).collect();
        ast.stmt_mut(block_id).kind = StmtKind::Block(combined);
    }
}

fn make_decl(
    is_const: bool,
    name: Ident,
    value: ExprId,
    doc: Option<crate::DocComment>,
) -> StmtKind {
    if is_const {
        StmtKind::Const {
            name,
            ty: None,
            value,
            doc,
        }
    } else {
        StmtKind::Let {
            name,
            ty: None,
            value,
            doc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lower;
    use crate::{Asi, Binding, Stmt, StmtKind, Token, TokenKind, parse};

    fn pipeline(source: &str) -> crate::Ast {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let t = asi.next_token();
            let is_eof = matches!(t.kind, TokenKind::Eof);
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let _ = asi.into_diagnostics();
        let (mut ast, diags) = parse(source, tokens, crate::FileId(0));
        assert!(diags.is_empty(), "unexpected parse diags: {diags:?}");
        lower(&mut ast);
        ast
    }

    fn no_patterns_left(ast: &crate::Ast) {
        for i in 0..ast.stmts_len() {
            let s = ast.stmt(crate::StmtId(i as u32));
            assert!(
                !matches!(
                    s.kind,
                    StmtKind::LetPattern { .. }
                        | StmtKind::ConstPattern { .. }
                        | StmtKind::ForOfPattern { .. }
                ),
                "found unlowered pattern at stmt #{i}: {:?}",
                s.kind,
            );
            if let StmtKind::Function { params, .. } = &s.kind {
                for (pi, p) in params.iter().enumerate() {
                    assert!(
                        p.pattern.is_none(),
                        "function param #{pi} still has a pattern after lowering",
                    );
                }
            }
        }
        for i in 0..ast.exprs_len() {
            let e = ast.expr(crate::ExprId(i as u32));
            if let crate::ExprKind::Arrow { params, .. } = &e.kind {
                for (pi, p) in params.iter().enumerate() {
                    assert!(
                        p.pattern.is_none(),
                        "arrow param #{pi} still has a pattern after lowering",
                    );
                }
            }
        }
    }

    #[test]
    fn lowers_simple_object_destructure() {
        let ast = pipeline("const { a, b } = obj;");
        no_patterns_left(&ast);
        // top_level: __dst, a-decl, b-decl
        assert_eq!(ast.top_level.len(), 3);
        let names: Vec<String> = ast
            .top_level
            .iter()
            .map(|id| match &ast.stmt(*id).kind {
                StmtKind::Const { name, .. } => name.name.clone(),
                other => panic!("expected Const, got {other:?}"),
            })
            .collect();
        assert!(names[0].starts_with("__dst_"));
        assert_eq!(names[1], "a");
        assert_eq!(names[2], "b");
    }

    #[test]
    fn lowers_renamed_object_destructure() {
        let ast = pipeline("const { a: x, b: y } = obj;");
        no_patterns_left(&ast);
        let names: Vec<String> = ast
            .top_level
            .iter()
            .map(|id| match &ast.stmt(*id).kind {
                StmtKind::Const { name, .. } => name.name.clone(),
                other => panic!("expected Const, got {other:?}"),
            })
            .collect();
        assert_eq!(names[1], "x");
        assert_eq!(names[2], "y");
    }

    #[test]
    fn lowers_array_destructure() {
        let ast = pipeline("const [x, y] = arr;");
        no_patterns_left(&ast);
        assert_eq!(ast.top_level.len(), 3);
        if let StmtKind::Const { value, .. } = &ast.stmt(ast.top_level[1]).kind {
            assert!(matches!(
                ast.expr(*value).kind,
                crate::ExprKind::IndexAccess { .. }
            ));
        }
    }

    #[test]
    fn lowers_array_with_hole_and_rest() {
        let ast = pipeline("const [, x, ...rest] = arr;");
        no_patterns_left(&ast);
        // __dst, x (= arr[1]), rest (= arr.slice(2))
        assert_eq!(ast.top_level.len(), 3);
        if let StmtKind::Const { name, value, .. } = &ast.stmt(ast.top_level[1]).kind {
            assert_eq!(name.name, "x");
            if let crate::ExprKind::IndexAccess { index, .. } = &ast.expr(*value).kind {
                assert_eq!(ast.expr(*index).kind, crate::ExprKind::Number(1.0));
            } else {
                panic!("expected IndexAccess");
            }
        }
        if let StmtKind::Const { name, value, .. } = &ast.stmt(ast.top_level[2]).kind {
            assert_eq!(name.name, "rest");
            match &ast.expr(*value).kind {
                crate::ExprKind::Call { callee, args, .. } => {
                    assert_eq!(args.len(), 2, "slice takes (start, end)");
                    assert_eq!(ast.expr(args[0]).kind, crate::ExprKind::Number(2.0));
                    match &ast.expr(*callee).kind {
                        crate::ExprKind::FieldAccess { name, .. } => {
                            assert_eq!(name.name, "slice");
                        }
                        other => panic!("expected slice FieldAccess, got {other:?}"),
                    }
                }
                other => panic!("expected Call, got {other:?}"),
            }
        }
    }

    #[test]
    fn lowers_object_rest_to_const_rest() {
        let ast = pipeline("const { a, ...rest } = obj;");
        no_patterns_left(&ast);
        assert_eq!(ast.top_level.len(), 3);
        match &ast.stmt(ast.top_level[2]).kind {
            StmtKind::ConstRest { name, exclude, .. } => {
                assert_eq!(name.name, "rest");
                assert_eq!(exclude.len(), 1);
                assert_eq!(exclude[0].name, "a");
            }
            other => panic!("expected ConstRest, got {other:?}"),
        }
    }

    #[test]
    fn lowers_function_param_pattern() {
        let src = "function f({ a, b }: T): number { return a + b; }";
        let ast = pipeline(src);
        no_patterns_left(&ast);
        let Stmt {
            kind: StmtKind::Function { body, params, .. },
            ..
        } = ast.stmt(ast.top_level[0]).clone()
        else {
            panic!("expected Function");
        };
        assert_eq!(params.len(), 1);
        assert!(params[0].pattern.is_none());
        assert!(params[0].name.name.starts_with("__p_"));
        let body_stmts = match &ast.stmt(body).kind {
            StmtKind::Block(s) => s.clone(),
            _ => panic!("body is not a Block"),
        };
        // a-decl, b-decl, return  (no __dst — params bind directly to __p_N)
        assert_eq!(body_stmts.len(), 3);
        if let StmtKind::Const { name, .. } = &ast.stmt(body_stmts[0]).kind {
            assert_eq!(name.name, "a");
        } else {
            panic!("expected Const a");
        }
        if let StmtKind::Const { name, .. } = &ast.stmt(body_stmts[1]).kind {
            assert_eq!(name.name, "b");
        } else {
            panic!("expected Const b");
        }
        assert!(matches!(ast.stmt(body_stmts[2]).kind, StmtKind::Return(_)));
    }

    #[test]
    fn lowers_arrow_expr_body_with_pattern() {
        let src = "const f = ({a, b}: T): number => a + b;";
        let ast = pipeline(src);
        no_patterns_left(&ast);
        let mut found = false;
        for i in 0..ast.exprs_len() {
            let e = ast.expr(crate::ExprId(i as u32));
            if let crate::ExprKind::Arrow { body, .. } = &e.kind {
                assert!(matches!(body, crate::ArrowBody::Block(_)));
                found = true;
            }
        }
        assert!(found, "no Arrow found in expr arena");
    }

    #[test]
    fn lowers_for_of_array_pattern() {
        // `for (const [a, b] of pairs) { … }` becomes
        // `for (const __dst_N of pairs) { const a = __dst_N[0];
        // const b = __dst_N[1]; }` after lowering.
        let src = "function main(): void { for (const [a, b] of pairs) { use(a, b); } }";
        let ast = pipeline(src);
        no_patterns_left(&ast);

        let Stmt {
            kind: StmtKind::Function { body, .. },
            ..
        } = ast.stmt(ast.top_level[0]).clone()
        else {
            panic!("expected Function");
        };
        let body_stmts = match &ast.stmt(body).kind {
            StmtKind::Block(s) => s.clone(),
            _ => panic!("function body is not a Block"),
        };
        assert_eq!(body_stmts.len(), 1, "main body has just the for-of");

        let (loop_name, loop_body) = match &ast.stmt(body_stmts[0]).kind {
            StmtKind::ForOf { name, body, .. } => (name.name.clone(), *body),
            other => panic!("expected ForOf, got {other:?}"),
        };
        assert!(
            loop_name.starts_with("__dst_"),
            "loop var was rewritten to a synth temp; got {loop_name}",
        );

        let loop_body_stmts = match &ast.stmt(loop_body).kind {
            StmtKind::Block(s) => s.clone(),
            _ => panic!("for-of body is not a Block"),
        };
        assert_eq!(loop_body_stmts.len(), 3);
        if let StmtKind::Const { name, value, .. } = &ast.stmt(loop_body_stmts[0]).kind {
            assert_eq!(name.name, "a");
            assert!(matches!(
                ast.expr(*value).kind,
                crate::ExprKind::IndexAccess { .. }
            ));
            // Origin-tagged so the typechecker can rephrase index errors.
            assert!(ast.pattern_origins.contains_key(value));
        } else {
            panic!("expected Const a");
        }
        if let StmtKind::Const { name, .. } = &ast.stmt(loop_body_stmts[1]).kind {
            assert_eq!(name.name, "b");
        } else {
            panic!("expected Const b");
        }
    }

    #[test]
    fn lowers_let_pattern_preserves_mutability() {
        // __dst is const; per-field bindings follow source kind (`let` stays `let`)
        let ast = pipeline("let { a } = obj;");
        no_patterns_left(&ast);
        // __dst is Const; `a` is Let.
        assert!(matches!(
            ast.stmt(ast.top_level[0]).kind,
            StmtKind::Const { .. }
        ));
        assert!(matches!(
            ast.stmt(ast.top_level[1]).kind,
            StmtKind::Let { .. }
        ));
    }

    #[test]
    fn no_op_on_plain_program() {
        let src = "const x = 1; function main(): void { }";
        let before = {
            let mut asi = Asi::new(src, crate::FileId(0));
            let mut tokens: Vec<Token> = Vec::new();
            loop {
                let t = asi.next_token();
                let is_eof = matches!(t.kind, TokenKind::Eof);
                tokens.push(t);
                if is_eof {
                    break;
                }
            }
            let _ = asi.into_diagnostics();
            let (ast, _) = parse(src, tokens, crate::FileId(0));
            ast
        };
        let after = pipeline(src);
        assert_eq!(format!("{before:?}"), format!("{after:?}"));
    }

    #[test]
    fn lowers_pattern_in_nested_block() {
        let src = "function main(): void { const { a } = obj; }";
        let ast = pipeline(src);
        no_patterns_left(&ast);
    }

    #[test]
    fn idempotent() {
        let src = "const { a, b } = obj;";
        let mut ast1 = pipeline(src);
        let snapshot = format!("{ast1:?}");
        lower(&mut ast1);
        assert_eq!(snapshot, format!("{ast1:?}"));
    }

    #[test]
    fn binding_span_preserved() {
        let (ast, _) = parse(
            "const { a } = obj;",
            tokens_of("const { a } = obj;"),
            crate::FileId(0),
        );
        match &ast.stmt(ast.top_level[0]).kind {
            crate::StmtKind::ConstPattern {
                binding: Binding::Object { span, .. },
                ..
            } => {
                // `{ a }` starts at offset 6 and ends at 11 (inclusive of `}`).
                assert_eq!(*span, crate::Span::new(crate::FileId(0), 6, 11));
            }
            other => panic!("expected ConstPattern Object, got {other:?}"),
        }
    }

    fn tokens_of(source: &str) -> Vec<Token> {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens = Vec::new();
        loop {
            let t = asi.next_token();
            let is_eof = matches!(t.kind, TokenKind::Eof);
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let _ = asi.into_diagnostics();
        tokens
    }
}
