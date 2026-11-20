//! Tagged template sites' template objects (`TypedExprKind::TemplateObject`).
//!
//! JavaScript caches a site's template object, so every evaluation of one node
//! yields the same object. Each node gets a global that starts null and is
//! filled on the node's first evaluation, as `function_adapters` caches a
//! function's closure. The object is a `$FrozenArray` of the cooked strings
//! whose `raw` field holds a `$FrozenArray` of the raw strings.
//!
//! `$FrozenArray`'s fields are `$Array`'s, which are mutable, so a store to an
//! array tests for it first ([`emit_write_guard`]); host mutators check it in
//! `runtime::array_storage`.

use wasm_encoder::{ConstExpr, GlobalSection, GlobalType, HeapType, Instruction, RefType, ValType};

use super::function_emitter::FunctionEmitter;
use super::symbol_table::SymbolTable;
use super::{CodegenCtx, internal_failure};
use crate::compiler_error::CompilerFailure;
use crate::{ExprId, TypedAst, TypedExprKind};

fn frozen_array_type(symbols: &SymbolTable) -> Result<u32, CompilerFailure> {
    symbols
        .intrinsic_type_indices()
        .map(|indices| indices.frozen_array)
        .ok_or_else(|| internal_failure("intrinsic types declared before template objects"))
}

/// Allocates one cache global per `TemplateObject` node. Returns how many.
pub(super) fn allocate_globals(
    ta: &TypedAst,
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next: &mut u32,
) -> Result<u32, CompilerFailure> {
    let heap_type = HeapType::Concrete(frozen_array_type(symbols)?);
    let mut count = 0usize;
    for id in ta.expr_ids().map_err(super::arena_failure)? {
        let expr = ta.try_expr(id).map_err(super::arena_failure)?;
        if !matches!(expr.kind, TypedExprKind::TemplateObject { .. }) {
            continue;
        }
        symbols.template_object_globals.insert(id, *next);
        super::next_index(next)?;
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type,
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(heap_type),
        );
        count += 1;
    }
    super::wasm_u32(count)
}

/// Pushes the site's template object, building and caching it on the first
/// evaluation.
pub(super) fn emit(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
    cooked: &[Option<String>],
    raw: &[String],
) -> Result<(), CompilerFailure> {
    if cooked.len() != raw.len() {
        return Err(internal_failure(
            "a template object has as many cooked strings as raw strings",
        ));
    }
    let global = ctx
        .symbols
        .template_object_globals
        .get(&id)
        .copied()
        .ok_or_else(|| internal_failure("a template object has a cache global"))?;
    let frozen_array = frozen_array_type(ctx.symbols)?;
    let array = ctx
        .symbols
        .array_type_idx()
        .ok_or_else(|| internal_failure("`$Array` declared before template objects"))?;

    emitter.instruction(Instruction::GlobalGet(global));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emit_frozen_array_fields(emitter, ctx, cooked.len(), |emitter, index| {
        match &cooked[index] {
            Some(text) => super::function_emitter::emit_const_string_by_text(emitter, ctx, text),
            None => super::function_emitter::expr::emit_undefined(emitter, ctx),
        }
    })?;
    emit_frozen_array_fields(emitter, ctx, raw.len(), |emitter, index| {
        super::function_emitter::emit_const_string_by_text(emitter, ctx, &raw[index])
    })?;
    // The `raw` array has no `raw` of its own.
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(array)));
    emitter.instruction(Instruction::StructNew(frozen_array));
    emitter.instruction(Instruction::StructNew(frozen_array));
    emitter.instruction(Instruction::GlobalSet(global));
    emitter.emit_end();
    emitter.instruction(Instruction::GlobalGet(global));
    emitter.instruction(Instruction::RefAsNonNull);
    Ok(())
}

/// Pushes `$Array`'s fields (vtable, backing, length) for `len` elements that
/// `element` pushes in order.
fn emit_frozen_array_fields(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    len: usize,
    mut element: impl FnMut(&mut FunctionEmitter, usize) -> Result<(), CompilerFailure>,
) -> Result<(), CompilerFailure> {
    let raw_array = ctx
        .symbols
        .raw_array_type_idx()
        .ok_or_else(|| internal_failure("`$rawArray` declared before template objects"))?;
    let vtable = ctx
        .symbols
        .prelude_global_idx("array_vtable")
        .ok_or_else(|| internal_failure("array_vtable imported from prelude"))?;
    emitter.instruction(Instruction::GlobalGet(vtable));
    for index in 0..len {
        element(emitter, index)?;
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: raw_array,
        array_size: super::wasm_u32(len)?,
    });
    emitter.instruction(Instruction::I32Const(
        i32::try_from(len).map_err(|_| internal_failure("template object too large"))?,
    ));
    Ok(())
}

/// Throws `TypeError` when the array in `array_local` is frozen, before a
/// store to it.
pub(crate) fn emit_write_guard(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    array_local: u32,
) -> Result<(), CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(array_local));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        frozen_array_type(ctx.symbols)?,
    )));
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    super::throw::emit_type_error_throw(
        emitter,
        ctx,
        crate::runtime::array_storage::FROZEN_ARRAY_WRITE,
    );
    emitter.emit_end();
    Ok(())
}
