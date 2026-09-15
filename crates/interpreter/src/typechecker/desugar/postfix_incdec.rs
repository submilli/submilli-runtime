//! Lowers statement-position `x++`/`x--` to assignment; expression-position stays as
//! `PostfixUnary` for codegen. Receiver/index ExprIds are shared between the read and
//! write halves — non-pure receivers like `getObj().f++` would double-evaluate.

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
        {
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
        PostfixTarget::Local {
            ident,
            boxed,
            target_ty,
        } => {
            let lhs = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::LocalRef {
                    ident: ident.clone(),
                    boxed,
                },
                span,
                ty: target_ty.clone(),
            });
            let (rhs, bin_ty) = one_rhs_and_binary_ty(ctx, &target_ty);
            let value = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: bin_op,
                    lhs,
                    rhs,
                },
                span,
                ty: bin_ty,
            });
            TypedStmtKind::AssignLocal {
                ident,
                target_ty,
                value,
                boxed,
                narrowed_shadow_ty: None,
            }
        }
        PostfixTarget::Global {
            name,
            mangled,
            target_ty,
        } => {
            let lhs = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::GlobalRef {
                    mangled: mangled.clone(),
                    name: name.clone(),
                },
                span,
                ty: target_ty.clone(),
            });
            let (rhs, bin_ty) = one_rhs_and_binary_ty(ctx, &target_ty);
            let value = ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: bin_op,
                    lhs,
                    rhs,
                },
                span,
                ty: bin_ty,
            });
            TypedStmtKind::AssignGlobal {
                ident: name,
                mangled,
                target_ty,
                value,
            }
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
