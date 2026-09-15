//! Lower `do body while (c);` to a `while`-true whose head tests `c` on every
//! pass but the first.

use crate::{StmtId, Type, TypedExpr, TypedExprKind, TypedStmtKind, UnOp};

use super::DesugarCtx;

pub(super) fn run(ctx: &mut DesugarCtx) {
    let mut i = 0;
    while i < ctx.ta.stmts_len() {
        let id = StmtId(i as u32);
        if matches!(ctx.ta.stmt(id).kind, TypedStmtKind::DoWhile { .. }) {
            lower(ctx, id);
        }
        i += 1;
    }
}

fn lower(ctx: &mut DesugarCtx, id: StmtId) {
    let span = ctx.ta.stmt(id).span;
    let TypedStmtKind::DoWhile { body, condition } = ctx.ta.stmt(id).kind.clone() else {
        return;
    };

    let not_cond = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::Unary {
            op: UnOp::Not,
            operand: condition,
        },
        span,
        ty: Type::Boolean,
    });
    let break_stmt = ctx.push_stmt(TypedStmtKind::Break, span);
    let then_block = ctx.push_stmt(TypedStmtKind::Block(vec![break_stmt]), span);
    let if_stmt = ctx.push_stmt(
        TypedStmtKind::If {
            condition: not_cond,
            then_block,
            else_block: None,
        },
        span,
    );

    let (flag, let_flag) = ctx.first_pass_flag("do_first", span);
    let step = ctx.skip_on_first_pass(&flag, vec![if_stmt], span);

    // The body keeps its own block. The narrowing fixed point may have wrapped it in
    // `NarrowRegion`s whose shadow bindings scope over everything inside, so splicing
    // its statements out beside the head would orphan them — and body-scoped
    // declarations would land in the head's scope, which the condition was not typed
    // against.
    let new_body = ctx.push_stmt(TypedStmtKind::Block(vec![step, body]), span);

    let cond_true = ctx.bool_true();
    let while_stmt = ctx.push_stmt(
        TypedStmtKind::While {
            condition: cond_true,
            body: new_body,
        },
        span,
    );
    ctx.ta.stmt_mut(id).kind = TypedStmtKind::Block(vec![let_flag, while_stmt]);
}
