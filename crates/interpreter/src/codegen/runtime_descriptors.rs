//! Runtime predicates passed through erased generic calls.

use super::function_emitter::{FunctionEmitter, cast};
use super::{CodegenCtx, field_guards, symbol_table::SymbolTable};
use crate::{Ident, Span, Type, TypedAst, TypedExprKind};
use std::collections::BTreeSet;
use wasm_encoder::{
    BlockType, ConstExpr, Function, GlobalSection, GlobalType, HeapType, Instruction, RefType,
    ValType,
};

pub fn environment_type(
    symbols: &SymbolTable,
) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(
            symbols
                .intrinsic_type_indices()
                .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
                .object_fields,
        ),
    }))
}

pub fn parameters(ty: &Type) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_parameters(ty, &mut names);
    names.into_iter().collect()
}

fn collect_parameters(ty: &Type, names: &mut BTreeSet<String>) {
    match ty.peel() {
        Type::TypeVar(name) | Type::GenericParam { name, .. } => {
            names.insert(name.clone());
        }
        Type::Array(element) => collect_parameters(element, names),
        Type::Tuple(crate::types::TupleType {
            elements: types, ..
        })
        | Type::Union(types) => {
            for ty in types {
                collect_parameters(ty, names);
            }
        }
        Type::ClassRef { args, .. }
        | Type::InterfaceRef { args, .. }
        | Type::AliasRef { args, .. } => {
            for ty in args {
                collect_parameters(ty, names);
            }
        }
        Type::Object { fields, index } => {
            if let Some(i) = index {
                collect_parameters(&i.value, names);
            }
            for field in fields.values() {
                collect_parameters(&field.ty, names);
            }
        }
        Type::Function { params, ret, .. } => {
            for ty in params {
                collect_parameters(ty, names);
            }
            collect_parameters(ret, names);
        }
        _ => {}
    }
}

pub fn bind(
    emitter: &mut FunctionEmitter,
    names: &[String],
    local: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for (slot, name) in names.iter().enumerate() {
        emitter
            .runtime_type_params
            .insert(name.clone(), (local, crate::codegen::wasm_u32(slot)?));
    }
    Ok(())
}

pub fn allocate(
    ta: &TypedAst,
    extra: &BTreeSet<Type>,
    symbols: &mut SymbolTable,
    next: &mut u32,
) -> Result<Vec<(Type, u32)>, crate::compiler_error::CompilerFailure> {
    let mut targets = BTreeSet::from([Type::Unknown]);
    targets.extend(extra.iter().cloned());
    let mut has_generic_calls = false;
    for index in ta.expr_ids().map_err(crate::codegen::arena_failure)? {
        if let TypedExprKind::GenericCall { type_args, .. } = &ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .kind
        {
            has_generic_calls = true;
            targets.extend(type_args.iter().cloned());
        }
    }
    for contexts in ta.runtime_class_contexts.values() {
        for context in contexts {
            targets.extend(context.args.iter().cloned());
        }
    }
    if !has_generic_calls
        && targets.len() == 1
        && ta.runtime_class_contexts.values().all(Vec::is_empty)
        && !ta
            .runtime_field_guards
            .values()
            .flatten()
            .any(|guard| !parameters(&guard.target).is_empty())
    {
        return Ok(Vec::new());
    }
    let mut descriptors = Vec::new();
    for ty in targets {
        if matches!(ty, Type::TypeVar(_) | Type::GenericParam { .. }) {
            continue;
        }
        symbols.type_descriptor_functions.insert(ty.clone(), *next);
        descriptors.push((ty, *next));
        crate::codegen::next_index(next)?;
    }
    Ok(descriptors)
}

/// Closed predicates are singletons so recursive calls retain effective type identity.
pub fn allocate_globals(
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next: &mut u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let closed: Vec<_> = symbols
        .type_descriptor_functions
        .keys()
        .filter(|ty| parameters(ty).is_empty())
        .cloned()
        .collect();
    if closed.is_empty() {
        return Ok(0);
    }
    let closure = symbols
        .closure_struct_type_idx(field_guards::signature())
        .ok_or_else(|| crate::codegen::internal_failure("descriptor closure"))?;
    for ty in &closed {
        symbols.type_descriptor_globals.insert(ty.clone(), *next);
        crate::codegen::next_index(next)?;
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(closure),
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(HeapType::Concrete(closure)),
        );
    }
    crate::codegen::wasm_u32(closed.len())
}

pub fn environment(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    types: &[Type],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for ty in types {
        emit(emitter, ctx, ty)?;
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: ctx
            .symbols
            .intrinsic_type_indices()
            .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
            .object_fields,
        array_size: crate::codegen::wasm_u32(types.len())?,
    });

    Ok(())
}

pub fn capture(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let types: Vec<_> = parameters(ty).into_iter().map(Type::TypeVar).collect();
    environment(emitter, ctx, &types)?;

    Ok(())
}

/// A generic helper binds declaration parameters, rather than free variables in
/// this particular instantiation. Pair arguments in declaration order, then sort
/// by parameter name to match `parameters()` and the helper's binding slots.
pub fn validator_environment(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    key: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if let Some((names, _)) = ctx.symbols.generic_runtime_validator(key)
        && let Type::AliasRef { args, .. } | Type::InterfaceRef { args, .. } = key.peel()
    {
        if names.len() != args.len() {
            return Err(crate::codegen::internal_failure(
                "validator type argument count mismatch",
            ));
        }
        let mut pairs: Vec<_> = names.iter().zip(args).collect();
        pairs.sort_by_key(|(name, _)| *name);
        environment(
            emitter,
            ctx,
            &pairs
                .into_iter()
                .map(|(_, arg)| arg.clone())
                .collect::<Vec<_>>(),
        )?;
        return Ok(());
    }
    capture(emitter, ctx, key)?;

    Ok(())
}

fn emit(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if let Type::TypeVar(name) | Type::GenericParam { name, .. } = ty.peel() {
        if let Some(&(local, slot)) = emitter.runtime_type_params.get(name) {
            emitter.instruction(Instruction::LocalGet(local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
                ctx.symbols
                    .intrinsic_type_indices()
                    .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
                    .object_fields,
            )));
            emitter.instruction(Instruction::I32Const(slot as i32));
            emitter.instruction(Instruction::ArrayGet(
                ctx.symbols
                    .intrinsic_type_indices()
                    .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
                    .object_fields,
            ));
            return Ok(());
        }
        // Legacy erased entry points have no descriptor to forward.
        emit(emitter, ctx, &Type::Unknown)?;
        return Ok(());
    }
    if let Some(&global) = ctx.symbols.type_descriptor_globals.get(ty) {
        emitter.instruction(Instruction::GlobalGet(global));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(BlockType::Empty);
        emit_new_descriptor(emitter, ctx, ty)?;
        emitter.instruction(Instruction::GlobalSet(global));
        emitter.emit_end();
        emitter.instruction(Instruction::GlobalGet(global));
        emitter.instruction(Instruction::RefAsNonNull);
        return Ok(());
    }
    emit_new_descriptor(emitter, ctx, ty)?;

    Ok(())
}

fn emit_new_descriptor(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let function = ctx
        .symbols
        .type_descriptor_functions
        .get(ty)
        .ok_or_else(|| crate::codegen::internal_failure("type descriptor allocated"))?;
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .closure_vtable_global_idx()
            .ok_or_else(|| crate::codegen::internal_failure("closure vtable"))?,
    ));
    emitter.instruction(Instruction::RefFunc(*function));
    capture(emitter, ctx, ty)?;
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(
        ctx.symbols
            .closure_struct_type_idx(field_guards::signature())
            .ok_or_else(|| crate::codegen::internal_failure("descriptor closure"))?,
    ));

    Ok(())
}

pub fn test_parameter(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    name: &str,
    value: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if !emitter.runtime_type_params.contains_key(name) {
        emitter.instruction(Instruction::I32Const(1));
        return Ok(());
    }
    let closure_type = ctx
        .symbols
        .closure_struct_type_idx(field_guards::signature())
        .ok_or_else(|| crate::codegen::internal_failure("descriptor closure"))?;
    let local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_type),
    }))?;
    emit(emitter, ctx, &Type::TypeVar(name.into()))?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        closure_type,
    )));
    emitter.instruction(Instruction::LocalSet(local));
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_type,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(value));
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_type,
        field_index: 1,
    });
    emitter.instruction(Instruction::CallRef(
        ctx.symbols
            .closure_func_type_idx(field_guards::signature())
            .ok_or_else(|| crate::codegen::internal_failure("descriptor signature"))?,
    ));
    cast::emit_cast_to(emitter, ctx, &Type::Boolean)?;

    Ok(())
}

pub fn body(
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let ident = |name: &str| Ident {
        name: name.into(),
        span: Span::at(ctx.file),
    };
    let params = [
        (
            ident("env"),
            ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::ANY,
            }),
        ),
        (ident("value"), ctx.symbols.value_type(&Type::Unknown)?),
    ];
    let mut emitter = FunctionEmitter::new(ctx, &params)?;
    bind(&mut emitter, &parameters(ty), 0)?;
    ctx.checking_standalone(ty, || {
        super::cast_check::emit_structural_test(&mut emitter, ctx, 1, ty, ty)
    })?;
    cast::emit_box(&mut emitter, ctx, &Type::Boolean)?;
    emitter.build()
}
