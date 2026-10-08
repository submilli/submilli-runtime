//! Module variables read or written before their declaration has run.
//!
//! A function can run during module initialization, before a later `let`,
//! `const` or static field has been initialized. JavaScript throws a
//! `ReferenceError` there. A Wasm global already holds a zero or null default,
//! so a guarded global gets an `i32` flag that the start function sets once its
//! binding is initialized; every other access checks the flag first.
//!
//! Each guarded global is initialized once its declaration stores its value.
//! A static field is guarded field by field, so a function that runs during
//! its class's static initialization reads an earlier static and throws on a
//! later one. JavaScript would find `undefined` there, which a typed Wasm slot
//! can't hold, and its default would be a wrong value or a null reference. The
//! message names the class, as Node's does for a class not yet initialized.
//!
//! A class is guarded the same way: constructing it, calling one of its
//! static methods or testing `instanceof` against it before its declaration
//! has run throws. Its flag is set where the declaration stands, before its
//! static fields initialize, so they can use the class.
//!
//! Only a binding declared after some top-level code that could call a
//! function is guarded. Before that point nothing can reach a function body,
//! so the binding is always initialized by the time any function uses it.

use std::collections::BTreeSet;

use wasm_encoder::{BlockType, ConstExpr, GlobalSection, GlobalType, Instruction, ValType};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::symbol_table::SymbolTable;
use crate::compiler_error::CompilerFailure;
use crate::{MangledName, StmtId, TypedAst, TypedExprKind, TypedStmtKind};

/// One guarded global and the top-level statement that declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct InitGuard {
    pub global: MangledName,
    /// The name the `ReferenceError` reports: the variable, or a static
    /// field's class.
    pub binding: String,
    pub declared_by: StmtId,
}

/// A class that can be used before its declaration has run.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassGuard {
    pub name: String,
    /// What a use of the class goes through: the class itself (for
    /// `instanceof`), its constructor and its static methods.
    pub uses: Vec<MangledName>,
    /// The index into the top-level statements before which the class is
    /// declared.
    pub declared_at: usize,
}

/// A guarded binding's flag, as codegen allocated it.
#[derive(Clone, Debug, PartialEq)]
pub struct InitFlag {
    pub flag_idx: u32,
    /// The statement that initializes a global; `None` for a class, which no
    /// statement writes.
    pub declared_by: Option<StmtId>,
    pub message: String,
}

/// The module globals that need an initialization flag, in declaration order.
pub fn guarded_globals(ta: &TypedAst) -> Result<Vec<InitGuard>, CompilerFailure> {
    let declared: BTreeSet<&MangledName> = ta.globals.iter().map(|g| &g.mangled_name).collect();
    let mut seen = BTreeSet::new();
    let mut code_may_have_run = false;
    let mut guards = Vec::new();
    for &stmt_id in &ta.top_level_statements {
        let stmt = ta
            .try_stmt(stmt_id)
            .map_err(crate::codegen::arena_failure)?;
        // The initializer runs before its own binding is initialized.
        code_may_have_run |= may_run_code(ta, stmt_id)?;
        let TypedStmtKind::AssignGlobal { ident, mangled, .. } = &stmt.kind else {
            continue;
        };
        // `seen` records every first declaration, guarded or not, so a later
        // write is never taken for one.
        let is_first_declaration = declared.contains(mangled) && seen.insert(mangled);
        if !is_first_declaration || !code_may_have_run {
            continue;
        }
        guards.push(InitGuard {
            global: mangled.clone(),
            binding: reported_binding(&ident.name).to_string(),
            declared_by: stmt_id,
        });
    }
    Ok(guards)
}

/// The top-level classes that need an initialization flag.
pub fn guarded_classes(ta: &TypedAst) -> Result<Vec<ClassGuard>, CompilerFailure> {
    let mut code_may_have_run = Vec::with_capacity(ta.top_level_statements.len() + 1);
    code_may_have_run.push(false);
    for &stmt_id in &ta.top_level_statements {
        let before = code_may_have_run.last().copied().unwrap_or(false);
        code_may_have_run.push(before || may_run_code(ta, stmt_id)?);
    }
    let mut guards = Vec::new();
    for decl in &ta.types {
        let crate::TypedTypeDecl::Class(class) = decl else {
            continue;
        };
        let Some(&declared_at) = ta.class_declaration_points.get(&class.mangled_name) else {
            continue;
        };
        if !code_may_have_run.get(declared_at).copied().unwrap_or(false) {
            continue;
        }
        let mangled = &class.mangled_name;
        let mut uses = vec![
            mangled.clone(),
            crate::mangle::extend(mangled, "constructor"),
        ];
        uses.extend(
            class
                .static_methods
                .keys()
                .map(|method| crate::mangle::static_member(mangled, method)),
        );
        guards.push(ClassGuard {
            name: class.name.name.clone(),
            uses,
            declared_at,
        });
    }
    Ok(guards)
}

/// Whether running a top-level statement can call a function.
fn may_run_code(ta: &TypedAst, stmt_id: StmtId) -> Result<bool, CompilerFailure> {
    let stmt = ta
        .try_stmt(stmt_id)
        .map_err(crate::codegen::arena_failure)?;
    match &stmt.kind {
        TypedStmtKind::AssignGlobal { value, .. } => Ok(!is_inert(ta, *value)?),
        _ => Ok(true),
    }
}

/// Static fields are lowered to globals named `Class.field`; the binding a
/// read of one needs is its class.
fn reported_binding(global_name: &str) -> &str {
    global_name
        .split_once('.')
        .map_or(global_name, |(class, _)| class)
}

/// The `ReferenceError` message for a binding, worded as Node words it.
pub fn before_initialization_message(binding: &str) -> String {
    format!("Cannot access '{binding}' before initialization")
}

/// Allocates one flag per guarded global and class, starting unset. Returns
/// how many globals were added.
pub fn allocate_flags(
    ta: &TypedAst,
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
) -> Result<u32, CompilerFailure> {
    let guards = guarded_globals(ta)?;
    for guard in &guards {
        let flag_idx = allocate_flag(globals, next_global_idx)?;
        symbols.record_init_guard(
            guard.global.clone(),
            InitFlag {
                flag_idx,
                declared_by: Some(guard.declared_by),
                message: before_initialization_message(&guard.binding),
            },
        );
    }
    let classes = guarded_classes(ta)?;
    for class in &classes {
        let flag_idx = allocate_flag(globals, next_global_idx)?;
        for used in &class.uses {
            symbols.record_init_guard(
                used.clone(),
                InitFlag {
                    flag_idx,
                    declared_by: None,
                    message: before_initialization_message(&class.name),
                },
            );
        }
        symbols.record_class_declaration_flag(class.declared_at, flag_idx);
    }
    crate::codegen::wasm_u32(guards.len() + classes.len())
}

fn allocate_flag(
    globals: &mut GlobalSection,
    next_global_idx: &mut u32,
) -> Result<u32, CompilerFailure> {
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(0),
    );
    let flag_idx = *next_global_idx;
    crate::codegen::next_index(next_global_idx)?;
    Ok(flag_idx)
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

/// Whether `stmt`, a write to `global`, is its declaration, which marks it
/// initialized; any other write is checked.
pub(crate) fn is_declaration(ctx: &CodegenCtx, global: &MangledName, stmt: StmtId) -> bool {
    ctx.symbols
        .init_guard(global)
        .is_some_and(|flag| flag.declared_by == Some(stmt))
}

/// Mark the classes declared before top-level statement `index` initialized.
pub(crate) fn emit_mark_classes_declared(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    index: usize,
) {
    for &flag_idx in ctx.symbols.class_declaration_flags(index) {
        emitter.instruction(Instruction::I32Const(1));
        emitter.instruction(Instruction::GlobalSet(flag_idx));
    }
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
        // An operator on an object can call its `toString` or `valueOf`.
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
