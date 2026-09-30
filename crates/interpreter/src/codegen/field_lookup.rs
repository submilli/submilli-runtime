//! Shared two-pass lookup for data and accessor names on ObjectShape values.
use wasm_encoder::{BlockType, Function, HeapType, Instruction, RefType, ValType};

use super::{CodegenCtx, function_emitter::FunctionEmitter};
use crate::{Ident, Span};

pub(super) fn allocate(
    types: &mut wasm_encoder::TypeSection,
    symbols: &mut super::symbol_table::SymbolTable,
    next_type: &mut u32,
    next_function: &mut u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let signature = *next_type;
    *next_type += 1;
    types.ty().function(
        [
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(
                    symbols
                        .object_shape_type_idx()
                        .expect("ObjectShape registered"),
                ),
            }),
            symbols.value_type(&crate::Type::String)?,
            ValType::I32,
        ],
        [ValType::I32],
    );
    symbols.field_lookup_function = Some(*next_function);
    *next_function += 1;
    Ok(signature)
}

pub(super) fn body(ctx: &CodegenCtx) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsic types registered");
    let params = [
        (
            "object",
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(intrinsics.object_shape),
            }),
        ),
        ("name", ctx.symbols.value_type(&crate::Type::String)?),
        ("accessor", ValType::I32),
    ]
    .map(|(name, ty)| {
        (
            Ident {
                name: name.into(),
                span: Span::at(ctx.file),
            },
            ty,
        )
    });
    let mut emitter = FunctionEmitter::new(ctx, &params);
    emit_lookup(&mut emitter, ctx)?;
    Ok(emitter.build())
}

fn emit_lookup(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let object_shape_idx = ctx
        .symbols
        .object_shape_type_idx()
        .expect("ObjectShape type registered");
    let field_names_type_idx = ctx
        .symbols
        .field_names_type_idx()
        .expect("field_names type registered");
    let names_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(field_names_type_idx),
    }));
    let i_local = emitter.add_anonymous_local(ValType::I32);
    let len_local = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::LocalGet(0));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(names_local));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::LocalSet(len_local));
    let string_eq_idx = ctx
        .symbols
        .prelude_func_idx("string_eq")
        .expect("submilli:prelude.string_eq imported");

    emitter.emit_block(BlockType::Result(ValType::I32));
    emit_field_name_scan_pass(
        emitter,
        field_names_type_idx,
        names_local,
        i_local,
        len_local,
        1,
        None,
        ctx,
        2,
    )?;
    emit_field_name_scan_pass(
        emitter,
        field_names_type_idx,
        names_local,
        i_local,
        len_local,
        1,
        Some(string_eq_idx),
        ctx,
        2,
    )?;
    emitter.instruction(Instruction::I32Const(-1));
    emitter.emit_end();

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_field_name_scan_pass(
    emitter: &mut FunctionEmitter,
    field_names_type_idx: u32,
    names_local: u32,
    i_local: u32,
    len_local: u32,
    name_local: u32,
    string_eq_idx: Option<u32>,
    ctx: &CodegenCtx,
    accessor_local: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::LocalGet(len_local));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::ArrayGet(field_names_type_idx));
    emitter.instruction(Instruction::LocalGet(name_local));
    if let Some(string_eq_idx) = string_eq_idx {
        emitter.instruction(Instruction::Call(string_eq_idx));
    } else {
        emitter.instruction(Instruction::RefEq);
    }
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::ArrayGet(field_names_type_idx));
    crate::codegen::field_names::emit_name_is_accessor(emitter, ctx)?;
    emitter.instruction(Instruction::LocalGet(accessor_local));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::I32And);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::Br(3));
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end();
    emitter.emit_end();

    Ok(())
}
