//! Runtime predicates passed through erased generic calls.

use super::function_emitter::{FunctionEmitter, cast};
use super::{CodegenCtx, field_guards, symbol_table::SymbolTable};
use crate::{Ident, Span, Type, TypedAst, TypedExprKind};
use std::collections::BTreeSet;
use wasm_encoder::{Function, HeapType, Instruction, RefType, ValType};

pub fn environment_type(symbols: &SymbolTable) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(
            symbols
                .intrinsic_type_indices()
                .expect("intrinsics")
                .object_fields,
        ),
    })
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
        Type::Tuple(types) | Type::Union(types) => {
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
        Type::Object { fields } => {
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

pub fn bind(emitter: &mut FunctionEmitter, names: &[String], local: u32) {
    for (slot, name) in names.iter().enumerate() {
        emitter
            .runtime_type_params
            .insert(name.clone(), (local, slot as u32));
    }
}

pub fn allocate(ta: &TypedAst, symbols: &mut SymbolTable, next: &mut u32) -> Vec<(Type, u32)> {
    let mut targets = BTreeSet::from([Type::Unknown]);
    for index in 0..ta.exprs_len() {
        if let TypedExprKind::GenericCall { type_args, .. } =
            &ta.expr(crate::ExprId(index as u32)).kind
        {
            targets.extend(type_args.iter().cloned());
        }
    }
    for contexts in ta.runtime_class_contexts.values() {
        for context in contexts {
            targets.extend(context.args.iter().cloned());
        }
    }
    if targets.len() == 1
        && ta.runtime_class_contexts.values().all(Vec::is_empty)
        && !ta
            .runtime_field_guards
            .values()
            .flatten()
            .any(|guard| !parameters(&guard.target).is_empty())
    {
        return Vec::new();
    }
    let mut descriptors = Vec::new();
    for ty in targets {
        if matches!(ty, Type::TypeVar(_) | Type::GenericParam { .. }) {
            continue;
        }
        symbols.type_descriptor_functions.insert(ty.clone(), *next);
        descriptors.push((ty, *next));
        *next += 1;
    }
    descriptors
}

pub fn environment(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, types: &[Type]) {
    for ty in types {
        emit(emitter, ctx, ty);
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics")
            .object_fields,
        array_size: types.len() as u32,
    });
}

pub fn capture(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, ty: &Type) {
    let types: Vec<_> = parameters(ty).into_iter().map(Type::TypeVar).collect();
    environment(emitter, ctx, &types);
}

fn emit(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, ty: &Type) {
    if let Type::TypeVar(name) | Type::GenericParam { name, .. } = ty.peel() {
        if let Some(&(local, slot)) = emitter.runtime_type_params.get(name) {
            emitter.instruction(Instruction::LocalGet(local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
                ctx.symbols
                    .intrinsic_type_indices()
                    .expect("intrinsics")
                    .object_fields,
            )));
            emitter.instruction(Instruction::I32Const(slot as i32));
            emitter.instruction(Instruction::ArrayGet(
                ctx.symbols
                    .intrinsic_type_indices()
                    .expect("intrinsics")
                    .object_fields,
            ));
            return;
        }
        // Legacy erased entry points have no descriptor to forward.
        emit(emitter, ctx, &Type::Unknown);
        return;
    }
    let function = ctx
        .symbols
        .type_descriptor_functions
        .get(ty)
        .expect("type descriptor allocated");
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .closure_vtable_global_idx()
            .expect("closure vtable"),
    ));
    emitter.instruction(Instruction::RefFunc(*function));
    capture(emitter, ctx, ty);
    emitter.instruction(Instruction::StructNew(
        ctx.symbols
            .closure_struct_type_idx(field_guards::signature())
            .expect("descriptor closure"),
    ));
}

pub fn test_parameter(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, name: &str, value: u32) {
    if !emitter.runtime_type_params.contains_key(name) {
        emitter.instruction(Instruction::I32Const(1));
        return;
    }
    let closure_type = ctx
        .symbols
        .closure_struct_type_idx(field_guards::signature())
        .expect("descriptor closure");
    let local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_type),
    }));
    emit(emitter, ctx, &Type::TypeVar(name.into()));
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
            .expect("descriptor signature"),
    ));
    cast::emit_cast_to(emitter, ctx, &Type::Boolean);
}

pub fn body(ctx: &CodegenCtx, ty: &Type) -> Function {
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
        (ident("value"), ctx.symbols.value_type(&Type::Unknown)),
    ];
    let mut emitter = FunctionEmitter::new(ctx, &params);
    bind(&mut emitter, &parameters(ty), 0);
    super::cast_check::emit_structural_test(&mut emitter, ctx, 1, ty);
    cast::emit_box(&mut emitter, ctx, &Type::Boolean);
    emitter.build()
}
