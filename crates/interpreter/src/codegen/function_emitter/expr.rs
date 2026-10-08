//! Recursive emitter for typed expressions.
//!
//! Walks a [`TypedExprKind`] tree and pushes Wasm instructions onto a
//! [`FunctionEmitter`]. Stateless — pure recursion over `&CodegenCtx` plus
//! mutation of the supplied emitter; both `_start`'s initializer code and
//! function bodies share this single dispatch.
//!
//! Missing emission metadata returns a compiler failure; the caller discards
//! the owned emitter and module before publishing Wasm.

use crate::codegen::CodegenCtx;
use crate::codegen::bounds::{
    emit_checked_index, emit_checked_index_with_length, stash_array_length, stash_index_operand,
};
use crate::codegen::cast_check::emit_structural_test;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::function_emitter::cast;
use crate::codegen::symbol_table::{MethodSlotAbi, may_hold_null};
use crate::typechecker::infer::narrowing::{
    BindingId, ReferencePath, cast_info_for, falsy_part, truthy_part,
};
use crate::typed_ast::field_runtime_type_is_testable;
use crate::{
    BinOp, ExprId, Ident, Intrinsic, Type, TypedExprKind, TypedObjectFieldSource,
    TypedObjectMember, UnOp,
};
use std::collections::HashMap;
use wasm_encoder::{BlockType, HeapType, Ieee64, Instruction, RefType, ValType};

/// Emit code for the expression at `id`, leaving its result on the Wasm
/// stack. Records the expression's source span against the emitter so the
/// DWARF wiring task can recover instruction-offset → source-position
/// mappings later.
pub fn emit_expr(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr_value(emitter, ctx, id)?;
    // Preserve concrete field validators while the expression still carries
    // its class arguments, before a surrounding cast or slot erases them.
    crate::codegen::field_guards::attach(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(id)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    Ok(())
}

fn emit_receiver(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr(emitter, ctx, id)?;
    let source = &ctx
        .ta
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .ty;
    let target = ctx
        .ta
        .source_type(id)
        .map_err(crate::codegen::arena_failure)?;
    let _: () = if source != target {
        crate::codegen::cast_check::emit_operation_cast_on_stack(emitter, ctx, source, target)?;
    };
    Ok(())
}

fn emit_expr_value(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let expr = ctx.ta.try_expr(id).map_err(crate::codegen::arena_failure)?;
    emitter.record_span(expr.span);
    // A node the enclosing statement already evaluated into a local — see
    // `FunctionEmitter::single_evaluations`.
    if let Some(slot) = emitter.evaluated_slot(id) {
        emitter.instruction(Instruction::LocalGet(slot));
        return Ok(());
    }
    let _: () = match &expr.kind {
        TypedExprKind::Number(v) => {
            emitter.instruction(Instruction::F64Const(Ieee64::from(*v)));
        }
        TypedExprKind::BigInt(digits) => emit_bigint_literal(emitter, ctx, digits)?,
        TypedExprKind::Boolean(b) => {
            emitter.instruction(Instruction::I32Const(if *b { 1 } else { 0 }));
        }
        // String literals materialize via the prelude's vtable global +
        // `array.new_data` + `struct.new $string`, reading from the
        // per-literal passive data segment that StringPool assigned.
        TypedExprKind::String(_) => {
            let pool_idx = ctx.strings.locations.get(&id).copied().ok_or_else(|| {
                crate::codegen::internal_failure("a string literal was not interned")
            })?;
            super::emit_pooled_string(emitter, ctx, pool_idx)?;
        }
        TypedExprKind::Regex { source, flags } => {
            emit_regex_literal(emitter, ctx, source, flags)?;
        }
        TypedExprKind::Sequence { stmts, result } => {
            for &stmt in stmts {
                super::stmt::emit_statement(emitter, ctx, stmt)?;
            }
            emit_expr(emitter, ctx, *result)?;
        }
        TypedExprKind::EffectThen { effect, result } => {
            emit_expr(emitter, ctx, *effect)?;
            // A `void` call leaves nothing on the stack, so there is nothing to drop.
            if !ctx
                .ta
                .try_expr(*effect)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .is_void()
            {
                emitter.instruction(Instruction::Drop);
            }
            emit_expr(emitter, ctx, *result)?;
        }
        TypedExprKind::Binary { op, lhs, rhs } => {
            emit_binary(emitter, ctx, *op, *lhs, *rhs, &expr.ty)?;
        }
        TypedExprKind::Unary { op, operand } => {
            emit_unary(emitter, ctx, *op, *operand, &expr.ty)?;
        }
        TypedExprKind::LocalRef { ident, boxed } => {
            // Function-scope binding (parameter, function-local
            // `let`/`const`, or captured-by-closure — captures are
            // materialized as ordinary locals at the closure
            // prologue, see `emit_closure_function`). Boxed reads
            // load the `(ref $box_T)` slot, `struct.get` the value
            // field, then recover the declared type from the cell's
            // erased payload. Non-boxed reads are a plain `local.get`.
            let slot = emitter.require_local_slot(&ident.name)?;
            emitter.instruction(Instruction::LocalGet(slot));
            if *boxed {
                let box_idx = ctx.symbols.box_type_idx(&expr.ty)?.ok_or_else(|| {
                    crate::codegen::internal_failure("box type registered for every boxed LocalRef")
                })?;
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
                cast::emit_unerase(emitter, ctx, &expr.ty)?;
            }
        }
        // A guard that rules out every value leaves no shadow to read, and
        // no value ever reaches the read.
        TypedExprKind::LocalNarrowRef { .. } if inferred_never(ctx, id)? => {
            emitter.instruction(Instruction::Unreachable);
        }
        TypedExprKind::LocalNarrowRef { binding, path, .. } => {
            emit_local_narrow_ref(emitter, ctx, binding, path, &expr.ty)?;
        }
        TypedExprKind::GlobalRef { mangled, .. } => {
            emit_global_ref(emitter, ctx, mangled, &expr.ty)?;
        }
        TypedExprKind::FunctionRef { mangled, .. } => {
            crate::codegen::init_guard::emit_check(emitter, ctx, mangled);
            emit_function_ref(emitter, ctx, mangled, &expr.ty)?;
        }
        TypedExprKind::Call { mangled, args, .. } => {
            crate::codegen::init_guard::emit_check(emitter, ctx, mangled);
            emit_symbol_call(emitter, ctx, mangled, args, &expr.ty)?;
        }
        TypedExprKind::SuperCtorCall { parent, args } => {
            // Direct call of the parent's constructor *init* fn on the current
            // `this` (self-first ABI), initializing the parent-declared fields.
            // A generic parent's declared ctor params erase to boxed slots.
            let this = emitter.this_local().ok_or_else(|| {
                crate::codegen::internal_failure("super(...) only inside a constructor body")
            })?;
            emitter.instruction(Instruction::LocalGet(this));
            let ctor_abi = ctx.symbols.class_ctor_abi(parent).map(<[ValType]>::to_vec);
            emit_args_into_slots(emitter, ctx, args, ctor_abi.as_deref())?;
            let init = ctx
                .symbols
                .class_ctor_init_func_idx(parent)
                .ok_or_else(|| crate::codegen::internal_failure("parent ctor init fn allocated"))?;
            emitter.instruction(Instruction::Call(init));
            // Own field initializers / parameter-property copies run right after
            // the parent is initialized, before the rest of the constructor body.
            if let Some(mangled) = emitter.ctor_class().cloned() {
                emit_class_field_setup(emitter, ctx, &mangled)?;
            }
        }
        TypedExprKind::SuperMethodCall { owner, name, args } => {
            // Direct call of the parent body that declares the method (skips
            // vtable dispatch, which would re-resolve to the override). The
            // owner's physical sig is the slot sig — same erasure handling as
            // the vtable path.
            let this = emitter.this_local().ok_or_else(|| {
                crate::codegen::internal_failure("super.method() only inside a method body")
            })?;
            emitter.instruction(Instruction::LocalGet(this));
            let abi = ctx.symbols.class_method_abi(owner, &name.name).cloned();
            emit_args_into_slots(
                emitter,
                ctx,
                args,
                abi.as_ref().map(|a| a.params.as_slice()),
            )?;
            let func = ctx
                .symbols
                .class_method_func_idx(owner, &name.name)
                .ok_or_else(|| {
                    crate::codegen::internal_failure("parent method body fn allocated")
                })?;
            emitter.instruction(Instruction::Call(func));
            emit_slot_return_cast(emitter, ctx, &expr.ty.clone(), abi.as_ref())?;
        }
        TypedExprKind::McpCall { server, tool, args } => {
            // No per-tool import: serialize the args to JSON, dispatch the single
            // `submilli:mcp.call` host fn, which returns guest objects as `unknown`.
            // Known-return tools are wrapped in a normal `Cast` by typecheck.
            super::mcp::emit_mcp_call(emitter, ctx, server, tool, args)?;
        }
        TypedExprKind::CallClosure { callee, args } => {
            // Indirect dispatch via `call_ref` through a closure
            // struct: `LocalRef` to a function-typed binding, an
            // inline `Closure` expression, a function-typed field,
            // etc. The typechecker decided this isn't a static
            // top-level call.
            emit_indirect_closure_call(emitter, ctx, *callee, args, &expr.ty)?;
        }
        TypedExprKind::GenericCall {
            mangled,
            type_args,
            args,
            return_cast,
            type_predicate: _,
        } => {
            crate::codegen::init_guard::emit_check(emitter, ctx, mangled);
            emit_generic_call(
                emitter,
                ctx,
                &expr.ty,
                mangled,
                type_args,
                args,
                return_cast,
            )?;
        }
        TypedExprKind::IntrinsicCall { kind, args } => {
            emit_intrinsic_call(emitter, ctx, *kind, args)?;
        }
        TypedExprKind::MethodCall {
            receiver,
            iface,
            name,
            args,
            type_predicate: _,
        } => {
            emit_method_call(
                emitter, ctx, *receiver, iface, &name.name, args, None, None, &expr.ty,
            )?;
        }
        TypedExprKind::GenericMethodCall {
            receiver,
            iface,
            name,
            args,
            return_cast,
            type_predicate: _,
        } => {
            let plain_args: Vec<ExprId> = args.iter().map(|a| a.expr).collect();
            emit_method_call(
                emitter,
                ctx,
                *receiver,
                iface,
                &name.name,
                &plain_args,
                Some(args),
                return_cast.as_ref(),
                &expr.ty,
            )?;
        }
        TypedExprKind::ObjectLiteral { members, fields } => {
            emit_object_literal(emitter, ctx, &expr.ty, members, fields)?;
        }
        TypedExprKind::FieldAccess { receiver, name } => {
            emit_field_access(emitter, ctx, id, &expr.ty, receiver, name)?;
        }
        TypedExprKind::InterfacePropertyAccess {
            receiver,
            iface,
            name,
        } => emit_interface_property(emitter, ctx, receiver, iface, name)?,
        TypedExprKind::ArrayLiteral { elements, .. } => {
            emit_array_literal(emitter, ctx, elements)?;
        }
        TypedExprKind::TupleLiteral { elements, .. } => {
            emit_tuple_literal(emitter, ctx, elements)?;
        }
        TypedExprKind::IndexAccess { receiver, index } => {
            emit_index_access(emitter, ctx, id, receiver, index, &expr.ty)?;
        }
        TypedExprKind::Null => {
            // Plan 75.8: `null` lowers to `ref.null none`.
            // The `none` heap type is the bottom of the GC reference
            // hierarchy, so the resulting `(ref null none)` is
            // assignable to any nullable struct ref — `(ref null
            // $Object)`, `(ref null $ObjectShape)`, `(ref null
            // $string)`, etc. Using a specific heap type would only
            // work when the surrounding slot is exactly that type;
            // made `Type::Object | null` lower to `(ref null
            // $ObjectShape)` rather than `(ref null $Object)`, so a
            // hard-coded `$Object` null no longer flows.
            let _ = ctx; // intrinsics unused after the heap-type switch
            emitter.instruction(Instruction::RefNull(HeapType::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::None,
            }));
        }
        TypedExprKind::This => {
            let slot = emitter.this_local().ok_or_else(|| {
                crate::codegen::internal_failure("`this` has a local receiver or captured binding")
            })?;
            if emitter.dynamic_this {
                emitter.instruction(Instruction::LocalGet(slot));
                emitter.instruction(Instruction::RefIsNull);
                emitter.emit_if(BlockType::Empty);
                crate::codegen::throw::emit_type_error_throw(
                    emitter,
                    ctx,
                    "Unbound function has no this receiver",
                );
                emitter.emit_end();
            }
            emitter.instruction(Instruction::LocalGet(slot));
            if emitter.dynamic_this {
                crate::codegen::cast_check::emit_operation_cast_on_stack(
                    emitter,
                    ctx,
                    &Type::Unknown,
                    &expr.ty,
                )?;
            }
        }
        TypedExprKind::NumberEnumMember { value, .. } => {
            // Numeric enum values are full `$Object` subtypes — same
            // Wasm shape as boxed `Number`. Materialize a fresh
            // `$BoxedNumber` here: vtable in slot 0, f64 value in
            // slot 1. Mirror of the `Type::Number` arm in
            // `cast::emit_box`, but inline because the f64 is a
            // compile-time constant.
            let boxed_idx = ctx
                .symbols
                .boxed_number_type_idx()
                .ok_or_else(|| crate::codegen::internal_failure("boxed_number type registered"))?;
            let vtable_global = ctx
                .symbols
                .prelude_global_idx("boxed_number_vtable")
                .ok_or_else(|| {
                    crate::codegen::internal_failure("boxed_number_vtable imported from prelude")
                })?;
            emitter.instruction(Instruction::GlobalGet(vtable_global));
            emitter.instruction(Instruction::F64Const(Ieee64::from(*value)));
            emitter.instruction(Instruction::StructNew(boxed_idx));
        }
        TypedExprKind::StringEnumMember { .. } => {
            // String enum values share the `$string` representation.
            // The StringPool collector records each `StringEnumMember`
            // by `ExprId` (see string_pool.rs), so the materialisation
            // path is identical to a regular string literal.
            let pool_idx = ctx.strings.locations.get(&id).copied().ok_or_else(|| {
                crate::codegen::internal_failure("a string enum value was not interned")
            })?;
            super::emit_pooled_string(emitter, ctx, pool_idx)?;
        }
        TypedExprKind::TypeofTag { value, tag } => {
            emit_typeof_tag(emitter, ctx, *value, *tag)?;
        }
        TypedExprKind::InstanceOf { value, class } => match class.peel() {
            Type::ClassRef { mangled, .. } => {
                let vtable_global =
                    ctx.symbols
                        .class_vtable_global_idx(mangled)
                        .ok_or_else(|| {
                            crate::codegen::internal_failure("class vtable global recorded")
                        })?;
                emit_expr(emitter, ctx, *value)?;
                crate::codegen::init_guard::emit_check(emitter, ctx, mangled);
                cast::emit_nominal_instance_test(emitter, ctx, vtable_global)?;
            }
            // Not a class, so there is no vtable to walk. `$Uint8Array` is
            // canonically unique, which makes the structural test the same
            // answer the nominal walk would give.
            Type::Uint8Array => {
                let idx = ctx.symbols.uint8_array_type_idx().ok_or_else(|| {
                    crate::codegen::internal_failure("$Uint8Array intrinsic registered")
                })?;
                emit_expr(emitter, ctx, *value)?;
                emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(idx)));
            }
            _ => {
                return Err(crate::codegen::internal_failure(format!(
                    "instanceof codegen with a non-class RHS: {class:?}"
                )));
            }
        },
        TypedExprKind::Narrowed {
            path,
            source,
            binding,
            cast_info,
            inner,
        } => {
            // Expression-scoped narrowing mirrors `NarrowRegion`: stable root
            // bindings snapshot once, while field/index paths register their
            // source for checked live reads. This scope lasts only for `inner`.
            emitter.push_scope();
            if path.chain.is_empty() {
                let shadow_val = ctx.symbols.value_type(&cast_info.to_ty)?;
                let shadow = emitter.define_local(binding, shadow_val)?;
                emit_expr(emitter, ctx, *source)?;
                crate::codegen::function_emitter::cast::emit_narrowing_cast(
                    emitter, ctx, cast_info,
                )?;
                emitter.instruction(Instruction::LocalSet(shadow));
            } else {
                emitter.register_narrow_source(&binding.name, *source)?;
            }
            emit_expr(emitter, ctx, *inner)?;
            emitter.pop_scope()?;
        }
        TypedExprKind::Closure {
            params,
            captured,
            runtime_generics,
            ..
        } => {
            emit_closure_value(
                emitter,
                ctx,
                id,
                &expr.ty,
                params,
                captured,
                runtime_generics,
            )?;
        }
        // `cond ? then_: else_`. Wasm `if`-with-result lifts
        // the branch values onto the parent stack. Each branch's value
        // is coerced to the chain's result Wasm slot so both arms
        // agree on the stack type. A `void` conditional has no result:
        // it runs a branch for its effects.
        TypedExprKind::Ternary { cond, then_, else_ } => {
            let result_ty = expr.ty.clone();
            let block_ty = conditional_block_type(ctx, &result_ty)?;
            let cond_ty = ctx
                .ta
                .try_expr(*cond)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            emit_expr(emitter, ctx, *cond)?;
            crate::codegen::function_emitter::cast::emit_condition_to_i32(emitter, ctx, &cond_ty)?;
            emitter.emit_if(block_ty);
            emit_conditional_operand(emitter, ctx, *then_, &result_ty)?;
            emitter.emit_else();
            emit_conditional_operand(emitter, ctx, *else_, &result_ty)?;
            emitter.emit_end();
        }
        // `a ?? b`. `a` is evaluated once into an anonymous
        // local; `ref.is_null` selects between rhs (null branch) and
        // the stashed lhs (non-null branch). Result Wasm type =
        // `union(strip_null(lhs), rhs)`'s slot.
        //
        // If `lhs` lowers to a non-ref Wasm type (i32 boolean, f64
        // number — Submilli has no `undefined` to box these against),
        // it can never be null at runtime. Infer emits a soft warning
        // for that case; codegen emits just the lhs side, skipping
        // the if-then-else entirely (the rhs branch is unreachable).
        //
        // A `void` right side has no result slot: see
        // `emit_void_nullish_coalesce`.
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            emit_nullish_coalesce(emitter, ctx, lhs, rhs, &expr.ty)?;
        }
        // optional chain. Each `?.` step opens a fresh
        // `if`-with-result that short-circuits to `null` on a null
        // receiver and recurses into the rest of the chain on the
        // non-null branch. Per-part access codegen is factored into
        // `emit_chain_access` (Field + Index for v1; Call/MethodCall/
        // InterfaceProperty land alongside their non-chain siblings'
        // refactor).
        TypedExprKind::OptionalChain { base, parts } => {
            let result_ty = expr.ty.clone();
            let base_ty = ctx
                .ta
                .try_expr(*base)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            let receiver = if matches!(parts.first(), Some(crate::TypedChainPart::Call { .. })) {
                emit_callee(emitter, ctx, *base)?
            } else {
                emit_expr(emitter, ctx, *base)?;
                None
            };
            emitter.call_receiver = receiver;
            emit_chain_parts(
                emitter,
                ctx,
                parts,
                0,
                &base_ty,
                &result_ty,
                ctx.ta.runtime_chain_types.get(&id).map(Vec::as_slice),
            )?;
        }
        TypedExprKind::PostfixUnary { op, target } => {
            if let crate::PostfixTarget::Index {
                receiver, index, ..
            } = target
                && ctx
                    .ta
                    .source_type(*receiver)
                    .map_err(crate::codegen::arena_failure)?
                    .is_structural_object()
            {
                emit_object_index_postfix(
                    emitter,
                    ctx,
                    *receiver,
                    *index,
                    *op,
                    ctx.ta
                        .source_type(id)
                        .map_err(crate::codegen::arena_failure)?,
                )?;
                cast::emit_coerce_to_slot(emitter, ctx, &Type::Unknown, &expr.ty)?;
            } else {
                emit_postfix_unary(emitter, ctx, *op, target, &expr.ty)?;
            }
        }
        TypedExprKind::NonNullAssert { value } => {
            crate::codegen::cast_check::emit_non_null_assert(emitter, ctx, *value, &expr.ty)?;
        }
        TypedExprKind::Cast {
            value,
            target_ty,
            check,
        } => {
            crate::codegen::cast_check::emit_cast(
                emitter,
                ctx,
                *value,
                target_ty,
                check.as_deref(),
                expr.span,
            )?;
        }
    };
    // A `never` expression doesn't complete, so what an enclosing expression
    // would do with its value is unreachable: `"a" + fail()` never concatenates.
    // A read does complete: a `never[]` an alias filled holds elements, as
    // TypeScript's types allow, so reading one yields what it holds.
    if matches!(expr.ty, Type::Never) && !completes_when_never(&expr.kind) {
        emitter.instruction(Instruction::Unreachable);
    }
    Ok(())
}

/// Whether a `never`-typed `kind` still yields a value or settles its own
/// reachability. A field or element read yields what the object or array
/// holds; a narrowed reference emits its own `unreachable` when it is `never`.
/// A `never` binding stays unreachable: its initializer or the call that bound
/// it diverged.
fn completes_when_never(kind: &TypedExprKind) -> bool {
    matches!(
        kind,
        TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::FieldAccess { .. }
            | TypedExprKind::IndexAccess { .. }
    )
}

fn emit_global_ref(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    mangled: &crate::MangledName,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Read a top-level `let` / `const` binding. The typed AST
    // split means we no longer dispatch on
    // ValueKind here — `FunctionRef` handles the
    // function-as-value path separately.
    //
    // Static-interface bindings (`console`, `Map`, `Temporal.Instant`) have
    // no global behind them (see `static_interface_of` in codegen::mod). As
    // a value, one reads as the host object that prints like the binding.
    if let Some(iface) = static_interface(ctx, result_ty) {
        emit_static_value(emitter, ctx, iface)?;
        if let ValType::Ref(RefType {
            nullable: false, ..
        }) = ctx.symbols.value_type(result_ty)?
        {
            emitter.instruction(Instruction::RefAsNonNull);
        }
        return Ok(());
    }
    crate::codegen::init_guard::emit_check(emitter, ctx, mangled);
    let idx = ctx.symbols.global_idx(mangled).ok_or_else(|| {
        crate::codegen::internal_failure("top-level let/const recorded during codegen")
    })?;
    emitter.instruction(Instruction::GlobalGet(idx));
    if let ValType::Ref(RefType {
        nullable: false, ..
    }) = ctx.symbols.value_type(result_ty)?
    {
        emitter.instruction(Instruction::RefAsNonNull);
    }

    Ok(())
}

/// The static-dispatch interface `ty` names, if any.
fn static_interface<'a>(ctx: &CodegenCtx, ty: &'a Type) -> Option<&'a crate::MangledName> {
    match ty.peel() {
        Type::InterfaceRef { mangled, .. }
            if ctx.symbols.iface_dispatch(mangled) == Some(crate::Dispatch::Static) =>
        {
            Some(mangled)
        }
        _ => None,
    }
}

fn emit_static_value(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    iface: &crate::MangledName,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let tag = ctx.symbols.static_value_tag(iface).ok_or_else(|| {
        crate::codegen::internal_failure("a static binding read as a value has a recorded tag")
    })?;
    super::emit_inline_string_literal(emitter, ctx, tag)?;
    let helper = ctx
        .symbols
        .prelude_func_idx("ObjectConstructor##staticValue")
        .ok_or_else(|| crate::codegen::internal_failure("the static value helper is imported"))?;
    emitter.instruction(Instruction::Call(helper));
    Ok(())
}

fn emit_function_ref(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    mangled: &crate::MangledName,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // top-level function used as a value. Wrap it in
    // a closure struct whose funcref points at a per-function
    // adapter (slot 0 = env, slot 1 = adapter funcref, slot 2
    // = env sentinel). The shared closure vtable reuses for
    // the env slot — adapter bodies ignore it, and the slot
    // just needs a non-null `(ref any)`. The closure is built
    // once and cached in a global, so every read is the same value.
    // A `FunctionRef` is always typed by its declared signature, so
    // `result_ty` classifies like the adapter's signature and the
    // cached struct type matches the global's.
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(crate::codegen::closures::classify(result_ty)?)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure struct type registered for every function-as-value",
            )
        })?;
    let adapter_idx = ctx.symbols.adapter_func_idx(mangled).ok_or_else(|| {
        crate::codegen::internal_failure("adapter func recorded for every function-as-value")
    })?;
    let closure_global_idx = ctx
        .symbols
        .adapter_closure_global_idx(mangled)
        .ok_or_else(|| {
            crate::codegen::internal_failure("closure global recorded for every function-as-value")
        })?;
    let vtable_idx = ctx.symbols.closure_vtable_global_idx().ok_or_else(|| {
        crate::codegen::internal_failure(
            "closure vtable global emitted whenever closures or adapters exist",
        )
    })?;
    emitter.instruction(Instruction::GlobalGet(closure_global_idx));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::GlobalGet(vtable_idx));
    emitter.instruction(Instruction::RefFunc(adapter_idx));
    emitter.instruction(Instruction::GlobalGet(vtable_idx));
    if let Some(metadata) = ctx.symbols.function_argument_metadata.get(mangled) {
        crate::codegen::call_arguments::wrap(emitter, ctx, metadata)?;
    }
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(closure_struct_idx));
    emitter.instruction(Instruction::GlobalSet(closure_global_idx));
    emitter.emit_end();
    emitter.instruction(Instruction::GlobalGet(closure_global_idx));
    if ctx.symbols.is_shared_closure_global(mangled) {
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            closure_struct_idx,
        )));
    } else {
        emitter.instruction(Instruction::RefAsNonNull);
    }

    Ok(())
}

fn emit_symbol_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    mangled: &crate::MangledName,
    args: &[ExprId],
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Static dispatch — the typechecker already resolved this to
    // a known top-level symbol (`mangled`), so it must be in the
    // top-level fn registry. Imported (prelude / host) and local
    // user functions take slightly different emission paths.
    // No call-boundary box/cast logic here — generic calls route
    // through [`GenericCall`](TypedExprKind::GenericCall) instead.
    //
    // note: variadic callees see one synthesized
    // `ArrayLiteral` in the rest slot — the typechecker
    // pre-packs the trailing args — so `args.len()` always
    // matches `target.params.len()` and no codegen branching
    // is needed.
    let target = ctx.symbols.top_level_fn(mangled).ok_or_else(|| {
        crate::codegen::internal_failure(format!(
            "Call references unknown top-level fn `{}`",
            mangled.as_str()
        ))
    })?;
    if crate::codegen::field_guards::guarded_constructor(ctx, mangled, result_ty)? {
        let Type::ClassRef { mangled: class, .. } = result_ty.peel() else {
            return Err(crate::codegen::internal_failure("constructor class"));
        };
        emit_args_into_slots(emitter, ctx, args, ctx.symbols.class_ctor_abi(class))?;
        crate::codegen::field_guards::constructor_argument(emitter, ctx, result_ty)?;
        emitter.instruction(Instruction::Call(target.wasm_idx));
    } else {
        emit_direct_call(
            emitter,
            ctx,
            target.is_host,
            target.wasm_idx,
            &target.params,
            &target.ret,
            args,
        )?;
    }
    if result_ty.is_void() && !target.ret.is_void() {
        emitter.instruction(Instruction::Drop);
    }

    Ok(())
}

fn emit_interface_property(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: &ExprId,
    iface: &crate::MangledName,
    name: &Ident,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // property dispatch. The typechecker resolved this
    // to an interface property at infer time and stamped the
    // interface's mangled name on the node, so codegen looks
    // up the getter under `<iface>#<name>` directly — no
    // receiver-type inspection needed.
    let key = crate::mangle::extend(iface, &name.name);
    // Static-interface properties (`Number.EPSILON`) import as
    // constant globals — no receiver, no getter call.
    if ctx.symbols.iface_dispatch(iface) == Some(crate::Dispatch::Static) {
        let global_idx = ctx.symbols.global_idx(&key).ok_or_else(|| {
            crate::codegen::internal_failure(
                "static interface property recorded as a global import",
            )
        })?;
        emitter.instruction(Instruction::GlobalGet(global_idx));
        return Ok(());
    }
    if let Some(struct_idx) = inline_length_struct_idx(ctx, iface, &name.name)? {
        emit_receiver(emitter, ctx, *receiver)?;
        emit_inline_length(emitter, ctx, struct_idx);
        return Ok(());
    }
    let func_idx = ctx.symbols.func_idx(&key).ok_or_else(|| {
        crate::codegen::internal_failure(
            "interface property getter recorded during the dependency-import pass",
        )
    })?;
    emit_receiver(emitter, ctx, *receiver)?;
    // InterfaceRef-typed receivers (`Map<K, V>`,
    // `Set<T>` — Direct dispatch) lower to
    // `(ref null $Object)`, but the property getter wrapper
    // takes `(ref $Object)` non-null. Coerce the same way
    // `emit_method_call` does for the method path. Primitive
    // Direct receivers (Array, Uint8Array, etc.) already have
    // non-null Wasm types so no coercion needed.
    //
    // `peel` so an aliased InterfaceRef
    // (`type Query = Map<string, string>` etc.) still hits
    // this branch — `matches!` on the raw type would miss
    // the wrapper.
    if matches!(
        ctx.ta
            .source_type(*receiver)
            .map_err(crate::codegen::arena_failure)?
            .peel(),
        Type::InterfaceRef { .. }
    ) {
        emitter.instruction(Instruction::RefAsNonNull);
    }
    emitter.instruction(Instruction::Call(func_idx));

    Ok(())
}

fn emit_index_access(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
    receiver: &ExprId,
    index: &ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Uint8Array uses a different storage shape than
    // Array (packed i8 vs boxed anyref slots), so the index
    // recipe forks on the static receiver type. Tuples lower
    // to `$Array` and fall through to the default arm.
    let recv_ty = ctx
        .ta
        .source_type(*receiver)
        .map_err(crate::codegen::arena_failure)?
        .clone();
    if recv_ty.is_structural_object() {
        emit_expr(emitter, ctx, *receiver)?;
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(*receiver)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
        emit_expr(emitter, ctx, *index)?;
        crate::codegen::cast_check::emit_operation_cast_on_stack(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(*index)
                .map_err(crate::codegen::arena_failure)?
                .ty,
            &Type::String,
        )?;
        let Some(symbol) = ctx.require(
            ctx.symbols.prelude_func_idx("ObjectConstructor##getField"),
            "dynamic read imported",
        ) else {
            return Ok(());
        };
        emitter.instruction(Instruction::Call(symbol));
        let checked_ty = ctx
            .ta
            .source_type(id)
            .map_err(crate::codegen::arena_failure)?;
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            checked_ty,
        )?;
        cast::emit_coerce_to_slot(emitter, ctx, checked_ty, result_ty)?;
    } else if recv_ty.peel() == &Type::Uint8Array {
        emit_expr(emitter, ctx, *receiver)?;
        emit_uint8_index_with_receiver_on_stack(emitter, ctx, *index)?;
    } else {
        emit_expr(emitter, ctx, *receiver)?;
        emit_bounds_checked_index_with_receiver_on_stack(emitter, ctx, *index, result_ty)?;
    }

    Ok(())
}

fn emit_nullish_coalesce(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: &ExprId,
    rhs: &ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let result_ty = result_ty.clone();
    if result_ty.is_void() {
        return emit_void_nullish_coalesce(emitter, ctx, *lhs, *rhs);
    }
    let result_val = ctx.symbols.value_type(&result_ty)?;
    let lhs_ty = ctx
        .ta
        .try_expr(*lhs)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    let lhs_val = ctx.symbols.value_type(&lhs_ty)?;
    let lhs_is_ref = matches!(lhs_val, ValType::Ref(_));
    if lhs_is_ref {
        let tmp = emitter.add_anonymous_local(lhs_val)?;
        emit_expr(emitter, ctx, *lhs)?;
        emitter.instruction(Instruction::LocalTee(tmp));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(BlockType::Result(result_val));
        emit_conditional_operand(emitter, ctx, *rhs, &result_ty)?;
        emitter.emit_else();
        emitter.instruction(Instruction::LocalGet(tmp));
        // The else branch knows the value isn't null; cast
        // from `lhs_ty`'s Wasm form to the result slot. For a
        // mixed-typed union (`string | null` → `(ref null
        // $Object)`) this is a ref-cast down to the result's
        // concrete heap type, which traps if the value's
        // runtime tag disagrees (it shouldn't — infer
        // enforced assignability). `emit_cast_to` handles
        // both the `ref.as_non_null` lift and the per-type
        // narrowing cast.
        crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, &result_ty)?;
        emitter.emit_end();
    } else {
        // Non-nullable, primitive-typed lhs — just emit it.
        emit_expr(emitter, ctx, *lhs)?;
        crate::codegen::function_emitter::cast::emit_coerce_to_slot(
            emitter, ctx, &lhs_ty, &result_ty,
        )?;
    }

    Ok(())
}

/// The Wasm block type of a `?:` or `??` typed `result_ty`: none when the
/// expression is `void`, else the result's value slot.
fn conditional_block_type(
    ctx: &CodegenCtx,
    result_ty: &Type,
) -> Result<BlockType, crate::compiler_error::CompilerFailure> {
    if result_ty.is_void() {
        return Ok(BlockType::Empty);
    }
    Ok(BlockType::Result(ctx.symbols.value_type(result_ty)?))
}

/// Emits one operand of a `?:` or `??` typed `result_ty`, leaving what the
/// enclosing block expects: the operand coerced to the result slot, or nothing
/// when the expression is `void`.
fn emit_conditional_operand(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    operand: ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let operand_ty = ctx
        .ta
        .try_expr(operand)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    emit_expr(emitter, ctx, operand)?;
    if !result_ty.is_void() {
        return cast::emit_coerce_to_slot(emitter, ctx, &operand_ty, result_ty);
    }
    if !operand_ty.is_void() {
        emitter.instruction(Instruction::Drop);
    }
    Ok(())
}

/// `a ?? b` where `b` is `void`: evaluates `b` for its effects when `a` is
/// null, and leaves nothing on the stack.
fn emit_void_nullish_coalesce(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let lhs_ty = &ctx
        .ta
        .try_expr(lhs)
        .map_err(crate::codegen::arena_failure)?
        .ty;
    let lhs_is_ref = matches!(ctx.symbols.value_type(lhs_ty)?, ValType::Ref(_));
    emit_expr(emitter, ctx, lhs)?;
    if !lhs_is_ref {
        // A primitive slot is never null, so the right side never runs.
        emitter.instruction(Instruction::Drop);
        return Ok(());
    }
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emit_conditional_operand(emitter, ctx, rhs, &Type::Void)?;
    emitter.emit_end();
    Ok(())
}

/// Read a refinement that the runtime-value pass proved representation-stable.
/// Mutable global, captured, and property reads whose values can outlive their
/// refinements have already been replaced with boxed reads. Remaining shadows
/// bridge assignment and branch-local narrowing slots to their inferred types.
// Keep allocation and member lowering out of the recursive expression dispatcher
// so unrelated branches do not enlarge every debug-build recursion frame.
fn emit_generic_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    result_ty: &Type,
    mangled: &crate::MangledName,
    type_args: &[Type],
    args: &[crate::GenericArgument],
    return_cast: &Option<Type>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Direct call to a user-declared top-level generic
    // function. Args whose unsubstituted slot is a bare
    // `Type::TypeVar` (`is_generic == true`) box at the
    // call boundary; ref-typed values and concrete-typed
    // args pass through unchanged. If `return_cast` is
    // `Some`, the callee returns `(ref $Object)` and we
    // emit `cast::emit_cast_to` to materialize the
    // call-site return type.
    //
    // note: variadic generic callees have their
    // trailing args pre-packed by the typechecker into a
    // synthesized `ArrayLiteral` filling the rest slot,
    // marked `is_generic: false` (the array's outer type is
    // concrete). No call-site branching is needed here.
    let (wasm_idx, target_params, target_return) = match ctx.symbols.top_level_fn(mangled) {
        Some(target) => (target.wasm_idx, target.params.clone(), target.ret.clone()),
        None => {
            return Err(crate::codegen::internal_failure(
                "generic call target is not registered",
            ));
        }
    };
    for (i, arg) in args.iter().enumerate() {
        emit_expr(emitter, ctx, arg.expr)?;
        if arg.is_generic {
            let arg_ty = ctx
                .ta
                .try_expr(arg.expr)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            crate::codegen::function_emitter::cast::emit_box(emitter, ctx, &arg_ty)?;
        } else if let Some(param_ty) = target_params.get(i) {
            // A non-generic arg can still land in a wider erased slot
            // (`count: number | null`, a `T | null` param): coerce a
            // primitive into the ref-typed param exactly like the
            // plain-call path. No-op when the Wasm types line up.
            let arg_ty = ctx
                .ta
                .try_expr(arg.expr)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            cast::emit_coerce_to_slot(emitter, ctx, &arg_ty, param_ty)?;
        }
    }
    if crate::codegen::field_guards::guarded_constructor(ctx, mangled, result_ty)? {
        crate::codegen::field_guards::constructor_argument(emitter, ctx, result_ty)?;
    }
    if ctx.symbols.runtime_generic_functions.contains(mangled) {
        crate::codegen::runtime_descriptors::environment(emitter, ctx, type_args)?;
    }
    emitter.instruction(Instruction::Call(wasm_idx));
    if let Some(ty) = return_cast {
        if ty.is_void() {
            emitter.instruction(Instruction::Drop);
        } else {
            cast::emit_cast_to(emitter, ctx, ty)?;
        }
    } else if result_ty.is_void() {
        if !target_return.is_void() {
            emitter.instruction(Instruction::Drop);
        }
    } else {
        cast::emit_coerce_to_slot(emitter, ctx, &target_return, result_ty)?;
    }

    Ok(())
}

fn emit_object_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    result_ty: &Type,
    members: &[TypedObjectMember],
    fields: &[crate::TypedObjectFieldOrigin],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if members
        .iter()
        .any(|member| matches!(member, TypedObjectMember::Computed { .. }))
    {
        emit_computed_object(emitter, ctx, members)?;
        return Ok(());
    }
    if !matches!(result_ty, Type::Object { .. } | Type::InterfaceRef { .. }) {
        return Err(crate::codegen::internal_failure(
            "object literal requires an object representation",
        ));
    }
    let structural_ty = crate::typed_ast::object_literal_layout(result_ty, fields);
    if members
        .iter()
        .any(|member| matches!(member, TypedObjectMember::Spread { .. }))
    {
        emit_object_spread(emitter, ctx, members, &structural_ty)?;
        return Ok(());
    }
    let vtable_idx = ctx
        .symbols
        .vtable_global_idx(&structural_ty)
        .ok_or_else(|| {
            crate::codegen::internal_failure("vtable global recorded during object-emission pass")
        })?;
    let object_shape_idx = ctx
        .symbols
        .object_subtype_idx(&structural_ty)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "object shape type recorded during object-emission pass",
            )
        })?;
    let Type::Object {
        fields: declared_fields,
        ..
    } = &structural_ty
    else {
        return Err(crate::codegen::internal_failure(
            "object literal has no structural representation",
        ));
    };
    let shape_key: Vec<_> = declared_fields
        .iter()
        .map(|(name, field)| crate::codegen::field_names::FieldName {
            name: name.clone(),
            optional: field.optional,
            is_accessor: false,
            is_private: false,
        })
        .collect();
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;

    // Plain literals still evaluate overwritten values in source order.
    let evaluated = evaluate_object_members(emitter, ctx, members)?;

    emitter.instruction(Instruction::GlobalGet(vtable_idx));
    crate::codegen::field_names::emit_instance_names(emitter, ctx, &shape_key, |name| {
        fields.iter().any(|field| {
            field.name.name == name && !matches!(field.source, TypedObjectFieldSource::Absent(_))
        })
    })?;

    // Walk the declared shape's field set in BTreeMap order
    // and emit one value per slot. The typed-AST `fields`
    // list is already in the same BTreeMap order — pair them
    // by name to stay robust against ordering drift.
    let by_name: std::collections::BTreeMap<&str, &crate::TypedObjectFieldOrigin> =
        fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
    for (name, field) in declared_fields {
        if let Some(origin) = by_name.get(name.as_str()) {
            let (TypedObjectFieldSource::Literal(value) | TypedObjectFieldSource::Absent(value)) =
                &origin.source
            else {
                return Err(crate::codegen::internal_failure(
                    "spread field reached plain object lowering",
                ));
            };
            emitter.instruction(Instruction::LocalGet(*evaluated.get(value).ok_or_else(
                || crate::codegen::internal_failure("object field value was not evaluated"),
            )?));
        } else {
            if !field.optional {
                return Err(crate::codegen::internal_failure(
                    "required object field is missing from the literal",
                ));
            }
            emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
        }
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.object_fields,
        array_size: crate::codegen::wasm_u32(declared_fields.len())?,
    });
    emitter.instruction(Instruction::RefNull(HeapType::ANY));
    emitter.instruction(Instruction::StructNew(object_shape_idx));

    Ok(())
}

fn emit_field_access(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
    result_ty: &Type,
    receiver: &ExprId,
    name: &Ident,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let receiver_ty = ctx
        .ta
        .source_type(*receiver)
        .map_err(crate::codegen::arena_failure)?
        .clone();
    if ctx.ta.has_string_index(&receiver_ty) {
        emit_expr(emitter, ctx, *receiver)?;
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(*receiver)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
        let Some(symbol) = ctx.require(
            ctx.symbols.field_name_string_global_idx(&name.name),
            "property name collected",
        ) else {
            return Ok(());
        };
        emitter.instruction(Instruction::GlobalGet(symbol));
        let Some(symbol) = ctx.require(
            ctx.symbols.prelude_func_idx("ObjectConstructor##getField"),
            "record read imported",
        ) else {
            return Ok(());
        };
        emitter.instruction(Instruction::Call(symbol));
        let checked_ty = ctx
            .ta
            .source_type(id)
            .map_err(crate::codegen::arena_failure)?;
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            checked_ty,
        )?;
        cast::emit_coerce_to_slot(emitter, ctx, checked_ty, result_ty)?;
        return Ok(());
    }
    // Class instance: nominal receiver, array-backed object payload.
    if let Type::ClassRef { mangled, .. } = receiver_ty.peel() {
        // Accessor property (no data slot): dispatch the synthetic getter.
        if ctx.symbols.class_field_slot(mangled, &name.name).is_none() {
            let getter = crate::codegen::classes::accessor_getter_name(&name.name);
            emit_method_call(
                emitter,
                ctx,
                *receiver,
                mangled,
                &getter,
                &[],
                None,
                None,
                &ctx.ta
                    .try_expr(id)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            return Ok(());
        }
        let struct_idx = ctx.symbols.class_struct_type_idx(mangled).ok_or_else(|| {
            crate::codegen::internal_failure("class struct type recorded in classes::emit")
        })?;
        let slot = ctx
            .symbols
            .class_field_slot(mangled, &name.name)
            .ok_or_else(|| {
                crate::codegen::internal_failure("class field slot recorded in classes::emit")
            })?;
        let intrinsics = ctx.symbols.intrinsic_type_indices().ok_or_else(|| {
            crate::codegen::internal_failure("intrinsics declared by codegen entry")
        })?;
        emit_receiver(emitter, ctx, *receiver)?;
        let object = emitter.add_anonymous_local(ctx.symbols.value_type(&receiver_ty)?)?;
        emitter.instruction(Instruction::LocalTee(object));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: struct_idx,
            field_index: 2,
        });
        emitter.instruction(Instruction::I32Const(slot as i32));
        emitter.instruction(Instruction::ArrayGet(intrinsics.object_fields));
        if ctx
            .symbols
            .class_field_narrowing_check(mangled, &name.name)
            .is_some()
        {
            crate::codegen::field_guards::check(emitter, ctx, object, mangled, &name.name)?;
        }
        emit_class_field_slot_cast_as(
            emitter,
            ctx,
            mangled,
            &name.name,
            &ctx.ta
                .try_expr(id)
                .map_err(crate::codegen::arena_failure)?
                .ty,
            ctx.ta
                .source_type(id)
                .map_err(crate::codegen::arena_failure)?,
        )?;
        return Ok(());
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    // Use the expression's physical type so a field read preserves
    // unexpected values after its source refinement becomes stale.
    let field_ty = ctx
        .ta
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    // Stashed so we can re-push it (as the call's self argument) and
    // then read its slot 2 to get the getter funcref.
    emit_receiver(emitter, ctx, *receiver)?;
    let rcv_local = stash_receiver_as_object_shape(emitter, &receiver_ty, intrinsics.object_shape)?;
    emit_object_property_read_as(
        emitter,
        ctx,
        rcv_local,
        &name.name,
        &field_ty,
        ctx.ta
            .source_type(id)
            .map_err(crate::codegen::arena_failure)?,
    )?;

    Ok(())
}

fn emit_closure_value(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: ExprId,
    result_ty: &Type,
    params: &[crate::TypedParam],
    captured: &[crate::CapturedVar],
    runtime_generics: &[String],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // emit the closure value at the creation site.
    // `$closure_<sig>` is `(sub $Object (struct (ref $VTable)
    // (ref $fn_<sig>) (ref any)))` — three fields, consumed
    // by `struct.new` in source order:
    //
    //   global.get $closure_vtable   ;; field 0: (ref $VTable)
    //   ref.func <closure_body>      ;; field 1: (ref $fn_<sig>)
    //   <captured-value loads>       ;; → env-field types
    //   struct.new $env_N            ;; field 2: (ref $env_N) →
    //                                 ;;   subtypes (ref any)
    //   struct.new $closure_<sig>
    //
    // Per-signature `$closure_<sig>` is registered from the
    // closure expression's `result_ty` (a `Type::Function`);
    // per-arrow `$env_N` and the body's func index are keyed
    // on `id` (the closure's `ExprId`). The closure vtable
    // global is shared across every closure.
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(crate::codegen::closures::classify(result_ty)?)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure struct type registered for every closure-typed expression",
            )
        })?;
    let env_type_idx = ctx.symbols.env_type_idx(id).ok_or_else(|| {
        crate::codegen::internal_failure("env type registered for every closure expression")
    })?;
    let closure_func_idx = ctx
        .symbols
        .closure_func_idx(id)
        .ok_or_else(|| crate::codegen::internal_failure("closure body function index allocated"))?;
    let closure_vtable_idx = ctx.symbols.closure_vtable_global_idx().ok_or_else(|| {
        crate::codegen::internal_failure("closure vtable global emitted whenever closures exist")
    })?;

    // Field 0: shared closure vtable (inherited from $Object).
    emitter.instruction(Instruction::GlobalGet(closure_vtable_idx));

    // Field 1: typed funcref to the closure body.
    emitter.instruction(Instruction::RefFunc(closure_func_idx));

    // Field 2: env. Build it inline by pushing each captured
    // value in order, then `struct.new`.
    for c in captured {
        emit_captured_load(emitter, ctx, c)?;
    }
    if !runtime_generics.is_empty() {
        let types: Vec<_> = runtime_generics
            .iter()
            .cloned()
            .map(Type::TypeVar)
            .collect();
        crate::codegen::runtime_descriptors::environment(emitter, ctx, &types)?;
    }
    let self_environment = if ctx.ta.closure_names.contains_key(&id) {
        emitter.instruction(Instruction::RefNull(HeapType::Concrete(
            ctx.symbols
                .intrinsic_type_indices()
                .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?
                .object,
        )));
        Some(emitter.add_anonymous_local(ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(env_type_idx),
        }))?)
    } else {
        None
    };
    emitter.instruction(Instruction::StructNew(env_type_idx));
    if let Some(local) = self_environment {
        emitter.instruction(Instruction::LocalTee(local));
    }
    if let Some(metadata) = crate::codegen::call_arguments::typed_metadata(params) {
        crate::codegen::call_arguments::wrap(emitter, ctx, &metadata)?;
    }

    if ctx.ta.closure_this.contains_key(&id) {
        crate::codegen::this_binding::wrap(emitter, ctx)?;
    }
    // Stack: vtable, funcref, env — `(ref $env_N)` subtypes
    // `(ref any)` so the env flows into field 2 implicitly.
    // Closure struct allocation consumes all four fields.
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(closure_struct_idx));
    if let Some(environment) = self_environment {
        let closure = emitter.add_anonymous_local(ctx.symbols.value_type(result_ty)?)?;
        emitter.instruction(Instruction::LocalSet(closure));
        emitter.instruction(Instruction::LocalGet(environment));
        emitter.instruction(Instruction::LocalGet(closure));
        emitter.instruction(Instruction::StructSet {
            struct_type_index: env_type_idx,
            field_index: crate::codegen::wasm_u32(
                captured
                    .len()
                    .checked_add(usize::from(!runtime_generics.is_empty()))
                    .ok_or_else(|| {
                        crate::codegen::internal_failure("closure capture count overflow")
                    })?,
            )?,
        });
        emitter.instruction(Instruction::LocalGet(closure));
    }

    Ok(())
}

/// Whether inference typed `id` as `never`. Runtime-value lowering may widen
/// an expression's type afterwards, so `expr.ty` alone can't tell.
fn inferred_never(
    ctx: &CodegenCtx,
    id: ExprId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let ty = ctx
        .ta
        .source_type(id)
        .map_err(crate::codegen::arena_failure)?;
    Ok(matches!(ty, Type::Never))
}

fn emit_local_narrow_ref(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    binding: &Ident,
    path: &ReferencePath,
    narrowed_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if !path.chain.is_empty()
        && let Some(source) = emitter.narrow_source(&binding.name)
    {
        // Calls preserve this typecheck-time fact but may mutate the referenced
        // slot, so re-read and validate every use instead of caching a snapshot.
        emit_expr(emitter, ctx, source)?;
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(source)
                .map_err(crate::codegen::arena_failure)?
                .ty,
            narrowed_ty,
        )?;
        return Ok(());
    }
    let Some((slot, slot_ty)) = emitter.narrowed_read_slot(&binding.name) else {
        // A rematerialized root view can outlive the synthetic shadow named by
        // its source (for example, an identifier narrowing carried into a
        // nested block across a call). Read the real binding that the path
        // identifies instead of recursively trying to emit the closed shadow.
        if let BindingId::Local { name, .. } = &path.root
            && let Some((slot, slot_ty)) = emitter.narrowed_read_slot(name)
        {
            emitter.instruction(Instruction::LocalGet(slot));
            let stack_ty = unbox_if_boxed(emitter, ctx, slot_ty);
            if stack_ty != ctx.symbols.value_type(narrowed_ty)? {
                cast::emit_cast_to(emitter, ctx, narrowed_ty)?;
            }
            return Ok(());
        }
        if let BindingId::Global(mangled) = &path.root {
            let idx = ctx.symbols.global_idx(mangled).ok_or_else(|| {
                crate::codegen::internal_failure("Inferer guarantees the binding exists")
            })?;
            emitter.instruction(Instruction::GlobalGet(idx));
            let source_ty = ctx.symbols.global_type(mangled).ok_or_else(|| {
                crate::codegen::internal_failure("language globals retain their declared type")
            })?;
            crate::codegen::cast_check::emit_checked_cast_on_stack(
                emitter,
                ctx,
                source_ty,
                narrowed_ty,
            )?;
            return Ok(());
        }
        if let Some(source) = emitter.narrow_source(&binding.name) {
            emit_expr(emitter, ctx, source)?;
            let source_ty = &ctx
                .ta
                .try_expr(source)
                .map_err(crate::codegen::arena_failure)?
                .ty;
            let cast_info = cast_info_for(source_ty.clone(), narrowed_ty.clone());
            cast::emit_narrowing_cast(emitter, ctx, &cast_info)?;
            return Ok(());
        }
        return Err(crate::codegen::internal_failure(format!(
            "narrow binding `{}` (path {path:?}) not registered in codegen scope — \
             the region's source names a shadow from a scope that already closed",
            binding.name,
        )));
    };
    emitter.instruction(Instruction::LocalGet(slot));
    let stack_ty = unbox_if_boxed(emitter, ctx, slot_ty);
    let _: () = if stack_ty != ctx.symbols.value_type(narrowed_ty)? {
        cast::emit_cast_to(emitter, ctx, narrowed_ty)?;
    };
    Ok(())
}

/// If `slot_ty` is a `(ref $box)` over a registered box struct, emit the
/// `struct.get` that reads its payload; return the type now on the stack.
///
/// Post-`if` join narrowing rebinds to the *original* ident. When that binding
/// is a captured-and-mutated `let` (e.g. a nullable `let` assigned inside a
/// `for` body — the loop body is a capture frame), its slot holds the
/// `(ref $box)`, not the value, and a cast applied to the box would trap.
fn unbox_if_boxed(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    slot_ty: ValType,
) -> ValType {
    let ValType::Ref(RefType {
        heap_type: HeapType::Concrete(idx),
        ..
    }) = slot_ty
    else {
        return slot_ty;
    };
    let Some(payload) = ctx.symbols.box_payload_type(idx) else {
        return slot_ty;
    };
    emitter.instruction(Instruction::StructGet {
        struct_type_index: idx,
        field_index: 0,
    });
    payload
}

// Keep literal construction outside the recursive expression dispatcher's
// debug stack frame, including its fallible lookup/conversion temporaries.
fn emit_array_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    elements: &[crate::TypedArrayElement],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // two codegen paths depending on whether any
    // element is a spread. With no spreads, the fast path
    // mirrors the original recipe:
    //   global.get $array_vtable
    //   <each element pushed + emit_box(elem.ty)>
    //   array.new_fixed $rawArray N
    //   struct.new $Array
    // With spreads, mirror the `Array#concat` two-pass
    // shape inline: measure total length across fixed + spread
    // sources, allocate via `array.new_default`, then walk in
    // order copying spread chunks with `array.copy` and
    // setting fixed elements with `array.set`.
    //
    // Box per *each element's actual type*, not the container's
    // `element_ty`. When the literal flows into a generic-arg
    // position with hint `T[]`, `element_ty` is `Type::Var("T")`
    // (the unresolved hint propagated through
    // `infer_array_literal`); each element is still a concrete
    // primitive on the stack — boxing has to happen by the
    // actual type or the slot type mismatch trips the validator.
    let array_idx = ctx.symbols.array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let raw_array_idx = ctx.symbols.raw_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let array_vtable_global = ctx
        .symbols
        .prelude_global_idx("array_vtable")
        .ok_or_else(|| crate::codegen::internal_failure("array_vtable imported from prelude"))?;

    let has_spread = elements
        .iter()
        .any(|e| matches!(e, crate::TypedArrayElement::Spread(_)));

    if has_spread {
        emit_spread_array_literal(emitter, ctx, elements)?;
    } else {
        emitter.instruction(Instruction::GlobalGet(array_vtable_global));
        for el in elements {
            let elem_id = el.expr_id();
            emit_expr(emitter, ctx, elem_id)?;
            let elem_ty = ctx
                .ta
                .try_expr(elem_id)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            crate::codegen::function_emitter::cast::emit_box(emitter, ctx, &elem_ty)?;
        }
        emitter.instruction(Instruction::ArrayNewFixed {
            array_type_index: raw_array_idx,
            array_size: crate::codegen::wasm_u32(elements.len())?,
        });
        emitter.instruction(Instruction::I32Const(
            i32::try_from(elements.len())
                .map_err(|_| crate::codegen::internal_failure("array literal too large"))?,
        ));
        emitter.instruction(Instruction::StructNew(array_idx));
    }
    Ok(())
}

fn emit_tuple_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    elements: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Tuple storage is erased, so box the value actually on the stack.
    let array_idx = ctx.symbols.array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let raw_array_idx = ctx.symbols.raw_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let array_vtable_global = ctx
        .symbols
        .prelude_global_idx("array_vtable")
        .ok_or_else(|| crate::codegen::internal_failure("array_vtable imported from prelude"))?;
    emitter.instruction(Instruction::GlobalGet(array_vtable_global));
    for &elem_id in elements {
        emit_expr(emitter, ctx, elem_id)?;
        crate::codegen::function_emitter::cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(elem_id)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: raw_array_idx,
        array_size: crate::codegen::wasm_u32(elements.len())?,
    });
    emitter.instruction(Instruction::I32Const(
        i32::try_from(elements.len())
            .map_err(|_| crate::codegen::internal_failure("array literal too large"))?,
    ));
    emitter.instruction(Instruction::StructNew(array_idx));
    Ok(())
}

fn emit_spread_array_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    elements: &[crate::TypedArrayElement],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let array = ctx
        .symbols
        .array_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("Array declared"))?;
    let raw = ctx
        .symbols
        .raw_array_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("raw Array declared"))?;
    let raw_type = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw),
    });
    let total = emitter.add_anonymous_local(ValType::I32)?;
    let offset = emitter.add_anonymous_local(ValType::I32)?;
    let destination = emitter.add_anonymous_local(raw_type)?;
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(total));
    // Snapshot every chunk in source order before later expressions can mutate
    // a spread's backing storage. A plain element is a one-element chunk.
    let mut chunks = Vec::with_capacity(elements.len());
    for element in elements {
        emit_literal_chunk(emitter, ctx, element, array, raw)?;
        let chunk = emitter.add_anonymous_local(raw_type)?;
        emitter.instruction(Instruction::LocalTee(chunk));
        emitter.instruction(Instruction::ArrayLen);
        emitter.instruction(Instruction::LocalGet(total));
        emitter.instruction(Instruction::I32Add);
        emitter.instruction(Instruction::LocalSet(total));
        chunks.push(chunk);
    }
    emitter.instruction(Instruction::LocalGet(total));
    emitter.instruction(Instruction::ArrayNewDefault(raw));
    emitter.instruction(Instruction::LocalSet(destination));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(offset));
    for chunk in chunks {
        emitter.instruction(Instruction::LocalGet(destination));
        emitter.instruction(Instruction::LocalGet(offset));
        emitter.instruction(Instruction::LocalGet(chunk));
        emitter.instruction(Instruction::I32Const(0));
        emitter.instruction(Instruction::LocalGet(chunk));
        emitter.instruction(Instruction::ArrayLen);
        emitter.instruction(Instruction::ArrayCopy {
            array_type_index_dst: raw,
            array_type_index_src: raw,
        });
        emitter.instruction(Instruction::LocalGet(offset));
        emitter.instruction(Instruction::LocalGet(chunk));
        emitter.instruction(Instruction::ArrayLen);
        emitter.instruction(Instruction::I32Add);
        emitter.instruction(Instruction::LocalSet(offset));
    }
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .prelude_global_idx("array_vtable")
            .ok_or_else(|| crate::codegen::internal_failure("array vtable declared"))?,
    ));
    emitter.instruction(Instruction::LocalGet(destination));
    emitter.instruction(Instruction::LocalGet(total));
    emitter.instruction(Instruction::StructNew(array));
    Ok(())
}

fn emit_literal_chunk(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    element: &crate::TypedArrayElement,
    array: u32,
    raw: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if let crate::TypedArrayElement::Value(value) = element {
        emit_expr(emitter, ctx, *value)?;
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(*value)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
        emitter.instruction(Instruction::ArrayNewFixed {
            array_type_index: raw,
            array_size: 1,
        });
        return Ok(());
    }
    // A source `runtime_values` widened may no longer hold the array its
    // narrowed type said; `emit_receiver` checks it as a field read's receiver.
    emit_receiver(emitter, ctx, element.expr_id())?;
    let raw_type = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw),
    });
    let source = emitter.add_anonymous_local(raw_type)?;
    let snapshot = emitter.add_anonymous_local(raw_type)?;
    let length = stash_array_length(emitter, array)?;
    emitter.instruction(Instruction::StructGet {
        struct_type_index: array,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(source));
    emitter.instruction(Instruction::LocalGet(length));
    emitter.instruction(Instruction::ArrayNewDefault(raw));
    emitter.instruction(Instruction::LocalTee(snapshot));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalGet(source));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalGet(length));
    emitter.instruction(Instruction::ArrayCopy {
        array_type_index_dst: raw,
        array_type_index_src: raw,
    });
    emitter.instruction(Instruction::LocalGet(snapshot));
    Ok(())
}

/// Read the original value, write its increment/decrement, and return the
/// original. Binding operands are read at the narrowed result type but written
/// back through their declared slot, including nullable and captured bindings.
fn emit_postfix_unary(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    op: crate::PostfixOp,
    target: &crate::PostfixTarget,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match target {
        crate::PostfixTarget::Local {
            ident,
            boxed,
            target_ty,
        } => {
            // A read-modify-write, so the write decides the slot.
            let slot = emitter.write_slot(&ident.name)?;
            let old = emitter.add_anonymous_local(ctx.symbols.value_type(result_ty)?)?;
            let box_idx = if *boxed {
                Some(ctx.symbols.box_type_idx(target_ty)?.ok_or_else(|| {
                    crate::codegen::internal_failure("box type registered for postfix operand")
                })?)
            } else {
                None
            };
            emitter.instruction(Instruction::LocalGet(slot));
            if let Some(box_idx) = box_idx {
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
            }
            let cast_info = cast_info_for(target_ty.clone(), result_ty.clone());
            cast::emit_narrowing_cast(emitter, ctx, &cast_info)?;
            emit_postfix_numeric(emitter, ctx, result_ty);
            emitter.instruction(Instruction::LocalSet(old));
            if box_idx.is_some() {
                emitter.instruction(Instruction::LocalGet(slot));
            }
            emitter.instruction(Instruction::LocalGet(old));
            emit_postfix_delta(emitter, ctx, op, result_ty);
            cast::emit_coerce_to_slot(emitter, ctx, result_ty, target_ty)?;
            if let Some(box_idx) = box_idx {
                emitter.instruction(Instruction::StructSet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
            } else {
                emitter.instruction(Instruction::LocalSet(slot));
            }
            emitter.instruction(Instruction::LocalGet(old));
        }
        crate::PostfixTarget::Global {
            mangled, target_ty, ..
        } => {
            let idx = ctx.symbols.global_idx(mangled).ok_or_else(|| {
                crate::codegen::internal_failure("Inferer guarantees the binding exists")
            })?;
            let old = emitter.add_anonymous_local(ctx.symbols.value_type(result_ty)?)?;
            crate::codegen::init_guard::emit_check(emitter, ctx, mangled);
            emitter.instruction(Instruction::GlobalGet(idx));
            // Reference globals start as null before module initialization.
            if let ValType::Ref(RefType {
                nullable: false, ..
            }) = ctx.symbols.value_type(target_ty)?
            {
                emitter.instruction(Instruction::RefAsNonNull);
            }
            let cast_info = cast_info_for(target_ty.clone(), result_ty.clone());
            cast::emit_narrowing_cast(emitter, ctx, &cast_info)?;
            emit_postfix_numeric(emitter, ctx, result_ty);
            emitter.instruction(Instruction::LocalSet(old));
            emitter.instruction(Instruction::LocalGet(old));
            emit_postfix_delta(emitter, ctx, op, result_ty);
            cast::emit_coerce_to_slot(emitter, ctx, result_ty, target_ty)?;
            emitter.instruction(Instruction::GlobalSet(idx));
            emitter.instruction(Instruction::LocalGet(old));
        }
        crate::PostfixTarget::Field {
            receiver,
            name,
            target_ty,
        } => {
            // A class instance resolves its payload slot statically, like every
            // other class field path. The by-name scan below would be wrong here:
            // a subclass that redeclares a parent field has two entries of that
            // name in the field-names array, and the scan finds the parent's.
            let receiver_ty = ctx
                .ta
                .source_type(*receiver)
                .map_err(crate::codegen::arena_failure)?
                .clone();
            if let Type::ClassRef { mangled, .. } = receiver_ty.peel() {
                emit_class_field_postfix(
                    emitter, ctx, *receiver, mangled, &name.name, op, target_ty,
                )?;
                return Ok(());
            }
            // Mirror AssignField + FieldAccess's vtable-dispatched calls.
            let intrinsics = ctx.symbols.intrinsic_type_indices().ok_or_else(|| {
                crate::codegen::internal_failure("intrinsics declared by codegen entry")
            })?;
            let tmp = emitter.add_anonymous_local(ctx.symbols.value_type(target_ty)?)?;
            emit_receiver(emitter, ctx, *receiver)?;
            let rcv_local =
                stash_receiver_as_object_shape(emitter, &receiver_ty, intrinsics.object_shape)?;
            emit_object_property_read(emitter, ctx, rcv_local, &name.name, target_ty)?;
            emit_postfix_numeric(emitter, ctx, target_ty);
            emitter.instruction(Instruction::LocalSet(tmp));
            emitter.instruction(Instruction::LocalGet(tmp));
            emit_postfix_delta(emitter, ctx, op, target_ty);
            let updated = emitter.add_anonymous_local(ctx.symbols.value_type(target_ty)?)?;
            emitter.instruction(Instruction::LocalSet(updated));
            emit_object_property_write_value(
                emitter,
                ctx,
                rcv_local,
                name,
                &ShapeArgument::Local {
                    slot: updated,
                    ty: target_ty.clone(),
                },
            )?;
            emitter.instruction(Instruction::LocalGet(tmp));
        }
        crate::PostfixTarget::Index {
            receiver, index, ..
        } => {
            emit_index_postfix(emitter, ctx, *receiver, *index, op)?;
        }
    };
    Ok(())
}

fn emit_index_postfix(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    op: crate::PostfixOp,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let is_uint8 = ctx
        .ta
        .source_type(receiver)
        .map_err(crate::codegen::arena_failure)?
        .peel()
        == &Type::Uint8Array;
    let result_ty = if is_uint8 {
        Type::Number
    } else {
        Type::Unknown
    };
    emit_expr(emitter, ctx, receiver)?;
    cast::emit_box(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(receiver)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    let receiver_local = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(receiver_local));
    emit_expr(emitter, ctx, index)?;
    let key = emitter.add_anonymous_local(
        ctx.symbols.value_type(
            &ctx.ta
                .try_expr(index)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?,
    )?;
    emitter.instruction(Instruction::LocalSet(key));
    let (backing, position, raw_type) = emit_index_location(
        emitter,
        ctx,
        receiver_local,
        key,
        &ctx.ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        is_uint8,
    )?;
    emitter.instruction(Instruction::LocalGet(backing));
    emitter.instruction(Instruction::LocalGet(position));
    if is_uint8 {
        emitter.instruction(Instruction::ArrayGetU(raw_type));
        emitter.instruction(Instruction::F64ConvertI32U);
    } else {
        emitter.instruction(Instruction::ArrayGet(raw_type));
    }
    emit_postfix_numeric(emitter, ctx, &result_ty);
    let old = emitter.add_anonymous_local(ctx.symbols.value_type(&result_ty)?)?;
    emitter.instruction(Instruction::LocalTee(old));
    emit_postfix_delta(emitter, ctx, op, &result_ty);
    let updated = emitter.add_anonymous_local(ctx.symbols.value_type(&result_ty)?)?;
    emitter.instruction(Instruction::LocalSet(updated));
    // GetValue and PutValue each convert the original property key. Conversion
    // can mutate the array, so reload the backing storage for the write.
    let (backing, position, _) = emit_index_location(
        emitter,
        ctx,
        receiver_local,
        key,
        &ctx.ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        is_uint8,
    )?;
    emitter.instruction(Instruction::LocalGet(backing));
    emitter.instruction(Instruction::LocalGet(position));
    emitter.instruction(Instruction::LocalGet(updated));
    if is_uint8 {
        emitter.instruction(Instruction::I32TruncSatF64S);
    }
    emitter.instruction(Instruction::ArraySet(raw_type));
    emitter.instruction(Instruction::LocalGet(old));
    Ok(())
}

fn emit_index_location(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: u32,
    key: u32,
    key_ty: &Type,
    is_uint8: bool,
) -> Result<(u32, u32, u32), crate::compiler_error::CompilerFailure> {
    let (container, raw, ty) = if is_uint8 {
        (
            ctx.symbols
                .uint8_array_type_idx()
                .ok_or_else(|| crate::codegen::internal_failure("Uint8Array declared"))?,
            ctx.symbols
                .raw_uint8_array_type_idx()
                .ok_or_else(|| crate::codegen::internal_failure("raw Uint8Array declared"))?,
            Type::Uint8Array,
        )
    } else {
        (
            ctx.symbols
                .array_type_idx()
                .ok_or_else(|| crate::codegen::internal_failure("Array declared"))?,
            ctx.symbols
                .raw_array_type_idx()
                .ok_or_else(|| crate::codegen::internal_failure("raw Array declared"))?,
            Type::Array(Box::new(Type::Unknown)),
        )
    };
    emitter.instruction(Instruction::LocalGet(receiver));
    crate::codegen::cast_check::emit_operation_cast_on_stack(emitter, ctx, &Type::Unknown, &ty)?;
    emitter.instruction(Instruction::LocalGet(key));
    emit_index_number(emitter, ctx, key_ty)?;
    let operand = stash_index_operand(emitter)?;
    let length = (!is_uint8)
        .then(|| stash_array_length(emitter, container))
        .transpose()?;
    emitter.instruction(Instruction::StructGet {
        struct_type_index: container,
        field_index: 1,
    });
    let backing = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw),
    }))?;
    emitter.instruction(Instruction::LocalSet(backing));
    let position = match length {
        Some(length) => emit_checked_index_with_length(emitter, ctx, length, operand)?,
        None => emit_checked_index(emitter, ctx, backing, operand)?,
    };
    Ok((backing, position, raw))
}

fn emit_postfix_numeric(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, ty: &Type) {
    ctx.latch(emit_postfix_numeric_checked(emitter, ctx, ty));
}

fn emit_postfix_numeric_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if matches!(ty, Type::Unknown) {
        let function = ctx
            .symbols
            .prelude_func_idx("__value_numeric")
            .ok_or_else(|| crate::codegen::internal_failure("dynamic update helper collected"))?;
        emitter.instruction(Instruction::Call(function));
    }

    Ok(())
}

fn emit_postfix_delta(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    op: crate::PostfixOp,
    ty: &Type,
) {
    ctx.latch(emit_postfix_delta_checked(emitter, ctx, op, ty));
}

fn emit_postfix_delta_checked(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    op: crate::PostfixOp,
    ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if matches!(ty, Type::Unknown) {
        let name = if matches!(op, crate::PostfixOp::Inc) {
            "inc"
        } else {
            "dec"
        };
        let function = ctx
            .symbols
            .prelude_func_idx(&format!("__value_{name}"))
            .ok_or_else(|| crate::codegen::internal_failure("dynamic update helper collected"))?;
        emitter.instruction(Instruction::Call(function));
        return Ok(());
    }
    if ty.is_bigint() {
        emit_bigint_pm_one(emitter, ctx, op);
        return Ok(());
    }
    emitter.instruction(Instruction::F64Const(Ieee64::from(1.0)));
    emitter.instruction(match op {
        crate::PostfixOp::Inc => Instruction::F64Add,
        crate::PostfixOp::Dec => Instruction::F64Sub,
        crate::PostfixOp::NonNullAssert => {
            return Err(crate::codegen::internal_failure(
                "non-null assertion is not PostfixUnary",
            ));
        }
    });

    Ok(())
}

/// `receiver.field++` on a class instance, leaving the *old* value on the stack.
/// Reads and writes the statically resolved payload slot, the same slot
/// `FieldAccess` and `AssignField` use. Accessor properties never reach here —
/// the typechecker rejects postfix on them, so there is no setter to dispatch.
#[allow(clippy::too_many_arguments)]
fn emit_class_field_postfix(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    receiver: ExprId,
    mangled: &crate::MangledName,
    field: &str,
    op: crate::PostfixOp,
    target_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let struct_idx = ctx.symbols.class_struct_type_idx(mangled).ok_or_else(|| {
        crate::codegen::internal_failure("class struct type recorded in classes::emit")
    })?;
    // A property with no slot is an accessor, which has no payload to
    // read-modify-write; `class_postfix_target` rejects those before codegen.
    let slot = ctx
        .symbols
        .class_field_slot(mangled, field)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "data-field slot recorded in classes::emit — accessors are rejected in infer",
            )
        })?;
    let fields_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object_fields),
    }))?;
    let old = emitter.add_anonymous_local(ctx.symbols.value_type(target_ty)?)?;
    let object = emitter.add_anonymous_local(
        ctx.symbols.value_type(
            ctx.ta
                .source_type(receiver)
                .map_err(crate::codegen::arena_failure)?,
        )?,
    )?;
    emit_receiver(emitter, ctx, receiver)?;
    emitter.instruction(Instruction::LocalTee(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: struct_idx,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalSet(fields_local));
    emitter.instruction(Instruction::LocalGet(fields_local));
    emitter.instruction(Instruction::I32Const(slot as i32));
    emitter.instruction(Instruction::ArrayGet(intrinsics.object_fields));
    if ctx
        .symbols
        .class_field_narrowing_check(mangled, field)
        .is_some()
    {
        crate::codegen::field_guards::check(emitter, ctx, object, mangled, field)?;
    }
    emit_class_field_slot_cast(emitter, ctx, mangled, field, target_ty)?;
    emit_postfix_numeric(emitter, ctx, target_ty);
    emitter.instruction(Instruction::LocalSet(old));
    emitter.instruction(Instruction::LocalGet(fields_local));
    emitter.instruction(Instruction::I32Const(slot as i32));
    emitter.instruction(Instruction::LocalGet(old));
    emit_postfix_delta(emitter, ctx, op, target_ty);
    crate::codegen::function_emitter::cast::emit_box(emitter, ctx, target_ty)?;
    emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
    emitter.instruction(Instruction::LocalGet(old));
    Ok(())
}

/// walk the typed `parts` list from `idx` to the end, with
/// the current receiver value already on the stack and `receiver_ty`
/// describing its static type. Each `optional: true` step opens an
/// `if (result … T | null)` that emits `null` on the short-circuit
/// branch and recurses on the non-null branch. The final receiver is
/// cast to `result_ty` so the chain's result Wasm type lines up at
/// the outer expression's slot.
fn emit_chain_parts(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    parts: &[crate::TypedChainPart],
    idx: usize,
    receiver_ty: &Type,
    result_ty: &Type,
    source_types: Option<&[Type]>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if source_types.is_some_and(|types| parts.len().checked_add(1) != Some(types.len())) {
        return Err(crate::codegen::internal_failure(
            "optional-chain source type count mismatch",
        ));
    }
    if idx == parts.len() {
        // A void-tailed chain left nothing on the stack — no value to
        // coerce, and `value_type(void)` has no lowering.
        if result_ty.is_void() {
            return Ok(());
        }
        // Receiver IS the chain's tail value; widen / box / re-cast
        // to the chain's outer `result_ty` slot. `emit_coerce_to_slot`
        // handles both directions (primitive → boxed for the widen
        // case, ref → ref for the same-shape case).
        crate::codegen::function_emitter::cast::emit_coerce_to_slot(
            emitter,
            ctx,
            receiver_ty,
            result_ty,
        )?;
        return Ok(());
    }
    let part = parts.get(idx).ok_or_else(|| {
        crate::codegen::internal_failure("optional-chain part index is out of bounds")
    })?;
    let next = idx
        .checked_add(1)
        .ok_or_else(|| crate::codegen::internal_failure("optional-chain index overflow"))?;
    let source_ty = source_types.and_then(|types| types.get(idx));
    let check_ty = source_types.and_then(|types| types.get(next));
    let saved_receiver = if matches!(
        part,
        crate::TypedChainPart::Field { .. }
            | crate::TypedChainPart::Index { .. }
            | crate::TypedChainPart::InterfaceProperty { .. }
    ) {
        let slot = emitter.add_anonymous_local(ctx.symbols.value_type(receiver_ty)?)?;
        emitter.instruction(Instruction::LocalTee(slot));
        Some(slot)
    } else if matches!(part, crate::TypedChainPart::NonNull { .. }) {
        emitter.call_receiver
    } else {
        None
    };
    let part_result_ty = part.result_ty().clone();
    // Only an optional step needs the receiver's slot, so don't ask for a
    // lowering a straight-line step never uses.
    let recv_val = part
        .is_optional()
        .then(|| ctx.symbols.value_type(receiver_ty))
        .transpose()?;
    // A receiver in a primitive slot cannot hold null, and `ref.is_null` does
    // not accept an f64/i32, so a `?.` on a non-nullable `number`/`boolean`
    // lowers as the straight-line access. The redundant-`?.` warning does not
    // gate this: it only fires on the chain's base, so the same shape mid-chain
    // arrives here undiagnosed.
    let _: () = if let Some(recv_val @ ValType::Ref(_)) = recv_val {
        // A void-tailed chain yields no value — empty block, and a no-op
        // null branch (and `value_type(void)` has no lowering).
        let is_void = result_ty.is_void();
        let block_ty = if is_void {
            BlockType::Empty
        } else {
            BlockType::Result(ctx.symbols.value_type(result_ty)?)
        };
        let tmp = emitter.add_anonymous_local(recv_val)?;
        emitter.instruction(Instruction::LocalTee(tmp));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(block_ty);
        // Null branch: produce a `null` cast to the chain's result
        // type. `emit_null_for_result_ty` handles the per-Wasm-shape
        // null literal — for ref-typed results that's `ref.null T`;
        // for an `unknown`-typed result it's `ref.null $Object`.
        if !is_void {
            emit_null_for_chain_result(emitter, ctx, result_ty)?;
        }
        emitter.emit_else();
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefAsNonNull);
        emit_chain_operation(emitter, ctx, part, receiver_ty, source_ty, check_ty)?;
        emitter.call_receiver = saved_receiver;
        emit_chain_parts(
            emitter,
            ctx,
            parts,
            next,
            &part_result_ty,
            result_ty,
            source_types,
        )?;
        emitter.emit_end();
    } else {
        emit_chain_operation(emitter, ctx, part, receiver_ty, source_ty, check_ty)?;
        emitter.call_receiver = saved_receiver;
        emit_chain_parts(
            emitter,
            ctx,
            parts,
            next,
            &part_result_ty,
            result_ty,
            source_types,
        )?;
    };
    Ok(())
}

fn emit_chain_operation(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    part: &crate::TypedChainPart,
    receiver_ty: &Type,
    source_ty: Option<&Type>,
    check_ty: Option<&Type>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if let crate::TypedChainPart::MethodCall {
        iface,
        name,
        result_ty,
        span,
        ..
    } = part
        && !iface.as_str().starts_with("submilli:")
        && let Some(args) = ctx.ta.authored_call_arguments(*span)
    {
        cast::emit_box(emitter, ctx, receiver_ty)?;
        emit_live_member_on_stack(emitter, ctx, iface, &name.name, Some(args))?;
        if result_ty.is_void() {
            emitter.instruction(Instruction::Drop);
        } else {
            cast::emit_cast_to(emitter, ctx, result_ty)?;
        }
        return Ok(());
    }
    if receiver_ty == &Type::Unknown {
        match part {
            crate::TypedChainPart::MethodCall {
                iface,
                name,
                args,
                result_ty,
                span,
                ..
            } if crate::codegen::runtime_values::dynamic_member_interface(iface) => {
                let args = ctx.ta.authored_call_arguments(*span).unwrap_or(args);
                emit_live_member_on_stack(emitter, ctx, iface, &name.name, Some(args))?;
                if result_ty.is_void() {
                    emitter.instruction(Instruction::Drop);
                }
                return Ok(());
            }
            crate::TypedChainPart::InterfaceProperty { iface, name, .. }
                if crate::codegen::runtime_values::dynamic_member_interface(iface) =>
            {
                emit_live_member_on_stack(emitter, ctx, iface, &name.name, None)?;
                return Ok(());
            }
            _ => {}
        }
    }
    let operation_ty =
        crate::typechecker::infer::narrowing::strip_null(source_ty.unwrap_or(receiver_ty));
    if !matches!(
        part,
        crate::TypedChainPart::Index { .. }
            | crate::TypedChainPart::Call { .. }
            | crate::TypedChainPart::NonNull { .. }
    ) {
        let receiver_slot = ctx.symbols.value_type(receiver_ty)?;
        if receiver_slot == ctx.symbols.value_type(&operation_ty)? {
            if matches!(receiver_slot, ValType::Ref(_)) {
                cast::emit_cast_to(emitter, ctx, &operation_ty)?;
            }
        } else {
            crate::codegen::cast_check::emit_operation_cast_on_stack(
                emitter,
                ctx,
                receiver_ty,
                &operation_ty,
            )?;
        }
    }
    emit_chain_access(
        emitter,
        ctx,
        part,
        &operation_ty,
        check_ty.unwrap_or(part.result_ty()),
    )?;
    Ok(())
}

fn emit_live_member_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    iface: &crate::MangledName,
    name: &str,
    args: Option<&[ExprId]>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    super::emit_const_string_by_text(emitter, ctx, name)?;
    super::emit_const_string_by_text(emitter, ctx, iface.as_str())?;
    let helper = if args.is_some() {
        "__value_member"
    } else {
        "__value_property"
    };
    emitter.instruction(Instruction::Call(
        ctx.symbols
            .prelude_func_idx(helper)
            .ok_or_else(|| crate::codegen::internal_failure("live member helper collected"))?,
    ));
    let Some(args) = args else {
        return Ok(());
    };
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .prelude_global_idx("array_vtable")
            .ok_or_else(|| crate::codegen::internal_failure("array vtable"))?,
    ));
    for &arg in args {
        emit_expr(emitter, ctx, arg)?;
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(arg)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intr.raw_array,
        array_size: crate::codegen::wasm_u32(args.len())?,
    });
    emitter.instruction(Instruction::I32Const(i32::try_from(args.len()).map_err(
        |_| crate::codegen::internal_failure("argument array too large"),
    )?));
    emitter.instruction(Instruction::StructNew(intr.array));
    emitter.instruction(Instruction::Call(
        ctx.symbols
            .prelude_func_idx("__value_invoke")
            .ok_or_else(|| crate::codegen::internal_failure("live invocation collected"))?,
    ));
    Ok(())
}

/// Intrinsic length properties have no host getter. Arrays store their logical
/// length; strings derive it from their packed code-unit backing.
fn inline_length_struct_idx(
    ctx: &CodegenCtx<'_>,
    iface: &crate::MangledName,
    prop: &str,
) -> Result<Option<u32>, crate::compiler_error::CompilerFailure> {
    let key = crate::mangle::extend(iface, prop);
    if !ctx.symbols.is_intrinsic_member(&key) {
        return Ok(None);
    }
    let index = if *iface == crate::mangle::prelude("String") && prop == "length" {
        ctx.symbols.string_type_idx()
    } else if *iface == crate::mangle::prelude("Array") && prop == "length" {
        ctx.symbols.array_type_idx()
    } else {
        return Err(crate::codegen::internal_failure(format!(
            "no inline lowering for intrinsic member `{}`",
            key.as_str()
        )));
    };
    index
        .map(Some)
        .ok_or_else(|| crate::codegen::internal_failure("length receiver intrinsic missing"))
}

/// Receiver (concrete `$string`/`$Array`) on stack → its element count as f64.
fn emit_inline_length(emitter: &mut FunctionEmitter<'_>, ctx: &CodegenCtx<'_>, struct_idx: u32) {
    let is_array = ctx.symbols.array_type_idx() == Some(struct_idx);
    emitter.instruction(Instruction::StructGet {
        struct_type_index: struct_idx,
        field_index: if is_array { 2 } else { 1 },
    });
    if !is_array {
        emitter.instruction(Instruction::ArrayLen);
    }
    emitter.instruction(Instruction::F64ConvertI32U);
}

/// Emit a `null` value sized for the chain's outer result type.
/// Always a ref-typed null because the chain's `result_ty` is
/// `union(tail, Null)` which lowers to `(ref null $Object)` (or a
/// narrower nullable ref if every member shares one heap type).
fn emit_null_for_chain_result(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let val = ctx.symbols.value_type(result_ty)?;
    match val {
        ValType::Ref(RefType {
            heap_type: HeapType::Concrete(idx),
            ..
        }) => {
            emitter.instruction(Instruction::RefNull(HeapType::Concrete(idx)));
        }
        ValType::Ref(RefType {
            heap_type: HeapType::Abstract { ty, .. },
            ..
        }) => {
            emitter.instruction(Instruction::RefNull(HeapType::Abstract {
                shared: false,
                ty,
            }));
        }
        // The chain's outer type carries `| Null`, so every member
        // path lowers to a ref-typed slot. Hitting a primitive here
        // means the chain wasn't actually nullable in the first
        // place — infer's redundant-`?.` warning would have fired.
        _ => {
            return Err(crate::codegen::internal_failure(format!(
                "optional chain result_ty `{result_ty}` lowers to a primitive — \
             chain must be ref-typed"
            )));
        }
    };
    Ok(())
}

/// Narrows a class field's raw payload slot value, on the stack, to the type the
/// read is typed at. Ordinarily that is the plain representation cast; a field
/// carrying a [`crate::FieldNarrowingCheck`] is read through its guard instead.
fn emit_class_field_slot_cast(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    class: &crate::MangledName,
    field: &str,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_class_field_slot_cast_as(emitter, ctx, class, field, result_ty, result_ty)?;
    Ok(())
}

fn emit_class_field_slot_cast_as(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    class: &crate::MangledName,
    field: &str,
    result_ty: &Type,
    check_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match ctx.symbols.class_field_narrowing_check(class, field) {
        Some(check) => {
            crate::codegen::cast_check::emit_narrowed_field_read_as(
                emitter, ctx, check, result_ty, check_ty,
            )?;
        }
        None => crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, result_ty)?,
    };
    Ok(())
}

fn emit_object_spread(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    sources: &[TypedObjectMember],
    shape: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let merge = ctx
        .symbols
        .prelude_func_idx("ObjectConstructor##spread")
        .ok_or_else(|| crate::codegen::internal_failure("spread helper collected"))?;
    let null_object = Instruction::RefNull(HeapType::Concrete(intrinsics.object));
    emitter.instruction(null_object.clone());
    for (index, source) in sources.iter().enumerate() {
        let final_shape = (index + 1 == sources.len()).then_some(shape);
        // A source `runtime_values` widened may no longer hold what its
        // narrowed type said; `emit_receiver` checks it against that type,
        // which the stash and the mask then read in place of `unknown`.
        emit_receiver(emitter, ctx, source.expr_id())?;
        let narrowed_ty = ctx
            .ta
            .source_type(source.expr_id())
            .map_err(crate::codegen::arena_failure)?;
        // A source that may hold `null` or a falsy primitive copies nothing
        // when it does: the merge then takes no source, which still applies
        // the final shape.
        let accumulator = if may_hold_non_object(narrowed_ty) {
            Some(emit_object_source_test(
                emitter,
                ctx,
                narrowed_ty,
                intrinsics.object,
                intrinsics.object_shape,
            )?)
        } else {
            None
        };
        let source_local =
            stash_receiver_as_object_shape(emitter, narrowed_ty, intrinsics.object_shape)?;
        emitter.instruction(Instruction::LocalGet(source_local));
        emit_spread_shape_argument(emitter, ctx, final_shape, &null_object);
        if matches!(source, TypedObjectMember::Spread { by_name: true, .. }) {
            emit_spread_mask(emitter, ctx, source_local, source.expr_id(), shape)?;
        } else {
            emitter.instruction(null_object.clone());
        }
        emitter.instruction(Instruction::Call(merge));
        if let Some(accumulator) = accumulator {
            emitter.emit_else();
            emitter.instruction(Instruction::LocalGet(accumulator));
            emitter.instruction(null_object.clone());
            emit_spread_shape_argument(emitter, ctx, final_shape, &null_object);
            emitter.instruction(null_object.clone());
            emitter.instruction(Instruction::Call(merge));
            emitter.emit_end();
        }
    }
    Ok(())
}

/// The merge's shape argument: the result shape on the last merge, which
/// restores its optional markers, and null before it.
fn emit_spread_shape_argument(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    final_shape: Option<&Type>,
    null_object: &Instruction<'static>,
) {
    match final_shape {
        Some(shape) => emit_spread_shape(emitter, ctx, shape),
        None => emitter.instruction(null_object.clone()),
    }
}

/// Whether a spread source of type `ty` may hold a value with no fields to
/// copy: `null`, or a falsy primitive such as the `false` of `c && { … }`.
fn may_hold_non_object(ty: &Type) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().any(may_hold_non_object),
        Type::Null
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Number
        | Type::NumberLiteral(_)
        | Type::String
        | Type::StringLiteral(_) => true,
        _ => false,
    }
}

/// With the accumulator and then the source on the stack, opens an `if` on
/// whether the source is an object, whose result is the merged object. Inside
/// it, the accumulator and the source, as an `$ObjectShape`, are on the stack.
/// Returns the local holding the accumulator, for the `else` arm.
fn emit_object_source_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source_ty: &Type,
    object: u32,
    object_shape: u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let source_type = match ctx.symbols.value_type(source_ty)? {
        ValType::Ref(reference) => ValType::Ref(RefType {
            nullable: true,
            ..reference
        }),
        other => other,
    };
    let source = emitter.add_anonymous_local(source_type)?;
    emitter.instruction(Instruction::LocalSet(source));
    let accumulator = emitter.add_anonymous_local(object_ref(object))?;
    emitter.instruction(Instruction::LocalSet(accumulator));
    emitter.instruction(Instruction::LocalGet(source));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        object_shape,
    )));
    emitter.emit_if(BlockType::Result(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(object_shape),
    })));
    emitter.instruction(Instruction::LocalGet(accumulator));
    emitter.instruction(Instruction::LocalGet(source));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        object_shape,
    )));
    Ok(accumulator)
}

/// Union sources may carry a known field with an incompatible hidden value.
/// Preserve the checked-field contract for those names without discarding
/// runtime fields outside the source's static view.
fn emit_spread_mask(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source: u32,
    source_expr: crate::ExprId,
    shape: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let checked = ctx
        .ta
        .spread_mask_fields
        .get(&source_expr)
        .ok_or_else(|| crate::codegen::internal_failure("by-name spread fields recorded"))?;
    // A name an object rest leaves out is masked whatever its value; any other
    // is masked when its value isn't of the field's type.
    let omitted = ctx.ta.spread_omitted_fields.get(&source_expr);
    let fields: Vec<(&String, Option<&Type>)> = checked
        .iter()
        .map(|(name, ty)| (name, Some(ty)))
        .chain(omitted.into_iter().flatten().map(|name| (name, None)))
        .collect();
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let vtable = ctx
        .symbols
        .vtable_global_idx(shape)
        .ok_or_else(|| crate::codegen::internal_failure("spread shape vtable collected"))?;
    emitter.instruction(Instruction::GlobalGet(vtable));
    for (name, _) in &fields {
        let global = ctx
            .symbols
            .field_name_string_global_idx(name)
            .ok_or_else(|| crate::codegen::internal_failure("spread name collected"))?;
        emitter.instruction(Instruction::GlobalGet(global));
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.field_names,
        array_size: crate::codegen::wasm_u32(fields.len())?,
    });
    for (name, ty) in fields.iter().copied() {
        let global = ctx
            .symbols
            .field_name_string_global_idx(name)
            .ok_or_else(|| crate::codegen::internal_failure("spread name collected"))?;
        let Some(ty) = ty else {
            emitter.instruction(Instruction::GlobalGet(global));
            continue;
        };
        let index = emitter.add_anonymous_local(ValType::I32)?;
        let value = emitter.add_anonymous_local(object_ref(intrinsics.object))?;
        emit_object_field_index_by_name(emitter, ctx, source, global);
        emitter.instruction(Instruction::LocalTee(index));
        emitter.instruction(Instruction::I32Const(0));
        emitter.instruction(Instruction::I32GeS);
        emitter.emit_if(BlockType::Result(ValType::I32));
        emit_field_slot_get(emitter, intrinsics, source, index);
        emitter.instruction(Instruction::LocalSet(value));
        emit_field_holds_value_of(emitter, ctx, source, index, value, ty)?;
        emitter.emit_else();
        emitter.instruction(Instruction::I32Const(0));
        emitter.emit_end();
        emitter.emit_if(BlockType::Result(object_ref(intrinsics.object)));
        emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
        emitter.emit_else();
        emitter.instruction(Instruction::GlobalGet(global));
        emitter.emit_end();
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.object_fields,
        array_size: crate::codegen::wasm_u32(fields.len())?,
    });
    emitter.instruction(Instruction::RefNull(HeapType::ANY));
    emitter.instruction(Instruction::StructNew(intrinsics.object_shape));
    Ok(())
}

/// The final merge restores optional markers and writable absent slots from
/// the inferred result shape, while retaining unnamed runtime fields.
fn emit_spread_shape(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, shape: &Type) {
    ctx.latch(emit_spread_shape_checked(emitter, ctx, shape));
}

fn emit_spread_shape_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    shape: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Type::Object { fields, .. } = shape else {
        return Err(crate::codegen::internal_failure("structural spread shape"));
    };
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let names: Vec<_> = fields
        .iter()
        .map(|(name, field)| crate::codegen::field_names::FieldName {
            name: name.clone(),
            optional: field.optional,
            is_accessor: false,
            is_private: false,
        })
        .collect();
    let names_global = ctx
        .symbols
        .field_names_global_idx(&names)
        .ok_or_else(|| crate::codegen::internal_failure("spread shape names collected"))?;
    let vtable = ctx
        .symbols
        .vtable_global_idx(shape)
        .ok_or_else(|| crate::codegen::internal_failure("spread shape vtable collected"))?;
    emitter.instruction(Instruction::GlobalGet(vtable));
    emitter.instruction(Instruction::GlobalGet(names_global));
    emitter.instruction(Instruction::I32Const(
        crate::codegen::wasm_u32(fields.len())? as i32,
    ));
    emitter.instruction(Instruction::ArrayNewDefault(intrinsics.object_fields));
    emitter.instruction(Instruction::RefNull(HeapType::ANY));
    emitter.instruction(Instruction::StructNew(intrinsics.object_shape));

    Ok(())
}

/// Coerces a field-read receiver already on the stack to `(ref $ObjectShape)`,
/// which is what the field-name scan takes. Only receivers that lower to the
/// universal `$Object` slot need it: an `InterfaceRef`, and any union carrying a
/// nominal member (`value_type`'s mixed-union arm). A union of object shapes
/// already lowers to an `$ObjectShape` subtype, and so does a class instance —
/// its struct's header slots 0-2 are the `$ObjectShape` prefix.
/// Coerce the receiver on the stack into an anonymous non-null
/// `(ref $ObjectShape)` local, and return the slot.
///
/// Every property path — read, write, and read-modify-write — needs the
/// receiver in a local it can push more than once, and every one of them needs
/// the same coercion first: the helpers take `(ref $ObjectShape)` non-null,
/// while an interface-typed receiver (or a union with a non-object member)
/// lowers to `(ref null $Object)`. A receiver that is already a narrower
/// `$ObjectShape` subtype needs nothing — Wasm subtyping accepts it.
pub(super) fn stash_receiver_as_object_shape(
    emitter: &mut FunctionEmitter<'_>,
    receiver_ty: &Type,
    object_shape: u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    coerce_field_receiver_to_object_shape(emitter, receiver_ty, object_shape);
    let rcv_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(object_shape),
    }))?;
    emitter.instruction(Instruction::LocalSet(rcv_local));
    Ok(rcv_local)
}

fn coerce_field_receiver_to_object_shape(
    emitter: &mut FunctionEmitter<'_>,
    receiver_ty: &Type,
    object_shape: u32,
) {
    let universal = match receiver_ty.peel() {
        Type::InterfaceRef { .. } | Type::Unknown => true,
        Type::Union(members) => members
            .iter()
            .any(|m| !matches!(m.peel(), Type::Object { .. })),
        _ => false,
    };
    if universal {
        emitter.instruction(Instruction::RefAsNonNull);
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            object_shape,
        )));
    }
}

/// Emit one chain step's access on a receiver already on the stack.
/// Mirrors the non-chain `TypedExprKind::FieldAccess` / `IndexAccess`
/// codegen at lines 391-457 / 560+ above, with the leading
/// `emit_expr(receiver)` elided. After this returns, the part's
/// result is on the stack with `part.result_ty`'s Wasm shape.
fn emit_chain_access(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    part: &crate::TypedChainPart,
    receiver_ty: &Type,
    check_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match part {
        crate::TypedChainPart::Field {
            name, result_ty, ..
        } => {
            if ctx.ta.has_string_index(receiver_ty) {
                let Some(symbol) = ctx.require(
                    ctx.symbols.field_name_string_global_idx(&name.name),
                    "property name collected",
                ) else {
                    return Ok(());
                };
                emitter.instruction(Instruction::GlobalGet(symbol));
                let Some(symbol) = ctx.require(
                    ctx.symbols.prelude_func_idx("ObjectConstructor##getField"),
                    "record read imported",
                ) else {
                    return Ok(());
                };
                emitter.instruction(Instruction::Call(symbol));
                crate::codegen::cast_check::emit_checked_cast_on_stack(
                    emitter,
                    ctx,
                    &Type::Unknown,
                    check_ty,
                )?;
                cast::emit_coerce_to_slot(emitter, ctx, check_ty, result_ty)?;
                return Ok(());
            }
            let intrinsics = ctx.symbols.intrinsic_type_indices().ok_or_else(|| {
                crate::codegen::internal_failure("intrinsics declared by codegen entry")
            })?;
            // Class receiver: resolve the slot statically, mirroring the
            // non-optional `FieldAccess` path. The name scan below would read a
            // null for an accessor property, which backs no payload slot.
            if let Type::ClassRef { mangled, .. } = receiver_ty.peel() {
                let Some(slot) = ctx.symbols.class_field_slot(mangled, &name.name) else {
                    let getter = crate::codegen::classes::accessor_getter_name(&name.name);
                    emit_class_vtable_dispatch(emitter, ctx, mangled, &getter, &[], result_ty)?;
                    return Ok(());
                };
                let struct_idx = ctx.symbols.class_struct_type_idx(mangled).ok_or_else(|| {
                    crate::codegen::internal_failure("class struct type recorded in classes::emit")
                })?;
                let object = emitter.add_anonymous_local(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(struct_idx),
                }))?;
                emitter.instruction(Instruction::LocalTee(object));
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: struct_idx,
                    field_index: 2,
                });
                emitter.instruction(Instruction::I32Const(slot as i32));
                emitter.instruction(Instruction::ArrayGet(intrinsics.object_fields));
                if ctx
                    .symbols
                    .class_field_narrowing_check(mangled, &name.name)
                    .is_some()
                {
                    crate::codegen::field_guards::check(emitter, ctx, object, mangled, &name.name)?;
                }
                emit_class_field_slot_cast_as(
                    emitter, ctx, mangled, &name.name, result_ty, check_ty,
                )?;
                return Ok(());
            }
            let rcv_local =
                stash_receiver_as_object_shape(emitter, receiver_ty, intrinsics.object_shape)?;
            emit_object_property_read_as(emitter, ctx, rcv_local, &name.name, result_ty, check_ty)?;
        }
        crate::TypedChainPart::Index { idx, result_ty, .. } => {
            if receiver_ty.is_structural_object() {
                emit_expr(emitter, ctx, *idx)?;
                crate::codegen::cast_check::emit_operation_cast_on_stack(
                    emitter,
                    ctx,
                    &ctx.ta
                        .try_expr(*idx)
                        .map_err(crate::codegen::arena_failure)?
                        .ty,
                    &Type::String,
                )?;
                let Some(symbol) = ctx.require(
                    ctx.symbols.prelude_func_idx("ObjectConstructor##getField"),
                    "record read imported",
                ) else {
                    return Ok(());
                };
                emitter.instruction(Instruction::Call(symbol));
                crate::codegen::cast_check::emit_checked_cast_on_stack(
                    emitter,
                    ctx,
                    &Type::Unknown,
                    check_ty,
                )?;
                cast::emit_coerce_to_slot(emitter, ctx, check_ty, result_ty)?;
            } else if receiver_ty.peel() == &Type::Uint8Array {
                emit_uint8_index_with_receiver_on_stack(emitter, ctx, *idx)?;
            } else {
                let source_ty = match receiver_ty.peel() {
                    Type::Array(element) => element.as_ref(),
                    Type::Tuple(elements) => match &ctx
                        .ta
                        .try_expr(*idx)
                        .map_err(crate::codegen::arena_failure)?
                        .kind
                    {
                        TypedExprKind::Number(index) => {
                            elements.get(*index as usize).unwrap_or(result_ty)
                        }
                        _ => result_ty,
                    },
                    _ => result_ty,
                };
                emit_bounds_checked_index_with_receiver_on_stack(emitter, ctx, *idx, source_ty)?;
                if source_ty != result_ty {
                    crate::codegen::cast_check::emit_checked_cast_on_stack(
                        emitter, ctx, source_ty, result_ty,
                    )?;
                }
            }
        }
        crate::TypedChainPart::InterfaceProperty { iface, name, .. } => {
            // same lookup the non-chain
            // `InterfacePropertyAccess` arm uses. The chain has the
            // receiver on stack already; the wrapper takes
            // `(ref $Object)` non-null, so coerce only when the static
            // receiver type is `InterfaceRef`.
            //
            // The getter returns the property's declared type, which is
            // exactly this part's `result_ty` — `?.` adds `| null` once,
            // to the whole chain, never to a step's own result. So the
            // value already sits in the slot the next step expects.
            if let Some(struct_idx) = inline_length_struct_idx(ctx, iface, &name.name)? {
                emit_inline_length(emitter, ctx, struct_idx);
                return Ok(());
            }
            let key = crate::mangle::extend(iface, &name.name);
            let func_idx = ctx.symbols.func_idx(&key).ok_or_else(|| {
                crate::codegen::internal_failure(
                    "interface property getter recorded during the dependency-import pass",
                )
            })?;
            if matches!(receiver_ty.peel(), Type::InterfaceRef { .. }) {
                emitter.instruction(Instruction::RefAsNonNull);
            }
            emitter.instruction(Instruction::Call(func_idx));
        }
        crate::TypedChainPart::Call { args, .. } => {
            // receiver on stack is the closure value
            // (`(ref $closure_N)` after `emit_chain_parts`'s
            // strip-null cast). Helper extracts env + funcref and
            // call_refs, unboxing the erased return to the function's
            // declared `ret` — which is this part's `result_ty`, so the
            // value already sits in the slot the next step expects.
            emit_indirect_closure_call_with_receiver_on_stack(
                emitter,
                ctx,
                receiver_ty,
                args,
                part.result_ty(),
            )?;
        }
        crate::TypedChainPart::MethodCall {
            iface,
            name,
            args,
            result_ty,
            ..
        } => {
            // dispatch through the same three paths the
            // non-chain `emit_method_call` uses (Direct/Static wrapper,
            // built-in vtable slot, shape-based fallback) — the helper
            // assumes the receiver is on the stack. `result_ty` is the
            // method's substituted return type, not the chain's
            // short-circuit union (`infer_optional_chain` adds `| null`
            // once, to the whole chain), so it is `call_ret_ty` verbatim:
            // a nullable `T | null` from `at` is the wrapper's real
            // return and must reach the next step still nullable.
            emit_method_call_with_receiver_on_stack(
                emitter,
                ctx,
                receiver_ty,
                iface,
                &name.name,
                args,
                None,
                None,
                result_ty,
            )?;
        }
        crate::TypedChainPart::NonNull { result_ty, .. } => {
            crate::codegen::cast_check::emit_non_null_assert_on_stack(
                emitter,
                ctx,
                receiver_ty,
                result_ty,
            )?;
        }
    };
    Ok(())
}

/// Emit the load for one captured-binding value at the closure-creation
/// site. The captured slot in the *outer* scope already has the right
/// representation for the env field — boxed captures store
/// `(ref $box_T)`, unboxed (`const`) captures store `T` — so we just
/// `local.get` the slot.
///
/// Transitive captures (the outer scope is itself a closure body)
/// work uniformly: the outer closure's prologue copied its captures
/// into ordinary Wasm locals, so the lookup here resolves to a
/// FunctionLocal slot with the right type either way.
fn emit_captured_load(
    emitter: &mut FunctionEmitter,
    _ctx: &CodegenCtx,
    c: &crate::CapturedVar,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let slot = emitter.local_slot(&c.name.name).ok_or_else(|| {
        crate::codegen::internal_failure("captured binding is missing from the outer scope")
    })?;
    emitter.instruction(Instruction::LocalGet(slot));
    Ok(())
}

fn emit_binary(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: BinOp,
    lhs: ExprId,
    rhs: ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let admits_never_operand = matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Pow
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
    );
    if admits_never_operand && try_emit_unreachable_for_never_operand(emitter, ctx, &[lhs, rhs])? {
        return Ok(());
    }
    let _: () = match op {
        // `+` dispatches on the result type the typechecker chose: numeric
        // operands → `f64.add`, string operands → `string_concat` from the
        // prelude. Mixed-type or other operand combinations were rejected
        // by the inferer.
        BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::UnsignedShr => {
            let name = op.bitwise_name().ok_or_else(|| {
                crate::codegen::internal_failure("missing bitwise host operation")
            })?;
            emit_bitwise_host(emitter, ctx, name, &[lhs, rhs], result_ty)?;
        }
        // A template of constants has a string literal type: concatenate it as
        // a `string`.
        BinOp::Add => match &result_ty.widen_literal() {
            Type::Number => {
                emit_primitive_operand(emitter, ctx, lhs)?;
                emit_primitive_operand(emitter, ctx, rhs)?;
                emitter.instruction(Instruction::F64Add);
            }
            Type::String => {
                emit_primitive_operand(emitter, ctx, lhs)?;
                emit_primitive_operand(emitter, ctx, rhs)?;
                let idx = ctx
                    .symbols
                    .prelude_func_idx("string_concat")
                    .ok_or_else(|| {
                        crate::codegen::internal_failure("string_concat imported from prelude")
                    })?;
                emitter.instruction(Instruction::Call(idx));
            }
            // `bigint + bigint` → inline host call.
            Type::BigInt => emit_bigint_binop_inline(emitter, ctx, lhs, rhs, "add")?,
            other => {
                return Err(crate::codegen::internal_failure(format!(
                    "typechecker rejects `+` for {other:?}"
                )));
            }
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div => {
            // bigint operands route to inline host calls.
            if result_ty.is_bigint() {
                let name = match op {
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    BinOp::Div => "div",
                    _ => {
                        return Err(crate::codegen::internal_failure(
                            "invalid compiler emission dispatch",
                        ));
                    }
                };
                emit_bigint_binop_inline(emitter, ctx, lhs, rhs, name)?;
                return Ok(());
            }
            if !matches!(result_ty, Type::Number) {
                return Err(crate::codegen::internal_failure(
                    "numeric operator has a non-number result type",
                ));
            }
            emit_primitive_operand(emitter, ctx, lhs)?;
            emit_primitive_operand(emitter, ctx, rhs)?;
            let inst = match op {
                BinOp::Sub => Instruction::F64Sub,
                BinOp::Mul => Instruction::F64Mul,
                BinOp::Div => Instruction::F64Div,
                _ => {
                    return Err(crate::codegen::internal_failure(
                        "invalid compiler emission dispatch",
                    ));
                }
            };
            emitter.instruction(inst);
        }
        BinOp::Pow => {
            // exponentiation. BigInt routes to the
            // `submilli:bigint.pow` host call (which validates the
            // exponent is non-negative and fits in u32, else traps).
            // Number routes to the prelude-host `Math#pow` function.
            if result_ty.is_bigint() {
                emit_bigint_binop_inline(emitter, ctx, lhs, rhs, "pow")?;
                return Ok(());
            }
            if !matches!(result_ty, Type::Number) {
                return Err(crate::codegen::internal_failure(
                    "numeric operator has a non-number result type",
                ));
            }
            emit_primitive_operand(emitter, ctx, lhs)?;
            emit_primitive_operand(emitter, ctx, rhs)?;
            let idx = ctx
                .symbols
                .func_idx(&crate::runtime::prelude::math::math_key("pow"))
                .ok_or_else(|| crate::codegen::internal_failure("Math#pow host import recorded"))?;
            emitter.instruction(Instruction::Call(idx));
        }
        BinOp::Rem => {
            // `bigint % bigint` → inline host call.
            if result_ty.is_bigint() {
                emit_bigint_binop_inline(emitter, ctx, lhs, rhs, "mod")?;
                return Ok(());
            }
            // Wasm has no `f64.rem`; lower to JS-truncation modulo using
            // `a - trunc(a / b) * b`. Two scratch f64 locals so each operand
            // is evaluated exactly once (preserves side-effect order).
            let a = emitter.add_anonymous_local(ValType::F64)?;
            let b = emitter.add_anonymous_local(ValType::F64)?;
            emit_primitive_operand(emitter, ctx, lhs)?;
            emitter.instruction(Instruction::LocalSet(a));
            emit_primitive_operand(emitter, ctx, rhs)?;
            emitter.instruction(Instruction::LocalSet(b));
            // Build `a - trunc(a/b)*b` on the stack.
            emitter.instruction(Instruction::LocalGet(a));
            emitter.instruction(Instruction::LocalGet(a));
            emitter.instruction(Instruction::LocalGet(b));
            emitter.instruction(Instruction::F64Div);
            emitter.instruction(Instruction::F64Trunc);
            emitter.instruction(Instruction::LocalGet(b));
            emitter.instruction(Instruction::F64Mul);
            emitter.instruction(Instruction::F64Sub);
        }
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            // bigint → inline `bigint.cmp` host call; number → f64 ops;
            // string → `string_cmp` (signed lexicographic diff) vs 0.
            let operand_ty = ctx
                .ta
                .try_expr(lhs)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .primitive_behavior()
                .clone();
            if operand_ty.is_bigint() {
                emit_bigint_cmp_inline(emitter, ctx, lhs, rhs, op)?;
                return Ok(());
            }
            emit_primitive_operand(emitter, ctx, lhs)?;
            emit_primitive_operand(emitter, ctx, rhs)?;
            if matches!(operand_ty, Type::String | Type::StringLiteral(_)) {
                let idx = ctx.symbols.prelude_func_idx("string_cmp").ok_or_else(|| {
                    crate::codegen::internal_failure("string_cmp imported from prelude")
                })?;
                emitter.instruction(Instruction::Call(idx));
                emitter.instruction(Instruction::I32Const(0));
                emitter.instruction(match op {
                    BinOp::Lt => Instruction::I32LtS,
                    BinOp::Gt => Instruction::I32GtS,
                    BinOp::Le => Instruction::I32LeS,
                    BinOp::Ge => Instruction::I32GeS,
                    _ => {
                        return Err(crate::codegen::internal_failure(
                            "invalid compiler emission dispatch",
                        ));
                    }
                });
            } else {
                emitter.instruction(match op {
                    BinOp::Lt => Instruction::F64Lt,
                    BinOp::Gt => Instruction::F64Gt,
                    BinOp::Le => Instruction::F64Le,
                    BinOp::Ge => Instruction::F64Ge,
                    _ => {
                        return Err(crate::codegen::internal_failure(
                            "invalid compiler emission dispatch",
                        ));
                    }
                });
            }
        }
        BinOp::Eq | BinOp::NotEq => emit_equality(emitter, ctx, op, lhs, rhs)?,
        BinOp::And | BinOp::Or => emit_logical(emitter, ctx, op, lhs, rhs, result_ty)?,
        BinOp::In => emit_in_operator(emitter, ctx, lhs, rhs)?,
        // infer lifts `Binary { op: NullishCoalesce,.. }`
        // into the dedicated `TypedExprKind::NullishCoalesce` node, so
        // this arm is unreachable in well-typed input.
        BinOp::NullishCoalesce => {
            return Err(crate::codegen::internal_failure(
                "BinOp::NullishCoalesce should be lifted into TypedExprKind::NullishCoalesce by infer",
            ));
        }
    };
    Ok(())
}

// Substantial recursive arms live outside the dispatcher so their temporaries
// do not enlarge every arithmetic expression's stack frame.
fn emit_equality(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: BinOp,
    lhs: ExprId,
    rhs: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Plan 75.8: when either operand is the `null` literal,
    // emit the non-null side followed by `ref.is_null`
    // (negated for `NotEq`). This covers `x === null` /
    // `x !== null` against any nullable value, the primary
    // null-narrowing predicate. Both-null collapses to a
    // constant.
    let lhs_is_null_lit = matches!(
        ctx.ta
            .try_expr(lhs)
            .map_err(crate::codegen::arena_failure)?
            .kind,
        TypedExprKind::Null
    );
    let rhs_is_null_lit = matches!(
        ctx.ta
            .try_expr(rhs)
            .map_err(crate::codegen::arena_failure)?
            .kind,
        TypedExprKind::Null
    );
    if lhs_is_null_lit && rhs_is_null_lit {
        let v: i32 = if matches!(op, BinOp::Eq) { 1 } else { 0 };
        emitter.instruction(Instruction::I32Const(v));
        return Ok(());
    }
    if lhs_is_null_lit || rhs_is_null_lit {
        let non_null_side = if lhs_is_null_lit { rhs } else { lhs };
        let non_null_ty = ctx
            .ta
            .try_expr(non_null_side)
            .map_err(crate::codegen::arena_failure)?
            .ty
            .clone();
        let non_null_val = ctx.symbols.value_type(&non_null_ty)?;
        // Plan 75.10 PR 2: when the non-null side has a
        // primitive Wasm type (f64 / i32 — e.g., a
        // narrowed-to-`Number` after assignment), `ref.is_null`
        // would be a validation error. The answer is
        // statically known — a primitive can never be null —
        // so emit the operand for side effects, drop, then
        // push the constant result.
        if matches!(non_null_val, ValType::F64 | ValType::I32) {
            emit_expr(emitter, ctx, non_null_side)?;
            emitter.instruction(Instruction::Drop);
            let v: i32 = if matches!(op, BinOp::NotEq) { 1 } else { 0 };
            emitter.instruction(Instruction::I32Const(v));
            return Ok(());
        }
        emit_expr(emitter, ctx, non_null_side)?;
        emitter.instruction(Instruction::RefIsNull);
        if matches!(op, BinOp::NotEq) {
            emitter.instruction(Instruction::I32Eqz);
        }
        return Ok(());
    }
    let lhs_ty = ctx
        .ta
        .try_expr(lhs)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    let rhs_ty = ctx
        .ta
        .try_expr(rhs)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    let operand_val = ctx.symbols.value_type(lhs_ty.primitive_behavior())?;
    // The typed dispatch below needs both operands in one Wasm shape. One that
    // admits null is a `(ref null $Object)` beside the other's f64 or
    // `(ref $string)`, and comparable operands need not share a representation
    // at all (`t === 1` with `t: T`, `n === e` with `e: {}`). Those pairs box both
    // sides to `(ref [null] $Object)` and dispatch through `vtable.equals`.
    if may_hold_null(&lhs_ty)
        || may_hold_null(&rhs_ty)
        || operand_val != ctx.symbols.value_type(rhs_ty.primitive_behavior())?
    {
        emit_boxed_eq(emitter, ctx, lhs, rhs, op)?;
        return Ok(());
    }
    // bigint equality short-circuits to a direct
    // `submilli:bigint.cmp == 0` call — a faster path than the
    // generic `ValType::Ref(_)` arm's vtable `equals` dispatch.
    if lhs_ty.is_bigint() {
        emit_bigint_cmp_eq_inline(emitter, ctx, lhs, rhs, op)?;
        return Ok(());
    }
    let string_idx = ctx.symbols.string_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("string type registered with intrinsics")
    })?;
    // Operands that share a Wasm `value_type` dispatch off it. The language
    // `Type` might be a literal-refined primitive or a literal-only union; at
    // the Wasm level those collapse to the same compare path as their base
    // primitive (equality is value-level; the literal refinement is a
    // typecheck-time restriction, not a runtime distinction).
    match operand_val {
        ValType::F64 => {
            emit_primitive_operand(emitter, ctx, lhs)?;
            emit_primitive_operand(emitter, ctx, rhs)?;
            emitter.instruction(if matches!(op, BinOp::Eq) {
                Instruction::F64Eq
            } else {
                Instruction::F64Ne
            });
        }
        ValType::I32 => {
            emit_expr(emitter, ctx, lhs)?;
            emit_expr(emitter, ctx, rhs)?;
            emitter.instruction(if matches!(op, BinOp::Eq) {
                Instruction::I32Eq
            } else {
                Instruction::I32Ne
            });
        }
        ValType::Ref(RefType {
            heap_type: HeapType::Concrete(idx),
            ..
        }) if idx == string_idx => {
            emit_expr(emitter, ctx, lhs)?;
            emit_expr(emitter, ctx, rhs)?;
            let func = ctx.symbols.prelude_func_idx("string_eq").ok_or_else(|| {
                crate::codegen::internal_failure("string_eq imported from prelude")
            })?;
            emitter.instruction(Instruction::Call(func));
            if matches!(op, BinOp::NotEq) {
                emitter.instruction(Instruction::I32Eqz);
            }
        }
        ValType::Ref(_) => {
            // Object / array / closure / generic-erased (all
            // subtypes of `$Object`): load lhs.vtable.equals
            // and call it. Each subtype's body runs the
            // structural compare for its own shape; nested
            // objects / arrays recurse through their own
            // vtable.equals slots.
            //
            // Enum values fall here too — they share the
            // `$BoxedNumber` / `$string` shapes whose
            // vtable `equals` slot is wired by the prelude.
            emit_expr(emitter, ctx, lhs)?;
            cast::emit_box(emitter, ctx, &lhs_ty)?;
            emit_expr(emitter, ctx, rhs)?;
            cast::emit_box(emitter, ctx, &rhs_ty)?;
            emit_vtable_equality(emitter, ctx, op);
        }
        other => {
            return Err(crate::codegen::internal_failure(format!(
                "typechecker rejects equality on Wasm value-type `{other:?}`"
            )));
        }
    }
    Ok(())
}

fn emit_logical(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: BinOp,
    lhs: ExprId,
    rhs: ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // JS value-returning short-circuit, mirroring the `??` lowering:
    // stash the LHS in an anonymous local, truthiness-test the teed
    // copy, then either evaluate the RHS or replay the kept LHS —
    // each branch coerced into the result union's slot. `&&` keeps
    // the LHS when falsy, `||` when truthy.
    let result_val = ctx.symbols.value_type(result_ty)?;
    let lhs_ty = ctx
        .ta
        .try_expr(lhs)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    let lhs_val = ctx.symbols.value_type(&lhs_ty)?;
    let lhs_is_ref = matches!(lhs_val, ValType::Ref(_));
    let tmp = emitter.add_anonymous_local(lhs_val)?;
    emit_expr(emitter, ctx, lhs)?;
    emitter.instruction(Instruction::LocalTee(tmp));
    crate::codegen::function_emitter::cast::emit_condition_to_i32(emitter, ctx, &lhs_ty)?;
    let emit_rhs_branch =
        |emitter: &mut FunctionEmitter| -> Result<(), crate::compiler_error::CompilerFailure> {
            let rhs_ty = ctx
                .ta
                .try_expr(rhs)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            emit_expr(emitter, ctx, rhs)?;
            crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                emitter, ctx, &rhs_ty, result_ty,
            )?;
            Ok(())
        };
    // An LHS narrowed to always-truthy (`&&`) or always-falsy (`||`)
    // is never the result, and the result slot has no room for it:
    // `c && n()` under `c === true` is a bare f64.
    let kept_lhs_ty = match op {
        BinOp::And => falsy_part(&lhs_ty),
        _ => truthy_part(&lhs_ty),
    };
    let lhs_is_never_kept = matches!(kept_lhs_ty.peel(), Type::Never);
    let emit_kept_lhs_branch = |emitter: &mut FunctionEmitter| {
        if lhs_is_never_kept {
            emitter.instruction(Instruction::Unreachable);
            return Ok(());
        }
        emitter.instruction(Instruction::LocalGet(tmp));
        if lhs_is_ref {
            // Ref-repr LHS: cast into the result slot's form —
            // `ref.as_non_null` lift, downcast, or unbox as needed
            // (same contract as the `??` kept branch).
            crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, result_ty)?;
        } else {
            crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                emitter, ctx, &lhs_ty, result_ty,
            )?;
        }
        Ok::<(), crate::compiler_error::CompilerFailure>(())
    };
    emitter.emit_if(BlockType::Result(result_val));
    match op {
        BinOp::And => emit_rhs_branch(emitter)?,
        _ => emit_kept_lhs_branch(emitter)?,
    }
    emitter.emit_else();
    match op {
        BinOp::And => emit_kept_lhs_branch(emitter)?,
        _ => emit_rhs_branch(emitter)?,
    }
    emitter.emit_end();
    Ok(())
}

/// Evaluates `operands` in order and ends in `unreachable` when any of them is
/// `never`, returning whether it did. The typechecker accepts arithmetic on a
/// `never` operand because no value of it exists, but that operand's slot is a
/// reference no numeric instruction takes, so the operator itself isn't emitted.
fn try_emit_unreachable_for_never_operand(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    operands: &[ExprId],
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let mut has_never_operand = false;
    for &operand in operands {
        let ty = &ctx
            .ta
            .try_expr(operand)
            .map_err(crate::codegen::arena_failure)?
            .ty;
        has_never_operand |= matches!(ty.peel(), Type::Never);
    }
    if !has_never_operand {
        return Ok(false);
    }
    for &operand in operands {
        emit_expr(emitter, ctx, operand)?;
        emitter.instruction(Instruction::Drop);
    }
    emitter.instruction(Instruction::Unreachable);
    Ok(true)
}

fn emit_primitive_operand(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    expr: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr(emitter, ctx, expr)?;
    let ty = &ctx
        .ta
        .try_expr(expr)
        .map_err(crate::codegen::arena_failure)?
        .ty;
    let _: () = if matches!(ty.primitive_behavior(), Type::Number)
        && ctx.symbols.value_type(ty)? != ValType::F64
    {
        super::cast::emit_cast_to(emitter, ctx, &Type::Number)?;
    };
    Ok(())
}

/// `"field" in obj` codegen. Walks the receiver's
/// `$ObjectShape.field_names` array twice — once with `ref.eq` (the
/// same-module fast path, sharing the per-name string global) and
/// once with the prelude `$string_eq` (the cross-module slow path).
/// Returns an i32 boolean (0/1) on the stack.
///
/// Presence uses the same slot scan as reads, without invoking accessors.
fn emit_in_operator(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr(emitter, ctx, lhs)?;
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(lhs)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        &Type::String,
    )?;
    let key = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::String)?)?;
    emitter.instruction(Instruction::LocalSet(key));
    emit_expr(emitter, ctx, rhs)?;
    cast::emit_box(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(rhs)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    emitter.instruction(Instruction::LocalGet(key));
    let Some(symbol) = ctx.require(
        ctx.symbols.prelude_func_idx("ObjectConstructor##hasField"),
        "presence helper imported",
    ) else {
        return Ok(());
    };
    emitter.instruction(Instruction::Call(symbol));
    Ok(())
}

/// Emit the constructor field-setup sequence for `mangled`: each own-field
/// initializer (and, later, parameter-property copy) as `this.field = value`
/// into the object-fields payload, in recorded order. Runs after `super(...)`
/// (or at the top of a base class's init fn), so `this`/parent fields are live.
pub(crate) fn emit_class_field_setup(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    mangled: &crate::MangledName,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    use crate::codegen::symbol_table::FieldSetup;
    let setup: Vec<FieldSetup> = match ctx.symbols.class_field_setup(mangled) {
        Some(s) if !s.is_empty() => s.to_vec(),
        _ => return Ok(()),
    };
    let struct_idx = ctx.symbols.class_struct_type_idx(mangled).ok_or_else(|| {
        crate::codegen::internal_failure("class struct type recorded in classes::emit")
    })?;
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let this = emitter.this_local().ok_or_else(|| {
        crate::codegen::internal_failure("field setup runs inside a constructor init fn")
    })?;
    for step in &setup {
        let field = match step {
            FieldSetup::Init { field, .. }
            | FieldSetup::ParamCopy { field, .. }
            | FieldSetup::Reset { field } => field,
        };
        let slot = ctx
            .symbols
            .class_field_slot(mangled, field)
            .ok_or_else(|| {
                crate::codegen::internal_failure("class field slot recorded in classes::emit")
            })?;
        match step {
            FieldSetup::Init { value, .. } => {
                let value_ty = ctx
                    .ta
                    .try_expr(*value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty
                    .clone();
                emit_expr(emitter, ctx, *value)?;
                crate::codegen::function_emitter::cast::emit_box(emitter, ctx, &value_ty)?;
            }
            FieldSetup::ParamCopy {
                param_local, ty, ..
            } => {
                emitter.instruction(Instruction::LocalGet(*param_local));
                crate::codegen::function_emitter::cast::emit_box(emitter, ctx, ty)?;
            }
            FieldSetup::Reset { .. } => {
                emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
            }
        }
        let stored = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
        emitter.instruction(Instruction::LocalSet(stored));
        emitter.instruction(Instruction::LocalGet(this));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: struct_idx,
            field_index: 2,
        });
        emitter.instruction(Instruction::I32Const(slot as i32));
        emitter.instruction(Instruction::LocalGet(stored));
        emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
        let index = emitter.add_anonymous_local(ValType::I32)?;
        emitter.instruction(Instruction::I32Const(slot as i32));
        emitter.instruction(Instruction::LocalSet(index));
        crate::codegen::field_names::emit_mark_present(emitter, ctx, this, index)?;
    }
    Ok(())
}

/// Push the payload slot at `index_local` from an `$ObjectShape`-typed
/// receiver. Slot 2 of the shape header is the fields array; the caller owns
/// proving the index is in range.
pub(crate) fn emit_field_slot_get(
    emitter: &mut FunctionEmitter,
    intrinsics: crate::codegen::intrinsics::IntrinsicTypeIndices,
    object_local: u32,
    index_local: u32,
) {
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::ArrayGet(intrinsics.object_fields));
}

pub(crate) fn emit_object_field_read_by_name(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
) {
    ctx.latch(emit_object_field_read_by_name_checked(
        emitter,
        ctx,
        object_local,
        name_global,
    ));
}

fn emit_object_field_read_by_name_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let Some(index_local) = ctx.latch(emitter.add_anonymous_local(ValType::I32)) else {
        return Ok(());
    };
    emit_object_field_index_by_name(emitter, ctx, object_local, name_global);
    emitter.instruction(Instruction::LocalSet(index_local));
    emitter.emit_block(BlockType::Result(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    })));
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32LtS);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
    emitter.instruction(Instruction::Br(1));
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::ArrayGet(intrinsics.object_fields));
    emitter.emit_end();

    Ok(())
}

pub(crate) fn emit_object_field_write_by_name(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let value_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))?;
    let index_local = emitter.add_anonymous_local(ValType::I32)?;
    emitter.instruction(Instruction::LocalSet(value_local));
    emit_object_field_index_by_name(emitter, ctx, object_local, name_global);
    emitter.instruction(Instruction::LocalTee(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
    crate::codegen::field_names::emit_mark_present(emitter, ctx, object_local, index_local)?;
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::GlobalGet(name_global));
    emitter.instruction(Instruction::LocalGet(value_local));
    let insert = ctx
        .symbols
        .prelude_func_idx("ObjectConstructor##insertField")
        .ok_or_else(|| {
            crate::codegen::internal_failure("field insertion helper is not imported")
        })?;
    emitter.instruction(Instruction::Call(insert));
    emitter.emit_end();
    Ok(())
}

/// Whether a shaped property read may emit its accessor branch, which scans for
/// `getter_name` and so needs its `$string` global. Always true today —
/// `analysis::note_shaped_property_access` interns the name for every access it
/// classifies as shaped, which is exactly the set reaching here. It stays as a
/// fail-soft guard against an unclassified receiver, which would otherwise panic
/// on the missing lookup.
pub(crate) fn accessor_branch_emittable(ctx: &CodegenCtx, getter_name: &str) -> bool {
    ctx.symbols
        .field_name_string_global_idx(getter_name)
        .is_some()
}

/// Pushes i32 1 if `object_local` is backed by the accessor `accessor_name`, 0
/// otherwise: the name scan finds a slot, the receiver's vtable is a
/// `$ClassVTable` (only a class declares accessors), and that slot holds a
/// closure of the accessor's shape. Callers must have checked the name has a
/// string global.
///
/// All three, because `get <prop>` is an ordinary string and a value can carry a
/// *data* field spelled exactly that — `JSON.parse('{"get x": 5}') as Foo` is
/// enough, and an object literal can even store a matching closure under it —
/// while the branch this gates goes straight on to `ref.cast` the slot to the
/// accessor's closure type. Anything else falls through to the data / absent
/// arms.
///
/// Returns the local holding the accessor slot's index, valid on the branch
/// where this pushed 1 — so the dispatch that follows can read the slot directly
/// instead of scanning the same name a second time.
pub(crate) fn emit_is_accessor_backed(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    accessor_name: &str,
    kind: crate::AccessorKind,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let accessor_global = ctx
        .symbols
        .field_name_string_global_idx(accessor_name)
        .ok_or_else(|| {
            crate::codegen::internal_failure("caller checked the accessor name has a string global")
        })?;
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(crate::codegen::classes::accessor_closure_sig(kind))
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "accessor closure struct collected by analysis::note_shaped_property_access",
            )
        })?;
    let index_local = emitter.add_anonymous_local(ValType::I32)?;
    emit_field_index_by_name(emitter, ctx, object_local, accessor_global, true);
    emitter.instruction(Instruction::LocalTee(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        intrinsics.class_vtable,
    )));
    emitter.instruction(Instruction::I32And);
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
    crate::codegen::field_names::emit_name_is_accessor(emitter, ctx)?;
    emit_field_slot_get(emitter, intrinsics, object_local, index_local);
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        closure_struct_idx,
    )));
    emitter.instruction(Instruction::I32And);
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end();
    Ok(index_local)
}

/// Read property `prop_name` from an `$ObjectShape`-typed receiver, accessor-aware:
/// a data field reads its payload slot; an accessor property invokes its
/// `get <prop>` method closure; a property with neither slot is absent and reads
/// `null`. The choice is made at runtime, since an interface property may be
/// backed by a field on one impl, an accessor on another, and nothing at all on a
/// third that reached the interface type through an optional member. Deciding it
/// on the data-slot miss alone cannot separate the last two, and traps on
/// whichever it does not pick. Leaves the value on the stack at `field_ty`'s
/// value type.
pub(crate) fn emit_object_property_read(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop_name: &str,
    field_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_object_property_read_as(emitter, ctx, object_local, prop_name, field_ty, field_ty)?;
    Ok(())
}

fn emit_object_property_read_as(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop_name: &str,
    field_ty: &Type,
    check_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let getter = crate::codegen::classes::accessor_getter_name(prop_name);
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(prop_name)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "per-name string global recorded during field-name-strings emission",
            )
        })?;
    if !accessor_branch_emittable(ctx, &getter) {
        emit_object_field_read_by_name(emitter, ctx, object_local, name_global);
        emit_dynamic_narrowed_property_read(
            emitter,
            ctx,
            object_local,
            prop_name,
            field_ty,
            check_ty,
        )?;
        return Ok(());
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let result_vt = ctx.symbols.value_type(field_ty)?;
    let index_local = emitter.add_anonymous_local(ValType::I32)?;
    emit_object_field_index_by_name(emitter, ctx, object_local, name_global);
    emitter.instruction(Instruction::LocalSet(index_local));
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(BlockType::Result(result_vt));
    // data slot
    emit_field_slot_get(emitter, intrinsics, object_local, index_local);
    emit_dynamic_narrowed_property_read(emitter, ctx, object_local, prop_name, field_ty, check_ty)?;
    emitter.emit_else();
    let getter_slot_local = emit_is_accessor_backed(
        emitter,
        ctx,
        object_local,
        &getter,
        crate::AccessorKind::Get,
    )?;
    emitter.emit_if(BlockType::Result(result_vt));
    // accessor slot
    emitter.instruction(Instruction::LocalGet(object_local));
    emit_interface_method_via_shape_with_receiver_on_stack(
        emitter,
        ctx,
        &getter,
        &[],
        field_ty,
        Some(getter_slot_local),
    )?;
    emitter.emit_else();
    // neither: absent — an optional member the value never materialised, so
    // `field_ty` admits null and the cast is a widen. An accessor-backed value
    // does not reach here even when the accessor is declared in another module:
    // the `get <prop>` name is interned off the *access*, not off any accessor
    // declaration this module can see (`analysis::note_shaped_property_access`).
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
    cast::emit_cast_to(emitter, ctx, field_ty)?;
    emitter.emit_end();
    emitter.emit_end();
    Ok(())
}

fn emit_dynamic_narrowed_property_read(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop_name: &str,
    field_ty: &Type,
    check_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let checks: Vec<_> = ctx
        .symbols
        .class_field_narrowing_checks(prop_name)
        .filter_map(|(class, check)| {
            ctx.symbols
                .class_vtable_global_idx(class)
                .map(|vtable| (vtable, check))
        })
        .collect();
    if checks.is_empty() {
        cast::emit_cast_to(emitter, ctx, field_ty)?;
        return Ok(());
    }

    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let raw = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))?;
    emitter.instruction(Instruction::LocalSet(raw));
    emit_dynamic_narrowing_check(
        emitter,
        ctx,
        object_local,
        raw,
        field_ty,
        check_ty,
        &checks,
        0,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_dynamic_narrowing_check(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    raw: u32,
    field_ty: &Type,
    check_ty: &Type,
    checks: &[(u32, &crate::FieldNarrowingCheck)],
    index: usize,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some((vtable, check)) = checks.get(index) else {
        emitter.instruction(Instruction::LocalGet(raw));
        cast::emit_cast_to(emitter, ctx, field_ty)?;
        return Ok(());
    };
    emitter.instruction(Instruction::LocalGet(object_local));
    crate::codegen::function_emitter::cast::emit_nominal_instance_test(emitter, ctx, *vtable)?;
    emitter.emit_if(BlockType::Result(ctx.symbols.value_type(field_ty)?));
    emitter.instruction(Instruction::LocalGet(raw));
    crate::codegen::cast_check::emit_narrowed_field_read_as(
        emitter, ctx, check, field_ty, check_ty,
    )?;
    emitter.emit_else();
    emit_dynamic_narrowing_check(
        emitter,
        ctx,
        object_local,
        raw,
        field_ty,
        check_ty,
        checks,
        index + 1,
    )?;
    emitter.emit_end();
    Ok(())
}

/// Write `value` to property `prop` on an `$ObjectShape`-typed receiver,
/// accessor-aware: a data field writes its payload slot; an accessor property
/// invokes its `set <prop>` method closure; a property backed by a getter with no
/// setter throws, since the target exists but is read-only; a property with none
/// of the three gets a new data slot. `value` is evaluated exactly once.
pub(crate) fn emit_object_property_write(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop: &Ident,
    value: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_object_property_write_value(
        emitter,
        ctx,
        object_local,
        prop,
        &ShapeArgument::Expression(value),
    )?;
    Ok(())
}

fn emit_object_property_write_value(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop: &Ident,
    value: &ShapeArgument,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let prop_name = prop.name.as_str();
    let setter = crate::codegen::classes::accessor_setter_name(prop_name);
    let getter = crate::codegen::classes::accessor_getter_name(prop_name);
    let has_setter_name = ctx.symbols.field_name_string_global_idx(&setter).is_some();
    let has_getter_name = ctx.symbols.field_name_string_global_idx(&getter).is_some();
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(prop_name)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "per-name string global recorded during field-name-strings emission",
            )
        })?;
    // Fail-soft twin of [`accessor_branch_emittable`], asked of both halves:
    // reaching here means analysis classified the access, which interns both
    // accessor names — so neither name can actually be missing.
    if !has_setter_name && !has_getter_name {
        let value_ty = value.ty(ctx)?;
        value.emit(emitter, ctx)?;
        cast::emit_box(emitter, ctx, &value_ty)?;
        emit_object_field_write_by_name(emitter, ctx, object_local, name_global)?;
        return Ok(());
    }
    let index_local = emitter.add_anonymous_local(ValType::I32)?;
    emit_object_field_index_by_name(emitter, ctx, object_local, name_global);
    emitter.instruction(Instruction::LocalSet(index_local));
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(BlockType::Empty);
    // Evaluate the RHS before loading the payload: it may insert another
    // field and replace the receiver's backing arrays.
    let value_ty = value.ty(ctx)?;
    value.emit(emitter, ctx)?;
    cast::emit_box(emitter, ctx, &value_ty)?;
    emit_object_field_write_by_name(emitter, ctx, object_local, name_global)?;
    emitter.emit_else();
    // One nested `if` per accessor the program declares, innermost `else` being
    // the absent case — so the arm list *is* the nesting depth. An accessor the
    // program never declares is skipped: its name has no string global to scan
    // for, and no value can be backed by it.
    let arms: Vec<(&str, AccessorWrite)> = [
        has_setter_name.then_some((setter.as_str(), AccessorWrite::Setter)),
        has_getter_name.then_some((getter.as_str(), AccessorWrite::ReadOnly)),
    ]
    .into_iter()
    .flatten()
    .collect();
    for (accessor, arm) in &arms {
        let slot_index_local =
            emit_is_accessor_backed(emitter, ctx, object_local, accessor, arm.scanned_kind())?;
        emitter.emit_if(BlockType::Empty);
        match arm {
            AccessorWrite::Setter => {
                emitter.instruction(Instruction::LocalGet(object_local));
                emit_shape_method_with_arguments(
                    emitter,
                    ctx,
                    accessor,
                    std::slice::from_ref(value),
                    &Type::Void,
                    Some(slot_index_local),
                )?;
            }
            // A getter with no setter: the property is there and is read-only.
            // Discarding the write here would lose it silently.
            AccessorWrite::ReadOnly => {
                value.emit(emitter, ctx)?;
                emitter.instruction(Instruction::Drop);
                // The value's own span was recorded last; point the backtrace at
                // the property instead, which is what the write failed on.
                emitter.record_span(prop.span);
                crate::codegen::throw::emit_type_error_throw(
                    emitter,
                    ctx,
                    crate::codegen::throw::READ_ONLY_PROPERTY_MESSAGE,
                );
            }
        }
        emitter.emit_else();
    }
    let value_ty = value.ty(ctx)?;
    value.emit(emitter, ctx)?;
    cast::emit_box(emitter, ctx, &value_ty)?;
    emit_object_field_write_by_name(emitter, ctx, object_local, name_global)?;
    for _ in &arms {
        emitter.emit_end();
    }
    emitter.emit_end();
    Ok(())
}

/// `<receiver>[idx]` with the `(ref $Array)` receiver already on the stack:
///
/// ```text
///   struct.get $Array 1; local.set $raw
///   <idx>; i32.trunc_sat_f64_u; local.set $i
///   bounds-check $i against len($raw)   ;; throws RangeError on a miss
///   local.get $raw; local.get $i; array.get $rawArray
///   emit_cast_to(result_ty)             ;; unboxes if primitive
/// ```
///
/// Shared by the plain `IndexAccess` lowering and the optional chain's `Index`
/// step, which differ only in how the receiver gets onto the stack. Uint8Array
/// has its own packed-storage path and does not come through here.
fn emit_bounds_checked_index_with_receiver_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    idx: ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let array_idx = ctx.symbols.array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let raw_array_idx = ctx.symbols.raw_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let raw_arr_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_array_idx),
    }))?;
    let receiver = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(receiver));
    emit_expr(emitter, ctx, idx)?;
    let key = emitter.add_anonymous_local(
        ctx.symbols.value_type(
            &ctx.ta
                .try_expr(idx)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?,
    )?;
    emitter.instruction(Instruction::LocalSet(key));
    emitter.instruction(Instruction::LocalGet(receiver));
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &Type::Unknown,
        &Type::Array(Box::new(Type::Unknown)),
    )?;
    emitter.instruction(Instruction::LocalGet(key));
    emit_index_number(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(idx)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    let idx_f64_local = stash_index_operand(emitter)?;
    let length = stash_array_length(emitter, array_idx)?;
    emitter.instruction(Instruction::StructGet {
        struct_type_index: array_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(raw_arr_local));
    let idx_local = emit_checked_index_with_length(emitter, ctx, length, idx_f64_local)?;
    emitter.instruction(Instruction::LocalGet(raw_arr_local));
    emitter.instruction(Instruction::LocalGet(idx_local));
    emitter.instruction(Instruction::ArrayGet(raw_array_idx));
    cast::emit_cast_to(emitter, ctx, result_ty)?;
    Ok(())
}

fn emit_uint8_index_with_receiver_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    index: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let uint8_idx = ctx.symbols.uint8_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Uint8Array requires intrinsic types declared")
    })?;
    let raw_uint8_idx = ctx.symbols.raw_uint8_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Uint8Array requires intrinsic types declared")
    })?;
    let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_uint8_idx),
    }))?;
    let receiver = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(receiver));
    emit_expr(emitter, ctx, index)?;
    let key = emitter.add_anonymous_local(
        ctx.symbols.value_type(
            &ctx.ta
                .try_expr(index)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?,
    )?;
    emitter.instruction(Instruction::LocalSet(key));
    emitter.instruction(Instruction::LocalGet(receiver));
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &Type::Unknown,
        &Type::Uint8Array,
    )?;
    emitter.instruction(Instruction::LocalGet(key));
    emit_index_number(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    let idx_f64_local = stash_index_operand(emitter)?;
    emitter.instruction(Instruction::StructGet {
        struct_type_index: uint8_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(raw_local));
    let idx_local = emit_checked_index(emitter, ctx, raw_local, idx_f64_local)?;
    emitter.instruction(Instruction::LocalGet(raw_local));
    emitter.instruction(Instruction::LocalGet(idx_local));
    emitter.instruction(Instruction::ArrayGetU(raw_uint8_idx));
    emitter.instruction(Instruction::F64ConvertI32U);
    Ok(())
}

/// Which accessor a write dispatches to when the data slot is missing.
#[derive(Clone, Copy)]
enum AccessorWrite {
    Setter,
    ReadOnly,
}

impl AccessorWrite {
    /// Which accessor slot this arm scans for. The read-only arm has no setter
    /// to find, so it scans the *getter* as proof the property is there at all.
    fn scanned_kind(self) -> crate::AccessorKind {
        match self {
            AccessorWrite::Setter => crate::AccessorKind::Set,
            AccessorWrite::ReadOnly => crate::AccessorKind::Get,
        }
    }
}

pub(crate) fn emit_object_field_index_by_name(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
) {
    emit_field_index_by_name(emitter, ctx, object_local, name_global, false);
}

fn emit_field_index_by_name(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
    accessor: bool,
) {
    ctx.latch(emit_field_index_by_name_checked(
        emitter,
        ctx,
        object_local,
        name_global,
        accessor,
    ));
}

fn emit_field_index_by_name_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
    accessor: bool,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::GlobalGet(name_global));
    emitter.instruction(Instruction::I32Const(i32::from(accessor)));
    emitter.instruction(Instruction::Call(
        ctx.symbols
            .field_lookup_function
            .ok_or_else(|| crate::codegen::internal_failure("field lookup allocated"))?,
    ));

    Ok(())
}

/// Emit a direct call to an imported function. Host imports get the
/// raw-string round-trip applied around the call (extract `(ref
/// $rawString)` on `Type::String` args, wrap on a `Type::String`
/// return). Non-host imports (prelude `string_concat`, etc.) skip
/// both conversions.
fn emit_direct_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    is_host: bool,
    wasm_idx: u32,
    params: &[Type],
    ret: &Type,
    args: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if params.len() != args.len() {
        return Err(crate::codegen::internal_failure(
            "direct-call argument count mismatch",
        ));
    }
    let string_type_idx = ctx.symbols.string_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("$string type registered before call codegen")
    })?;
    let raw_string_type_idx = ctx.symbols.raw_string_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("$rawString type registered before call codegen")
    })?;
    let mut evaluated = Vec::with_capacity(args.len());
    for &arg in args {
        emit_expr(emitter, ctx, arg)?;
        let slot = emitter.add_anonymous_local(
            ctx.symbols.value_type(
                &ctx.ta
                    .try_expr(arg)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?,
        )?;
        emitter.instruction(Instruction::LocalSet(slot));
        evaluated.push(slot);
    }
    for (i, (&arg_id, local)) in args.iter().zip(evaluated).enumerate() {
        emitter.instruction(Instruction::LocalGet(local));
        // Plan 75.8: coerce a primitive arg into a wider ref-typed
        // param slot (e.g., calling `f(x: number | null)` with `5`
        // requires boxing the f64 before the call). No-op when the
        // value's Wasm type already satisfies the param slot via
        // subtyping.
        if let Some(param_ty) = params.get(i) {
            let arg_ty = ctx
                .ta
                .try_expr(arg_id)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            cast::emit_coerce_to_slot(emitter, ctx, &arg_ty, param_ty)?;
        }
        // follow-up: peel param / return so aliased strings
        // (`type S = string`) still trigger the raw-string unwrap/
        // rewrap that matches the host import's `$rawString`
        // signature.
        if is_host
            && params
                .get(i)
                .is_some_and(|p| matches!(p.peel(), Type::String))
        {
            emitter.instruction(Instruction::StructGet {
                struct_type_index: string_type_idx,
                field_index: 1,
            });
        }
    }
    emitter.instruction(Instruction::Call(wasm_idx));
    let _: () = if is_host && matches!(ret.peel(), Type::String) {
        let scratch = emitter.add_anonymous_local(ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(raw_string_type_idx),
        }))?;
        emitter.instruction(Instruction::LocalSet(scratch));
        let vtable_global = ctx
            .symbols
            .prelude_global_idx("string_vtable")
            .ok_or_else(|| {
                crate::codegen::internal_failure("string_vtable imported from prelude")
            })?;
        emitter.instruction(Instruction::GlobalGet(vtable_global));
        emitter.instruction(Instruction::LocalGet(scratch));
        emitter.instruction(Instruction::I64Const(0));
        emitter.instruction(Instruction::StructNew(string_type_idx));
    };
    Ok(())
}

/// Emit an indirect call via `call_ref` through a closure value
/// (`LocalRef` / `GlobalRef` to a function-typed binding, an inline
/// `Closure`, etc.).
///
/// Under uniform closure ABI, every closure param and the
/// return are erased to `(ref $Object)` at the Wasm boundary. This
/// call site evaluates and boxes arguments before checking the callable. The
/// result uses the physical type chosen by runtime-value propagation.
fn emit_indirect_closure_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    callee: ExprId,
    args: &[ExprId],
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let callee_ty = ctx
        .ta
        .source_type(callee)
        .map_err(crate::codegen::arena_failure)?
        .clone();
    let receiver = emit_callee(emitter, ctx, callee)?;
    emitter.call_receiver = receiver;
    emit_indirect_closure_call_with_receiver_on_stack(emitter, ctx, &callee_ty, args, result_ty)?;
    Ok(())
}

/// Preserve a property reference until invocation without evaluating its object twice.
fn emit_callee(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    callee: ExprId,
) -> Result<Option<u32>, crate::compiler_error::CompilerFailure> {
    let reference = reference_expression(ctx.ta, callee)?;
    let receiver = match &ctx
        .ta
        .try_expr(reference)
        .map_err(crate::codegen::arena_failure)?
        .kind
    {
        TypedExprKind::FieldAccess { receiver, .. }
        | TypedExprKind::IndexAccess { receiver, .. }
        | TypedExprKind::InterfacePropertyAccess { receiver, .. } => Some(*receiver),
        _ => None,
    };
    let mark = emitter.single_evaluation_mark();
    let slot = receiver
        .map(|receiver| {
            emit_expr(emitter, ctx, receiver)?;
            let slot = emitter.add_anonymous_local(
                ctx.symbols.value_type(
                    &ctx.ta
                        .try_expr(receiver)
                        .map_err(crate::codegen::arena_failure)?
                        .ty,
                )?,
            )?;
            emitter.instruction(Instruction::LocalSet(slot));
            emitter.record_single_evaluation(receiver, slot)?;
            Ok::<_, crate::compiler_error::CompilerFailure>(slot)
        })
        .transpose()?;
    emit_expr(emitter, ctx, callee)?;
    emitter.end_single_evaluations(mark)?;
    Ok(
        if matches!(
            ctx.ta
                .try_expr(reference)
                .map_err(crate::codegen::arena_failure)?
                .kind,
            TypedExprKind::OptionalChain { .. }
        ) {
            emitter.call_receiver.take()
        } else {
            slot
        },
    )
}

fn reference_expression(
    ast: &crate::TypedAst,
    mut id: ExprId,
) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
    loop {
        match ast
            .try_expr(id)
            .map_err(crate::codegen::arena_failure)?
            .kind
        {
            TypedExprKind::Cast { value, .. } | TypedExprKind::NonNullAssert { value } => {
                id = value;
            }
            _ => return Ok(id),
        }
    }
}

/// Same as [`emit_indirect_closure_call`], but assumes the closure
/// value is already on the stack (cast to its per-signature struct
/// type). Used optional-chain `Call` arm, where
/// `emit_chain_parts` has already evaluated, null-checked, and
/// ref-cast the receiver before dispatching.
fn emit_indirect_closure_call_with_receiver_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    callee_ty: &Type,
    args: &[ExprId],
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let receiver = emitter.call_receiver.take();
    let callee = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(callee));
    let mut arguments = Vec::with_capacity(args.len());
    for &arg in args {
        emit_expr(emitter, ctx, arg)?;
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(arg)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
        let slot = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
        emitter.instruction(Instruction::LocalSet(slot));
        arguments.push(slot);
    }
    emitter.instruction(Instruction::LocalGet(callee));
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &Type::Unknown,
        callee_ty,
    )?;
    // chain receivers can arrive aliased
    // (`type NumberFn = (x: number) => number; let f: NumberFn | null`
    // → after `strip_null`, the type is the alias, not the underlying
    // function). `classify` panics on non-Function inputs, so peel.
    let callee_ty = callee_ty.peel();
    let closure_sig = crate::codegen::closures::classify(callee_ty)?;
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(closure_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure struct type registered for every closure-typed callee",
            )
        })?;
    let fn_type_idx = ctx
        .symbols
        .closure_func_type_idx(closure_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure funcref type registered for every closure-typed callee",
            )
        })?;
    let closure_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_struct_idx),
    }))?;
    emitter.instruction(Instruction::LocalTee(closure_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 2,
    });
    if let Some(receiver) = receiver {
        crate::codegen::this_binding::bind(emitter, ctx, receiver)?;
    }
    // note: variadic closures arrive with their trailing args
    // already packed into a synthesized `ArrayLiteral` filling the
    // rest slot, so `args.len()` matches the closure's funcref
    // signature arity. The per-arg `emit_box` below is still correct
    // — the synthesized array's static type is a concrete
    // `Type::Array(_)`, so `emit_box` upcasts it to `(ref $Object)`
    // via the same ref-extension that any array arg uses.
    for argument in arguments {
        emitter.instruction(Instruction::LocalGet(argument));
    }
    emitter.instruction(Instruction::LocalGet(closure_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::CallRef(fn_type_idx));
    // Unbox the erased return to the language-level type. Void
    // closures emit no result; primitives unwrap from `$BoxedNumber`
    // / `$BoxedBoolean`; ref types `ref.cast` back from `(ref $Object)`.
    let Type::Function { ret, .. } = callee_ty.peel() else {
        return Err(crate::codegen::internal_failure(
            "indirect call requires a function type",
        ));
    };
    let _: () = if !ret.is_void() {
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            result_ty,
        )?;
    };
    Ok(())
}

/// Classify the current boxed value rather than folding the retained static
/// refinement. Closures use the intrinsic closure supertype regardless of arity.
fn emit_typeof_tag(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value: ExprId,
    tag: crate::TypeofTagKind,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if ctx
        .ta
        .try_expr(value)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .is_void()
    {
        emit_expr(emitter, ctx, value)?;
        emitter.instruction(Instruction::I32Const(0));
        return Ok(());
    }
    let _: () = match tag {
        crate::TypeofTagKind::Number => {
            // `typeof x === "number"` lowers directly to
            // TypeofTag(Number). `ref.test (ref $BoxedNumber)` —
            // returns 1 iff the value is a `$BoxedNumber`. Handles
            // nullable LHS (returns 0 for null) without an explicit
            // null check.
            let boxed = ctx.symbols.boxed_number_type_idx().ok_or_else(|| {
                crate::codegen::internal_failure("$BoxedNumber intrinsic type registered")
            })?;
            emit_expr(emitter, ctx, value)?;
            cast::emit_box(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(boxed)));
        }
        crate::TypeofTagKind::String => {
            let string = ctx.symbols.string_type_idx().ok_or_else(|| {
                crate::codegen::internal_failure("$string intrinsic type registered")
            })?;
            emit_expr(emitter, ctx, value)?;
            cast::emit_box(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(string)));
        }
        crate::TypeofTagKind::Boolean => {
            let boxed = ctx.symbols.boxed_boolean_type_idx().ok_or_else(|| {
                crate::codegen::internal_failure("$BoxedBoolean intrinsic type registered")
            })?;
            emit_expr(emitter, ctx, value)?;
            cast::emit_box(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(boxed)));
        }
        crate::TypeofTagKind::Function => {
            let closure_idx = ctx.symbols.closure_type_idx().ok_or_else(|| {
                crate::codegen::internal_failure("$Closure intrinsic type registered")
            })?;
            emit_expr(emitter, ctx, value)?;
            cast::emit_box(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(closure_idx)));
        }
        crate::TypeofTagKind::Object => {
            let scratch_ty = ctx.symbols.value_type(&Type::Unknown)?;
            let nullable = matches!(scratch_ty, ValType::Ref(rt) if rt.nullable);
            let scratch = emitter.add_anonymous_local(scratch_ty)?;
            emit_expr(emitter, ctx, value)?;
            cast::emit_box(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            emitter.instruction(Instruction::LocalSet(scratch));

            // Stated as the complement of the primitive tags rather than a list
            // of object-like types: every runtime value is an `$Object`
            // subtype, so a positive list silently answers "not an object" for
            // any struct nobody remembered to add. The exclusions below are
            // exhaustive by construction — they are the `$Object` subtypes that
            // carry a primitive tag of their own.
            let intrinsics = ctx.symbols.intrinsic_type_indices().ok_or_else(|| {
                crate::codegen::internal_failure("intrinsics declared by codegen entry")
            })?;
            emitter.instruction(Instruction::LocalGet(scratch));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
                intrinsics.object,
            )));
            for primitive in [
                intrinsics.string,
                intrinsics.boxed_number,
                intrinsics.boxed_boolean,
                intrinsics.bigint,
                intrinsics.closure,
            ] {
                emitter.instruction(Instruction::LocalGet(scratch));
                emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(primitive)));
                emitter.instruction(Instruction::I32Eqz);
                emitter.instruction(Instruction::I32And);
            }
            if nullable {
                // JS quirk: `typeof null === "object"`. Only OR in
                // the null check when the source's Wasm type admits
                // null — `ref.is_null` is a validator error on a
                // non-nullable ref.
                emitter.instruction(Instruction::LocalGet(scratch));
                emitter.instruction(Instruction::RefIsNull);
                emitter.instruction(Instruction::I32Or);
            }
        }
    };
    Ok(())
}

fn emit_bitwise_host(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    name: &str,
    operands: &[ExprId],
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for &operand in operands {
        emit_expr(emitter, ctx, operand)?;
        let ty = &ctx
            .ta
            .try_expr(operand)
            .map_err(crate::codegen::arena_failure)?
            .ty;
        cast::emit_box(emitter, ctx, ty)?;
    }
    let mangled = crate::mangle::prelude(&format!("__value_{name}"));
    let host = ctx.symbols.func_idx(&mangled).ok_or_else(|| {
        crate::codegen::internal_failure(format!("missing bitwise host import: {name}"))
    })?;
    emitter.instruction(Instruction::Call(host));
    cast::emit_cast_to(emitter, ctx, result_ty)
}

fn emit_unary(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: UnOp,
    operand: ExprId,
    result_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if matches!(op, UnOp::Neg | UnOp::Pos)
        && try_emit_unreachable_for_never_operand(emitter, ctx, &[operand])?
    {
        return Ok(());
    }
    let _: () = match op {
        UnOp::BitNot => emit_bitwise_host(emitter, ctx, "bitnot", &[operand], result_ty)?,
        UnOp::Neg => {
            // bigint negation routes to inline host call.
            if ctx
                .ta
                .try_expr(operand)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .is_bigint()
            {
                emit_primitive_operand(emitter, ctx, operand)?;
                emit_bigint_extract_to_stack(emitter, ctx);
                let host_idx = ctx
                    .symbols
                    .func_idx(&crate::mangle::host(
                        crate::runtime::BIGINT_MODULE_NAME,
                        "neg",
                    ))
                    .ok_or_else(|| {
                        crate::codegen::internal_failure(
                            "submilli:bigint.neg import recorded by codegen bootstrap",
                        )
                    })?;
                emit_bigint_wrap_host_result(emitter, ctx, host_idx);
                return Ok(());
            }
            emit_primitive_operand(emitter, ctx, operand)?;
            emitter.instruction(Instruction::F64Neg);
        }
        UnOp::Pos => {
            // On a string, `+` is the explicit numeric coercion and lowers to
            // the same host parse `Number(s)` calls. On a number it is the
            // identity, so the operand's value is already what we want.
            emit_primitive_operand(emitter, ctx, operand)?;
            if ctx
                .ta
                .try_expr(operand)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .is_string_shaped()
            {
                let host_idx = ctx
                    .symbols
                    .func_idx(&crate::mangle::host(
                        crate::runtime::NUMBER_MODULE_NAME,
                        "toNumber",
                    ))
                    .ok_or_else(|| {
                        crate::codegen::internal_failure(
                            "submilli:number.toNumber import recorded by codegen analysis",
                        )
                    })?;
                emitter.instruction(Instruction::Call(host_idx));
            }
        }
        UnOp::Not => {
            // `!x` on a boolean is `i32.eqz` — 0 → 1, anything else → 0.
            // Plan 75.9: the inferer also accepts nullable
            // operands (e.g., `!s` with `s: string | null`); for those
            // we first coerce to the i32 truthiness convention
            // (`ref.is_null; i32.eqz`) before negating.
            let operand_ty = ctx
                .ta
                .try_expr(operand)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            emit_expr(emitter, ctx, operand)?;
            crate::codegen::function_emitter::cast::emit_condition_to_i32(
                emitter,
                ctx,
                &operand_ty,
            )?;
            emitter.instruction(Instruction::I32Eqz);
        }
    };
    Ok(())
}

/// Emit code for a compiler intrinsic call. Each intrinsic inlines
/// directly — no Wasm function is emitted, the trap (if any) lands
/// in the user's calling function so DWARF backtraces point at the
/// data-driven method-dispatch codegen. Every method-shaped
/// call routes to one of three paths:
///
/// 1. **Direct dispatch** — the receiver's interface has
///    `direct_dispatch: true` and the method has no method-level
///    generics. Codegen emits a single `call $<Iface>#<method>`
///    against the prelude's mangled-name wrapper.
/// 2. **Vtable dispatch** — the receiver's interface has
///    `direct_dispatch: false` (today only `Object`, which covers
///    `Type::Object` / `Type::TypeVar` / `Type::GenericParam`
///    receivers). The method name maps to a slot in the universal
///    `$VTable`; codegen emits the standard
///    `local.get; struct.get $Object 0; struct.get $VTable <slot>;
///     call_ref` recipe.
/// 3. **Trap** — direct-dispatch interface, but the method has
///    method-level generics (today only `Array.map<U>`). The
///    typechecker accepts the call shape but the runtime needs
///    closure-value calls before there's a real
///    implementation; emit `unreachable` so any actual call traps
///    with a clear failure at the source span.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_method_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    iface: &crate::MangledName,
    method: &str,
    args: &[ExprId],
    generic_args: Option<&[crate::GenericArgument]>,
    return_cast: Option<&Type>,
    call_ret_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let recv_ty = ctx
        .ta
        .source_type(receiver)
        .map_err(crate::codegen::arena_failure)?
        .clone();
    if !emit_dropped_static_receiver(emitter, ctx, receiver, iface)? {
        emit_receiver(emitter, ctx, receiver)?;
    }
    emit_method_call_with_receiver_on_stack(
        emitter,
        ctx,
        &recv_ty,
        iface,
        method,
        args,
        generic_args,
        return_cast,
        call_ret_ty,
    )?;
    Ok(())
}

/// A static-dispatch method drops its receiver, so a read of the binding it
/// is called on needn't build the object the binding stands for: push a
/// typed null in its place. Returns whether it did.
fn emit_dropped_static_receiver(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    iface: &crate::MangledName,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let expr = ctx
        .ta
        .try_expr(receiver)
        .map_err(crate::codegen::arena_failure)?;
    if !matches!(expr.kind, TypedExprKind::GlobalRef { .. })
        || static_interface(ctx, &expr.ty) != Some(iface)
    {
        return Ok(false);
    }
    match ctx.symbols.value_type(&expr.ty)? {
        ValType::Ref(RefType { heap_type, .. }) => {
            emitter.instruction(Instruction::RefNull(heap_type));
        }
        other => {
            return Err(crate::codegen::internal_failure(format!(
                "static-interface binding lowered to {other:?}"
            )));
        }
    }
    Ok(true)
}

/// Same as [`emit_method_call`], but assumes the receiver has
/// already been evaluated onto the stack at its
/// strip-null-then-`ref.cast` non-null Wasm shape (the contract
/// `emit_chain_parts` establishes before calling
/// [`emit_chain_access`]). Used optional-chain
/// `MethodCall` arm; the same three dispatch paths apply.
#[allow(clippy::too_many_arguments)]
fn emit_method_call_with_receiver_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    recv_ty: &Type,
    iface: &crate::MangledName,
    method: &str,
    args: &[ExprId],
    generic_args: Option<&[crate::GenericArgument]>,
    return_cast: Option<&Type>,
    call_ret_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let concrete_receiver = crate::typechecker::infer::narrowing::strip_null(recv_ty);
    crate::codegen::field_guards::attach(emitter, ctx, &concrete_receiver)?;
    let direct_key = crate::mangle::extend(iface, method);
    if let Some(func_idx) = ctx.symbols.func_idx(&direct_key) {
        if matches!(recv_ty.primitive_behavior(), Type::Number)
            && ctx.symbols.value_type(recv_ty)? != ValType::F64
        {
            super::cast::emit_cast_to(emitter, ctx, &Type::Number)?;
        }
        // Direct or Static dispatch. The receiver is on the stack;
        // adjust per the interface's dispatch kind:
        // - `Dispatch::Direct` — keep it (with a defensive
        //   `ref.as_non_null` for InterfaceRef receivers whose
        //   declared Wasm shape is `(ref null $Object)`).
        // - `Dispatch::Static` (Console, Uint8ArrayConstructor) —
        //   drop it; the wrapper sees only the user args.
        // - `Dispatch::VTable` / unrecorded — keep it as-is.
        match ctx.symbols.iface_dispatch(iface) {
            Some(crate::Dispatch::Direct) => {
                // InterfaceRef-typed receivers (instance
                // interface values like `TextEncoder`, future
                // `File` / `Stream`) lower to `(ref null $Object)`.
                // The Direct wrapper takes `(ref $Object)` (non-
                // null), so coerce. Primitive Direct receivers
                // (Number/Boolean/String/Array/Uint8Array) already
                // have non-null Wasm value-types — no extra cast.
                //
                // peel so aliased InterfaceRef (e.g.
                // `type Query = Map<string, string>`) still
                // triggers the coercion.
                if matches!(recv_ty.peel(), Type::InterfaceRef { .. }) {
                    emitter.instruction(Instruction::RefAsNonNull);
                }
            }
            Some(crate::Dispatch::Static) => {
                emitter.instruction(Instruction::Drop);
            }
            // Defensive: a registered direct-call wrapper without a
            // recorded dispatch means the imports loop didn't tag the
            // interface. Treat as Direct (the common case before this
            // refactor) so existing behaviour is preserved.
            None | Some(crate::Dispatch::VTable) => {}
        }
        // note: variadic methods (`Array#concat(...others)`)
        // arrive here with a synthesized `ArrayLiteral` already
        // occupying the rest slot — the typechecker pre-packs. So
        // `args.len()` matches the wrapper sig's user-arg count and
        // no codegen branching is needed.
        //
        // An optional-chain `MethodCall` reaches codegen with the signature
        // already substituted to concrete types, so the ABI is its only
        // surviving record of which slots the interface's generics erased.
        let abi = wrapper_abi_for_call(
            ctx,
            &direct_key,
            args,
            generic_args,
            return_cast,
            call_ret_ty,
        )?;
        emit_args_into_slots(emitter, ctx, args, Some(&abi.params))?;
        emitter.instruction(Instruction::Call(func_idx));
        emit_slot_return_cast(emitter, ctx, call_ret_ty, Some(&abi))?;
        return Ok(());
    }
    // Class method (static path): the receiver is `(ref $Foo)` on the stack.
    // Dispatch through the class's own vtable slot (4+); `class_method_slot` is
    // only populated for classes, so a `Some` here means a genuine class method
    // (a user method named `toString` etc. also resolves here, not slot 0–3).
    if ctx.symbols.class_method_slot(iface, method).is_some() {
        emit_class_vtable_dispatch(emitter, ctx, iface, method, args, call_ret_ty)?;
        return Ok(());
    }
    if let Some(slot) = vtable_slot_for_method(method) {
        emit_vtable_dispatch_on_object_stack(emitter, ctx, slot);
        return Ok(());
    }
    // shape-based dispatch for user-declared interface
    // methods. The receiver is `Type::InterfaceRef`, the method
    // came through `find_method` against a `VTable`-dispatch
    // interface, and there's no built-in vtable slot. Lower as
    // field-name scan → cast to closure → call_ref, exactly as if
    // the user had written `(recv.method)(args)` against a
    // function-typed field. Reuses the `$ObjectShape` field-name scan
    // machinery and the closure ABI.
    if matches!(recv_ty.peel(), Type::InterfaceRef { .. }) {
        emit_interface_method_via_shape_with_receiver_on_stack(
            emitter,
            ctx,
            method,
            args,
            call_ret_ty,
            None,
        )?;
        return Ok(());
    }
    // Fallthrough: no direct-dispatch wrapper, no vtable slot,
    // not an interface-shape dispatch. The typechecker accepted
    // this call, so an emit path landed here in error — trap with
    // a clear failure at the source span.
    emitter.instruction(Instruction::Unreachable);
    Ok(())
}

/// shape-based interface method dispatch. The receiver —
/// an `InterfaceRef`-typed value carrying its `$Object` shape — is
/// already on the stack; the method's closure lives in a field slot
/// named by `method`. Pull it via the `$ObjectShape` field-name scan
/// (slot 2), cast to the matching closure struct, then invoke
/// through the closure ABI (env + boxed args + funcref). The
/// leading `RefAsNonNull` + `RefCastNonNull($ObjectShape)` is
/// retained because the on-stack value's static type may be a wider
/// `(ref $Object)` (or `(ref null $Object)`) — the cast pins it to
/// the object shape the field scan expects.
///
/// `slot_index_local` names a local that already holds the method slot's index.
/// The accessor paths resolve it to decide *whether* to dispatch, and the scan
/// is the expensive part of this sequence, so they hand it over rather than
/// repeat it; every other caller passes `None`.
fn emit_interface_method_via_shape_with_receiver_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    method: &str,
    args: &[ExprId],
    call_ret_ty: &Type,
    slot_index_local: Option<u32>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let args: Vec<_> = args
        .iter()
        .copied()
        .map(ShapeArgument::Expression)
        .collect();
    emit_shape_method_with_arguments(emitter, ctx, method, &args, call_ret_ty, slot_index_local)?;
    Ok(())
}

enum ShapeArgument {
    Expression(ExprId),
    Local { slot: u32, ty: Type },
}

impl ShapeArgument {
    fn ty(&self, ctx: &CodegenCtx) -> Result<Type, crate::compiler_error::CompilerFailure> {
        Ok(match self {
            Self::Expression(id) => ctx
                .ta
                .try_expr(*id)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone(),
            Self::Local { ty, .. } => ty.clone(),
        })
    }

    fn emit(
        &self,
        emitter: &mut FunctionEmitter,
        ctx: &CodegenCtx,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let _: () = match self {
            Self::Expression(id) => emit_expr(emitter, ctx, *id)?,
            Self::Local { slot, .. } => {
                emitter.instruction(Instruction::LocalGet(*slot));
            }
        };
        Ok(())
    }
}

fn emit_shape_method_with_arguments(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    method: &str,
    args: &[ShapeArgument],
    call_ret_ty: &Type,
    slot_index_local: Option<u32>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;

    // Build the closure signature from the arg types and the
    // call's return type. `classify` reduces to (arity, is_void).
    let arg_tys: Vec<Type> = args.iter().map(|a| a.ty(ctx)).collect::<Result<_, _>>()?;
    let fn_ty = Type::Function {
        params: arg_tys,
        ret: Box::new(call_ret_ty.clone()),
        predicate: None,
        // call-site arg shape — variadic packing has already
        // happened upstream by this point, so this signature is the
        // fixed-arity post-pack form.
        has_rest: false,
    };
    let closure_sig = crate::codegen::closures::classify(&fn_ty)?;
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(closure_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure struct registered for the method's signature (SUB-166)",
            )
        })?;
    let fn_type_idx = ctx
        .symbols
        .closure_func_type_idx(closure_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure funcref type registered for the method's signature",
            )
        })?;

    let rcv_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object_shape),
    }))?;
    let closure_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_struct_idx),
    }))?;

    // 1. Receiver is on stack as `(ref [null] $Object)` (or a
    //    narrower subtype). The field scan expects `(ref
    //    $ObjectShape)` (non-null subtype). Strip null and downcast.
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    emitter.instruction(Instruction::LocalSet(rcv_local));

    // 2. Read the method's slot: by index when the caller already scanned it,
    //    else by the field-name scan.
    if let Some(index_local) = slot_index_local {
        emit_field_slot_get(emitter, intrinsics, rcv_local, index_local);
    } else {
        let name_global = ctx
            .symbols
            .field_name_string_global_idx(method)
            .ok_or_else(|| {
                crate::codegen::internal_failure(
                    ("per-name string global recorded during field-name-strings emission \
             for interface method names")
                        .to_string(),
                )
            })?;
        emit_object_field_read_by_name(emitter, ctx, rcv_local, name_global);
    }

    let method_value = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(method_value));
    let mut evaluated_args = Vec::with_capacity(args.len());
    for arg in args {
        arg.emit(emitter, ctx)?;
        cast::emit_box(emitter, ctx, &arg.ty(ctx)?)?;
        let slot = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
        emitter.instruction(Instruction::LocalSet(slot));
        evaluated_args.push(slot);
    }
    emitter.instruction(Instruction::LocalGet(method_value));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        intrinsics.closure,
    )));
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    crate::codegen::throw::emit_type_error_throw(emitter, ctx, "Value is not callable");
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(method_value));

    // 3. Cast to the matching closure struct type. closure
    // ABI: every Function-typed value stored in an `$Object` slot is
    // a `(ref $closure_<sig>)`.
    crate::codegen::closure_coercions::emit_erased_cast(
        emitter,
        ctx,
        crate::codegen::closures::ClosureSig::of(args.len(), call_ret_ty)?,
    )?;
    emitter.instruction(Instruction::LocalTee(closure_local));

    // 4. Push the closure env (slot 2) as the implicit first arg.
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 2,
    });

    crate::codegen::this_binding::bind(emitter, ctx, rcv_local)?;
    // 5. Emit each user arg, boxing to `(ref $Object)` per the
    // closure ABI's erasure.
    for slot in evaluated_args {
        emitter.instruction(Instruction::LocalGet(slot));
    }

    // 6. Push the funcref (slot 1) and call_ref.
    emitter.instruction(Instruction::LocalGet(closure_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::CallRef(fn_type_idx));

    // 7. Validate and unbox the erased return. Callers hand this the declared
    // return type verbatim; a mismatched implementation throws `TypeError`.
    let _: () = if !call_ret_ty.is_void() {
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            call_ret_ty,
        )?;
    };
    Ok(())
}

/// Map a built-in method name to its slot index in the universal
/// `$VTable`. The four-slot layout (toString / toJson / equals /
/// hash) is hard-coded everywhere in the runtime — see
/// [`crate::codegen::intrinsics::IntrinsicTypeIndices`] — and stays
/// hardcoded here until per-interface vtables make slot
/// indices interface-relative.
///
/// Returns `None` for any name not in the four-slot table; the
/// caller treats `None` as "no vtable dispatch path for this
/// method".
fn vtable_slot_for_method(method: &str) -> Option<u32> {
    match method {
        "toString" => Some(0),
        "toJson" => Some(1),
        "equals" => Some(2),
        "hash" => Some(3),
        _ => None,
    }
}

/// Emit `recv.vtable.<slot>(recv)` for an `$Object`-shaped receiver.
/// Stack at exit: the slot's return type (today: `(ref $string)`
/// for slot 0, the only slot reachable through `emit_method_call`).
/// `Type::Array` also flows here through generic receivers because
/// `$Array <: $Object`; the implicit subtype upcast lets the
/// `(ref $Object)` field-0 access work uniformly.
/// Dispatches a vtable method (`slot`) on a receiver — a non-null
/// `(ref $Object)` (or subtype) — already on the stack. The nullable-union
/// `JSON.stringify` arm evaluates the receiver once into a local, null-checks
/// it, then re-pushes the non-null cast into this tail.
pub(super) fn emit_vtable_dispatch_on_object_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    slot: u32,
) {
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsics declared by codegen entry",
    ) else {
        return;
    };
    // Per-slot funcref type. Slots 0 (toString) and 1 (toJson) share
    // operational signature `(ref $Object) -> (ref $string)` but are
    // declared as distinct sub-funcrefs inside the `$Object` rec
    // group, so `call_ref` needs the slot's own type index.
    let fn_type_idx = match slot {
        0 => intrinsics.to_string_fn,
        1 => intrinsics.to_json_fn,
        _ => {
            ctx.fail("vtable string dispatch requires toString or toJson slot");
            return;
        }
    };
    let Some(obj_local) = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))) else {
        return;
    };
    let Some(fn_local) = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(fn_type_idx),
    }))) else {
        return;
    };
    // the receiver may arrive as `(ref null $Object)` when
    // it's typed `unknown`. Coerce to non-null before stashing in
    // `obj_local`. Traps on null at runtime — calling `.toString()`
    // on null isn't meaningful, and narrowed-away null branches
    // never reach here.
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalTee(obj_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: slot,
    });
    emitter.instruction(Instruction::LocalSet(fn_local));
    emitter.instruction(Instruction::LocalGet(obj_local));
    emitter.instruction(Instruction::LocalGet(fn_local));
    emitter.instruction(Instruction::CallRef(fn_type_idx));
}

/// Push each argument, coercing it into the callee's recorded slot type.
/// `slots` is `None` when no ABI was recorded, in which case the arguments are
/// already at their declared types.
fn emit_args_into_slots(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    args: &[ExprId],
    slots: Option<&[ValType]>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if slots.is_some_and(|slots| slots.len() != args.len()) {
        return Err(crate::codegen::internal_failure(
            "call ABI argument count mismatch",
        ));
    }
    let locals: Vec<_> = args
        .iter()
        .map(|&arg| {
            emit_expr(emitter, ctx, arg)?;
            let local = emitter.add_anonymous_local(
                ctx.symbols.value_type(
                    &ctx.ta
                        .try_expr(arg)
                        .map_err(crate::codegen::arena_failure)?
                        .ty,
                )?,
            )?;
            emitter.instruction(Instruction::LocalSet(local));
            Ok::<_, crate::compiler_error::CompilerFailure>(local)
        })
        .collect::<Result<_, _>>()?;
    for (i, (&arg, local)) in args.iter().zip(locals).enumerate() {
        emitter.instruction(Instruction::LocalGet(local));
        if let Some(slot) = slots.and_then(|s| s.get(i).copied()) {
            cast::emit_coerce_to_wasm_slot(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(arg)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
                slot,
            )?;
        }
    }
    Ok(())
}

/// The physical signature a Direct/Static-dispatch wrapper call must be
/// coerced into: the recorded one, or the same shape rebuilt from a
/// [`GenericMethodCall`](TypedExprKind::GenericMethodCall)'s typecheck-time
/// metadata, which describes the same erasure one call at a time.
///
/// A wrapper's `func_idx` and its recorded ABI come from two loops gated on
/// different sets — `is_value_used` for the value-symbol walk, versus
/// `is_interface_member_used` (which also skips `Dispatch::VTable` interfaces
/// wholesale) for the walk that records the ABI. A promotion pass keeps the
/// two aligned; nothing in either loop *enforces* it, so a resolvable wrapper
/// with no record stays possible, in the same sense as the "defensive" arm on
/// the dispatch match above. Rebuilding costs a branch and removes a silent
/// mis-emit — args pushed at their declared types into erased slots — from the
/// failure modes of a future gating change.
fn wrapper_abi_for_call(
    ctx: &CodegenCtx,
    direct_key: &crate::MangledName,
    args: &[ExprId],
    generic_args: Option<&[crate::GenericArgument]>,
    return_cast: Option<&Type>,
    call_ret_ty: &Type,
) -> Result<MethodSlotAbi, crate::compiler_error::CompilerFailure> {
    if let Some(recorded) = ctx.symbols.iface_method_abi(direct_key) {
        return Ok(recorded.clone());
    }
    let erased = ctx.symbols.value_type(&Type::Unknown)?;
    let slot_is_erased =
        |i: usize| generic_args.is_some_and(|flags| flags.get(i).is_some_and(|g| g.is_generic));
    Ok(MethodSlotAbi {
        params: args
            .iter()
            .enumerate()
            .map(|(i, &a)| {
                Ok::<_, crate::compiler_error::CompilerFailure>(if slot_is_erased(i) {
                    erased
                } else {
                    ctx.symbols.value_type(
                        &ctx.ta
                            .try_expr(a)
                            .map_err(crate::codegen::arena_failure)?
                            .ty,
                    )?
                })
            })
            .collect::<Result<_, _>>()?,
        ret: match return_cast {
            Some(_) => Some(erased),
            None => (!call_ret_ty.is_void())
                .then(|| ctx.symbols.value_type(call_ret_ty))
                .transpose()?,
        },
    })
}

/// Cast an erased slot result back to the call-site type. A slot whose result
/// already matches needs nothing.
fn emit_slot_return_cast(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    call_ret_ty: &Type,
    abi: Option<&MethodSlotAbi>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if matches!(call_ret_ty.peel(), Type::Never) {
        emitter.instruction(Instruction::Unreachable);
        return Ok(());
    }
    if call_ret_ty.is_void() {
        return Ok(());
    }
    if matches!(call_ret_ty, Type::Unknown) {
        match abi.and_then(|abi| abi.ret) {
            Some(ValType::F64) => cast::emit_box(emitter, ctx, &Type::Number)?,
            Some(ValType::I32) => cast::emit_box(emitter, ctx, &Type::Boolean)?,
            _ => {}
        }
        return Ok(());
    }
    if abi.and_then(|a| a.ret) != Some(ctx.symbols.value_type(call_ret_ty)?) {
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            call_ret_ty,
        )?;
    }
    crate::codegen::field_guards::attach(emitter, ctx, call_ret_ty)?;
    Ok(())
}

/// Static-path class method dispatch. The receiver `(ref $Foo)` is on the stack.
/// Loads the vtable (struct slot 0) of the receiver's *static* class, reads the
/// method funcref at its slot, then `call_ref`s it with the receiver as self +
/// the args. Slots are inherited at the same index, so a parent-typed receiver
/// dispatches to the runtime override automatically.
///
/// Generic-class slots are erased: the physical sig takes/returns
/// `(ref null $Object)` where the declared type was a class type param. The
/// slot's recorded [`MethodSlotAbi`] drives coercion of the args and the
/// cast of the erased return back to `call_ret_ty` — self-sufficiently, since
/// plain `MethodCall`, optional-chain parts, and the synthesized accessor
/// calls carry no generic metadata.
fn emit_class_vtable_dispatch(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    class: &crate::MangledName,
    method: &str,
    args: &[ExprId],
    call_ret_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let struct_idx = ctx
        .symbols
        .class_struct_type_idx(class)
        .ok_or_else(|| crate::codegen::internal_failure("class struct type recorded"))?;
    let vtable_idx = ctx
        .symbols
        .class_vtable_type_idx(class)
        .ok_or_else(|| crate::codegen::internal_failure("class vtable type recorded"))?;
    let slot = ctx
        .symbols
        .class_method_slot(class, method)
        .ok_or_else(|| crate::codegen::internal_failure("class method slot recorded"))?;
    let sig_idx = ctx
        .symbols
        .class_method_sig(class, method)
        .ok_or_else(|| crate::codegen::internal_failure("class method sig recorded"))?;

    let rcv_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(struct_idx),
    }))?;
    // Defensive cast: the receiver is `(ref $Foo)` on the direct path, but the
    // optional-chain path may hand us a wider `(ref null $Object)`.
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(struct_idx)));
    emitter.instruction(Instruction::LocalSet(rcv_local));

    // Load the method funcref from the vtable, stash it.
    emitter.instruction(Instruction::LocalGet(rcv_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: struct_idx,
        field_index: 0,
    });
    emitter.instruction(Instruction::StructGet {
        struct_type_index: vtable_idx,
        field_index: slot,
    });
    let fn_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(sig_idx),
    }))?;
    emitter.instruction(Instruction::LocalSet(fn_local));

    // self (`(ref $Foo)` <: the sig's `(ref $Object)`), then args, then funcref.
    let abi = ctx.symbols.class_method_abi(class, method).cloned();
    emitter.instruction(Instruction::LocalGet(rcv_local));
    emit_args_into_slots(
        emitter,
        ctx,
        args,
        abi.as_ref().map(|a| a.params.as_slice()),
    )?;
    emitter.instruction(Instruction::LocalGet(fn_local));
    emitter.instruction(Instruction::CallRef(sig_idx));
    emit_slot_return_cast(emitter, ctx, call_ret_ty, abi.as_ref())?;
    Ok(())
}

fn emit_intrinsic_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    kind: Intrinsic,
    args: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match kind {
        Intrinsic::Assert => {
            // `assert(cond, msg)` — throw `new Error(msg)` on false.
            // JS-style left-to-right argument evaluation: cond first,
            // then msg (stashed in a local for the throw path).
            if args.len() != 2 {
                return Err(crate::codegen::internal_failure(
                    "assert requires two arguments",
                ));
            }
            emit_expr(emitter, ctx, args[0])?;
            cast::emit_condition_to_i32(
                emitter,
                ctx,
                &ctx.ta
                    .try_expr(args[0])
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
            emit_expr(emitter, ctx, args[1])?;
            let message_type = &ctx
                .ta
                .try_expr(args[1])
                .map_err(crate::codegen::arena_failure)?
                .ty;
            let msg_local = emitter.add_anonymous_local(ctx.symbols.value_type(message_type)?)?;
            emitter.instruction(Instruction::LocalSet(msg_local));
            emitter.instruction(Instruction::I32Eqz);
            emitter.emit_if(BlockType::Empty);
            emitter.instruction(Instruction::LocalGet(msg_local));
            cast::emit_coerce_to_slot(emitter, ctx, message_type, &Type::String)?;
            let ctor_idx = ctx
                .symbols
                .func_idx(&crate::mangle::prelude("Error#constructor"))
                .ok_or_else(|| {
                    crate::codegen::internal_failure("Error#constructor exported from prelude")
                })?;
            emitter.instruction(Instruction::Call(ctor_idx));
            crate::codegen::throw::emit_error_throw(emitter, ctx);
            emitter.emit_end();
        }
        Intrinsic::JsonStringify => {
            super::json::emit_stringify(emitter, ctx, args)?;
        }
        Intrinsic::JsonParse => {
            super::json::emit_parse(emitter, ctx, args)?;
        }
        // `BigInt.fromString(s)` — the remaining
        // bigint-producing intrinsic after retired
        // `BigIntFromNumber` / `NumberFromBigInt` in favour of the
        // call-signature wrappers on `BigIntConstructor` /
        // `NumberConstructor`. Wraps the host result `(i32 sign, ref
        // $rawBigInt limbs)` into a `$bigint` struct.
        Intrinsic::BigIntFromString => {
            if args.len() != 1 {
                return Err(crate::codegen::internal_failure(
                    "BigInt.fromString requires one argument",
                ));
            }
            emit_expr(emitter, ctx, args[0])?;
            // The arg is `(ref $string)`; the host fn takes
            // `(ref $rawString)`. Pull the raw array out via slot 1.
            let string_type_idx = ctx
                .symbols
                .string_type_idx()
                .ok_or_else(|| crate::codegen::internal_failure("$string registered"))?;
            emitter.instruction(Instruction::StructGet {
                struct_type_index: string_type_idx,
                field_index: 1,
            });
            let host_idx = ctx
                .symbols
                .func_idx(&crate::mangle::host(
                    crate::runtime::BIGINT_MODULE_NAME,
                    "fromString",
                ))
                .ok_or_else(|| {
                    crate::codegen::internal_failure(
                        "submilli:bigint.fromString import recorded by codegen bootstrap",
                    )
                })?;
            emit_bigint_wrap_host_result(emitter, ctx, host_idx);
        }
    };
    Ok(())
}

/// Emit `lhs.vtable.equals(lhs, rhs)` for two operands of `$Object`
/// subtypes (objects, arrays, functions, boxed values). Stack at entry: `[lhs, rhs]`.
/// Stack at exit: `[i32]` (1 if equal, 0 otherwise — flipped via
/// `I32Eqz` for `BinOp::NotEq`).
fn emit_vtable_equality(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, op: BinOp) {
    ctx.latch(emit_vtable_equality_checked(emitter, ctx, op));
}

fn emit_vtable_equality_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: BinOp,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    // The operands' own types may differ (two function types of different
    // arities), so both are stashed at the common `$Object` shape.
    let operand_ref = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let object_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let equals_fn_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.equals_fn),
    });

    // Stack: [lhs, rhs]. Stash both, load equals fn, restack args.
    let Some(lhs_local) = ctx.latch(emitter.add_anonymous_local(operand_ref)) else {
        return Ok(());
    };
    let Some(rhs_local) = ctx.latch(emitter.add_anonymous_local(operand_ref)) else {
        return Ok(());
    };
    let Some(lhs_object) = ctx.latch(emitter.add_anonymous_local(object_ref)) else {
        return Ok(());
    };
    let Some(rhs_object) = ctx.latch(emitter.add_anonymous_local(object_ref)) else {
        return Ok(());
    };
    let Some(eq_fn_local) = ctx.latch(emitter.add_anonymous_local(equals_fn_ref)) else {
        return Ok(());
    };

    emitter.instruction(Instruction::LocalSet(rhs_local));
    emitter.instruction(Instruction::LocalSet(lhs_local));

    emitter.instruction(Instruction::LocalGet(lhs_local));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalSet(lhs_object));
    emitter.instruction(Instruction::LocalGet(rhs_local));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalSet(rhs_object));

    emitter.instruction(Instruction::LocalGet(lhs_object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalSet(eq_fn_local));

    emitter.instruction(Instruction::LocalGet(lhs_object));
    emitter.instruction(Instruction::LocalGet(rhs_object));
    emitter.instruction(Instruction::LocalGet(eq_fn_local));
    emitter.instruction(Instruction::CallRef(intrinsics.equals_fn));

    if matches!(op, BinOp::NotEq) {
        emitter.instruction(Instruction::I32Eqz);
    }

    Ok(())
}

/// Boxed, null-aware `===` / `!==` dispatch. Used when either operand's static
/// type admits null, or when the operands have different Wasm representations.
/// Bridges the operands to a
/// common Wasm shape (`(ref null $Object)`) via `emit_box`, then
/// emits a 4-way condition:
///
/// - both null → equal (1).
/// - lhs null, rhs non-null → unequal (0).
/// - lhs non-null, rhs null → unequal (0).
/// - both non-null → `lhs.vtable.equals(lhs, rhs)`.
///
/// Final `I32Eqz` flips the result for `NotEq`.
fn emit_boxed_eq(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op: BinOp,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let object_null_ref = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let object_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let equals_fn_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.equals_fn),
    });

    let lhs_ty = ctx
        .ta
        .try_expr(lhs)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    let rhs_ty = ctx
        .ta
        .try_expr(rhs)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();

    // Materialize lhs as (ref null $Object). `emit_box` wraps
    // primitive Wasm values into their `$Boxed*` shape and is a
    // no-op for ref types / unions, so both sides land on the
    // stack at the universal nullable shape.
    let lhs_local = emitter.add_anonymous_local(object_null_ref)?;
    let rhs_local = emitter.add_anonymous_local(object_null_ref)?;
    let lhs_nonnull_local = emitter.add_anonymous_local(object_ref)?;
    let eq_fn_local = emitter.add_anonymous_local(equals_fn_ref)?;

    emit_expr(emitter, ctx, lhs)?;
    cast::emit_box(emitter, ctx, &lhs_ty)?;
    emitter.instruction(Instruction::LocalSet(lhs_local));

    emit_expr(emitter, ctx, rhs)?;
    cast::emit_box(emitter, ctx, &rhs_ty)?;
    emitter.instruction(Instruction::LocalSet(rhs_local));

    // if lhs is null: result = (rhs is null)
    //     covers both-null (1) and lhs-only-null (0).
    // else: if rhs is null → 0; else vtable.equals dispatch.
    emitter.instruction(Instruction::LocalGet(lhs_local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(rhs_local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(rhs_local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_else();
    // Both non-null — vtable.equals on lhs's vtable.
    emitter.instruction(Instruction::LocalGet(lhs_local));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalTee(lhs_nonnull_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalSet(eq_fn_local));
    emitter.instruction(Instruction::LocalGet(lhs_nonnull_local));
    emitter.instruction(Instruction::LocalGet(rhs_local));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalGet(eq_fn_local));
    emitter.instruction(Instruction::CallRef(intrinsics.equals_fn));
    emitter.emit_end();
    emitter.emit_end();

    let _: () = if matches!(op, BinOp::NotEq) {
        emitter.instruction(Instruction::I32Eqz);
    };
    Ok(())
}

/// Emit a bigint literal. Small literals (≤ ±2^53 - 1)
/// fast-path through `submilli:bigint.fromNumber`; larger literals
/// are looked up in the bigint constants pool and materialized via
/// `array.new_data` over a per-program data segment, then wrapped
/// directly into `$bigint` with the prelude's vtable.
fn emit_bigint_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    digits: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    use crate::codegen::internal_failure;
    if let Some(entry) = ctx.bigints.lookup(digits)? {
        // Large literal — pack limbs from data segment, wrap with
        // bigint vtable directly (no host fn involved). Data-segment
        // index is offset by the string-pool count because string
        // segments come first in the module's data section.
        let raw_bigint_idx = ctx
            .symbols
            .raw_bigint_type_idx()
            .ok_or_else(|| internal_failure("the $rawBigInt intrinsic is not registered"))?;
        let bigint_idx = ctx
            .symbols
            .bigint_type_idx()
            .ok_or_else(|| internal_failure("the $bigint intrinsic is not registered"))?;
        let vtable_global = ctx
            .symbols
            .prelude_global_idx("bigint_vtable")
            .ok_or_else(|| internal_failure("bigint_vtable is not imported from the prelude"))?;
        let data_idx = crate::codegen::wasm_u32(ctx.strings.strings.len())?
            .checked_add(entry.data_idx)
            .ok_or_else(|| internal_failure("the module has too many data segments"))?;
        emitter.instruction(Instruction::GlobalGet(vtable_global));
        emitter.instruction(Instruction::I32Const(i32::from(entry.sign)));
        emitter.instruction(Instruction::I32Const(0));
        emitter.instruction(Instruction::I32Const(entry.limb_count.cast_signed()));
        emitter.instruction(Instruction::ArrayNewData {
            array_type_index: raw_bigint_idx,
            array_data_index: data_idx,
        });
        emitter.instruction(Instruction::StructNew(bigint_idx));
        return Ok(());
    }
    // The pool leaves only literals within f64's safe-integer range, so this
    // conversion is exact.
    // Construct via `submilli:bigint.fromNumber` + standard wrap.
    let v: i64 = digits
        .parse()
        .map_err(|_| internal_failure("an unpooled bigint literal is not a safe integer"))?;
    let host_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            "fromNumber",
        ))
        .ok_or_else(|| internal_failure("submilli:bigint.fromNumber is not imported"))?;
    emitter.instruction(Instruction::F64Const(Ieee64::from(v as f64)));
    emit_bigint_wrap_host_result(emitter, ctx, host_idx);
    Ok(())
}

/// regex-literal lowering. Each literal site allocates a
/// fresh `$regex` (JS-faithful per-site `lastIndex` semantics; see
/// `docs/regex.md`). Emit the source and flags as constant `$string`
/// values, then call the prelude's `RegExpConstructor#new` wrapper —
/// which invokes the `submilli:regex.compile` host fn and assembles
/// the receiver struct.
///
/// The pattern + flags have already been validated at infer time
/// (`typechecker::infer::expr::infer_regex` calls
/// `runtime::prelude::regex::engine::build_regex`), so the wrapper's compile call
/// here only fails on memory-cap exhaustion, which traps cleanly.
fn emit_regex_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source: &str,
    flags: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    super::emit_const_string_by_text(emitter, ctx, source)?;
    super::emit_const_string_by_text(emitter, ctx, flags)?;
    let ctor_idx = ctx
        .symbols
        .func_idx(&crate::mangle::prelude("RegExpConstructor#new"))
        .ok_or_else(|| crate::codegen::internal_failure("RegExpConstructor#new is not imported"))?;
    emitter.instruction(Instruction::Call(ctor_idx));
    Ok(())
}

/// helper — given a `(ref $bigint)` on top of the stack,
/// destructure it into `(i32 sign, ref $rawBigInt limbs)` on the
/// stack. Used by every codegen site that hands a bigint to a host
/// fn (`bigint.add` / `bigint.cmp` / `bigint.neg` /
/// `number.fromBigInt`).
fn emit_bigint_extract_to_stack(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(emit_bigint_extract_to_stack_checked(emitter, ctx));
}

fn emit_bigint_extract_to_stack_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let bigint_idx = ctx
        .symbols
        .bigint_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$bigint registered"))?;
    // Stash the receiver in a scratch local since we need two
    // struct.get reads.
    let Some(scratch) = ctx.latch(emitter.add_anonymous_local(wasm_encoder::ValType::Ref(
        wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(bigint_idx),
        },
    ))) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(scratch));
    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: bigint_idx,
        field_index: 1, // sign
    });
    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: bigint_idx,
        field_index: 2, // limbs
    });

    Ok(())
}

/// helper — call `host_func_idx` (whose result shape is
/// the standard `(i32 sign, ref $rawBigInt limbs)` bigint pair),
/// then wrap into a fresh `$bigint` struct with the prelude's
/// vtable. Stack at exit: `(ref $bigint)`.
fn emit_bigint_wrap_host_result(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    host_func_idx: u32,
) {
    ctx.latch(emit_bigint_wrap_host_result_checked(
        emitter,
        ctx,
        host_func_idx,
    ));
}

fn emit_bigint_wrap_host_result_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    host_func_idx: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::Call(host_func_idx));
    let bigint_idx = ctx
        .symbols
        .bigint_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$bigint registered"))?;
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$rawBigInt registered"))?;
    let vtable_global = ctx
        .symbols
        .prelude_global_idx("bigint_vtable")
        .ok_or_else(|| {
            crate::codegen::internal_failure("bigint_vtable imported from prelude bootstrap")
        })?;
    // Stack: [sign, limbs]. Need [vtable, sign, limbs] for
    // struct.new $bigint. Use two scratch locals to reorder.
    let Some(scratch_limbs) = ctx.latch(emitter.add_anonymous_local(wasm_encoder::ValType::Ref(
        wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        },
    ))) else {
        return Ok(());
    };
    let Some(scratch_sign) = ctx.latch(emitter.add_anonymous_local(wasm_encoder::ValType::I32))
    else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(scratch_limbs));
    emitter.instruction(Instruction::LocalSet(scratch_sign));
    emitter.instruction(Instruction::GlobalGet(vtable_global));
    emitter.instruction(Instruction::LocalGet(scratch_sign));
    emitter.instruction(Instruction::LocalGet(scratch_limbs));
    emitter.instruction(Instruction::StructNew(bigint_idx));

    Ok(())
}

/// inline binary bigint op. `op_name` is one of
/// "add" / "sub" / "mul" / "div" / "mod" — looked up
/// directly against `submilli:bigint.<op>`.
fn emit_bigint_binop_inline(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op_name: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr(emitter, ctx, lhs)?;
    emit_bigint_extract_to_stack(emitter, ctx);
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$rawBigInt registered"))?;
    let lhs_limbs =
        emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        }))?;
    let lhs_sign = emitter.add_anonymous_local(wasm_encoder::ValType::I32)?;
    emitter.instruction(Instruction::LocalSet(lhs_limbs));
    emitter.instruction(Instruction::LocalSet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_limbs));
    emit_expr(emitter, ctx, rhs)?;
    emit_bigint_extract_to_stack(emitter, ctx);
    let host_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            op_name,
        ))
        .ok_or_else(|| {
            crate::codegen::internal_failure(format!(
                "submilli:bigint.{op_name} import recorded by codegen bootstrap"
            ))
        })?;
    emit_bigint_wrap_host_result(emitter, ctx, host_idx);
    Ok(())
}

/// postfix `++` / `--` compute step for a bigint operand —
/// stack `[orig (ref $bigint)]` → `[new (ref $bigint)]`. Mirrors
/// [`emit_bigint_binop_inline`] but skips the lhs `emit_expr` (it's
/// already on the stack) and emits the rhs as a synthesized `1n`
/// literal via [`emit_bigint_literal`] (small-literal path:
/// `f64.const 1` + `submilli:bigint.fromNumber`).
fn emit_bigint_pm_one(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, op: crate::PostfixOp) {
    ctx.latch(emit_bigint_pm_one_checked(emitter, ctx, op));
}

fn emit_bigint_pm_one_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: crate::PostfixOp,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_bigint_extract_to_stack(emitter, ctx);
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$rawBigInt registered"))?;
    let Some(lhs_limbs) = ctx.latch(emitter.add_anonymous_local(wasm_encoder::ValType::Ref(
        wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        },
    ))) else {
        return Ok(());
    };
    let Some(lhs_sign) = ctx.latch(emitter.add_anonymous_local(wasm_encoder::ValType::I32)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(lhs_limbs));
    emitter.instruction(Instruction::LocalSet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_limbs));
    if ctx.latch(emit_bigint_literal(emitter, ctx, "1")).is_none() {
        return Ok(());
    }
    emit_bigint_extract_to_stack(emitter, ctx);
    let op_name = match op {
        crate::PostfixOp::Inc => "add",
        crate::PostfixOp::Dec => "sub",
        crate::PostfixOp::NonNullAssert => {
            return Err(crate::codegen::internal_failure(
                "non-null assertion is not PostfixUnary",
            ));
        }
    };
    let host_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            op_name,
        ))
        .ok_or_else(|| {
            crate::codegen::internal_failure(format!(
                "submilli:bigint.{op_name} import recorded by codegen bootstrap"
            ))
        })?;
    emit_bigint_wrap_host_result(emitter, ctx, host_idx);

    Ok(())
}

/// inline `cmp` + signed compare against 0 for
/// `<` / `>` / `<=` / `>=`.
fn emit_bigint_cmp_inline(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op: BinOp,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_bigint_cmp_call(emitter, ctx, lhs, rhs)?;
    emitter.instruction(Instruction::I32Const(0));
    let inst = match op {
        BinOp::Lt => Instruction::I32LtS,
        BinOp::Gt => Instruction::I32GtS,
        BinOp::Le => Instruction::I32LeS,
        BinOp::Ge => Instruction::I32GeS,
        _ => {
            return Err(crate::codegen::internal_failure(
                "emit_bigint_cmp_inline called with non-cmp op",
            ));
        }
    };
    emitter.instruction(inst);
    Ok(())
}

/// `===` / `!==` on bigint — equal iff `cmp == 0`.
fn emit_bigint_cmp_eq_inline(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op: BinOp,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_bigint_cmp_call(emitter, ctx, lhs, rhs)?;
    emitter.instruction(Instruction::I32Eqz);
    let _: () = if matches!(op, BinOp::NotEq) {
        emitter.instruction(Instruction::I32Eqz);
    };
    Ok(())
}

/// shared "two bigint operands → submilli:bigint.cmp"
/// setup. Stack at exit: `i32` (the raw cmp result, -1/0/1).
fn emit_bigint_cmp_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr(emitter, ctx, lhs)?;
    emit_bigint_extract_to_stack(emitter, ctx);
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$rawBigInt registered"))?;
    let lhs_limbs =
        emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        }))?;
    let lhs_sign = emitter.add_anonymous_local(wasm_encoder::ValType::I32)?;
    emitter.instruction(Instruction::LocalSet(lhs_limbs));
    emitter.instruction(Instruction::LocalSet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_limbs));
    emit_expr(emitter, ctx, rhs)?;
    emit_bigint_extract_to_stack(emitter, ctx);
    let cmp_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            "cmp",
        ))
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "submilli:bigint.cmp import recorded by codegen bootstrap",
            )
        })?;
    emitter.instruction(Instruction::Call(cmp_idx));
    Ok(())
}

/// Evaluate plain literal values once, including overwritten expressions.
fn evaluate_object_members(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    members: &[TypedObjectMember],
) -> Result<HashMap<ExprId, u32>, crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    let mut values = HashMap::new();
    for member in members {
        let TypedObjectMember::Value(value_id) = *member else {
            return Err(crate::codegen::internal_failure(
                "spread field reached plain object lowering",
            ));
        };
        let value_ty = &ctx
            .ta
            .try_expr(value_id)
            .map_err(crate::codegen::arena_failure)?
            .ty;
        emit_expr(emitter, ctx, value_id)?;
        cast::emit_box(emitter, ctx, value_ty)?;
        let local = emitter.add_anonymous_local(object_ref(intrinsics.object))?;
        emitter.instruction(Instruction::LocalSet(local));
        values.insert(value_id, local);
    }
    Ok(values)
}

/// Pushes whether the field at `index`, whose slot holds `value`, is present
/// with a value of `field_ty`, using the same presence flag as `in`.
fn emit_field_holds_value_of(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    object: u32,
    index: u32,
    value: u32,
    field_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
    crate::codegen::field_names::emit_name_presence(emitter, ctx)?;
    emitter.instruction(Instruction::LocalGet(value));
    emitter.instruction(Instruction::RefIsNull);
    emitter.instruction(Instruction::I32Eqz);
    emitter.instruction(Instruction::I32Or);
    let _: () = if field_runtime_type_is_testable(field_ty) {
        emit_structural_test(emitter, ctx, value, field_ty, field_ty)?;
        emitter.instruction(Instruction::I32And);
    };
    Ok(())
}

fn object_ref(object_type: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_type),
    })
}

/// Array indices use JavaScript's string-hint property-key conversion.
pub(super) fn emit_index_number(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if ty == &Type::Unknown {
        let function = ctx
            .symbols
            .prelude_func_idx("__value_to_index")
            .ok_or_else(|| crate::codegen::internal_failure("dynamic index helper collected"))?;
        emitter.instruction(Instruction::Call(function));
    } else {
        cast::emit_coerce_to_slot(emitter, ctx, ty, &Type::Number)?;
    };
    Ok(())
}

fn emit_computed_object(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    members: &[TypedObjectMember],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(intr) = ctx.require(ctx.symbols.intrinsic_type_indices(), "intrinsics declared")
    else {
        return Ok(());
    };
    let Some(merge) = ctx.require(
        ctx.symbols.prelude_func_idx("ObjectConstructor##spread"),
        "spread helper imported",
    ) else {
        return Ok(());
    };
    let object = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.object_shape),
    }))?;
    for _ in 0..4 {
        emitter.instruction(Instruction::RefNull(HeapType::Concrete(intr.object)));
    }
    emitter.instruction(Instruction::Call(merge));
    emitter.instruction(Instruction::LocalSet(object));
    for member in members {
        emitter.instruction(Instruction::LocalGet(object));
        match member {
            TypedObjectMember::Computed { key, value } => {
                emit_expr(emitter, ctx, *key)?;
                crate::codegen::cast_check::emit_operation_cast_on_stack(
                    emitter,
                    ctx,
                    &ctx.ta
                        .try_expr(*key)
                        .map_err(crate::codegen::arena_failure)?
                        .ty,
                    &Type::String,
                )?;
                emit_expr(emitter, ctx, *value)?;
                cast::emit_box(
                    emitter,
                    ctx,
                    &ctx.ta
                        .try_expr(*value)
                        .map_err(crate::codegen::arena_failure)?
                        .ty,
                )?;
                let Some(symbol) = ctx.require(
                    ctx.symbols.prelude_func_idx("ObjectConstructor##setField"),
                    "dynamic write imported",
                ) else {
                    return Ok(());
                };
                emitter.instruction(Instruction::Call(symbol));
            }
            TypedObjectMember::Spread { source, .. } => {
                emit_expr(emitter, ctx, *source)?;
                for _ in 0..2 {
                    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intr.object)));
                }
                emitter.instruction(Instruction::Call(merge));
                emitter.instruction(Instruction::LocalSet(object));
            }
            TypedObjectMember::Value(_) => {
                ctx.fail("computed literal contains an unlowered named field");
                return Ok(());
            }
        }
    }
    emitter.instruction(Instruction::LocalGet(object));
    Ok(())
}

fn emit_object_index_postfix(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    op: crate::PostfixOp,
    read_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emit_expr(emitter, ctx, receiver)?;
    cast::emit_box(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(receiver)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    let object = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(object));
    emit_expr(emitter, ctx, index)?;
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        &Type::String,
    )?;
    let key = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::String)?)?;
    emitter.instruction(Instruction::LocalSet(key));
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::LocalGet(key));
    let Some(symbol) = ctx.require(
        ctx.symbols.prelude_func_idx("ObjectConstructor##getField"),
        "record read imported",
    ) else {
        return Ok(());
    };
    emitter.instruction(Instruction::Call(symbol));
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &Type::Unknown,
        read_ty,
    )?;
    cast::emit_box(emitter, ctx, read_ty)?;
    emit_postfix_numeric(emitter, ctx, &Type::Unknown);
    let old = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalTee(old));
    emit_postfix_delta(emitter, ctx, op, &Type::Unknown);
    let updated = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(updated));
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::LocalGet(key));
    emitter.instruction(Instruction::LocalGet(updated));
    let Some(symbol) = ctx.require(
        ctx.symbols.prelude_func_idx("ObjectConstructor##setField"),
        "record write imported",
    ) else {
        return Ok(());
    };
    emitter.instruction(Instruction::Call(symbol));
    emitter.instruction(Instruction::LocalGet(old));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::invariant_tests::{assert_internal, with_context};
    use crate::{FileId, Span, TypedAst, TypedExpr};

    #[test]
    fn call_and_chain_metadata_are_checked_before_operands() {
        with_context(
            &TypedAst::new(),
            &crate::codegen::SymbolTable::default(),
            |ctx| {
                let mut emitter = FunctionEmitter::new(ctx, &[]).unwrap();
                assert_internal(
                    emit_intrinsic_call(&mut emitter, ctx, Intrinsic::Assert, &[]).unwrap_err(),
                );
                assert_internal(
                    emit_intrinsic_call(&mut emitter, ctx, Intrinsic::BigIntFromString, &[])
                        .unwrap_err(),
                );
                assert_internal(
                    emit_direct_call(
                        &mut emitter,
                        ctx,
                        false,
                        0,
                        &[Type::Number],
                        &Type::Void,
                        &[],
                    )
                    .unwrap_err(),
                );
                assert_internal(
                    emit_args_into_slots(&mut emitter, ctx, &[], Some(&[ValType::F64]))
                        .unwrap_err(),
                );
                assert_internal(
                    emit_chain_parts(
                        &mut emitter,
                        ctx,
                        &[],
                        1,
                        &Type::Number,
                        &Type::Number,
                        None,
                    )
                    .unwrap_err(),
                );
                assert_internal(
                    emit_chain_parts(
                        &mut emitter,
                        ctx,
                        &[],
                        0,
                        &Type::Number,
                        &Type::Number,
                        Some(&[]),
                    )
                    .unwrap_err(),
                );
            },
        );
    }

    #[test]
    fn uninterned_literal_fails_and_a_fresh_emitter_still_works() {
        let mut ta = TypedAst::new();
        let string = ta
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::String("missing".into()),
                ty: Type::String,
                span: Span::at(FileId(0)),
            })
            .unwrap();
        let number = ta
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(3.0),
                ty: Type::Number,
                span: Span::at(FileId(0)),
            })
            .unwrap();
        with_context(&ta, &crate::codegen::SymbolTable::default(), |ctx| {
            let mut failed = FunctionEmitter::new(ctx, &[]).unwrap();
            assert_internal(emit_expr(&mut failed, ctx, string).unwrap_err());
            drop(failed);
            let mut healthy = FunctionEmitter::new(ctx, &[]).unwrap();
            emit_expr(&mut healthy, ctx, number).unwrap();
            healthy.instruction(Instruction::Drop);
            healthy.build().unwrap();
        });
    }
}
