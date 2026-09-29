//! Lower `do body while (c);` to a `while`-true whose head tests `c` on every
//! pass but the first.

use crate::{StmtId, Type, TypedExpr, TypedExprKind, TypedStmtKind, UnOp};

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
            TypedStmtKind::DoWhile { .. }
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
    let TypedStmtKind::DoWhile { body, condition } = ctx
        .ta
        .try_stmt(id)
        .map_err(crate::typechecker::arena_failure)?
        .kind
        .clone()
    else {
        return Ok(());
    };

    ctx.ta
        .try_stmt(body)
        .map_err(crate::typechecker::arena_failure)?;
    let condition_span = ctx
        .ta
        .try_expr(condition)
        .map_err(crate::typechecker::arena_failure)?
        .span;

    // The test is the condition's own code, so it keeps the condition's span.
    let not_cond = ctx
        .ta
        .try_push_expr(TypedExpr {
            kind: TypedExprKind::Unary {
                op: UnOp::Not,
                operand: condition,
            },
            span: condition_span,
            ty: Type::Boolean,
        })
        .map_err(crate::typechecker::arena_failure)?;
    let break_stmt = ctx.push_stmt(TypedStmtKind::Break, span)?;
    let then_block = ctx.push_stmt(TypedStmtKind::Block(vec![break_stmt]), span)?;
    let if_stmt = ctx.push_stmt(
        TypedStmtKind::If {
            condition: not_cond,
            then_block,
            else_block: None,
        },
        span,
    )?;

    let (flag, let_flag) = ctx.first_pass_flag("do_first", span)?;
    let step = ctx.skip_on_first_pass(&flag, vec![if_stmt], span)?;

    // The body keeps its own block. The narrowing fixed point may have wrapped it in
    // `NarrowRegion`s whose shadow bindings scope over everything inside, so splicing
    // its statements out beside the head would orphan them — and body-scoped
    // declarations would land in the head's scope, which the condition was not typed
    // against.
    let new_body = ctx.push_stmt(TypedStmtKind::Block(vec![step, body]), span)?;

    let cond_true = ctx.bool_true()?;
    let while_stmt = ctx.push_stmt(
        TypedStmtKind::While {
            condition: cond_true,
            body: new_body,
        },
        span,
    )?;
    ctx.ta
        .try_stmt_mut(id)
        .map_err(crate::typechecker::arena_failure)?
        .kind = TypedStmtKind::Block(vec![let_flag, while_stmt]);
    Ok(())
}
