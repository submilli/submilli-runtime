//! Module variables read or written before their declaration has run.
//!
//! A function can run during module initialization, before a later `let`,
//! `const` or static field has been initialized. JavaScript throws a
//! `ReferenceError` there. A Wasm global already holds a zero or null default,
//! so a guarded global gets an `i32` flag that the start function sets once its
//! binding is initialized; every other access checks the flag first.
//!
//! A `let` or `const` is initialized once its declaration stores its value. A
//! static field's binding is its class, which JavaScript initializes before any
//! static initializer runs, so the statics of one class share a flag set before
//! the first of them; a static read during the class's own static
//! initialization finds the field's default, where JavaScript finds
//! `undefined`.
//!
//! Only a global declared after some top-level code that could call a function
//! is guarded. Before that point nothing can reach a function body, so the
//! global is always initialized by the time any function reads it.

use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{BlockType, ConstExpr, GlobalSection, GlobalType, Instruction, ValType};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::symbol_table::SymbolTable;
use crate::compiler_error::CompilerFailure;
use crate::{MangledName, StmtId, TypedAst, TypedExprKind, TypedStmtKind};

/// One guarded global, and how its binding is marked initialized.
#[derive(Clone, Debug, PartialEq)]
pub struct InitGuard {
    pub global: MangledName,
    /// What the flag stands for: the variable, or a static field's class.
    pub binding: String,
    /// The top-level statement that marks the binding initialized.
    pub marked_by: StmtId,
    pub mark: Mark,
}

/// When the marking statement sets the flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Before the statement computes its value: a class's statics.
    BeforeValue,
    /// After the statement stores its value: a `let` or `const`.
    AfterStore,
}

/// A guarded global's flag, as codegen allocated it.
#[derive(Clone, Debug, PartialEq)]
pub struct InitFlag {
    pub flag_idx: u32,
    pub marked_by: StmtId,
    pub mark: Mark,
    pub message: String,
}

/// The module globals that need an initialization flag, in declaration order.
pub fn guarded_globals(ta: &TypedAst) -> Result<Vec<InitGuard>, CompilerFailure> {
    let declared: BTreeSet<&MangledName> = ta.globals.iter().map(|g| &g.mangled_name).collect();
    let mut seen = BTreeSet::new();
    let mut class_marks: BTreeMap<String, StmtId> = BTreeMap::new();
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
        if !declared.contains(mangled) {
            continue;
        }
        if !seen.insert(mangled) {
            continue;
        }
        if !code_may_have_run {
            continue;
        }
        guards.push(match ident.name.split_once('.') {
            Some((class, _)) => InitGuard {
                global: mangled.clone(),
                binding: class.to_string(),
                marked_by: *class_marks.entry(class.to_string()).or_insert(stmt_id),
                mark: Mark::BeforeValue,
            },
            None => InitGuard {
                global: mangled.clone(),
                binding: ident.name.clone(),
                marked_by: stmt_id,
                mark: Mark::AfterStore,
            },
        });
    }
    Ok(guards)
}

/// The `ReferenceError` message for a binding, worded as Node words it.
pub fn before_initialization_message(binding: &str) -> String {
    format!("Cannot access '{binding}' before initialization")
}

/// Allocates one flag per guarded binding, starting unset. Returns how many
/// globals were added.
pub fn allocate_flags(
    ta: &TypedAst,
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
) -> Result<u32, CompilerFailure> {
    let mut flags: BTreeMap<String, u32> = BTreeMap::new();
    for guard in guarded_globals(ta)? {
        let flag_idx = match flags.get(&guard.binding) {
            Some(&idx) => idx,
            None => {
                globals.global(
                    GlobalType {
                        val_type: ValType::I32,
                        mutable: true,
                        shared: false,
                    },
                    &ConstExpr::i32_const(0),
                );
                let idx = *next_global_idx;
                crate::codegen::next_index(next_global_idx)?;
                flags.insert(guard.binding.clone(), idx);
                idx
            }
        };
        symbols.record_init_guard(
            guard.global,
            InitFlag {
                flag_idx,
                marked_by: guard.marked_by,
                mark: guard.mark,
                message: before_initialization_message(&guard.binding),
            },
        );
    }
    crate::codegen::wasm_u32(flags.len())
}

/// Before an access to a guarded global, throw unless its binding is
/// initialized.
pub(crate) fn emit_check(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, global: &MangledName) {
    let Some(flag) = ctx.symbols.init_guard(global) else {
        return;
    };
    emitter.instruction(Instruction::GlobalGet(flag.flag_idx));
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    crate::codegen::throw::emit_reference_error_throw(emitter, ctx, &flag.message);
    emitter.emit_end();
}

/// When `stmt`, a write to `global`, marks the global's binding initialized:
/// `None` when it doesn't, and the write is checked instead.
pub(crate) fn mark_at(ctx: &CodegenCtx, global: &MangledName, stmt: StmtId) -> Option<Mark> {
    ctx.symbols
        .init_guard(global)
        .filter(|flag| flag.marked_by == stmt)
        .map(|flag| flag.mark)
}

pub(crate) fn emit_mark_initialized(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    global: &MangledName,
) {
    let Some(flag) = ctx.symbols.init_guard(global) else {
        return;
    };
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::GlobalSet(flag.flag_idx));
}

/// Whether evaluating `expr` can never run a function body.
fn is_inert(ta: &TypedAst, expr: crate::ExprId) -> Result<bool, CompilerFailure> {
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

fn is_primitive_literal(ta: &TypedAst, expr: crate::ExprId) -> Result<bool, CompilerFailure> {
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
) -> Result<bool, CompilerFailure> {
    exprs.try_fold(true, |inert, id| Ok(inert && is_inert(ta, id)?))
}
