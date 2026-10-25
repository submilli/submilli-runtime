//! Lower `for (init; cond; update) body` to a `while` loop.

use crate::{ExprId, Ident, Span, StmtId, Type, TypedStmtKind};

use super::DesugarCtx;

pub(super) fn run(ctx: &mut DesugarCtx) -> Result<(), crate::compiler_error::CompilerFailure> {
    for id in ctx
        .ta
        .stmt_ids()
        .map_err(crate::typechecker::arena_failure)?
    {
        if matches!(
            ctx.ta
                .try_stmt(id)
                .map_err(crate::typechecker::arena_failure)?
                .kind,
            TypedStmtKind::For { .. }
        ) {
            lower(ctx, id)?;
        }
    }
    Ok(())
}

fn lower(ctx: &mut DesugarCtx, id: StmtId) -> Result<(), crate::compiler_error::CompilerFailure> {
    let span = ctx
        .ta
        .try_stmt(id)
        .map_err(crate::typechecker::arena_failure)?
        .span;
    let TypedStmtKind::For {
        init,
        condition,
        update,
        body,
    } = ctx
        .ta
        .try_stmt(id)
        .map_err(crate::typechecker::arena_failure)?
        .kind
        .clone()
    else {
        return Ok(());
    };

    for &statement in init.iter().chain(&update).chain(std::iter::once(&body)) {
        ctx.ta
            .try_stmt(statement)
            .map_err(crate::typechecker::arena_failure)?;
    }
    if let Some(condition) = condition {
        ctx.ta
            .try_expr(condition)
            .map_err(crate::typechecker::arena_failure)?;
    }

    // The reboxes precede the update because JS copies the bindings into the new
    // per-iteration environment first and *then* runs the update in it, so the
    // update mutates the new binding rather than the one the last pass's closures
    // captured. With neither to do, the plain `while` shape suffices.
    let rebox = per_iteration_rebox(ctx, &init, span)?;
    let (pre_while, while_stmt) = if update.is_some() || !rebox.is_empty() {
        let (flag, let_flag) = ctx.first_pass_flag("for_first", span)?;
        let head: Vec<StmtId> = rebox.into_iter().chain(update).collect();
        let step = ctx.skip_on_first_pass(&flag, head, span)?;
        let guarded = guard_body(ctx, condition, body, span)?;
        let while_body = ctx.push_stmt(TypedStmtKind::Block(vec![step, guarded]), span)?;
        let cond_true = ctx.bool_true()?;
        let w = ctx.push_stmt(
            TypedStmtKind::While {
                condition: cond_true,
                body: while_body,
            },
            span,
        )?;
        (Some(let_flag), w)
    } else {
        let cond = condition.map_or_else(|| ctx.bool_true(), Ok)?;
        let w = ctx.push_stmt(
            TypedStmtKind::While {
                condition: cond,
                body,
            },
            span,
        )?;
        (None, w)
    };

    let mut outer_stmts = Vec::with_capacity(init.len() + 2);
    outer_stmts.extend(init);
    outer_stmts.extend(pre_while);
    outer_stmts.push(while_stmt);
    ctx.ta
        .try_stmt_mut(id)
        .map_err(crate::typechecker::arena_failure)?
        .kind = TypedStmtKind::Block(outer_stmts);
    Ok(())
}

/// A `ReboxLocal` for each of the head's `let`s that is boxed — i.e. captured, the
/// only case where sharing one cell across iterations is observable. A `const`
/// head needs none: it can't be reassigned, so every copy would hold the same
/// value, and a captured `const` is copied into the closure env rather than boxed.
fn per_iteration_rebox(
    ctx: &mut DesugarCtx,
    init: &[StmtId],
    span: Span,
) -> Result<Vec<StmtId>, crate::compiler_error::CompilerFailure> {
    let mut reboxes = Vec::new();
    for &id in init {
        if let Some((ident, ty)) = boxed_let(ctx, id)? {
            reboxes.push(ctx.push_stmt(TypedStmtKind::ReboxLocal { ident, ty }, span)?);
        }
    }
    Ok(reboxes)
}

fn boxed_let(
    ctx: &DesugarCtx,
    id: StmtId,
) -> Result<Option<(Ident, Type)>, crate::compiler_error::CompilerFailure> {
    Ok(
        match &ctx
            .ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedStmtKind::Let {
                name, ty, boxed, ..
            } if *boxed => Some((name.clone(), ty.clone())),
            _ => None,
        },
    )
}

/// With a condition, wraps the body in `if (cond) { body } else { break; }` so the test
/// runs after the top-of-loop update. Without one, the body runs unconditionally.
fn guard_body(
    ctx: &mut DesugarCtx,
    condition: Option<ExprId>,
    body: StmtId,
    span: Span,
) -> Result<StmtId, crate::compiler_error::CompilerFailure> {
    let Some(cond) = condition else {
        return Ok(body);
    };
    let body_stmts = ctx.body_as_stmts(body)?;
    let then_block = ctx.push_stmt(TypedStmtKind::Block(body_stmts), span)?;
    let break_stmt = ctx.push_stmt(TypedStmtKind::Break, span)?;
    let else_block = ctx.push_stmt(TypedStmtKind::Block(vec![break_stmt]), span)?;
    ctx.push_stmt(
        TypedStmtKind::If {
            condition: cond,
            then_block,
            else_block: Some(else_block),
        },
        span,
    )
}
