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

pub fn emit_bodies(metas: &[AdapterMeta], code: &mut CodeSection, ctx: &CodegenCtx<'_>) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
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
        let target_idx = ctx
            .symbols
            .func_idx(&meta.mangled)
            .expect("adapter target function recorded during user-function pre-pass");
        let Type::Function { params, ret, .. } = &meta.signature else {
            panic!(
                "AdapterMeta.signature must be Type::Function, got {:?}",
                meta.signature
            );
        };

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
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params);

        for (i, p_ty) in params.iter().enumerate() {
            emitter.instruction(Instruction::LocalGet((i + 1) as u32));
            cast::emit_cast_to(&mut emitter, ctx, p_ty);
        }
        emitter.instruction(Instruction::Call(target_idx));
        if !ret.is_void() {
            cast::emit_box(&mut emitter, ctx, ret);
        }
        let built = emitter.build();
        code.function(&built);
    }
}
