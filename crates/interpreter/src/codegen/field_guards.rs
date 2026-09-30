//! Concrete field validators attached to instances created from erased classes.

use wasm_encoder::{Function, HeapType, Instruction, RefType, ValType};

use super::closures::ClosureSig;
use super::function_emitter::{FunctionEmitter, cast};
use super::{CodegenCtx, symbol_table::SymbolTable};
use crate::{FieldNarrowingCheck, Ident, Span, Type, TypedAst};

pub(super) struct Guard {
    pub target: Type,
    pub check: FieldNarrowingCheck,
    pub function: u32,
}

pub(super) fn signature() -> ClosureSig {
    ClosureSig {
        arity: 1,
        is_void: false,
    }
}

pub(super) fn allocate(ta: &TypedAst, symbols: &mut SymbolTable, next: &mut u32) -> Vec<Guard> {
    let mut guards = Vec::new();
    for (class, descriptors) in &ta.runtime_field_guards {
        let Type::ClassRef { .. } = class else {
            continue;
        };
        for descriptor in descriptors {
            let Some(declaration) = &descriptor.check.declaration else {
                continue;
            };
            let function = *next;
            *next += 1;
            symbols.record_instance_field_guard(
                class.clone(),
                declaration.clone(),
                descriptor.field.clone(),
                function,
            );
            symbols
                .field_guard_targets
                .insert(function, descriptor.target.clone());
            guards.push(Guard {
                target: descriptor.target.clone(),
                check: descriptor.check.clone(),
                function,
            });
        }
    }
    guards
}

pub(super) fn body(
    ctx: &CodegenCtx,
    guard: &Guard,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let params = [
        (
            Ident {
                name: "env".into(),
                span: Span::at(ctx.file),
            },
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::ANY,
            }),
        ),
        (
            Ident {
                name: "value".into(),
                span: Span::at(ctx.file),
            },
            ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(intr.object),
            }),
        ),
    ];
    let mut emitter = FunctionEmitter::new(ctx, &params);
    super::runtime_descriptors::bind(
        &mut emitter,
        &super::runtime_descriptors::parameters(&guard.target),
        0,
    );
    emitter.instruction(Instruction::LocalGet(1));
    let mut check = guard.check.clone();
    if !emitter.runtime_type_params.is_empty() {
        check.test = crate::FieldNarrowingTest::Shape(guard.target.clone());
    }
    ctx.checking_standalone(&guard.target, || {
        super::cast_check::emit_narrowed_field_read(&mut emitter, ctx, &check, &guard.target)
    })?;
    cast::emit_box(&mut emitter, ctx, &guard.target)?;
    Ok(emitter.build())
}

pub(super) fn guarded_constructor(
    ctx: &CodegenCtx,
    function: &crate::MangledName,
    result: &Type,
) -> bool {
    let Type::ClassRef { mangled, .. } = result.peel() else {
        return false;
    };
    *function == crate::mangle::extend(mangled, "constructor")
        && ctx.symbols.class_guard_layout(mangled).has_instance_guards
}

/// Build the constructor's hidden argument before its initializer can read fields.
pub(super) fn constructor_argument(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    class: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Type::ClassRef { mangled, .. } = class.peel() else {
        return Err(crate::codegen::internal_failure(
            "guarded constructor requires a class type",
        ));
    };
    let layout = ctx.symbols.class_guard_layout(mangled);
    let depth = layout.inheritance_depth;
    let named_len = layout.named_payload_len;
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let array = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.object_fields),
    }));
    emitter.instruction(Instruction::I32Const(
        ((depth + 1) * (named_len + 1)) as i32,
    ));
    emitter.instruction(Instruction::ArrayNewDefault(intr.object_fields));
    emitter.instruction(Instruction::LocalSet(array));
    let guards: Vec<_> = ctx.symbols.instance_field_guards(class).collect();
    if !guards.is_empty() {
        let closure = ctx
            .symbols
            .closure_struct_type_idx(signature())
            .ok_or_else(|| crate::codegen::internal_failure("guard closure"))?;
        let vtable = ctx
            .symbols
            .closure_vtable_global_idx()
            .ok_or_else(|| crate::codegen::internal_failure("closure vtable"))?;
        for (declaration, field, function) in guards {
            let offset = ctx
                .symbols
                .class_guard_layout(declaration)
                .inheritance_depth
                * (named_len + 1)
                + ctx
                    .symbols
                    .class_field_slot(declaration, field)
                    .ok_or_else(|| crate::codegen::internal_failure("guard field"))?;
            emitter.instruction(Instruction::LocalGet(array));
            emitter.instruction(Instruction::I32Const(offset as i32));
            emitter.instruction(Instruction::GlobalGet(vtable));
            emitter.instruction(Instruction::RefFunc(function));
            super::runtime_descriptors::capture(
                emitter,
                ctx,
                ctx.symbols
                    .field_guard_targets
                    .get(&function)
                    .ok_or_else(|| {
                        crate::codegen::internal_failure("field guard target is not registered")
                    })?,
            )?;
            emitter.instruction(Instruction::StructNew(closure));
            emitter.instruction(Instruction::ArraySet(intr.object_fields));
        }
    }
    if let Some(contexts) = ctx.ta.runtime_class_contexts.get(class) {
        for context in contexts {
            let offset = ctx
                .symbols
                .class_guard_layout(&context.declaration)
                .inheritance_depth
                * (named_len + 1)
                + named_len;
            emitter.instruction(Instruction::LocalGet(array));
            emitter.instruction(Instruction::I32Const(offset as i32));
            emitter.instruction(Instruction::GlobalGet(
                ctx.symbols
                    .closure_vtable_global_idx()
                    .ok_or_else(|| crate::codegen::internal_failure("closure vtable"))?,
            ));
            emitter.instruction(Instruction::RefFunc(
                *ctx.symbols
                    .type_descriptor_functions
                    .get(&Type::Unknown)
                    .ok_or_else(|| {
                        crate::codegen::internal_failure(
                            "class context descriptor is not registered",
                        )
                    })?,
            ));
            super::runtime_descriptors::environment(emitter, ctx, &context.args)?;
            emitter.instruction(Instruction::StructNew(
                ctx.symbols
                    .closure_struct_type_idx(signature())
                    .ok_or_else(|| crate::codegen::internal_failure("descriptor closure"))?,
            ));
            emitter.instruction(Instruction::ArraySet(intr.object_fields));
        }
    }
    emitter.instruction(Instruction::LocalGet(array));

    Ok(())
}

/// Constructor result on the stack; retain the concrete validators on its payload.
pub(super) fn attach(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    result: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let class = crate::typechecker::infer::narrowing::strip_null(result);
    if ctx.symbols.instance_field_guards(&class).next().is_none() {
        return Ok(());
    }
    if result == &class {
        attach_non_null(emitter, ctx, &class)?;
        return Ok(());
    }
    let value = emitter.add_anonymous_local(ctx.symbols.value_type(result)?);
    emitter.instruction(Instruction::LocalSet(value));
    emitter.instruction(Instruction::LocalGet(value));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(value));
    cast::emit_cast_to(emitter, ctx, &class)?;
    attach_non_null(emitter, ctx, &class)?;
    emitter.instruction(Instruction::Drop);
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(value));

    Ok(())
}

fn attach_non_null(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    class: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let guards: Vec<_> = ctx.symbols.instance_field_guards(class).collect();
    if guards.is_empty() {
        return Ok(());
    }
    let object = emitter.add_anonymous_local(ctx.symbols.value_type(class)?);
    emitter.instruction(Instruction::LocalSet(object));
    let closure = ctx
        .symbols
        .closure_struct_type_idx(signature())
        .ok_or_else(|| crate::codegen::internal_failure("field guard closure registered"))?;
    let vtable = ctx
        .symbols
        .closure_vtable_global_idx()
        .ok_or_else(|| crate::codegen::internal_failure("closure vtable emitted"))?;
    for (declaration, field, function) in guards {
        let slot = ctx
            .symbols
            .class_field_slot(declaration, field)
            .ok_or_else(|| crate::codegen::internal_failure("guarded field slot"))?;
        let depth = ctx
            .symbols
            .class_guard_layout(declaration)
            .inheritance_depth;
        emit_slot(emitter, ctx, object, depth, slot)?;
        emitter.instruction(Instruction::ArrayGet(
            ctx.symbols
                .intrinsic_type_indices()
                .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
                .object_fields,
        ));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(wasm_encoder::BlockType::Empty);
        emit_slot(emitter, ctx, object, depth, slot)?;
        emitter.instruction(Instruction::GlobalGet(vtable));
        emitter.instruction(Instruction::RefFunc(function));
        super::runtime_descriptors::capture(
            emitter,
            ctx,
            ctx.symbols
                .field_guard_targets
                .get(&function)
                .ok_or_else(|| {
                    crate::codegen::internal_failure("field guard target is not registered")
                })?,
        )?;
        emitter.instruction(Instruction::StructNew(closure));
        emitter.instruction(Instruction::ArraySet(
            ctx.symbols
                .intrinsic_type_indices()
                .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
                .object_fields,
        ));
        emitter.emit_end();
    }
    emitter.instruction(Instruction::LocalGet(object));

    Ok(())
}

/// Raw field value on stack; invoke the instance's concrete guard if present.
pub(super) fn check(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object: u32,
    class: &crate::MangledName,
    field: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if !ctx.symbols.class_guard_layout(class).has_instance_guards {
        return Ok(());
    }
    let slot = ctx
        .symbols
        .class_field_slot(class, field)
        .ok_or_else(|| crate::codegen::internal_failure("guarded field slot"))?;
    let declaration = ctx
        .symbols
        .class_field_narrowing_check(class, field)
        .and_then(|check| check.declaration.as_ref())
        .unwrap_or(class);
    let depth = ctx
        .symbols
        .class_guard_layout(declaration)
        .inheritance_depth;
    let closure_type = ctx
        .symbols
        .closure_struct_type_idx(signature())
        .ok_or_else(|| {
            crate::codegen::internal_failure("field guard closure type is not registered")
        })?;
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let raw = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intr.object),
    }));
    let closure = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intr.object),
    }));
    emitter.instruction(Instruction::LocalSet(raw));
    emit_slot(emitter, ctx, object, depth, slot)?;
    emitter.instruction(Instruction::ArrayGet(intr.object_fields));
    emitter.instruction(Instruction::LocalTee(closure));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        closure_type,
    )));
    emitter.emit_if(wasm_encoder::BlockType::Result(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intr.object),
    })));
    emitter.instruction(Instruction::LocalGet(closure));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        closure_type,
    )));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_type,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(raw));
    emitter.instruction(Instruction::LocalGet(closure));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        closure_type,
    )));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_type,
        field_index: 1,
    });
    emitter.instruction(Instruction::CallRef(
        ctx.symbols
            .closure_func_type_idx(signature())
            .ok_or_else(|| crate::codegen::internal_failure("guard signature registered"))?,
    ));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(raw));
    emitter.emit_end();

    Ok(())
}

fn emit_slot(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object: u32,
    depth: u32,
    slot: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.object_shape,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::I32Const((depth + 1) as i32));
    emitter.instruction(Instruction::I32Mul);
    emitter.instruction(Instruction::I32Const((depth + slot) as i32));
    emitter.instruction(Instruction::I32Add);

    Ok(())
}

/// Recover this declaration's generic arguments, including substituted ancestors.
pub(super) fn bind_receiver(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object: u32,
    class: &crate::MangledName,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(names) = ctx
        .symbols
        .class_type_parameters
        .get(class)
        .filter(|names| !names.is_empty())
    else {
        return Ok(());
    };
    let layout = ctx.symbols.class_guard_layout(class);
    if !layout.has_instance_guards {
        return Ok(());
    }
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    let closure = ctx
        .symbols
        .closure_struct_type_idx(signature())
        .ok_or_else(|| crate::codegen::internal_failure("descriptor closure"))?;
    let env =
        emitter.add_anonymous_local(super::runtime_descriptors::environment_type(ctx.symbols)?);
    emit_slot(emitter, ctx, object, layout.inheritance_depth, 0)?;
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::ArrayGet(intr.object_fields));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(closure)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure,
        field_index: 2,
    });
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        intr.object_fields,
    )));
    emitter.instruction(Instruction::LocalSet(env));
    super::runtime_descriptors::bind(emitter, names, env);

    Ok(())
}
