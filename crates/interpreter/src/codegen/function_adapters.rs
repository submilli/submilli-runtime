//! Collects top-level functions used in value position and emits closure-shaped adapter bodies for them.

use wasm_encoder::{
    CodeSection, ConstExpr, GlobalSection, GlobalType, HeapType, Instruction, RefType, ValType,
};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::{FunctionEmitter, cast};
use crate::codegen::symbol_table::SymbolTable;
use crate::{Ident, Span, Type};

#[derive(Clone, Debug)]
pub struct AdapterMeta {
    pub name: String,
    pub mangled: crate::MangledName,
    pub signature: Type,
}

/// Allocates one global per adapter that caches the function's closure, so every
/// read of a top-level function yields the same value and `f === f` holds. The
/// closure has no environment, so one instance serves every read. Each starts
/// null and is filled on first read. Returns how many globals were added.
pub fn allocate_closure_globals(
    metas: &[AdapterMeta],
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    for meta in metas {
        let closure_struct_idx = symbols
            .closure_struct_type_idx(super::closures::classify(&meta.signature)?)
            .ok_or_else(|| {
                crate::codegen::internal_failure(
                    "closure struct type registered for every function-as-value",
                )
            })?;
        let heap_type = HeapType::Concrete(closure_struct_idx);
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
        symbols.record_adapter_closure_global_idx(meta.mangled.clone(), *next_global_idx);
        crate::codegen::next_index(next_global_idx)?;
    }
    crate::codegen::wasm_u32(metas.len())
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
