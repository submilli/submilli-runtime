//! Runtime type descriptors: what every generic call passes per type
//! parameter, in its trailing `$types` environment.
//!
//! A descriptor is a `$TypeDescriptor` struct, not a closure: its `test` field
//! is the type's structural predicate (a `$closure_1` whose body is
//! [`body`]), beside the type's name and its JSON Schema when it has one
//! (`codegen::descriptor_schema`). Compiled generics read the predicate (`test_parameter`);
//! host generics read all of it (`stdlib::abi::read_type_argument`). Every
//! field is a function of the type, so a closed type's descriptor is built
//! once into a lazily initialised global and its identity holds for the
//! `ref.eq` tests of recursive validators.

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
    match ty.lowered() {
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
        Type::Object {
            fields,
            index,
            more_indexes,
        } => {
            for i in crate::stored_index_signatures(index, more_indexes) {
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
    let mut passes_type_arguments = false;
    for index in ta.expr_ids().map_err(crate::codegen::arena_failure)? {
        match &ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .kind
        {
            TypedExprKind::GenericCall { type_args, .. } => {
                passes_type_arguments = true;
                targets.extend(type_args.iter().cloned());
            }
            // A runtime-generic host method (`Response#json<T>`); every
            // other generic method call carries no type arguments.
            TypedExprKind::GenericMethodCall { type_args, .. } if !type_args.is_empty() => {
                passes_type_arguments = true;
                targets.extend(type_args.iter().cloned());
            }
            _ => {}
        }
    }
    for contexts in ta.runtime_class_contexts.values() {
        for context in contexts {
            targets.extend(context.args.iter().cloned());
        }
    }
    let main_type_args = main_type_arguments(ta);
    if !main_type_args.is_empty() {
        passes_type_arguments = true;
        targets.extend(main_type_args);
    }
    if !passes_type_arguments
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

/// [`entry_type_arguments`] of `ta`'s `main`, if it has one.
pub fn main_type_arguments(ta: &TypedAst) -> Vec<Type> {
    ta.functions
        .iter()
        .find(|f| f.name.name == "main")
        .map(entry_type_arguments)
        .unwrap_or_default()
}

/// The type arguments the entry shim passes to `main`, in declaration
/// order: each type parameter's bound, or `unknown` when it has none.
/// Nothing in a program can name another type argument for the entry call.
/// A bound that names a type parameter has no closed descriptor for the
/// shim to build, so that parameter takes `unknown` as well.
pub fn entry_type_arguments(main: &crate::TypedFunction) -> Vec<Type> {
    main.generics
        .iter()
        .map(|name| match main.generics.constraints.get(name) {
            Some(bound) if parameters(bound).is_empty() => bound.clone(),
            _ => Type::Unknown,
        })
        .collect()
}

/// Closed types' descriptors are singletons so recursive calls retain effective type identity.
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
    let descriptor = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .type_descriptor;
    for ty in &closed {
        symbols.type_descriptor_globals.insert(ty.clone(), *next);
        crate::codegen::next_index(next)?;
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(descriptor),
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(HeapType::Concrete(descriptor)),
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

/// A schema call at a type the program names (`llm.call<Verdict>`)
/// passed the typechecker's schema gate, so codegen must have emitted a schema
/// for that type: the gate and `codegen::descriptor_schema` decide it
/// independently, and a disagreement would compile a call that always throws.
/// A type parameter forwards its caller's descriptor and is checked when the
/// call runs instead.
pub fn require_direct_schema(
    ctx: &CodegenCtx,
    mangled: &crate::MangledName,
    type_args: &[Type],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(ty) = type_args.first() else {
        return Ok(());
    };
    if !crate::stdlib::checked_call::checked_call(mangled).is_some_and(|c| c.sends_schema())
        || !parameters(ty).is_empty()
        || ctx.symbols.descriptor_schemas.contains_key(ty)
    {
        return Ok(());
    }
    Err(crate::codegen::internal_failure(
        "a type the schema gate admitted has a descriptor schema",
    ))
}

/// A descriptor text from the string pool. Codegen interns the name and
/// schema of every allocated descriptor type before any function is emitted
/// ([`prepare_descriptor_data`]), so a miss is a compiler invariant failure, not a slow
/// path.
fn emit_text(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let pool_idx = ctx
        .strings
        .lookup_text(text)
        .ok_or_else(|| crate::codegen::internal_failure("descriptor text interned"))?;
    crate::codegen::function_emitter::emit_pooled_string(emitter, ctx, pool_idx)
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
        && let Type::AliasRef { args, .. } | Type::InterfaceRef { args, .. } = key.lowered()
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
    if let Type::TypeVar(name) | Type::GenericParam { name, .. } = ty.lowered() {
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
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?;
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .global_idx(&crate::mangle::prelude("opaque_vtable"))
            .ok_or_else(|| crate::codegen::internal_failure("opaque vtable imported"))?,
    ));
    // `test`: the predicate closure, capturing its free parameters' descriptors.
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
            .ok_or_else(|| crate::codegen::internal_failure("predicate closure type"))?,
    ));
    emit_text(emitter, ctx, &ty.to_string())?;
    match ctx.symbols.descriptor_schemas.get(ty) {
        Some(schema) => emit_text(emitter, ctx, schema)?,
        None => {
            emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.string)));
        }
    }
    emitter.instruction(Instruction::StructNew(intrinsics.type_descriptor));

    Ok(())
}

/// Compute the JSON Schema of every allocated descriptor type that has one
/// (`codegen::descriptor_schema`), and intern each descriptor's name and
/// schema so [`emit_text`] finds them. Run once [`allocate`] has fixed the
/// descriptor types and before any function is emitted.
pub fn prepare_descriptor_data(
    pool: &mut crate::codegen::string_pool::StringPool,
    ta: &TypedAst,
    symbols: &mut SymbolTable,
) {
    let types: Vec<Type> = symbols.type_descriptor_functions.keys().cloned().collect();
    for ty in types {
        pool.intern_text(&ty.to_string());
        if let Some(schema) = super::descriptor_schema::schema_of(ta, &ty) {
            pool.intern_text(&schema);
            symbols.descriptor_schemas.insert(ty, schema);
        }
    }
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
        .ok_or_else(|| crate::codegen::internal_failure("predicate closure type"))?;
    let descriptor = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .type_descriptor;
    let local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_type),
    }))?;
    emit(emitter, ctx, &Type::TypeVar(name.into()))?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(descriptor)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: descriptor,
        field_index: 1,
    });
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
