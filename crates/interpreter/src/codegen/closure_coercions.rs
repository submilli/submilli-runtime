//! Bridges the value-returning and void closure ABIs without changing the callee.

use wasm_encoder::{
    CodeSection, Function, FunctionSection, HeapType, Instruction, RefType, ValType,
};

use super::CodegenCtx;
use super::closures::ClosureSig;
use super::function_emitter::FunctionEmitter;
use super::symbol_table::SymbolTable;

pub fn allocate(symbols: &mut SymbolTable, next_func: &mut u32) -> Vec<ClosureSig> {
    let signatures: Vec<_> = symbols.closure_signatures().collect();
    let mut targets = Vec::new();
    for target in signatures {
        if symbols.closure_struct_type_idx(opposite(target)).is_none() {
            continue;
        }
        symbols.record_closure_coercion(target, *next_func);
        *next_func += 1;
        targets.push(target);
    }
    targets
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
) {
    for &target in targets {
        functions.function(
            symbols
                .closure_func_type_idx(target)
                .expect("target closure type"),
        );
    }
}

pub fn emit_bodies(targets: &[ClosureSig], code: &mut CodeSection, ctx: &CodegenCtx<'_>) {
    for &target in targets {
        code.function(&emit_body(target, ctx));
    }
}

fn emit_body(target: ClosureSig, ctx: &CodegenCtx<'_>) -> Function {
    let source = opposite(target);
    let structure = ctx
        .symbols
        .closure_struct_type_idx(source)
        .expect("source closure type");
    let signature = ctx
        .symbols
        .closure_func_type_idx(source)
        .expect("source function type");
    let wrapper = ctx.symbols.this_environment_type.expect("this environment");
    let object = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics")
        .object;
    let original = u32::from(target.arity) + 1;
    let receiver = original + 1;
    let env = original + 2;
    let mut body = Function::new([
        (
            1,
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(structure),
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
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::LocalSet(original));
    // The wrapper's environment is the original closure. Preserve its own
    // environment and pass every erased argument through unchanged.
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: structure,
        field_index: 2,
    });
    for instruction in super::this_binding::binding_instructions(wrapper, env, receiver) {
        body.instruction(&instruction);
    }
    for i in 1..=u32::from(target.arity) {
        body.instruction(&Instruction::LocalGet(i));
    }
    body.instruction(&Instruction::LocalGet(original));
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    body.instruction(&Instruction::StructGet {
        struct_type_index: structure,
        field_index: 1,
    });
    body.instruction(&Instruction::CallRef(signature));
    if target.is_void {
        body.instruction(&Instruction::Drop);
    } else {
        let object = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics")
            .object;
        body.instruction(&Instruction::RefNull(HeapType::Concrete(object)));
    }
    body.instruction(&Instruction::End);
    body
}

/// Wrap a known function value when its physical return convention differs
/// from the target slot. Other reference conversions remain ordinary casts.
pub fn emit_coercion(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    source: &crate::Type,
    target_slot: ValType,
) -> bool {
    let crate::Type::Function { .. } = source.peel() else {
        return false;
    };
    let target = opposite(super::closures::classify(source));
    let Some(structure) = ctx.symbols.closure_struct_type_idx(target) else {
        return false;
    };
    if target_slot
        != ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(structure),
        })
    {
        return false;
    }
    let function = ctx
        .symbols
        .closure_coercion(target)
        .expect("closure coercion allocated");
    let original = emitter.add_anonymous_local(ctx.symbols.value_type(source));
    emitter.instruction(Instruction::LocalSet(original));
    emit_identity_vtable(emitter, ctx, original);
    emitter.instruction(Instruction::RefFunc(function));
    emitter.instruction(Instruction::LocalGet(original));
    super::this_binding::wrap(emitter, ctx);
    emitter.instruction(Instruction::StructNew(structure));
    true
}

/// An erased field can carry either return convention after generic substitution.
/// Adapt only a matching-arity closure; unrelated values still fail the cast.
pub fn emit_erased_cast(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    target: ClosureSig,
) {
    let target_struct = ctx
        .symbols
        .closure_struct_type_idx(target)
        .expect("target closure");
    let target_slot = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(target_struct),
    });
    let source = opposite(target);
    let source_struct = ctx
        .symbols
        .closure_struct_type_idx(source)
        .expect("source closure");
    let original = emitter.add_anonymous_local(ctx.symbols.value_type(&crate::Type::Unknown));
    emitter.instruction(Instruction::LocalTee(original));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        source_struct,
    )));
    emitter.emit_if(wasm_encoder::BlockType::Result(target_slot));
    emit_identity_vtable(emitter, ctx, original);
    emitter.instruction(Instruction::RefFunc(
        ctx.symbols
            .closure_coercion(target)
            .expect("coercion allocated"),
    ));
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefAsNonNull);
    super::this_binding::wrap(emitter, ctx);
    emitter.instruction(Instruction::StructNew(target_struct));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(original));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        target_struct,
    )));
    emitter.emit_end();
}

/// The fifth field distinguishes adapter vtables from ordinary and class
/// vtables and carries the original function identity across module boundaries.
pub fn emit_vtable_type(
    types: &mut wasm_encoder::TypeSection,
    symbols: &mut SymbolTable,
    next_type: &mut u32,
) {
    use wasm_encoder::{
        CompositeInnerType, CompositeType, FieldType, StorageType, StructType, SubType,
    };
    let intrinsics = symbols.intrinsic_type_indices().expect("intrinsics");
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
    *next_type += 1;
}

fn emit_identity_vtable(emitter: &mut FunctionEmitter<'_>, ctx: &CodegenCtx<'_>, original: u32) {
    let intrinsics = ctx.symbols.intrinsic_type_indices().expect("intrinsics");
    let vtable = ctx
        .symbols
        .closure_vtable_global_idx()
        .expect("closure vtable");
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
        ctx.symbols.closure_coercion_vtable_type(),
    ));
}

pub fn emit_equals(symbols: &SymbolTable) -> Function {
    let mut body = Function::new([]);
    emit_original_identity(&mut body, symbols, 0);
    emit_original_identity(&mut body, symbols, 1);
    body.instruction(&Instruction::LocalGet(0));
    body.instruction(&Instruction::LocalGet(1));
    body.instruction(&Instruction::RefEq);
    body.instruction(&Instruction::End);
    body
}

fn emit_original_identity(body: &mut Function, symbols: &SymbolTable, local: u32) {
    let object = symbols.intrinsic_type_indices().expect("intrinsics").object;
    let wrapper = symbols.closure_coercion_vtable_type();
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
        field_index: 4,
    });
    body.instruction(&Instruction::LocalSet(local));
    body.instruction(&Instruction::Br(0));
    body.instruction(&Instruction::End);
    body.instruction(&Instruction::End);
}
