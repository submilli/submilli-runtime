//! Argument metadata for calls whose target is resolved at runtime.
use super::{
    CodegenCtx,
    function_emitter::{FunctionEmitter, emit_inline_string_literal},
};
use crate::{DefaultValue, TypedParam};
use wasm_encoder::{HeapType, Instruction};

pub(crate) fn metadata<'a>(
    params: impl Iterator<Item = (Option<&'a DefaultValue>, bool)>,
) -> Option<String> {
    let params: Vec<_> = params.collect();
    if !params
        .iter()
        .any(|(default, rest)| default.is_some() || *rest)
    {
        return None;
    }
    Some(serde_json::to_string(&params).expect("literal call defaults serialize"))
}

pub(crate) fn typed_metadata(params: &[TypedParam]) -> Option<String> {
    metadata(
        params
            .iter()
            .map(|param| (param.default.as_ref(), param.rest)),
    )
}

pub(crate) fn wrap(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, metadata: &str) {
    emit_inline_string_literal(emitter, ctx, metadata);
    emitter.instruction(Instruction::StructNew(
        ctx.symbols.call_metadata_type.expect("call metadata type"),
    ));
}

pub(crate) fn unwrap(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let ty = ctx.symbols.call_metadata_type.expect("call metadata type");
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(ty)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: ty,
        field_index: 0,
    });
}
