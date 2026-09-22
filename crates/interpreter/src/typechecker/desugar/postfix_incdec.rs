//! Lowers statement-position field/index postfix to assignment. Binding postfix
//! stays as `PostfixUnary` so codegen retains both the declared storage type and
//! the narrowed numeric result type. Byte postfix also stays an expression so
//! its modulo wrapping is identical in statement and expression positions.

use crate::{
    BinOp, PostfixOp, PostfixTarget, StmtId, Type, TypedExpr, TypedExprKind, TypedStmtKind,
};

use super::DesugarCtx;

pub(super) fn run(ctx: &mut DesugarCtx) {
    let mut i = 0;
    while i < ctx.ta.stmts_len() {
        let id = StmtId(i as u32);
        if let TypedStmtKind::Expr(eid) = ctx.ta.stmt(id).kind
            && let TypedExprKind::PostfixUnary { op, target } = ctx.ta.expr(eid).kind.clone()
            && !matches!(
                target,
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. }
            )
        {
            if matches!(&target, PostfixTarget::Index { receiver, .. }
                if ctx.ta.expr(*receiver).ty.peel() == &Type::Uint8Array)
            {
                i += 1;
                continue;
            }
            lower(ctx, id, op, target);
        }
        i += 1;
    }
}

fn lower(ctx: &mut DesugarCtx, stmt_id: StmtId, op: PostfixOp, target: PostfixTarget) {
    let span = ctx.ta.stmt(stmt_id).span;
    let bin_op = match op {
        PostfixOp::Inc => BinOp::Add,
        PostfixOp::Dec => BinOp::Sub,
        PostfixOp::NonNullAssert => unreachable!("non-null assertion is not PostfixUnary"),
    };
    let new_kind = match target {
        PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {
            unreachable!("binding postfix stays an expression to preserve its narrowed read type")
        }
        PostfixTarget::Field {
            receiver,
            name,
            target_ty,
        } => {
            let read = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::FieldAccess {
                    receiver,
                    name: name.clone(),
                },
                span,
                ty: target_ty.clone(),
            });
            let (rhs, bin_ty) = one_rhs_and_binary_ty(ctx, &target_ty);
            let value = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: bin_op,
                    lhs: read,
                    rhs,
                },
                span,
                ty: bin_ty,
            });
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
            let read = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::IndexAccess { receiver, index },
                span,
                ty: elem_ty.clone(),
            });
            let (rhs, bin_ty) = one_rhs_and_binary_ty(ctx, &elem_ty);
            let value = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: bin_op,
                    lhs: read,
                    rhs,
                },
                span,
                ty: bin_ty,
            });
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                elem_ty,
            }
        }
    };
    ctx.ta.stmt_mut(stmt_id).kind = new_kind;
}

fn one_rhs_and_binary_ty(ctx: &mut DesugarCtx, target_ty: &Type) -> (crate::ExprId, Type) {
    if matches!(target_ty.peel(), Type::BigInt) {
        (ctx.bigint_lit("1"), Type::BigInt)
    } else {
        (ctx.number_lit(1.0), Type::Number)
    }
}
