//! The hidden export that checks a stored finalizer argument: `(unknown) ->
//! unknown`, failing as `value as T` fails, and returning the value in the
//! boxed form the package's exported `finalizer` takes it in.
use wasm_encoder::{Function, HeapType, RefType, ValType};

use super::{CodegenCtx, function_emitter::FunctionEmitter, symbol_table::SymbolTable};
use crate::{FinalizerArgumentCheck, Ident, Span};

pub(super) struct FinalizerArgument<'a> {
    pub check: &'a FinalizerArgumentCheck,
    pub type_idx: u32,
    pub func_idx: u32,
}

pub(super) fn allocate<'a>(
    check: &'a FinalizerArgumentCheck,
    types: &mut wasm_encoder::TypeSection,
    symbols: &SymbolTable,
    next_type: &mut u32,
    next_function: &mut u32,
) -> Result<FinalizerArgument<'a>, crate::compiler_error::CompilerFailure> {
    let type_idx = *next_type;
    crate::codegen::next_index(next_type)?;
    types
        .ty()
        .function([unknown_type(symbols)?], [unknown_type(symbols)?]);
    let func_idx = *next_function;
    crate::codegen::next_index(next_function)?;
    Ok(FinalizerArgument {
        check,
        type_idx,
        func_idx,
    })
}

pub(super) fn body(
    ctx: &CodegenCtx,
    argument: &FinalizerArgument,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let params = [(
        Ident {
            name: "value".into(),
            span: Span::at(ctx.file),
        },
        unknown_type(ctx.symbols)?,
    )];
    let mut emitter = FunctionEmitter::new(ctx, &params)?;
    let FinalizerArgumentCheck { target, check } = argument.check;
    emitter.instruction(wasm_encoder::Instruction::LocalGet(0));
    super::cast_check::emit_shape_cast_on_stack(&mut emitter, ctx, target, check, None)?;
    super::function_emitter::cast::emit_box(&mut emitter, ctx, target)?;
    emitter.build()
}

fn unknown_type(symbols: &SymbolTable) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsic types registered"))?;
    Ok(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))
}
