//! Give directly annotated JSON reads the ordinary checked-cast path before
//! inference starts. HTTP Response.json already infers its checked type argument.

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::{Ast, Expr, ExprKind, StmtKind};

pub(crate) fn lower(ast: &mut Ast) -> Result<(), CompilerFailure> {
    let arena_failure =
        |error: crate::arena::ArenaError| error.into_compiler_failure(CompilerStage::Infer);
    for id in ast.stmt_ids().map_err(arena_failure)? {
        let (ty, value) = match &ast.try_stmt(id).map_err(arena_failure)?.kind {
            StmtKind::Let {
                ty: Some(ty),
                value,
                ..
            }
            | StmtKind::Const {
                ty: Some(ty),
                value,
                ..
            } => (ty.clone(), *value),
            _ => continue,
        };
        let call = ast.try_peel_parens(value).map_err(arena_failure)?;
        let ExprKind::Call { callee, .. } = &ast.try_expr(call).map_err(arena_failure)?.kind else {
            continue;
        };
        let receiver = match &ast.try_expr(*callee).map_err(arena_failure)?.kind {
            ExprKind::FieldAccess { receiver, name } if name.name == "parse" => *receiver,
            ExprKind::IndexAccess { receiver, index }
                if matches!(&ast.try_expr(*index).map_err(arena_failure)?.kind,
                    ExprKind::String(key) if key == "parse") =>
            {
                *receiver
            }
            _ => continue,
        };
        if !matches!(&ast.try_expr(receiver).map_err(arena_failure)?.kind,
                ExprKind::Identifier(ident) if ident.name == "JSON")
        {
            continue;
        }
        let span = ast.try_expr(value).map_err(arena_failure)?.span;
        let cast = ast
            .try_push_expr(Expr {
                kind: ExprKind::As { expr: value, ty },
                span,
            })
            .map_err(arena_failure)?;
        ast.implicit_json_casts.insert(cast, value);
        match &mut ast.try_stmt_mut(id).map_err(arena_failure)?.kind {
            StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => *value = cast,
            _ => {}
        }
    }
    Ok(())
}
