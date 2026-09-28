//! Lowers statement-position field/index postfix to assignment. Binding postfix
//! stays as `PostfixUnary` so codegen retains both the declared storage type and
//! the narrowed numeric result type. Byte postfix also stays an expression so
//! its modulo wrapping is identical in statement and expression positions.

use crate::{
    BinOp, PostfixOp, PostfixTarget, StmtId, Type, TypedExpr, TypedExprKind, TypedStmtKind,
};

use super::DesugarCtx;

pub(super) fn run(ctx: &mut DesugarCtx) -> Result<(), crate::compiler_error::CompilerFailure> {
    for id in ctx
        .ta
        .stmt_ids()
        .map_err(crate::typechecker::arena_failure)?
    {
        if let TypedStmtKind::Expr(eid) = ctx
            .ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
            && let TypedExprKind::PostfixUnary { op, target } = ctx
                .ta
                .try_expr(eid)
                .map_err(crate::typechecker::arena_failure)?
                .kind
                .clone()
            && !matches!(
                target,
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. }
            )
        {
            match &target {
                PostfixTarget::Field { receiver, .. } => {
                    ctx.ta
                        .try_expr(*receiver)
                        .map_err(crate::typechecker::arena_failure)?;
                }
                PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    ctx.ta
                        .try_expr(*receiver)
                        .map_err(crate::typechecker::arena_failure)?;
                    ctx.ta
                        .try_expr(*index)
                        .map_err(crate::typechecker::arena_failure)?;
                }
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {}
            }
            if matches!(&target, PostfixTarget::Index { receiver, .. }
                if ctx.ta.try_expr(*receiver).map_err(crate::typechecker::arena_failure)?.ty.peel() == &Type::Uint8Array)
            {
                continue;
            }
            lower(ctx, id, op, target)?;
        }
    }
    Ok(())
}

fn lower(
    ctx: &mut DesugarCtx,
    stmt_id: StmtId,
    op: PostfixOp,
    target: PostfixTarget,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let span = ctx
        .ta
        .try_stmt(stmt_id)
        .map_err(crate::typechecker::arena_failure)?
        .span;
    let bin_op = match op {
        PostfixOp::Inc => BinOp::Add,
        PostfixOp::Dec => BinOp::Sub,
        PostfixOp::NonNullAssert => {
            return Err(crate::typechecker::invariant_failure(
                "non-null assertion is not PostfixUnary",
            ));
        }
    };
    let new_kind = match target {
        PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {
            return Err(crate::typechecker::invariant_failure(
                "binding postfix must remain an expression",
            ));
        }
        PostfixTarget::Field {
            receiver,
            name,
            target_ty,
        } => {
            let read = ctx
                .ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::FieldAccess {
                        receiver,
                        name: name.clone(),
                    },
                    span,
                    ty: target_ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            let (rhs, bin_ty) = one_rhs_and_binary_ty(ctx, &target_ty)?;
            let value = ctx
                .ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Binary {
                        op: bin_op,
                        lhs: read,
                        rhs,
                    },
                    span,
                    ty: bin_ty,
                })
                .map_err(crate::typechecker::arena_failure)?;
            TypedStmtKind::AssignField {
                receiver,
                name,
                value,
            }
        }
        PostfixTarget::Index {
            receiver,
            index,
            elem_ty,
        } => {
            let read = ctx
                .ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::IndexAccess { receiver, index },
                    span,
                    ty: elem_ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            let (rhs, bin_ty) = one_rhs_and_binary_ty(ctx, &elem_ty)?;
            let value = ctx
                .ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Binary {
                        op: bin_op,
                        lhs: read,
                        rhs,
                    },
                    span,
                    ty: bin_ty,
                })
                .map_err(crate::typechecker::arena_failure)?;
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                elem_ty,
            }
        }
    };
    ctx.ta
        .try_stmt_mut(stmt_id)
        .map_err(crate::typechecker::arena_failure)?
        .kind = new_kind;
    Ok(())
}

fn one_rhs_and_binary_ty(
    ctx: &mut DesugarCtx,
    target_ty: &Type,
) -> Result<(crate::ExprId, Type), crate::compiler_error::CompilerFailure> {
    Ok(if matches!(target_ty.peel(), Type::BigInt) {
        (ctx.bigint_lit("1")?, Type::BigInt)
    } else {
        (ctx.number_lit(1.0)?, Type::Number)
    })
}
