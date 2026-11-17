//! Per-call type arguments for closures of a generic function type.
//!
//! A generic closure (`<T>(x: T) => x`, a generic object-literal method) is
//! compiled once, with every `T` slot erased, like a generic top-level
//! function. A top-level function takes its descriptors as a trailing
//! `$types` argument; a closure can't, because every closure of an arity
//! shares one Wasm signature and the arity adapters forward arguments
//! JavaScript-style. So the descriptors travel in the closure's environment
//! instead, the way a bound receiver does (`this_binding`).
//!
//! A generic closure's environment is wrapped at creation in a
//! `$generic_environment { env, types }` whose `types` is null. A call that
//! passes type arguments, or an instantiation (`InstantiateFunction`),
//! rebuilds that wrapper with the descriptors: [`emit_rebind`] calls the
//! module's `$rebind_generic` helper, which looks through the call-metadata
//! and receiver wrappers outside it. The stored closure stays immutable, so
//! reentrant calls with different type arguments each get their own wrapper.
//! A closure whose environment has no generic wrapper is not generic itself
//! and ignores the descriptors. Only an empty wrapper is filled: an
//! instantiated function stays instantiated, as in tsc.
//!
//! The typechecker lets a generic value reach a call only with its own type
//! arguments (a call through its generic type, or an `InstantiateFunction`
//! at a conversion); it rejects every other conversion. A closure still
//! reached without them — through `unknown`, or called by the host on its
//! own — binds poisoned descriptors ([`emit_known_types`]): a run-time test
//! of one of its type parameters throws, and a body that never tests one
//! runs.
//!
//! The layers, innermost first: the closure's `$env`, the generic wrapper,
//! call metadata (`call_arguments`), receiver bindings (`this_binding`). The
//! host only looks through receiver bindings to find call metadata, so it
//! never sees the generic wrapper.

use wasm_encoder::{
    BlockType, Function, HeapType, Instruction, RefType, StorageType, TypeSection, ValType,
};

use super::function_emitter::FunctionEmitter;
use super::symbol_table::SymbolTable;
use super::{CodegenCtx, runtime_descriptors};
use crate::{ExprId, Type};

/// Whether `ta` binds type arguments in a closure environment anywhere: a
/// generic closure, a call or instantiation that passes type arguments to
/// one, or a generic function read as a value. Only such a module declares
/// the generic wrapper and its helper.
pub(super) fn needed(
    ta: &crate::TypedAst,
    symbols: &SymbolTable,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    // A generic class method's payload closure binds them too, for a local
    // class or an imported one.
    Ok(symbols.has_generic_class_slot() || binds_type_arguments(ta, &[])?)
}

/// [`needed`], read from `ta` and the declarations of its dependencies
/// alone, before the symbol table records class slots.
pub(super) fn binds_type_arguments(
    ta: &crate::TypedAst,
    dependencies: &[&crate::PackageDeclaration],
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    use crate::TypedExprKind;
    if ta.types.iter().any(|decl| {
        matches!(decl, crate::TypedTypeDecl::Class(class)
            if class.methods.iter().any(|method| !method.generics.is_empty()))
    }) || dependencies
        .iter()
        .flat_map(|declaration| declaration.types.values())
        .any(|symbol| {
            matches!(&symbol.kind, crate::TypeKind::Class { methods, .. }
                if methods.values().any(|method| !method.generics.is_empty()))
        })
    {
        return Ok(true);
    }
    for id in ta.expr_ids().map_err(crate::codegen::arena_failure)? {
        let expr = ta.try_expr(id).map_err(crate::codegen::arena_failure)?;
        let binds = match &expr.kind {
            TypedExprKind::Closure { generics, .. } => !generics.is_empty(),
            TypedExprKind::CallClosure { type_args, .. } => !type_args.is_empty(),
            TypedExprKind::GenericMethodCall {
                iface, type_args, ..
            } => !type_args.is_empty() && !crate::codegen::is_host_package(iface.as_str()),
            TypedExprKind::InstantiateFunction { .. } => true,
            TypedExprKind::FunctionRef { .. } => expr.ty.is_generic_function(),
            _ => false,
        };
        if binds {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The `$rebind_generic` helper's type and function indices.
#[derive(Clone, Copy, Debug)]
pub struct RebindHelper {
    pub type_idx: u32,
    pub func_idx: u32,
}

/// Declares `$generic_environment = struct { (ref any), (ref null
/// $object_fields) }`. Runs beside the other environment wrapper types.
pub(super) fn declare_environment_type(
    types: &mut TypeSection,
    symbols: &mut SymbolTable,
    next_type_idx: &mut u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let object_fields = intrinsics(symbols)?.object_fields;
    types.ty().struct_([
        wasm_encoder::FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::ANY,
            })),
            mutable: false,
        },
        wasm_encoder::FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(object_fields),
            })),
            mutable: false,
        },
    ]);
    symbols.generic_environment_type = Some(*next_type_idx);
    crate::codegen::next_index(next_type_idx)?;
    Ok(())
}

/// Allocates `$rebind_generic: (ref any, ref null $object_fields) -> (ref any)`.
pub(super) fn allocate_rebind(
    types: &mut TypeSection,
    symbols: &mut SymbolTable,
    next_type: &mut u32,
    next_function: &mut u32,
) -> Result<RebindHelper, crate::compiler_error::CompilerFailure> {
    let type_idx = *next_type;
    crate::codegen::next_index(next_type)?;
    types
        .ty()
        .function([any(), types_value_type(symbols)?], [any()]);
    let func_idx = *next_function;
    crate::codegen::next_index(next_function)?;
    let helper = RebindHelper { type_idx, func_idx };
    symbols.generic_rebind = Some(helper);
    Ok(helper)
}

/// `$rebind_generic(env, types)`: `env` with its empty generic wrapper
/// filled with `types` (nullable), under the same call-metadata and receiver
/// wrappers; `env` itself when it has no generic wrapper or the wrapper
/// already holds descriptors.
pub(super) fn rebind_body(
    symbols: &SymbolTable,
    helper: RebindHelper,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let receiver = symbols
        .this_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("this environment"))?;
    let metadata = symbols
        .call_metadata_type
        .ok_or_else(|| crate::codegen::internal_failure("call metadata type"))?;
    let generic = generic_type(symbols)?;
    let result = BlockType::Result(any());
    let mut body = Function::new([]);
    let mut ins = |instruction: Instruction<'_>| {
        body.instruction(&instruction);
    };
    // A receiver binding: rebind inside it, keep the receiver.
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefTestNonNull(HeapType::Concrete(receiver)));
    ins(Instruction::If(result));
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(receiver)));
    ins(Instruction::StructGet {
        struct_type_index: receiver,
        field_index: 0,
    });
    ins(Instruction::LocalGet(1));
    ins(Instruction::Call(helper.func_idx));
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(receiver)));
    ins(Instruction::StructGet {
        struct_type_index: receiver,
        field_index: 1,
    });
    ins(Instruction::StructNew(receiver));
    ins(Instruction::Else);
    // Call metadata: rebind inside it, keep its text and cache id.
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefTestNonNull(HeapType::Concrete(metadata)));
    ins(Instruction::If(result));
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(metadata)));
    ins(Instruction::StructGet {
        struct_type_index: metadata,
        field_index: 0,
    });
    ins(Instruction::RefAsNonNull);
    ins(Instruction::LocalGet(1));
    ins(Instruction::Call(helper.func_idx));
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(metadata)));
    ins(Instruction::StructGet {
        struct_type_index: metadata,
        field_index: 1,
    });
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(metadata)));
    ins(Instruction::StructGet {
        struct_type_index: metadata,
        field_index: 2,
    });
    ins(Instruction::StructNew(metadata));
    ins(Instruction::Else);
    // The generic wrapper: a new one with these descriptors, unless it
    // already holds some (an instantiated function).
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefTestNonNull(HeapType::Concrete(generic)));
    ins(Instruction::If(result));
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(generic)));
    ins(Instruction::StructGet {
        struct_type_index: generic,
        field_index: 1,
    });
    ins(Instruction::RefIsNull);
    ins(Instruction::If(result));
    ins(Instruction::LocalGet(0));
    ins(Instruction::RefCastNonNull(HeapType::Concrete(generic)));
    ins(Instruction::StructGet {
        struct_type_index: generic,
        field_index: 0,
    });
    ins(Instruction::LocalGet(1));
    ins(Instruction::StructNew(generic));
    ins(Instruction::Else);
    ins(Instruction::LocalGet(0));
    ins(Instruction::End);
    ins(Instruction::Else);
    // Not generic: the closure ignores type arguments.
    ins(Instruction::LocalGet(0));
    ins(Instruction::End);
    ins(Instruction::End);
    ins(Instruction::End);
    ins(Instruction::End);
    Ok(body)
}

/// With a generic closure's `$env` on the stack, wraps it for creation: no
/// descriptors until a call or an instantiation binds them.
pub(super) fn emit_creation_wrapper(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(
        intrinsics(ctx.symbols)?.object_fields,
    )));
    emitter.instruction(Instruction::StructNew(generic_type(ctx.symbols)?));
    Ok(())
}

/// With a closure environment on the stack, replaces it with one whose
/// generic wrapper holds the descriptors of `type_args`. Type parameters of
/// the enclosing generic bodies forward their own descriptors.
pub(super) fn emit_rebind(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    type_args: &[Type],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let helper = ctx
        .symbols
        .generic_rebind
        .ok_or_else(|| crate::codegen::internal_failure("generic rebind helper allocated"))?;
    runtime_descriptors::environment(emitter, ctx, type_args)?;
    emitter.instruction(Instruction::Call(helper.func_idx));
    Ok(())
}

/// In a generic closure's prologue, with its environment (receiver binding
/// already removed) on the stack: leaves the environment inside the generic
/// wrapper and binds the closure's own type parameters to the wrapper's
/// descriptors, poisoned when it has none.
pub(super) fn emit_prologue_unwrap(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    generics: &crate::GenericParameters,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let bound = emit_split(emitter, ctx)?;
    let types = emit_known_types(emitter, ctx, &generics.names, bound)?;
    runtime_descriptors::bind(emitter, &generics.names, types)?;
    Ok(())
}

/// With a closure environment inside its generic wrapper on the stack
/// (receiver bindings and call metadata already removed), leaves the
/// environment inside the wrapper and returns a local holding the wrapper's
/// descriptors, null when nothing bound them.
pub(super) fn emit_split(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let generic = generic_type(ctx.symbols)?;
    let wrapper = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(generic),
    }))?;
    let bound = emitter.add_anonymous_local(types_value_type(ctx.symbols)?)?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(generic)));
    emitter.instruction(Instruction::LocalTee(wrapper));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: generic,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(bound));
    emitter.instruction(Instruction::LocalGet(wrapper));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: generic,
        field_index: 0,
    });
    Ok(bound)
}

/// Returns a local holding the descriptors of `names` from the nullable
/// descriptors in `bound`: those, or poisoned ones when `bound` is null (a
/// generic function reached without its type arguments).
pub(super) fn emit_known_types(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    names: &[String],
    bound: u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let object_fields = intrinsics(ctx.symbols)?.object_fields;
    let environment = runtime_descriptors::environment_type(ctx.symbols)?;
    let types = emitter.add_anonymous_local(environment)?;
    emitter.instruction(Instruction::LocalGet(bound));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(environment));
    for _ in names {
        emit_poisoned_descriptor(emitter, ctx)?;
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: object_fields,
        array_size: crate::codegen::wasm_u32(names.len())?,
    });
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(bound));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.emit_end();
    emitter.instruction(Instruction::LocalSet(types));
    Ok(types)
}

/// Pushes null descriptors, for a call into a generic class slot that has
/// none to pass: a `super` call, which reaches only a plain override (one
/// that ignores them).
pub(super) fn emit_no_types(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(
        intrinsics(ctx.symbols)?.object_fields,
    )));
    Ok(())
}

/// The value type of a generic wrapper's descriptors, and of a generic class
/// slot's trailing `$types`: nullable, null when nothing bound them.
pub(super) fn types_value_type(
    symbols: &SymbolTable,
) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics(symbols)?.object_fields),
    }))
}

/// The name a poisoned descriptor shows in the error a test against it
/// throws.
pub(super) const POISONED_NAME: &str = "a type argument its caller could not pass";

/// Pushes a descriptor standing for a type parameter of a generic function
/// reached without its type arguments: its test is `never`'s, so checking
/// any value against it throws, and its name says why.
fn emit_poisoned_descriptor(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = intrinsics(ctx.symbols)?;
    let type_descriptor = intrinsics.type_descriptor;
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .global_idx(&crate::mangle::prelude("opaque_vtable"))
            .ok_or_else(|| crate::codegen::internal_failure("opaque vtable imported"))?,
    ));
    runtime_descriptors::emit(emitter, ctx, &Type::Never)?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        type_descriptor,
    )));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: type_descriptor,
        field_index: 1,
    });
    let pool_idx = ctx
        .strings
        .lookup_text(POISONED_NAME)
        .ok_or_else(|| crate::codegen::internal_failure("poisoned name interned"))?;
    crate::codegen::function_emitter::emit_pooled_string(emitter, ctx, pool_idx)?;
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.string)));
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.closure)));
    emitter.instruction(Instruction::StructNew(type_descriptor));
    Ok(())
}

/// `InstantiateFunction`: binds the type arguments once, into a copy of the
/// closure whose environment holds them; the copy is then converted to the
/// target type like any other function value.
pub(super) fn emit_instantiation(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    id: ExprId,
    value: ExprId,
    type_args: &[Type],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let source_ty = ctx
        .ta
        .source_type(value)
        .map_err(crate::codegen::arena_failure)?
        .clone();
    let target_ty = ctx
        .ta
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    super::function_emitter::expr::emit_expr(emitter, ctx, value)?;
    let sig = super::closures::classify(source_ty.lowered())?;
    let structure = ctx
        .symbols
        .closure_struct_type_idx(sig)
        .ok_or_else(|| crate::codegen::internal_failure("instantiated closure type"))?;
    let closure = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(structure),
    }))?;
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(structure)));
    emitter.instruction(Instruction::LocalSet(closure));
    for field_index in 0..2 {
        emitter.instruction(Instruction::LocalGet(closure));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: structure,
            field_index,
        });
    }
    emitter.instruction(Instruction::LocalGet(closure));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: structure,
        field_index: 2,
    });
    emit_rebind(emitter, ctx, type_args)?;
    for field_index in [3, 4] {
        emitter.instruction(Instruction::LocalGet(closure));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: structure,
            field_index,
        });
    }
    emitter.instruction(Instruction::StructNew(structure));
    let instantiated = instantiated_type(&source_ty, type_args);
    super::function_emitter::cast::emit_coerce_to_slot(emitter, ctx, &instantiated, &target_ty)
}

/// `source`, a generic function type, with its type parameters replaced by
/// `type_args`.
pub(super) fn instantiated_type(source: &Type, type_args: &[Type]) -> Type {
    let Type::Function {
        generics,
        params,
        optional,
        ret,
        predicate,
        has_rest,
    } = source.lowered()
    else {
        return source.clone();
    };
    let bindings: std::collections::BTreeMap<&str, &Type> = generics
        .names
        .iter()
        .map(String::as_str)
        .zip(type_args)
        .collect();
    let substitute = |ty: &Type| {
        crate::type_size::substitute_type_vars(ty, &|name| {
            bindings.get(name).map(|ty| (*ty).clone())
        })
    };
    Type::Function {
        generics: crate::GenericParameters::default(),
        params: params.iter().map(substitute).collect(),
        optional: *optional,
        ret: Box::new(substitute(ret)),
        predicate: predicate.as_ref().map(|predicate| {
            Box::new(crate::TypePredicate {
                asserted_type: substitute(&predicate.asserted_type),
                ..(**predicate).clone()
            })
        }),
        has_rest: *has_rest,
    }
}

fn generic_type(symbols: &SymbolTable) -> Result<u32, crate::compiler_error::CompilerFailure> {
    symbols
        .generic_environment_type
        .ok_or_else(|| crate::codegen::internal_failure("generic environment type"))
}

fn intrinsics(
    symbols: &SymbolTable,
) -> Result<crate::codegen::intrinsics::IntrinsicTypeIndices, crate::compiler_error::CompilerFailure>
{
    symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))
}

fn any() -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::ANY,
    })
}
