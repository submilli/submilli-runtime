//! Lower `for (init; cond; update) body` to a `while` loop.

use crate::{ExprId, Ident, Span, StmtId, Type, TypedStmtKind};

use super::DesugarCtx;

pub(super) fn run(ctx: &mut DesugarCtx) {
    let mut i = 0;
    while i < ctx.ta.stmts_len() {
        let id = StmtId(i as u32);
        if matches!(ctx.ta.stmt(id).kind, TypedStmtKind::For { .. }) {
            lower(ctx, id);
        }
        i += 1;
    }
}

fn lower(ctx: &mut DesugarCtx, id: StmtId) {
    let span = ctx.ta.stmt(id).span;
    let TypedStmtKind::For {
        init,
        condition,
        update,
        body,
    } = ctx.ta.stmt(id).kind.clone()
    else {
        return;
    };

    // The reboxes precede the update because JS copies the bindings into the new
    // per-iteration environment first and *then* runs the update in it, so the
    // update mutates the new binding rather than the one the last pass's closures
    // captured. With neither to do, the plain `while` shape suffices.
    let rebox = per_iteration_rebox(ctx, init, span);
    let (pre_while, while_stmt) = if update.is_some() || rebox.is_some() {
        let (flag, let_flag) = ctx.first_pass_flag("for_first", span);
        let head: Vec<StmtId> = rebox.into_iter().chain(update).collect();
        let step = ctx.skip_on_first_pass(&flag, head, span);
        let guarded = guard_body(ctx, condition, body, span);
        let while_body = ctx.push_stmt(TypedStmtKind::Block(vec![step, guarded]), span);
        let cond_true = ctx.bool_true();
        let w = ctx.push_stmt(
            TypedStmtKind::While {
                condition: cond_true,
                body: while_body,
            },
            span,
        );
        (Some(let_flag), w)
    } else {
        let cond = condition.unwrap_or_else(|| ctx.bool_true());
        let w = ctx.push_stmt(
            TypedStmtKind::While {
                condition: cond,
                body,
            },
            span,
        );
        (None, w)
    };

    let mut outer_stmts = Vec::with_capacity(3);
    outer_stmts.extend(init);
    outer_stmts.extend(pre_while);
    outer_stmts.push(while_stmt);
    ctx.ta.stmt_mut(id).kind = TypedStmtKind::Block(outer_stmts);
}

/// A `ReboxLocal` for the head's `let`, when it is boxed — i.e. captured, the only
/// case where sharing one cell across iterations is observable. A `const` head
/// needs none: it can't be reassigned, so every copy would hold the same value,
/// and a captured `const` is copied into the closure env rather than boxed.
fn per_iteration_rebox(ctx: &mut DesugarCtx, init: Option<StmtId>, span: Span) -> Option<StmtId> {
    let (ident, ty) = init.and_then(|id| boxed_let(ctx, id))?;
    Some(ctx.push_stmt(TypedStmtKind::ReboxLocal { ident, ty }, span))
}

fn boxed_let(ctx: &DesugarCtx, id: StmtId) -> Option<(Ident, Type)> {
    match &ctx.ta.stmt(id).kind {
        TypedStmtKind::Let {
            name, ty, boxed, ..
        } if *boxed => Some((name.clone(), ty.clone())),
        _ => None,
    }
}

/// With a condition, wraps the body in `if (cond) { body } else { break; }` so the test
/// runs after the top-of-loop update. Without one, the body runs unconditionally.
fn guard_body(ctx: &mut DesugarCtx, condition: Option<ExprId>, body: StmtId, span: Span) -> StmtId {
    let Some(cond) = condition else {
        return body;
    };
    let body_stmts = ctx.body_as_stmts(body);
    let then_block = ctx.push_stmt(TypedStmtKind::Block(body_stmts), span);
    let break_stmt = ctx.push_stmt(TypedStmtKind::Break, span);
    let else_block = ctx.push_stmt(TypedStmtKind::Block(vec![break_stmt]), span);
    ctx.push_stmt(
        TypedStmtKind::If {
            condition: cond,
            then_block,
            else_block: Some(else_block),
        },
        span,
    )
}
