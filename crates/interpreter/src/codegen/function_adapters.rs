//! Collects top-level functions used in value position and emits closure-shaped adapter bodies for them.

use wasm_encoder::{CodeSection, HeapType, Instruction, RefType, ValType};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::{FunctionEmitter, cast};
use crate::{Ident, Span, Type};

#[derive(Clone, Debug)]
pub struct AdapterMeta {
    pub name: String,
    pub mangled: crate::MangledName,
    pub signature: Type,
}

pub fn emit_bodies(
    metas: &[AdapterMeta],
    code: &mut CodeSection,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    // adapter erased-arg slots are nullable to match the
    // closure ABI.
    let object_ref = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let any_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::ANY,
    });
    for meta in metas {
        let target_idx = ctx.symbols.func_idx(&meta.mangled).ok_or_else(|| {
            crate::codegen::internal_failure(
                "adapter target function recorded during user-function pre-pass",
            )
        })?;
        let Type::Function { params, ret, .. } = &meta.signature else {
            return Err(super::internal_failure(
                "adapter signature requires a function type",
            ));
        };

        super::closures::classify(&meta.signature)?;

        // names are unused; adapter bodies access params by Wasm slot index
        let env_name = Ident {
            name: "$__env__".to_string(),
            span: Span::at(ctx.file),
        };
        let mut wasm_params: Vec<(Ident, ValType)> = vec![(env_name, any_ref)];
        for (i, _) in params.iter().enumerate() {
            wasm_params.push((
                Ident {
                    name: format!("$__arg{i}__"),
                    span: Span::at(ctx.file),
                },
                object_ref,
            ));
        }
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params)?;

        let target = ctx
            .symbols
            .top_level_fn(&meta.mangled)
            .ok_or_else(|| crate::codegen::internal_failure("adapter target signature recorded"))?;
        if target.params.len() != params.len() || target.ret.is_void() != ret.is_void() {
            return Err(super::internal_failure(
                "adapter signature disagrees with its target",
            ));
        }
        for (i, p_ty) in target.params.iter().enumerate() {
            emitter.instruction(Instruction::LocalGet(super::parameter_local(i)?));
            crate::codegen::cast_check::emit_checked_parameter_cast_on_stack(
                &mut emitter,
                ctx,
                &crate::Type::Unknown,
                p_ty,
            )?;
        }
        emitter.instruction(Instruction::Call(target_idx));
        if !ret.is_void() {
            cast::emit_box(&mut emitter, ctx, &target.ret)?;
        }
        let built = emitter.build()?;
        code.function(&built);
    }
    Ok(())
}
