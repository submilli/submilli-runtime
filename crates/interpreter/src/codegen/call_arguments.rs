//! Argument metadata for calls whose target is resolved at runtime.
use super::internal_failure;
use super::{
    CodegenCtx,
    function_emitter::{FunctionEmitter, emit_inline_string_literal},
};
use crate::compiler_error::CompilerFailure;
use crate::{DefaultValue, TypedParam};
use wasm_encoder::{HeapType, Instruction};

pub(crate) fn metadata<'a>(
    params: impl Iterator<Item = (Option<&'a DefaultValue>, bool)>,
) -> Result<Option<String>, CompilerFailure> {
    let params: Vec<_> = params.collect();
    if !params
        .iter()
        .any(|(default, rest)| default.is_some() || *rest)
    {
        return Ok(None);
    }
    serde_json::to_string(&params).map(Some).map_err(|error| {
        internal_failure(format!("call defaults could not be serialized: {error}"))
    })
}

pub(crate) fn typed_metadata(params: &[TypedParam]) -> Result<Option<String>, CompilerFailure> {
    metadata(
        params
            .iter()
            .map(|param| (param.default.as_ref(), param.rest)),
    )
}

pub(crate) fn wrap(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    metadata: &str,
) -> Result<(), CompilerFailure> {
    let ty = metadata_type(ctx)?;
    emit_inline_string_literal(emitter, ctx, metadata)?;
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(ty));
    Ok(())
}

pub(crate) fn unwrap(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), CompilerFailure> {
    let ty = metadata_type(ctx)?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(ty)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: ty,
        field_index: 0,
    });
    Ok(())
}

fn metadata_type(ctx: &CodegenCtx) -> Result<u32, CompilerFailure> {
    ctx.symbols
        .call_metadata_type
        .ok_or_else(|| internal_failure("the call metadata type is not declared"))
}
