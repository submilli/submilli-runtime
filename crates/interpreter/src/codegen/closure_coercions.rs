//! Bridges closure return conventions, omitted default arguments, and ignored
//! trailing arguments without changing the callee.

use wasm_encoder::{
    CodeSection, Function, FunctionSection, HeapType, Instruction, RefType, ValType,
};

use super::CodegenCtx;
use super::closures::ClosureSig;
use super::function_emitter::FunctionEmitter;
use super::symbol_table::SymbolTable;

pub fn allocate(
    symbols: &mut SymbolTable,
    next_func: &mut u32,
) -> Result<Vec<ClosureSig>, crate::compiler_error::CompilerFailure> {
    let signatures: Vec<_> = symbols.closure_signatures().collect();
    super::closures::ensure_index_capacity(*next_func, Some(signatures.len()), 0)?;
    let mut targets = Vec::new();
    for target in signatures {
        if symbols.closure_struct_type_idx(opposite(target)).is_none() {
            continue;
        }
        symbols.record_closure_coercion(target, *next_func);
        *next_func += 1;
        targets.push(target);
    }
    Ok(targets)
}

fn opposite(sig: ClosureSig) -> ClosureSig {
    ClosureSig {
        is_void: !sig.is_void,
        ..sig
    }
}

pub fn emit_entries(
    targets: &[ClosureSig],
    functions: &mut FunctionSection,
    symbols: &SymbolTable,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for &target in targets {
        functions.function(
            symbols
                .closure_func_type_idx(target)
                .ok_or_else(|| crate::codegen::internal_failure("target closure type"))?,
        );
    }
    Ok(())
}

pub fn emit_bodies(
    targets: &[ClosureSig],
    code: &mut CodeSection,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for &target in targets {
        code.function(&emit_body(target, ctx)?);
    }
    Ok(())
}

fn emit_body(
    target: ClosureSig,
    ctx: &CodegenCtx<'_>,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let wrapper = ctx
        .symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    let object = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .object;
    let original = u32::from(target.arity) + 1;
    let receiver = original + 1;
    let env = original + 2;
    let mut body = Function::new([
        (
            1,
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(object),
            }),
        ),
        (
            1,
            ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(object),
            }),
        ),
        (
            1,
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::ANY,
            }),
        ),
    ]);
    body.instruction(&Instruction::LocalGet(0));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(wrapper)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: wrapper,
        field_index: 1,
    });
    body.instruction(&Instruction::LocalSet(receiver));
    body.instruction(&Instruction::LocalGet(0));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(wrapper)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: wrapper,
        field_index: 0,
    });
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(object)));
    body.instruction(&Instruction::LocalSet(original));
    // Call what an adapter of an adapter ultimately wraps, which takes every
    // argument JavaScript would pass it: an inner adapter would drop those
    // past its own arity. An adapter binds no receiver of its own.
    emit_original_identity(&mut body, ctx.symbols, original)?;
    let result = if target.is_void {
        wasm_encoder::BlockType::Empty
    } else {
        wasm_encoder::BlockType::Result(ctx.symbols.value_type(&crate::Type::Unknown)?)
    };
    let sources = direct_sources(target, ctx.symbols);
    for &source in &sources {
        let structure = ctx
            .symbols
            .closure_struct_type_idx(source)
            .ok_or_else(|| crate::codegen::internal_failure("source closure type"))?;
        emit_is_directly_callable(&mut body, ctx, structure, original, env)?;
        body.instruction(&Instruction::If(result));
        emit_direct_call(&mut body, ctx, source, target, original, receiver, env)?;
        body.instruction(&Instruction::Else);
    }
    emit_default_adapter_call(&mut body, ctx, target, original, receiver)?;
    for _ in &sources {
        body.instruction(&Instruction::End);
    }
    body.instruction(&Instruction::End);
    Ok(body)
}

/// The closures an adapter to `target` calls without re-entering the host: the
/// other return convention at the same arity, and every smaller arity, whose
/// closures ignore the trailing arguments. Every argument such a closure
/// declares is supplied, so its defaults are never needed, but a rest closure
/// expects the arguments past its fixed parameters packed, so
/// [`emit_is_directly_callable`] leaves those to the host.
fn direct_sources(target: ClosureSig, symbols: &SymbolTable) -> Vec<ClosureSig> {
    let smaller = (0..target.arity)
        .rev()
        .flat_map(|arity| [false, true].map(|is_void| ClosureSig { arity, is_void }));
    std::iter::once(opposite(target))
        .chain(smaller)
        .filter(|source| symbols.closure_struct_type_idx(*source).is_some())
        .collect()
}

/// Push whether the closure in `original` is a `structure` closure without
/// argument metadata, which an adapter can call directly. A closure with
/// metadata may have a rest parameter, and only the host's binding packs its
/// arguments. The metadata sits inside any receiver bindings of the closure's
/// environment; `env` is free to use as scratch until the call binds it.
fn emit_is_directly_callable(
    body: &mut Function,
    ctx: &CodegenCtx<'_>,
    structure: u32,
    original: u32,
    env: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let metadata = ctx
        .symbols
        .call_metadata_type
        .ok_or_else(|| crate::codegen::internal_failure("call metadata type"))?;
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
        ValType::I32,
    )));
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: structure,
        field_index: 2,
    });
    body.instruction(&Instruction::LocalSet(env));
    emit_unwrap_receiver_bindings(body, ctx, env)?;
    body.instruction(&Instruction::LocalGet(env));
    body.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(metadata)));
    body.instruction(&Instruction::I32Eqz);
    body.instruction(&Instruction::Else);
    body.instruction(&Instruction::I32Const(0));
    body.instruction(&Instruction::End);
    Ok(())
}

/// Replace the environment in `env` with the one inside its receiver
/// bindings (`this_environment` wrappers), however many are nested.
fn emit_unwrap_receiver_bindings(
    body: &mut Function,
    ctx: &CodegenCtx<'_>,
    env: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let wrapper = ctx
        .symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    body.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
    body.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
    body.instruction(&Instruction::LocalGet(env));
    body.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(wrapper)));
    body.instruction(&Instruction::I32Eqz);
    body.instruction(&Instruction::BrIf(1));
    body.instruction(&Instruction::LocalGet(env));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(wrapper)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: wrapper,
        field_index: 0,
    });
    body.instruction(&Instruction::LocalSet(env));
    body.instruction(&Instruction::Br(0));
    body.instruction(&Instruction::End);
    body.instruction(&Instruction::End);
    Ok(())
}

/// Call the original closure with its own environment and the leading
/// `source.arity` arguments, then convert its result to `target`'s convention.
fn emit_direct_call(
    body: &mut Function,
    ctx: &CodegenCtx<'_>,
    source: ClosureSig,
    target: ClosureSig,
    original: u32,
    receiver: u32,
    env: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let wrapper = ctx
        .symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    let structure = ctx
        .symbols
        .closure_struct_type_idx(source)
        .ok_or_else(|| crate::codegen::internal_failure("source closure type"))?;
    let signature = ctx
        .symbols
        .closure_func_type_idx(source)
        .ok_or_else(|| crate::codegen::internal_failure("source function type"))?;
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: structure,
        field_index: 2,
    });
    for instruction in super::this_binding::binding_instructions(wrapper, env, receiver) {
        body.instruction(&instruction);
    }
    for i in 1..=u32::from(source.arity) {
        body.instruction(&Instruction::LocalGet(i));
    }
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: structure,
        field_index: 1,
    });
    body.instruction(&Instruction::CallRef(signature));
    if !source.is_void && target.is_void {
        body.instruction(&Instruction::Drop);
    }
    if source.is_void && !target.is_void {
        let object = ctx
            .symbols
            .intrinsic_type_indices()
            .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
            .object;
        body.instruction(&Instruction::RefNull(HeapType::Concrete(object)));
    };
    Ok(())
}

fn emit_default_adapter_call(
    body: &mut Function,
    ctx: &CodegenCtx<'_>,
    target: ClosureSig,
    original: u32,
    receiver: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::LocalGet(receiver));
    body.instruction(&Instruction::GlobalGet(
        ctx.symbols
            .prelude_global_idx("array_vtable")
            .ok_or_else(|| crate::codegen::internal_failure("array vtable"))?,
    ));
    for i in 1..=u32::from(target.arity) {
        body.instruction(&Instruction::LocalGet(i));
    }
    body.instruction(&Instruction::ArrayNewFixed {
        array_type_index: intr.raw_array,
        array_size: u32::from(target.arity),
    });
    body.instruction(&Instruction::I32Const(i32::from(target.arity)));
    body.instruction(&Instruction::StructNew(intr.array));
    body.instruction(&Instruction::Call(
        ctx.symbols
            .prelude_func_idx("__value_invoke_defaults")
            .ok_or_else(|| crate::codegen::internal_failure("default invocation collected"))?,
    ));
    if target.is_void {
        body.instruction(&Instruction::Drop);
    };
    Ok(())
}

/// Normalize reference values to a concrete closure slot, preserving the
/// underlying function identity across return-convention and default adapters.
pub fn emit_coercion(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    source: &crate::Type,
    target_slot: ValType,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    if !matches!(ctx.symbols.value_type(source)?, ValType::Ref(_)) {
        return Ok(false);
    }
    let target = ctx.symbols.closure_signatures().find(|signature| {
        ctx.symbols
            .closure_struct_type_idx(*signature)
            .is_some_and(|structure| {
                target_slot
                    == ValType::Ref(RefType {
                        nullable: false,
                        heap_type: HeapType::Concrete(structure),
                    })
            })
    });
    let Some(target) = target else {
        return Ok(false);
    };
    if takes_fewer_arguments(source, target) || packs_rest_arguments(source, target) {
        let original =
            emitter.add_anonymous_local(ctx.symbols.value_type(&crate::Type::Unknown)?)?;
        emitter.instruction(Instruction::LocalSet(original));
        emit_wrap(emitter, ctx, target, original)?;
    } else {
        emit_erased_cast(emitter, ctx, target)?;
    }
    Ok(true)
}

/// A function with a rest parameter in a slot of another arity stands for a
/// fixed-arity function: a rest function type it fits has as many parameters.
/// Its adapter binds the arguments through the host, which packs the rest.
fn packs_rest_arguments(source: &crate::Type, target: ClosureSig) -> bool {
    matches!(
        source.peel(),
        crate::Type::Function { params, has_rest: true, .. } if params.len() != usize::from(target.arity)
    )
}

/// A function statically known to declare fewer parameters than `target`
/// passes, and no rest parameter, always needs the adapter, so the runtime
/// check [`emit_erased_cast`] makes is skipped.
fn takes_fewer_arguments(source: &crate::Type, target: ClosureSig) -> bool {
    matches!(
        source.peel(),
        crate::Type::Function { params, has_rest: false, .. } if params.len() < usize::from(target.arity)
    )
}

/// An erased field can carry either return convention or a method with trailing
/// defaults. Validate omitted arguments before adapting to the target ABI.
pub fn emit_erased_cast(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    target: ClosureSig,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let target_struct = ctx
        .symbols
        .closure_struct_type_idx(target)
        .ok_or_else(|| crate::codegen::internal_failure("target closure"))?;
    let target_slot = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(target_struct),
    });
    let source = opposite(target);
    let source_struct = ctx
        .symbols
        .closure_struct_type_idx(source)
        .ok_or_else(|| crate::codegen::internal_failure("source closure"))?;
    let original = emitter.add_anonymous_local(ctx.symbols.value_type(&crate::Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalTee(original));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        source_struct,
    )));
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        target_struct,
    )));
    emitter.instruction(Instruction::I32Or);
    // Same-arity values need no metadata lookup or allocation.
    emitter.emit_if(wasm_encoder::BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        source_struct,
    )));
    emitter.emit_else();
    emit_defaults_fit(emitter, ctx, original, target.arity, None)?;
    emitter.emit_end();
    emitter.emit_if(wasm_encoder::BlockType::Result(target_slot));
    emit_wrap(emitter, ctx, target, original)?;
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        target_struct,
    )));
    emitter.emit_end();

    Ok(())
}

/// Wrap the function in `original` in `target`'s adapter, keeping its identity.
fn emit_wrap(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    target: ClosureSig,
    original: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_identity_vtable(emitter, ctx, original)?;
    emitter.instruction(Instruction::RefFunc(
        ctx.symbols
            .closure_coercion(target)
            .ok_or_else(|| crate::codegen::internal_failure("coercion allocated"))?,
    ));
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefAsNonNull);
    super::this_binding::wrap(emitter, ctx)?;
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(
        ctx.symbols
            .closure_struct_type_idx(target)
            .ok_or_else(|| crate::codegen::internal_failure("target closure"))?,
    ));

    Ok(())
}

pub(super) fn emit_defaults_fit(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    function: u32,
    arity: u8,
    is_void: Option<bool>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(function));
    emitter.instruction(Instruction::F64Const(f64::from(arity).into()));
    super::function_emitter::cast::emit_box(emitter, ctx, &crate::Type::Number)?;
    let results = is_void.map_or(-1.0, |is_void| if is_void { 0.0 } else { 1.0 });
    emitter.instruction(Instruction::F64Const(results.into()));
    super::function_emitter::cast::emit_box(emitter, ctx, &crate::Type::Number)?;
    emitter.instruction(Instruction::Call(
        ctx.symbols
            .prelude_func_idx("__value_defaults_fit")
            .ok_or_else(|| crate::codegen::internal_failure("default compatibility collected"))?,
    ));
    super::function_emitter::cast::emit_cast_to(emitter, ctx, &crate::Type::Boolean)?;

    Ok(())
}

/// The adapter vtable's field holding the function the adapter wraps. The
/// host reads it too (`closure::original`), so it must stay in step with
/// [`emit_vtable_type`].
pub(crate) const ADAPTER_ORIGINAL_FIELD: u32 = 4;

/// The fifth field ([`ADAPTER_ORIGINAL_FIELD`]) distinguishes adapter vtables
/// from ordinary and class vtables and carries the original function identity
/// across module boundaries.
pub fn emit_vtable_type(
    types: &mut wasm_encoder::TypeSection,
    symbols: &mut SymbolTable,
    next_type: &mut u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    use wasm_encoder::{
        CompositeInnerType, CompositeType, FieldType, StorageType, StructType, SubType,
    };
    let following_type_idx = next_type.checked_add(1).ok_or_else(|| {
        crate::codegen::internal_failure("closure coercion vtable exhausts the Wasm index space")
    })?;
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    let fields = [
        intrinsics.to_string_fn,
        intrinsics.to_json_fn,
        intrinsics.equals_fn,
        intrinsics.hash_fn,
        intrinsics.object,
    ]
    .into_iter()
    .map(|index| FieldType {
        element_type: StorageType::Val(ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(index),
        })),
        mutable: false,
    })
    .collect::<Vec<_>>();
    types.ty().subtype(&SubType {
        is_final: true,
        supertype_idx: Some(intrinsics.vtable),
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
                fields: fields.into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });
    symbols.record_closure_coercion_vtable_type(*next_type);
    symbols.record_struct_supertype(*next_type, intrinsics.vtable);
    *next_type = following_type_idx;

    Ok(())
}

fn emit_identity_vtable(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    original: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    let vtable = ctx
        .symbols
        .closure_vtable_global_idx()
        .ok_or_else(|| crate::codegen::internal_failure("closure vtable"))?;
    for field in 0..4 {
        emitter.instruction(Instruction::GlobalGet(vtable));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: intrinsics.vtable,
            field_index: field,
        });
    }
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::StructNew(
        ctx.symbols.closure_coercion_vtable_type()?,
    ));

    Ok(())
}

pub fn emit_equals(
    symbols: &SymbolTable,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let mut body = Function::new([]);
    emit_original_identity(&mut body, symbols, 0)?;
    emit_original_identity(&mut body, symbols, 1)?;
    body.instruction(&Instruction::LocalGet(0));
    body.instruction(&Instruction::LocalGet(1));
    body.instruction(&Instruction::RefEq);
    body.instruction(&Instruction::End);
    Ok(body)
}

fn emit_original_identity(
    body: &mut Function,
    symbols: &SymbolTable,
    local: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let object = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .object;
    let wrapper = symbols.closure_coercion_vtable_type()?;
    body.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
    body.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
    body.instruction(&Instruction::LocalGet(local));
    body.instruction(&Instruction::RefIsNull);
    body.instruction(&Instruction::BrIf(1));
    body.instruction(&Instruction::LocalGet(local));
    body.instruction(&Instruction::StructGet {
        struct_type_index: object,
        field_index: 0,
    });
    body.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(wrapper)));
    body.instruction(&Instruction::I32Eqz);
    body.instruction(&Instruction::BrIf(1));
    body.instruction(&Instruction::LocalGet(local));
    body.instruction(&Instruction::StructGet {
        struct_type_index: object,
        field_index: 0,
    });
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(wrapper)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: wrapper,
        field_index: ADAPTER_ORIGINAL_FIELD,
    });
    body.instruction(&Instruction::LocalSet(local));
    body.instruction(&Instruction::Br(0));
    body.instruction(&Instruction::End);
    body.instruction(&Instruction::End);

    Ok(())
}
