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

pub(super) fn allocate(
    ta: &TypedAst,
    symbols: &mut SymbolTable,
    next: &mut u32,
) -> Result<Vec<Guard>, crate::compiler_error::CompilerFailure> {
    let mut guards = Vec::new();
    for (class, descriptors) in &ta.runtime_field_guards {
        let Type::ClassRef { .. } = class else {
            return Err(crate::codegen::internal_failure(
                "field guard target requires a class",
            ));
        };
        for descriptor in descriptors {
            let declaration = descriptor.check.declaration.as_ref().ok_or_else(|| {
                crate::codegen::internal_failure("field guard declaration is missing")
            })?;
            let function = *next;
            crate::codegen::next_index(next)?;
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
    Ok(guards)
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
    let mut emitter = FunctionEmitter::new(ctx, &params)?;
    super::runtime_descriptors::bind(
        &mut emitter,
        &super::runtime_descriptors::parameters(&guard.target),
        0,
    )?;
    emitter.instruction(Instruction::LocalGet(1));
    let preserve_non_null = matches!(guard.check.test, crate::FieldNarrowingTest::NonNull);
    let mut check = guard.check.clone();
    if !emitter.runtime_type_params.is_empty() {
        check.test = crate::FieldNarrowingTest::Shape(guard.target.clone());
    }
    ctx.checking_standalone(&guard.target, || {
        super::cast_check::emit_narrowed_field_read_guarded(
            &mut emitter,
            ctx,
            &check,
            &guard.target,
            preserve_non_null,
        )
    })?;
    cast::emit_box(&mut emitter, ctx, &guard.target)?;
    emitter.build()
}

pub(super) fn guarded_constructor(
    ctx: &CodegenCtx,
    function: &crate::MangledName,
    result: &Type,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let Type::ClassRef { mangled, .. } = result.lowered() else {
        return Ok(false);
    };
    Ok(*function == crate::mangle::extend(mangled, "constructor")
        && ctx
            .symbols
            .recorded_class_guard_layout(mangled)?
            .has_instance_guards)
}

/// Build the constructor's hidden argument before its initializer can read fields.
pub(super) fn constructor_argument(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    class: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Type::ClassRef { mangled, .. } = class.lowered() else {
        return Err(crate::codegen::internal_failure(
            "guarded constructor requires a class type",
        ));
    };
    let layout = ctx.symbols.recorded_class_guard_layout(mangled)?;
    let depth = layout.inheritance_depth;
    let named_len = layout.named_payload_len;
    let payload_count = super::classes::guard_slot_count(depth, named_len)?;
    let width = named_len
        .checked_add(1)
        .ok_or_else(|| super::internal_failure("field guard width overflow"))?;
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let array = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.object_fields),
    }))?;
    emitter.instruction(Instruction::I32Const(payload_count as i32));
    emitter.instruction(Instruction::ArrayNewDefault(intr.object_fields));
    emitter.instruction(Instruction::LocalSet(array));
    let guards: Vec<_> = ctx.symbols.instance_field_guards(class).collect();
    let mut row_contexts: std::collections::BTreeMap<&crate::MangledName, RowContextEntry<'_>> =
        std::collections::BTreeMap::new();
    if let Some(contexts) = ctx.ta.runtime_class_contexts.get(class) {
        for context in contexts {
            row_contexts
                .entry(&context.declaration)
                .or_default()
                .context_args = Some(context.args.as_slice());
        }
    }
    if guards.is_empty() && row_contexts.is_empty() {
        emitter.instruction(Instruction::LocalGet(array));
        return Ok(());
    }
    let types = GuardClosureTypes::lookup(ctx)?;
    for (declaration, field, function) in guards {
        if let Some(abstract_slot) = ctx.symbols.class_abstract_guard_slot(declaration, field) {
            row_contexts
                .entry(declaration)
                .or_default()
                .abstract_guards
                .push((abstract_slot, field, function));
            continue;
        }
        let slot = ctx
            .symbols
            .class_field_slot(declaration, field)
            .ok_or_else(|| super::internal_failure("guard field"))?;
        let row = ctx
            .symbols
            .recorded_class_guard_layout(declaration)?
            .inheritance_depth;
        let offset = guard_offset(row, width, slot, payload_count)?;
        emitter.instruction(Instruction::LocalGet(array));
        emitter.instruction(Instruction::I32Const(offset as i32));
        emit_guard_closure(emitter, ctx, function, types.vtable, types.closure)?;
        emitter.instruction(Instruction::ArraySet(types.object_fields));
    }
    if !row_contexts.is_empty() {
        let unknown_desc = *ctx
            .symbols
            .type_descriptor_functions
            .get(&Type::Unknown)
            .ok_or_else(|| {
                crate::codegen::internal_failure("class context descriptor is not registered")
            })?;
        for (declaration, entry) in row_contexts {
            let row = ctx
                .symbols
                .recorded_class_guard_layout(declaration)?
                .inheritance_depth;
            let offset = guard_offset(row, width, named_len, payload_count)?;
            let env_len = context_env_len(
                ctx.symbols,
                declaration,
                entry.context_args.map_or(0, <[_]>::len),
            )?;
            let env = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(types.object_fields),
            }))?;
            emitter.instruction(Instruction::I32Const(env_len as i32));
            emitter.instruction(Instruction::ArrayNewDefault(types.object_fields));
            emitter.instruction(Instruction::LocalSet(env));
            if let Some(args) = entry.context_args {
                for (index, arg) in args.iter().enumerate() {
                    emitter.instruction(Instruction::LocalGet(env));
                    emitter.instruction(Instruction::I32Const(
                        crate::codegen::wasm_u32(index)? as i32
                    ));
                    super::runtime_descriptors::emit(emitter, ctx, arg)?;
                    emitter.instruction(Instruction::ArraySet(types.object_fields));
                }
            }
            for (abstract_slot, field, function) in entry.abstract_guards {
                let (name_slot, guard_slot) =
                    abstract_guard_env_slots(ctx.symbols, declaration, abstract_slot)?;
                let name_global =
                    ctx.symbols
                        .field_name_string_global_idx(field)
                        .ok_or_else(|| {
                            crate::codegen::internal_failure("guarded abstract field name interned")
                        })?;
                emit_store_abstract_guard_pair(
                    emitter,
                    ctx,
                    env,
                    (name_slot, guard_slot),
                    name_global,
                    function,
                    types,
                )?;
            }
            emitter.instruction(Instruction::LocalGet(array));
            emitter.instruction(Instruction::I32Const(offset as i32));
            emit_context_closure(emitter, ctx, env, unknown_desc, types.vtable, types.closure)?;
            emitter.instruction(Instruction::ArraySet(types.object_fields));
        }
    }
    emitter.instruction(Instruction::LocalGet(array));

    Ok(())
}

#[derive(Clone, Copy)]
struct GuardClosureTypes {
    vtable: u32,
    closure: u32,
    object_fields: u32,
    object: u32,
}

impl GuardClosureTypes {
    fn lookup(ctx: &CodegenCtx) -> Result<Self, crate::compiler_error::CompilerFailure> {
        let intr = ctx
            .symbols
            .intrinsic_type_indices()
            .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
        let closure = ctx
            .symbols
            .closure_struct_type_idx(signature())
            .ok_or_else(|| crate::codegen::internal_failure("field guard closure registered"))?;
        let vtable = ctx
            .symbols
            .closure_vtable_global_idx()
            .ok_or_else(|| crate::codegen::internal_failure("closure vtable emitted"))?;
        Ok(Self {
            vtable,
            closure,
            object_fields: intr.object_fields,
            object: intr.object,
        })
    }
}

#[derive(Default)]
struct RowContextEntry<'a> {
    context_args: Option<&'a [Type]>,
    abstract_guards: Vec<(u32, &'a str, u32)>,
}

fn guard_offset(
    row: u32,
    width: u32,
    slot: u32,
    count: u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    if slot >= width {
        return Err(super::internal_failure(
            "field guard column exceeds its row",
        ));
    }
    row.checked_mul(width)
        .and_then(|base| base.checked_add(slot))
        .filter(|&offset| offset < count)
        .ok_or_else(|| super::internal_failure("field guard offset exceeds its payload"))
}

/// Constructor result on the stack; retain the concrete validators on its payload.
pub(super) fn attach(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    result: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let class = crate::typechecker::infer::narrowing::strip_nullish(result);
    if ctx.symbols.instance_field_guards(&class).next().is_none() {
        return Ok(());
    }
    if result == &class {
        attach_non_null(emitter, ctx, &class)?;
        return Ok(());
    }
    let value = emitter.add_anonymous_local(ctx.symbols.value_type(result)?)?;
    emitter.instruction(Instruction::LocalSet(value));
    emitter.instruction(Instruction::LocalGet(value));
    super::function_emitter::expr::emit_is_nullish(emitter, ctx)?;
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
    let object = emitter.add_anonymous_local(ctx.symbols.value_type(class)?)?;
    emitter.instruction(Instruction::LocalSet(object));
    let types = GuardClosureTypes::lookup(ctx)?;
    for (declaration, field, function) in guards {
        let depth = ctx
            .symbols
            .recorded_class_guard_layout(declaration)?
            .inheritance_depth;
        if let Some(abstract_slot) = ctx.symbols.class_abstract_guard_slot(declaration, field) {
            emit_attach_abstract_guard(
                emitter,
                ctx,
                object,
                declaration,
                (depth, abstract_slot),
                (field, function),
                types,
            )?;
        } else {
            let slot = ctx
                .symbols
                .class_field_slot(declaration, field)
                .ok_or_else(|| crate::codegen::internal_failure("guarded field slot"))?;
            emit_slot(emitter, ctx, object, depth, slot)?;
            emitter.instruction(Instruction::ArrayGet(types.object_fields));
            emitter.instruction(Instruction::RefIsNull);
            emitter.emit_if(wasm_encoder::BlockType::Empty);
            emit_slot(emitter, ctx, object, depth, slot)?;
            emit_guard_closure(emitter, ctx, function, types.vtable, types.closure)?;
            emitter.instruction(Instruction::ArraySet(types.object_fields));
            emitter.emit_end();
        }
    }
    emitter.instruction(Instruction::LocalGet(object));

    Ok(())
}

fn emit_attach_abstract_guard(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object: u32,
    declaration: &crate::MangledName,
    (depth, abstract_slot): (u32, u32),
    (field, function): (&str, u32),
    types: GuardClosureTypes,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let unknown_desc = *ctx
        .symbols
        .type_descriptor_functions
        .get(&Type::Unknown)
        .ok_or_else(|| {
            crate::codegen::internal_failure("class context descriptor is not registered")
        })?;
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(field)
        .ok_or_else(|| crate::codegen::internal_failure("guarded abstract field name interned"))?;
    let (name_slot, guard_slot) =
        abstract_guard_env_slots(ctx.symbols, declaration, abstract_slot)?;
    let env_len = context_env_len(ctx.symbols, declaration, 0)?;
    let ctx_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(types.object),
    }))?;
    let env_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(types.object_fields),
    }))?;
    emit_context_slot(emitter, ctx, object, depth)?;
    emitter.instruction(Instruction::ArrayGet(types.object_fields));
    emitter.instruction(Instruction::LocalTee(ctx_local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emitter.instruction(Instruction::I32Const(env_len as i32));
    emitter.instruction(Instruction::ArrayNewDefault(types.object_fields));
    emitter.instruction(Instruction::LocalSet(env_local));
    emit_store_abstract_guard_pair(
        emitter,
        ctx,
        env_local,
        (name_slot, guard_slot),
        name_global,
        function,
        types,
    )?;
    emit_context_slot(emitter, ctx, object, depth)?;
    emit_context_closure(
        emitter,
        ctx,
        env_local,
        unknown_desc,
        types.vtable,
        types.closure,
    )?;
    emitter.instruction(Instruction::ArraySet(types.object_fields));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(ctx_local));
    emit_context_env(emitter, types.closure, types.object_fields);
    emitter.instruction(Instruction::LocalTee(env_local));
    emitter.instruction(Instruction::I32Const(guard_slot as i32));
    emitter.instruction(Instruction::ArrayGet(types.object_fields));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emit_store_abstract_guard_pair(
        emitter,
        ctx,
        env_local,
        (name_slot, guard_slot),
        name_global,
        function,
        types,
    )?;
    emitter.emit_end();
    emitter.emit_end();
    Ok(())
}

fn emit_store_abstract_guard_pair(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    env: u32,
    (name_slot, guard_slot): (u32, u32),
    name_global: u32,
    function: u32,
    types: GuardClosureTypes,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(env));
    emitter.instruction(Instruction::I32Const(name_slot as i32));
    emitter.instruction(Instruction::GlobalGet(name_global));
    emitter.instruction(Instruction::ArraySet(types.object_fields));
    emitter.instruction(Instruction::LocalGet(env));
    emitter.instruction(Instruction::I32Const(guard_slot as i32));
    emit_guard_closure(emitter, ctx, function, types.vtable, types.closure)?;
    emitter.instruction(Instruction::ArraySet(types.object_fields));
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
    if !ctx
        .symbols
        .recorded_class_guard_layout(class)?
        .has_instance_guards
    {
        return Ok(());
    }
    // `allocate` requires `check.declaration.is_some()`, so no runtime instance
    // guard exists when `declaration` is absent (for example, a deserialized
    // declaration omitting it); fall through to the static narrowing check.
    let Some(declaration) = ctx
        .symbols
        .class_field_narrowing_check(class, field)
        .and_then(|check| check.declaration.as_ref())
    else {
        return Ok(());
    };
    let depth = ctx
        .symbols
        .recorded_class_guard_layout(declaration)?
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
    }))?;
    emitter.instruction(Instruction::LocalSet(raw));
    if let Some(abstract_slot) = ctx.symbols.class_abstract_guard_slot(declaration, field) {
        let (_, guard_slot) = abstract_guard_env_slots(ctx.symbols, declaration, abstract_slot)?;
        let ctx_local = emitter.add_anonymous_local(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(intr.object),
        }))?;
        emit_context_slot(emitter, ctx, object, depth)?;
        emitter.instruction(Instruction::ArrayGet(intr.object_fields));
        emitter.instruction(Instruction::LocalTee(ctx_local));
        emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
            closure_type,
        )));
        emitter.emit_if(wasm_encoder::BlockType::Result(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(intr.object),
        })));
        emitter.instruction(Instruction::LocalGet(ctx_local));
        emit_context_env(emitter, closure_type, intr.object_fields);
        emitter.instruction(Instruction::I32Const(guard_slot as i32));
        emitter.instruction(Instruction::ArrayGet(intr.object_fields));
        emit_invoke_guard_if_present(emitter, ctx, raw, closure_type, intr.object)?;
        emitter.emit_else();
        emitter.instruction(Instruction::LocalGet(raw));
        emitter.emit_end();
        return Ok(());
    }
    let slot = ctx
        .symbols
        .class_field_slot(class, field)
        .ok_or_else(|| crate::codegen::internal_failure("guarded field slot"))?;
    emit_slot(emitter, ctx, object, depth, slot)?;
    emitter.instruction(Instruction::ArrayGet(intr.object_fields));
    emit_invoke_guard_if_present(emitter, ctx, raw, closure_type, intr.object)
}

fn emit_invoke_guard_if_present(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    raw: u32,
    closure_type: u32,
    object_type: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let closure = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_type),
    }))?;
    emitter.instruction(Instruction::LocalTee(closure));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        closure_type,
    )));
    emitter.emit_if(wasm_encoder::BlockType::Result(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_type),
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

fn emit_guard_closure(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    function: u32,
    vtable: u32,
    closure: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
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
    crate::codegen::closures::emit_property_state(emitter, ctx)?;
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(closure));
    Ok(())
}

fn emit_context_closure(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    env: u32,
    unknown_desc: u32,
    vtable: u32,
    closure: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::GlobalGet(vtable));
    emitter.instruction(Instruction::RefFunc(unknown_desc));
    emitter.instruction(Instruction::LocalGet(env));
    crate::codegen::closures::emit_property_state(emitter, ctx)?;
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(closure));
    Ok(())
}

/// Returns `(name_slot, guard_slot)` inside `declaration`'s context-row
/// environment. Abstract guards are stored as `(field_name, guard_closure)`
/// pairs after the declaration's type-argument descriptors so runtime dynamic
/// reads (`object::dynamic::checked_data_value`) can match them by name while
/// static reads index `guard_slot` directly in O(1).
fn abstract_guard_env_slots(
    symbols: &SymbolTable,
    declaration: &crate::MangledName,
    abstract_slot: u32,
) -> Result<(u32, u32), crate::compiler_error::CompilerFailure> {
    let type_params = symbols
        .class_type_parameters
        .get(declaration)
        .map_or(0, Vec::len);
    let offset = abstract_slot
        .checked_mul(2)
        .ok_or_else(|| super::internal_failure("abstract field guard slot overflow"))?;
    let name_slot = crate::codegen::wasm_u32(type_params)?
        .checked_add(offset)
        .ok_or_else(|| super::internal_failure("abstract field guard slot overflow"))?;
    let guard_slot = name_slot
        .checked_add(1)
        .ok_or_else(|| super::internal_failure("abstract field guard slot overflow"))?;
    Ok((name_slot, guard_slot))
}

fn context_env_len(
    symbols: &SymbolTable,
    declaration: &crate::MangledName,
    context_args_len: usize,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let type_params = symbols
        .class_type_parameters
        .get(declaration)
        .map_or(0, Vec::len)
        .max(context_args_len);
    let abstract_slots = symbols
        .class_abstract_guard_count(declaration)
        .checked_mul(2)
        .ok_or_else(|| super::internal_failure("class context env overflow"))?;
    crate::codegen::wasm_u32(type_params)?
        .checked_add(abstract_slots)
        .ok_or_else(|| super::internal_failure("class context env overflow"))
}

/// Unpack the `$ObjectFields` environment array (field 2) from a context
/// closure already on the stack.
fn emit_context_env(emitter: &mut FunctionEmitter, closure: u32, object_fields: u32) {
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(closure)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure,
        field_index: 2,
    });
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        object_fields,
    )));
}

fn emit_context_slot(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object: u32,
    depth: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    emit_slot(emitter, ctx, object, depth, 0)?;
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::I32Add);
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
    emitter.instruction(Instruction::I32Const(
        depth
            .checked_add(1)
            .ok_or_else(|| super::internal_failure("field guard row overflow"))? as i32,
    ));
    emitter.instruction(Instruction::I32Mul);
    emitter.instruction(Instruction::I32Const(
        depth
            .checked_add(slot)
            .ok_or_else(|| super::internal_failure("field guard slot overflow"))? as i32,
    ));
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
    let layout = ctx.symbols.recorded_class_guard_layout(class)?;
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
        .ok_or_else(|| crate::codegen::internal_failure("predicate closure type"))?;
    let env =
        emitter.add_anonymous_local(super::runtime_descriptors::environment_type(ctx.symbols)?)?;
    emit_context_slot(emitter, ctx, object, layout.inheritance_depth)?;
    emitter.instruction(Instruction::ArrayGet(intr.object_fields));
    emit_context_env(emitter, closure, intr.object_fields);
    emitter.instruction(Instruction::LocalSet(env));
    super::runtime_descriptors::bind(emitter, names, env)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::invariant_tests::{assert_internal, with_context};

    #[test]
    fn recorded_guard_layout_cannot_wrap_payload_size() {
        let name = crate::mangle::package_symbol("main", "Box");
        let class = Type::class_ref(crate::Package("main".into()), "Box", name.clone(), vec![]);
        let mut symbols = crate::codegen::tests::mock_symbols_with_intrinsics();
        symbols
            .record_class_guard_layout(name, None, u32::MAX, true)
            .unwrap();
        with_context(&TypedAst::new(), &symbols, |ctx| {
            let mut emitter = FunctionEmitter::new(ctx, &[]).unwrap();
            assert_internal(constructor_argument(&mut emitter, ctx, &class).unwrap_err());
            assert_internal(emit_slot(&mut emitter, ctx, 0, u32::MAX, 0).unwrap_err());
            assert_internal(emit_slot(&mut emitter, ctx, 0, 1, u32::MAX).unwrap_err());
        });
    }

    #[test]
    fn guard_offsets_stay_within_the_recorded_rows() {
        assert_eq!(guard_offset(2, 4, 3, 12).unwrap(), 11);
        for (row, width, slot, count) in [
            (u32::MAX, 2, 0, 12),
            (3, 4, 0, 12),
            (0, 4, 4, 12),
            (1, u32::MAX, 1, u32::MAX),
        ] {
            assert_internal(guard_offset(row, width, slot, count).unwrap_err());
        }
    }
}
