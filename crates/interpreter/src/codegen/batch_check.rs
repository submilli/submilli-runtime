//! Validate typed LLM outcomes independently after the host has returned them.
use super::{
    CodegenCtx, cast_check,
    function_emitter::{FunctionEmitter, cast, expr::emit_undefined},
};
use crate::{Type, compiler_error::CompilerFailure};
use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

pub(super) fn emit(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    shape: &Type,
) -> Result<(), CompilerFailure> {
    let Type::Array(element) = shape.peel() else {
        return Err(super::internal_failure(
            "typed batch result must be an array",
        ));
    };
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| super::internal_failure("intrinsics declared"))?;
    let reference = |index| {
        ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(index),
        })
    };
    let array = emitter.add_anonymous_local(reference(intr.array))?;
    let value = emitter.add_anonymous_local(reference(intr.object))?;
    let fields = emitter.add_anonymous_local(reference(intr.object_fields))?;
    let index = emitter.add_anonymous_local(ValType::I32)?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(intr.array)));
    emitter.instruction(Instruction::LocalSet(array));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(index));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::LocalGet(array));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.array,
        field_index: 2,
    });
    emitter.instruction(Instruction::I32GeU);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(array));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.array,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::ArrayGet(intr.raw_array));
    emitter.instruction(Instruction::LocalSet(value));
    cast_check::emit_structural_test(emitter, ctx, value, element, element)?;
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    // build_typed_batch_result owns this carrier: all five fields are present
    // in canonical order, and no guest code runs before this validation.
    emitter.instruction(Instruction::LocalGet(value));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        intr.object_shape,
    )));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.object_shape,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalSet(fields));
    emitter.instruction(Instruction::LocalGet(fields));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Const(0));
    cast::emit_box(emitter, ctx, &Type::Boolean)?;
    emitter.instruction(Instruction::ArraySet(intr.object_fields));
    emitter.instruction(Instruction::LocalGet(fields));
    emitter.instruction(Instruction::I32Const(3));
    cast_check::emit_inline_string(
        emitter,
        ctx,
        "type mismatch: model response does not match the requested type",
    );
    emitter.instruction(Instruction::ArraySet(intr.object_fields));
    emitter.instruction(Instruction::LocalGet(fields));
    emitter.instruction(Instruction::I32Const(4));
    emit_undefined(emitter, ctx)?;
    emitter.instruction(Instruction::ArraySet(intr.object_fields));
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(index));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end();
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(array));
    emitter.instruction(Instruction::RefAsNonNull);
    Ok(())
}
