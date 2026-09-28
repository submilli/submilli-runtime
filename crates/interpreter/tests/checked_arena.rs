use interpreter::{
    Ast, Expr, ExprId, ExprKind, FileId, Span, Stmt, StmtId, StmtKind, Type, TypedAst, TypedExpr,
    TypedExprKind, TypedStmt, TypedStmtKind,
    arena::{ArenaError, ArenaKind, ArenaOperation},
};

#[test]
fn checked_arena_parsed_access_rejects_invalid_ids_without_mutation() {
    let mut ast = Ast::new();
    assert!(ast.try_expr(ExprId(0)).is_err());
    assert!(ast.try_stmt(StmtId(0)).is_err());
    let span = Span::at(FileId(0));
    let expr = ast
        .try_push_expr(Expr {
            kind: ExprKind::Number(7.0),
            span,
        })
        .unwrap();
    let stmt = ast
        .try_push_stmt(Stmt {
            kind: StmtKind::Expr(expr),
            span,
        })
        .unwrap();
    for id in [1, u32::MAX] {
        assert_invalid(
            ast.try_expr(ExprId(id)).unwrap_err(),
            ArenaKind::Expressions,
            ArenaOperation::Read,
            id,
        );
        assert_invalid(
            ast.try_expr_mut(ExprId(id)).unwrap_err(),
            ArenaKind::Expressions,
            ArenaOperation::Mutate,
            id,
        );
        assert_invalid(
            ast.try_stmt(StmtId(id)).unwrap_err(),
            ArenaKind::Statements,
            ArenaOperation::Read,
            id,
        );
        assert_invalid(
            ast.try_stmt_mut(StmtId(id)).unwrap_err(),
            ArenaKind::Statements,
            ArenaOperation::Mutate,
            id,
        );
    }
    assert_eq!(ast.try_expr(expr).unwrap().kind, ExprKind::Number(7.0));
    assert_eq!(ast.try_stmt(stmt).unwrap().kind, StmtKind::Expr(expr));
    ast.try_expr_mut(expr).unwrap().kind = ExprKind::Boolean(true);
    ast.try_stmt_mut(stmt).unwrap().span = Span::at(FileId(1));
    assert_eq!(ast.try_expr(expr).unwrap().kind, ExprKind::Boolean(true));
    assert_eq!(ast.try_stmt(stmt).unwrap().span.file, FileId(1));
    assert_eq!(
        ast.try_push_expr(Expr {
            kind: ExprKind::Null,
            span
        })
        .unwrap(),
        ExprId(1)
    );
    assert_eq!(ast.exprs_len(), 2);
    assert_eq!(ast.stmts_len(), 1);
}

#[test]
fn checked_arena_typed_access_rejects_invalid_ids_without_mutation() {
    let mut ast = TypedAst::new();
    assert!(ast.try_expr_mut(ExprId(0)).is_err());
    assert!(ast.try_stmt_mut(StmtId(0)).is_err());
    let span = Span::at(FileId(0));
    let expr = ast
        .try_push_expr(TypedExpr {
            kind: TypedExprKind::Number(7.0),
            span,
            ty: Type::Number,
        })
        .unwrap();
    let stmt = ast
        .try_push_stmt(TypedStmt {
            kind: TypedStmtKind::Expr(expr),
            span,
        })
        .unwrap();
    for id in [1, u32::MAX] {
        assert_invalid(
            ast.try_expr(ExprId(id)).unwrap_err(),
            ArenaKind::TypedExpressions,
            ArenaOperation::Read,
            id,
        );
        assert_invalid(
            ast.try_expr_mut(ExprId(id)).unwrap_err(),
            ArenaKind::TypedExpressions,
            ArenaOperation::Mutate,
            id,
        );
        assert_invalid(
            ast.try_stmt(StmtId(id)).unwrap_err(),
            ArenaKind::TypedStatements,
            ArenaOperation::Read,
            id,
        );
        assert_invalid(
            ast.try_stmt_mut(StmtId(id)).unwrap_err(),
            ArenaKind::TypedStatements,
            ArenaOperation::Mutate,
            id,
        );
    }
    assert_eq!(ast.try_expr(expr).unwrap().ty, Type::Number);
    assert_eq!(ast.try_stmt(stmt).unwrap().kind, TypedStmtKind::Expr(expr));
    ast.try_expr_mut(expr).unwrap().span = Span::at(FileId(1));
    ast.try_stmt_mut(stmt).unwrap().span = Span::at(FileId(2));
    assert_eq!(ast.try_expr(expr).unwrap().span.file, FileId(1));
    assert_eq!(ast.try_stmt(stmt).unwrap().span.file, FileId(2));
    assert_eq!(
        ast.try_push_stmt(TypedStmt {
            kind: TypedStmtKind::Expr(expr),
            span
        })
        .unwrap(),
        StmtId(1)
    );
    assert_eq!(ast.exprs_len(), 1);
    assert_eq!(ast.stmts_len(), 2);
}

#[test]
fn checked_arena_id_iteration_is_an_owned_snapshot() {
    let mut parsed = Ast::new();
    let mut typed = TypedAst::new();
    assert_eq!(parsed.expr_ids().unwrap().count(), 0);
    assert_eq!(parsed.stmt_ids().unwrap().count(), 0);
    assert_eq!(typed.expr_ids().unwrap().count(), 0);
    assert_eq!(typed.stmt_ids().unwrap().count(), 0);
    let span = Span::at(FileId(0));
    for _ in 0..2 {
        let expr = parsed
            .try_push_expr(Expr {
                kind: ExprKind::Null,
                span,
            })
            .unwrap();
        parsed
            .try_push_stmt(Stmt {
                kind: StmtKind::Expr(expr),
                span,
            })
            .unwrap();
        let expr = typed
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Null,
                ty: Type::Null,
                span,
            })
            .unwrap();
        typed
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Expr(expr),
                span,
            })
            .unwrap();
    }
    let parsed_exprs = parsed.expr_ids().unwrap();
    let parsed_stmts = parsed.stmt_ids().unwrap();
    let typed_exprs = typed.expr_ids().unwrap();
    let typed_stmts = typed.stmt_ids().unwrap();
    parsed
        .try_push_expr(Expr {
            kind: ExprKind::Null,
            span,
        })
        .unwrap();
    parsed
        .try_push_stmt(Stmt {
            kind: StmtKind::Expr(ExprId(0)),
            span,
        })
        .unwrap();
    typed
        .try_push_expr(TypedExpr {
            kind: TypedExprKind::Null,
            ty: Type::Null,
            span,
        })
        .unwrap();
    typed
        .try_push_stmt(TypedStmt {
            kind: TypedStmtKind::Expr(ExprId(0)),
            span,
        })
        .unwrap();
    assert_eq!(parsed_exprs.collect::<Vec<_>>(), [ExprId(0), ExprId(1)]);
    assert_eq!(
        parsed_stmts.rev().collect::<Vec<_>>(),
        [StmtId(1), StmtId(0)]
    );
    assert_eq!(
        typed_exprs.rev().collect::<Vec<_>>(),
        [ExprId(1), ExprId(0)]
    );
    assert_eq!(typed_stmts.collect::<Vec<_>>(), [StmtId(0), StmtId(1)]);
}

fn assert_invalid(error: ArenaError, arena: ArenaKind, operation: ArenaOperation, id: u32) {
    let ArenaError::InvalidId {
        arena: actual_arena,
        operation: actual_operation,
        id: actual_id,
        len,
    } = error
    else {
        panic!("expected an invalid-ID failure, got {error}");
    };
    assert_eq!(actual_arena, arena);
    assert_eq!(actual_operation, operation);
    assert_eq!(actual_id, id);
    assert_eq!(len, 1);
}
