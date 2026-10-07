//! Module variables read or written before their declaration has run.
//!
//! A function can run during module initialization, before a later `let`,
//! `const` or static field has been initialized. JavaScript throws a
//! `ReferenceError` there. A Wasm global already holds a zero or null default,
//! so a guarded global gets an `i32` flag that the start function sets right
//! after the declaration's initializer stores it; every other access checks
//! the flag first.
//!
//! Only a global declared after some top-level code that could call a function
//! is guarded. Before that point nothing can reach a function body, so the
//! global is always initialized by the time any function reads it.

use std::collections::BTreeSet;

use wasm_encoder::{BlockType, Instruction};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::{MangledName, StmtId, TypedAst, TypedExprKind, TypedStmtKind};

/// One guarded global: the top-level statement that initializes it, and the
/// message its `ReferenceError` carries.
#[derive(Clone, Debug, PartialEq)]
pub struct InitGuard {
    pub global: MangledName,
    pub declaration: StmtId,
    pub message: String,
}

/// A guarded global's flag, as codegen allocated it.
#[derive(Clone, Debug, PartialEq)]
pub struct InitFlag {
    pub flag_idx: u32,
    pub declaration: StmtId,
    pub message: String,
}

/// The module globals that need an initialization flag, in declaration order.
pub fn guarded_globals(
    ta: &TypedAst,
) -> Result<Vec<InitGuard>, crate::compiler_error::CompilerFailure> {
    let declared: BTreeSet<&MangledName> = ta.globals.iter().map(|g| &g.mangled_name).collect();
    let mut seen = BTreeSet::new();
    let mut code_may_have_run = false;
    let mut guards = Vec::new();
    for &stmt_id in &ta.top_level_statements {
        let stmt = ta
            .try_stmt(stmt_id)
            .map_err(crate::codegen::arena_failure)?;
        let TypedStmtKind::AssignGlobal {
            ident,
            mangled,
            value,
            ..
        } = &stmt.kind
        else {
            code_may_have_run = true;
            continue;
        };
        // The initializer runs before its own binding is initialized.
        code_may_have_run |= !is_inert(ta, *value)?;
        if !declared.contains(mangled) || !seen.insert(mangled) || !code_may_have_run {
            continue;
        }
        guards.push(InitGuard {
            global: mangled.clone(),
            declaration: stmt_id,
            message: before_initialization_message(&ident.name),
        });
    }
    Ok(guards)
}

/// Node names the class, not the field, when a static field is reached before
/// its class declaration has run; a static field's ident is `Class.field`.
fn before_initialization_message(name: &str) -> String {
    let binding = name.split('.').next().unwrap_or(name);
    format!("Cannot access '{binding}' before initialization")
}

/// Whether evaluating `expr` can never run a function body.
fn is_inert(
    ta: &TypedAst,
    expr: crate::ExprId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let expr = ta.try_expr(expr).map_err(crate::codegen::arena_failure)?;
    Ok(match &expr.kind {
        TypedExprKind::Number(_)
        | TypedExprKind::BigInt(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Boolean(_)
        | TypedExprKind::Null
        | TypedExprKind::FunctionRef { .. } => true,
        TypedExprKind::Unary { operand, .. } => is_primitive_literal(ta, *operand)?,
        TypedExprKind::Binary { lhs, rhs, .. } => {
            is_primitive_literal(ta, *lhs)? && is_primitive_literal(ta, *rhs)?
        }
        TypedExprKind::TupleLiteral { elements, .. } => all_inert(ta, elements.iter().copied())?,
        TypedExprKind::ArrayLiteral { elements, .. } => {
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                let crate::TypedArrayElement::Value(id) = element else {
                    return Ok(false);
                };
                values.push(*id);
            }
            all_inert(ta, values.into_iter())?
        }
        _ => false,
    })
}

fn is_primitive_literal(
    ta: &TypedAst,
    expr: crate::ExprId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let expr = ta.try_expr(expr).map_err(crate::codegen::arena_failure)?;
    Ok(matches!(
        expr.kind,
        TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
    ))
}

fn all_inert(
    ta: &TypedAst,
    mut exprs: impl Iterator<Item = crate::ExprId>,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    exprs.try_fold(true, |inert, id| Ok(inert && is_inert(ta, id)?))
}

/// Before an access to a guarded global, throw unless its declaration has run.
pub(crate) fn emit_check(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, global: &MangledName) {
    let Some(guard) = ctx.symbols.init_guard(global) else {
        return;
    };
    emitter.instruction(Instruction::GlobalGet(guard.flag_idx));
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    crate::codegen::throw::emit_reference_error_throw(emitter, ctx, &guard.message);
    emitter.emit_end();
}

/// Whether `stmt` is the initializer of a guarded global, which marks it
/// initialized rather than checking it.
pub(crate) fn is_declaration(ctx: &CodegenCtx, global: &MangledName, stmt: StmtId) -> bool {
    ctx.symbols
        .init_guard(global)
        .is_some_and(|guard| guard.declaration == stmt)
}

pub(crate) fn emit_mark_initialized(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    global: &MangledName,
) {
    let Some(guard) = ctx.symbols.init_guard(global) else {
        return;
    };
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::GlobalSet(guard.flag_idx));
}
