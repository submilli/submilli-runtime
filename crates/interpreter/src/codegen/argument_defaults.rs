//! Materialize native parameter defaults after all arguments have been evaluated.

use super::function_emitter::{FunctionEmitter, cast, emit_inline_string_literal, expr};
use super::{CodegenCtx, internal_failure};
use crate::compiler_error::CompilerFailure;
use crate::{DefaultValue, Type};
use wasm_encoder::{BlockType, HeapType, Instruction, ValType};

pub(super) fn emit_argument(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    source: &Type,
    slot: ValType,
    default: Option<&DefaultValue>,
) -> Result<(), CompilerFailure> {
    let source_slot = ctx.symbols.value_type(source)?;
    let Some(default) = default.filter(|_| matches!(source_slot, ValType::Ref(_))) else {
        return cast::emit_coerce_to_wasm_slot(emitter, ctx, source, slot);
    };
    if matches!(source.peel(), Type::Undefined) {
        emitter.instruction(Instruction::Drop);
        let ty = emit_default(emitter, ctx, default)?;
        return cast::emit_coerce_to_wasm_slot(emitter, ctx, &ty, slot);
    }
    let saved = emitter.add_anonymous_local(ctx.symbols.value_type(source)?)?;
    emitter.instruction(Instruction::LocalTee(saved));
    expr::emit_is_undefined(emitter, ctx)?;
    emitter.emit_if(BlockType::Result(slot));
    let ty = emit_default(emitter, ctx, default)?;
    cast::emit_coerce_to_wasm_slot(emitter, ctx, &ty, slot)?;
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(saved));
    // The value is known not to be `undefined` here, so a primitive slot
    // takes the defined part's representation.
    let present = crate::typechecker::infer::narrowing::strip_undefined(source);
    if present != *source {
        cast::emit_cast_to(emitter, ctx, &present)?;
    }
    cast::emit_coerce_to_wasm_slot(emitter, ctx, &present, slot)?;
    emitter.emit_end();
    Ok(())
}

fn emit_default(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    value: &DefaultValue,
) -> Result<Type, CompilerFailure> {
    Ok(match value {
        DefaultValue::Undefined => {
            expr::emit_undefined(emitter, ctx)?;
            Type::Undefined
        }
        // Only an explicit `undefined` argument reaches here; an omitted one
        // was filled at the call site.
        DefaultValue::Number(value)
        | DefaultValue::OmittedNumber {
            undefined: value, ..
        } => {
            emitter.instruction(Instruction::F64Const((*value).into()));
            Type::Number
        }
        DefaultValue::String(value) => {
            emit_inline_string_literal(emitter, ctx, value)?;
            Type::String
        }
        DefaultValue::Boolean(value) => {
            emitter.instruction(Instruction::I32Const(i32::from(*value)));
            Type::Boolean
        }
        DefaultValue::Null => {
            emitter.instruction(Instruction::RefNull(HeapType::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::None,
            }));
            Type::Null
        }
        DefaultValue::GlobalConst(name) => {
            let index = ctx
                .symbols
                .global_idx(name)
                .ok_or_else(|| internal_failure("native default global was not imported"))?;
            let ty = ctx
                .symbols
                .global_type(name)
                .cloned()
                .ok_or_else(|| internal_failure("native default global has no type"))?;
            emitter.instruction(Instruction::GlobalGet(index));
            if matches!(ctx.symbols.value_type(&ty)?, ValType::Ref(reference) if !reference.nullable)
            {
                emitter.instruction(Instruction::RefAsNonNull);
            }
            ty
        }
        DefaultValue::EnumVariant { value, .. } => match value {
            crate::EnumVariantValue::Number(value) => {
                emitter.instruction(Instruction::F64Const((*value).into()));
                Type::Number
            }
            crate::EnumVariantValue::String(value) => {
                emit_inline_string_literal(emitter, ctx, value)?;
                Type::String
            }
        },
        DefaultValue::EmptyArray | DefaultValue::EmptyObject => {
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .ok_or_else(|| internal_failure("native default intrinsics were not declared"))?;
            if matches!(value, DefaultValue::EmptyArray) {
                let vtable = ctx
                    .symbols
                    .prelude_global_idx("array_vtable")
                    .ok_or_else(|| internal_failure("array vtable was not imported"))?;
                emitter.instruction(Instruction::GlobalGet(vtable));
                emitter.instruction(Instruction::ArrayNewFixed {
                    array_type_index: intrinsics.raw_array,
                    array_size: 0,
                });
                emitter.instruction(Instruction::I32Const(0));
                emitter.instruction(Instruction::StructNew(intrinsics.array));
                Type::Array(Box::new(Type::Unknown))
            } else {
                let vtable = ctx
                    .symbols
                    .prelude_global_idx("object_vtable")
                    .ok_or_else(|| internal_failure("object vtable was not imported"))?;
                emitter.instruction(Instruction::GlobalGet(vtable));
                emitter.instruction(Instruction::ArrayNewFixed {
                    array_type_index: intrinsics.field_names,
                    array_size: 0,
                });
                emitter.instruction(Instruction::ArrayNewFixed {
                    array_type_index: intrinsics.object_fields,
                    array_size: 0,
                });
                emitter.instruction(Instruction::RefNull(HeapType::ANY));
                emitter.instruction(Instruction::StructNew(intrinsics.object_shape));
                Type::Unknown
            }
        }
    })
}
