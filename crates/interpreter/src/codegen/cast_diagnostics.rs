//! Tracks failing locations during the existing validation walk.
//! No value is re-read to explain a failure, and diagnostics never copy payloads.

use super::{CodegenCtx, function_emitter::FunctionEmitter};
use crate::compiler_error::CompilerFailure;
use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

// The recursive validator's visited array holds two cells per active frame.
pub(super) const PATH_SLOT: i32 = super::cast_check::RECURSIVE_VALIDATOR_CAPACITY * 2;
pub(super) const FAILURE_SLOT: i32 = PATH_SLOT + 1;

/// UTF-16 units of a field name in an emitted path charged as one more step.
const PATH_UNITS_PER_STEP: usize = 64;

#[derive(Clone)]
pub(super) struct Locals {
    current: u32,
    failure: u32,
    key: Option<u32>,
    path: Vec<Segment>,
}

#[derive(Clone)]
enum Segment {
    Field(String),
    Index(u32),
}

/// Scalar checks retain their established root-only message and need no paths.
pub(super) fn has_nested_paths(ty: &crate::Type) -> bool {
    use crate::Type;
    match ty.peel() {
        Type::Union(members) => members.iter().any(has_nested_paths),
        Type::Number
        | Type::NumberLiteral(_)
        | Type::NumberEnum { .. }
        | Type::String
        | Type::StringLiteral(_)
        | Type::StringEnum { .. }
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::BigInt
        | Type::BigIntLiteral(_)
        | Type::Uint8Array
        | Type::Null
        | Type::Undefined
        | Type::Void
        | Type::Unknown
        | Type::Never
        | Type::Error
        | Type::Function { .. } => false,
        _ => true,
    }
}

pub(super) fn begin(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) -> Option<Locals> {
    let previous = emitter.cast_diagnostic.take();
    let current = ctx.latch(emitter.add_anonymous_local(ctx.latch(string_slot(ctx))?))?;
    let failure = ctx.latch(emitter.add_anonymous_local(ctx.latch(string_slot(ctx))?))?;
    super::cast_check::emit_inline_string(emitter, ctx, "$");
    emitter.instruction(Instruction::LocalSet(current));
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(
        ctx.latch(string_index(ctx))?,
    )));
    emitter.instruction(Instruction::LocalSet(failure));
    emitter.cast_diagnostic = Some(Locals {
        current,
        failure,
        key: None,
        path: Vec::new(),
    });
    previous
}

pub(super) fn session_key(emitter: &mut FunctionEmitter, key: Option<u32>) {
    if let Some(state) = &mut emitter.cast_diagnostic {
        state.key = key;
    }
}

pub(super) fn checkpoint(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) -> Option<u32> {
    let state = emitter.cast_diagnostic.clone()?;
    let saved = ctx.latch(emitter.add_anonymous_local(ctx.latch(string_slot(ctx))?))?;
    emitter.instruction(Instruction::LocalGet(state.failure));
    emitter.instruction(Instruction::LocalSet(saved));
    Some(saved)
}

/// Consume and reproduce a conformance bit. Successful alternatives discard
/// their speculative failures; a failing parent preserves its child's location.
pub(super) fn finish(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    saved: Option<u32>,
) -> Result<(), CompilerFailure> {
    let (Some(state), Some(saved)) = (emitter.cast_diagnostic.clone(), saved) else {
        return Ok(());
    };
    let result = emitter.add_anonymous_local(ValType::I32)?;
    emitter.instruction(Instruction::LocalTee(result));
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(saved));
    emitter.instruction(Instruction::LocalSet(state.failure));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(state.failure));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emit_path(emitter, ctx, &state)?;
    emitter.instruction(Instruction::LocalSet(state.failure));
    emitter.emit_end();
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(result));
    Ok(())
}

/// Each union alternative starts with an independent failure. Prefer a longer
/// path when all alternatives fail, so a null/primitive mismatch does not hide
/// a nested field failure. Ties retain the first alternative's explanation.
pub(super) fn union_start(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) -> Option<u32> {
    emitter.cast_diagnostic.as_ref()?;
    let best = ctx.latch(emitter.add_anonymous_local(ctx.latch(string_slot(ctx))?))?;
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(
        ctx.latch(string_index(ctx))?,
    )));
    emitter.instruction(Instruction::LocalSet(best));
    Some(best)
}

pub(super) fn union_next(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(union_next_checked(emitter, ctx));
}

fn union_next_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(string_index(ctx)?)));
    emitter.instruction(Instruction::LocalSet(state.failure));

    Ok(())
}

pub(super) fn union_keep(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, best: Option<u32>) {
    let (Some(state), Some(best)) = (emitter.cast_diagnostic.clone(), best) else {
        return;
    };
    path_length(emitter, ctx, state.failure);
    path_length(emitter, ctx, best);
    emitter.instruction(Instruction::I32GtU);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(state.failure));
    emitter.instruction(Instruction::LocalSet(best));
    emitter.emit_end();
}

pub(super) fn union_end(emitter: &mut FunctionEmitter, best: Option<u32>, previous: Option<u32>) {
    let (Some(state), Some(best), Some(previous)) =
        (emitter.cast_diagnostic.clone(), best, previous)
    else {
        return;
    };
    emitter.instruction(Instruction::LocalGet(previous));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(best));
    emitter.instruction(Instruction::LocalSet(state.failure));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(previous));
    emitter.instruction(Instruction::LocalSet(state.failure));
    emitter.emit_end();
}

fn path_length(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, local: u32) {
    ctx.latch(path_length_checked(emitter, ctx, local));
}

fn path_length_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    local: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_index(ctx)?,
        field_index: 1,
    });
    emitter.instruction(Instruction::ArrayLen);
    emitter.emit_end();

    Ok(())
}

pub(super) fn field(emitter: &mut FunctionEmitter, _ctx: &CodegenCtx, name: &str) -> Option<usize> {
    let state = emitter.cast_diagnostic.as_mut()?;
    let mark = state.path.len();
    state.path.push(Segment::Field(name.into()));
    Some(mark)
}

pub(super) fn index(emitter: &mut FunctionEmitter, _ctx: &CodegenCtx, index: u32) -> Option<usize> {
    let state = emitter.cast_diagnostic.as_mut()?;
    let mark = state.path.len();
    state.path.push(Segment::Index(index));
    Some(mark)
}

pub(super) fn pop(emitter: &mut FunctionEmitter, mark: Option<usize>) {
    if let (Some(state), Some(mark)) = (emitter.cast_diagnostic.as_mut(), mark) {
        state.path.truncate(mark);
    }
}

/// Inline paths are assembled only when a check fails. Index locals still
/// contain the failing iteration's index when this code runs.
///
/// Every test emits the path to the value it tests, so a check emits its
/// nesting depth in segments per test. Each segment is charged to the check as
/// a step, and a field name as one more per [`PATH_UNITS_PER_STEP`] UTF-16
/// units, since each unit is an instruction.
fn emit_path(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    state: &Locals,
) -> Result<(), CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(state.current));
    for segment in &state.path {
        match segment {
            Segment::Field(name) => {
                let units = name.encode_utf16().count() / PATH_UNITS_PER_STEP;
                let name_steps = u64::try_from(units).unwrap_or(u64::MAX);
                ctx.charge_validator_steps(emitter, name_steps.saturating_add(1))?;
                let quoted = serde_json::to_string(name).map_err(|error| {
                    super::internal_failure(format!("cannot quote a field name: {error}"))
                })?;
                super::cast_check::emit_inline_string(emitter, ctx, &format!("[{quoted}]"));
                concat(emitter, ctx);
            }
            Segment::Index(index) => {
                ctx.charge_validator_step(emitter)?;
                super::cast_check::emit_inline_string(emitter, ctx, "[");
                concat(emitter, ctx);
                emitter.instruction(Instruction::LocalGet(*index));
                emitter.instruction(Instruction::F64ConvertI32U);
                emitter.instruction(Instruction::F64Const(10.0_f64.into()));
                let number_to_string =
                    ctx.symbols
                        .prelude_func_idx("Number#toString")
                        .ok_or_else(|| {
                            super::internal_failure(
                                "Number#toString is not imported from the prelude",
                            )
                        })?;
                emitter.instruction(Instruction::Call(number_to_string));
                concat(emitter, ctx);
                super::cast_check::emit_inline_string(emitter, ctx, "]");
                concat(emitter, ctx);
            }
        }
    }
    Ok(())
}

pub(super) fn append_failure(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(append_failure_checked(emitter, ctx));
}

fn append_failure_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    let Some(message) = ctx.latch(emitter.add_anonymous_local(string_slot(ctx)?)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(message));
    // A root mismatch keeps the established scalar diagnostic unchanged.
    path_length(emitter, ctx, state.failure);
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32GtU);
    emitter.emit_if(BlockType::Result(string_slot(ctx)?));
    emitter.instruction(Instruction::LocalGet(message));
    super::cast_check::emit_inline_string(emitter, ctx, " at ");
    concat(emitter, ctx);
    emitter.instruction(Instruction::LocalGet(state.failure));
    concat(emitter, ctx);
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(message));
    emitter.emit_end();
    emitter.instruction(Instruction::RefAsNonNull);
    if let Some(key) = state.key {
        super::cast_check::emit_inline_string(emitter, ctx, " for session key ");
        concat(emitter, ctx);
        emitter.instruction(Instruction::LocalGet(key));
        concat(emitter, ctx);
    }

    Ok(())
}

pub(super) fn save_recursive(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    visited: u32,
) -> Result<(), CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    let array = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| super::internal_failure("intrinsic types are not declared"))?
        .raw_array;
    for (index, local) in [(PATH_SLOT, state.current), (FAILURE_SLOT, state.failure)] {
        emitter.instruction(Instruction::LocalGet(visited));
        emitter.instruction(Instruction::I32Const(index));
        if index == PATH_SLOT {
            emit_path(emitter, ctx, &state)?;
        } else {
            emitter.instruction(Instruction::LocalGet(local));
        }
        emitter.instruction(Instruction::ArraySet(array));
    }
    Ok(())
}

pub(super) fn load_recursive(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, visited: u32) {
    ctx.latch(load_recursive_checked(emitter, ctx, visited));
}

fn load_recursive_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    visited: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    let array = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .raw_array;
    emitter.instruction(Instruction::LocalGet(visited));
    emitter.instruction(Instruction::I32Const(FAILURE_SLOT));
    emitter.instruction(Instruction::ArrayGet(array));
    emitter.instruction(Instruction::RefCastNullable(HeapType::Concrete(
        string_index(ctx)?,
    )));
    emitter.instruction(Instruction::LocalSet(state.failure));

    Ok(())
}

pub(super) fn enter_recursive(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(enter_recursive_checked(emitter, ctx));
}

fn enter_recursive_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    begin(emitter, ctx);
    let state = emitter
        .cast_diagnostic
        .clone()
        .ok_or_else(|| crate::codegen::internal_failure("validator diagnostic"))?;
    let array = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .raw_array;
    emitter.instruction(Instruction::LocalGet(1));
    emitter.instruction(Instruction::I32Const(PATH_SLOT));
    emitter.instruction(Instruction::ArrayGet(array));
    emitter.instruction(Instruction::RefCastNullable(HeapType::Concrete(
        string_index(ctx)?,
    )));
    let Some(incoming) = ctx.latch(emitter.add_anonymous_local(string_slot(ctx)?)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalTee(incoming));
    emitter.instruction(Instruction::RefIsNull);
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(incoming));
    emitter.instruction(Instruction::LocalSet(state.current));
    emitter.emit_end();
    load_recursive(emitter, ctx, 1);

    Ok(())
}

pub(super) fn concat(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(concat_checked(emitter, ctx));
}

fn concat_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(right) = ctx.latch(emitter.add_anonymous_local(string_slot(ctx)?)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(right));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalGet(right));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::Call(
        ctx.symbols
            .prelude_func_idx("string_concat")
            .ok_or_else(|| crate::codegen::internal_failure("string concat"))?,
    ));

    Ok(())
}

fn string_index(ctx: &CodegenCtx) -> Result<u32, crate::compiler_error::CompilerFailure> {
    ctx.symbols
        .string_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("string"))
}
fn string_slot(ctx: &CodegenCtx) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(string_index(ctx)?),
    }))
}
