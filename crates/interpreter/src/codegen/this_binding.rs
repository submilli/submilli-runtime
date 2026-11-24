//! Per-invocation receivers for ordinary function expressions.
use super::{CodegenCtx, function_emitter::FunctionEmitter};
use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

/// A bound null must survive closure adapters and lexical captures, which forward
/// an Object reference and reserve ref.null for an unbound call. The private
/// wrapper field distinguishes this WasmGC type from language-level object types.
pub(super) fn declare_null_receiver(
    types: &mut wasm_encoder::TypeSection,
    intr: super::intrinsics::IntrinsicTypeIndices,
    wrapper: u32,
) {
    let tag = wasm_encoder::FieldType {
        element_type: wasm_encoder::StorageType::Val(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(wrapper),
        })),
        mutable: false,
    };
    types.ty().subtype(&super::intrinsics::substruct(
        vec![super::intrinsics::fieldtype_ref(intr.vtable), tag],
        Some(intr.object),
    ));
}

fn null_receiver_type(ctx: &CodegenCtx<'_>) -> Result<u32, crate::compiler_error::CompilerFailure> {
    ctx.symbols
        .null_receiver_type
        .ok_or_else(|| crate::codegen::internal_failure("explicit null receiver type"))
}

pub(super) fn preserve_explicit_null(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    slot: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(slot));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .prelude_global_idx("object_vtable")
            .ok_or_else(|| crate::codegen::internal_failure("object vtable"))?,
    ));
    let wrapper = ctx
        .symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(wrapper)));
    emitter.instruction(Instruction::StructNew(null_receiver_type(ctx)?));
    emitter.instruction(Instruction::LocalSet(slot));
    emitter.emit_end();
    Ok(())
}

pub(super) fn read_bound_receiver(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    slot: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(slot));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        null_receiver_type(ctx)?,
    )));
    emitter.emit_if(BlockType::Result(
        ctx.symbols.value_type(&crate::Type::Unknown)?,
    ));
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intr.object)));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(slot));
    emitter.emit_end();
    Ok(())
}

pub(super) fn wrap(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let object = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .object;
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(object)));
    emitter.instruction(Instruction::StructNew(
        ctx.symbols
            .this_environment_type
            .ok_or_else(|| crate::codegen::internal_failure("this environment"))?,
    ));

    Ok(())
}

/// The stored environment remains immutable: concurrent/reentrant calls get separate wrappers.
pub(super) fn bind(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    receiver: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let any = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::ANY,
    });
    let env = emitter.add_anonymous_local(any)?;
    let wrapper = ctx
        .symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    for instruction in binding_instructions(wrapper, env, receiver) {
        emitter.instruction(instruction);
    }
    Ok(())
}

pub(super) fn binding_instructions(
    wrapper: u32,
    env: u32,
    receiver: u32,
) -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalTee(env),
        Instruction::RefTestNonNull(HeapType::Concrete(wrapper)),
        Instruction::If(BlockType::Result(ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::ANY,
        }))),
        Instruction::LocalGet(env),
        Instruction::RefCastNonNull(HeapType::Concrete(wrapper)),
        Instruction::StructGet {
            struct_type_index: wrapper,
            field_index: 0,
        },
        Instruction::LocalGet(receiver),
        Instruction::StructNew(wrapper),
        Instruction::Else,
        Instruction::LocalGet(env),
        Instruction::End,
    ]
}

pub(super) fn load_receiver(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let wrapper = ctx
        .symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    let receiver = emitter.define_local(
        &crate::Ident {
            name: "this".to_string(),
            span: crate::Span::at(ctx.file),
        },
        ctx.symbols.value_type(&crate::Type::Unknown)?,
    )?;
    emitter.instruction(Instruction::LocalGet(0));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(wrapper)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: wrapper,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(receiver));
    emitter.set_this_local(receiver)?;
    emitter.dynamic_this = true;

    Ok(())
}
