use crate::codegen::CodegenCtx;
use crate::codegen::bounds::{
    emit_checked_index, emit_checked_index_with_length, stash_array_length, stash_index_operand,
};
use crate::codegen::function_emitter::FunctionEmitter;
use crate::{
    EnumVariantPayload, ExprId, Ident, StmtId, Type, TypedStmtKind, TypedSwitchCase,
    TypedSwitchValue,
};
use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

use super::cast;
use super::expr::{
    emit_expr, emit_method_call, emit_object_property_write, stash_receiver_as_object_shape,
};

/// Does this statement contain a body some execution can skip, or re-enter?
///
/// That is exactly where emission order stops implying execution order, which
/// is the one thing a narrowing shadow depends on — see
/// [`FunctionEmitter::clear_all_narrow_shadows`]. Clearing is conservative (a
/// missing shadow only costs the narrowed read a cast at use), so a statement
/// kind that gains a body opts *out* of the rule rather than having to
/// remember to opt in.
fn branches(kind: &TypedStmtKind) -> bool {
    matches!(
        kind,
        TypedStmtKind::If { .. }
            | TypedStmtKind::While { .. }
            | TypedStmtKind::DoWhile { .. }
            | TypedStmtKind::For { .. }
            | TypedStmtKind::ForOf { .. }
            | TypedStmtKind::Switch { .. }
            | TypedStmtKind::Try { .. }
    )
}

pub fn emit_statement(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    id: StmtId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let stmt = ctx.ta.try_stmt(id).map_err(crate::codegen::arena_failure)?;
    emitter.record_span(stmt.span);
    // Before as well as after: a loop's back edge re-runs the reads above its
    // body's writes, so a shadow taken before the loop is already unsound at
    // the loop head.
    if branches(&stmt.kind) {
        emitter.clear_all_narrow_shadows();
    }
    match &stmt.kind {
        TypedStmtKind::Block(stmts) => {
            // Wasm locals are function-scoped, so a source-level block only
            // needs a fresh name-resolution scope — no Wasm `block`.
            emitter.push_scope();
            for &child in stmts {
                emit_statement(emitter, ctx, child)?;
            }
            emitter.pop_scope()?;
        }
        TypedStmtKind::Return(value) => {
            if let Some(expr_id) = value {
                emit_expr(emitter, ctx, *expr_id)?;
                cast::emit_coerce_to_return_slot(
                    emitter,
                    ctx,
                    &ctx.ta
                        .try_expr(*expr_id)
                        .map_err(crate::codegen::arena_failure)?
                        .ty,
                )?;
            }
            super::finally::emit_transfer(emitter, super::finally::Transfer::Return)?;
        }
        TypedStmtKind::Expr(expr_id) => {
            emit_expr(emitter, ctx, *expr_id)?;
            // Expression statements discard the result; `void` calls leave
            // nothing on the stack and need no Drop.
            let ty = &ctx
                .ta
                .try_expr(*expr_id)
                .map_err(crate::codegen::arena_failure)?
                .ty;
            if !ty.is_void() {
                emitter.instruction(Instruction::Drop);
            }
        }
        // top-level decls become global.set in _start; these are function-local
        TypedStmtKind::Let {
            name,
            ty,
            value,
            boxed,
            ..
        } => {
            emit_expr(emitter, ctx, *value)?;
            let value_ty = ctx
                .ta
                .try_expr(*value)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            cast::emit_coerce_to_slot(emitter, ctx, &value_ty, ty)?;
            if *boxed {
                let box_idx = ctx.symbols.box_type_idx(ty)?.ok_or_else(|| {
                    crate::codegen::internal_failure("box type registered for every boxed Let")
                })?;
                emitter.instruction(Instruction::StructNew(box_idx));
                let box_val = ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(box_idx),
                });
                let slot = emitter.define_local(name, box_val)?;
                emitter.instruction(Instruction::LocalSet(slot));
            } else {
                let slot = emitter.define_local(name, ctx.symbols.value_type(ty)?)?;
                emitter.instruction(Instruction::LocalSet(slot));
            }
        }
        TypedStmtKind::ReboxLocal { ident, ty } => {
            let slot = emitter.write_slot(&ident.name)?;
            let box_idx = ctx.symbols.box_type_idx(ty)?.ok_or_else(|| {
                crate::codegen::internal_failure("box type registered for every boxed Let")
            })?;
            emitter.instruction(Instruction::LocalGet(slot));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: box_idx,
                field_index: 0,
            });
            emitter.instruction(Instruction::StructNew(box_idx));
            emitter.instruction(Instruction::LocalSet(slot));
        }
        TypedStmtKind::Const {
            name, ty, value, ..
        } => {
            // captured const is copied into closure env, not boxed
            emit_expr(emitter, ctx, *value)?;
            let value_ty = ctx
                .ta
                .try_expr(*value)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            cast::emit_coerce_to_slot(emitter, ctx, &value_ty, ty)?;
            let slot = emitter.define_local(name, ctx.symbols.value_type(ty)?)?;
            emitter.instruction(Instruction::LocalSet(slot));
        }
        TypedStmtKind::AssignLocal {
            ident,
            target_ty,
            value,
            boxed,
            narrowed_shadow_ty,
        } => {
            let slot = emitter.write_slot(&ident.name)?;
            if *boxed {
                // The box's inner field is keyed on the binding's
                // declared type (`target_ty`), not the RHS type, so
                // a primitive RHS into a wider boxed slot lands the
                // boxed primitive in the right box variant.
                let box_idx = ctx.symbols.box_type_idx(target_ty)?.ok_or_else(|| {
                    crate::codegen::internal_failure(
                        "box type registered for every boxed AssignLocal",
                    )
                })?;
                let value_ty = ctx
                    .ta
                    .try_expr(*value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty
                    .clone();
                emitter.instruction(Instruction::LocalGet(slot));
                emit_expr(emitter, ctx, *value)?;
                cast::emit_coerce_to_slot(emitter, ctx, &value_ty, target_ty)?;
                emitter.instruction(Instruction::StructSet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
            } else {
                emit_expr(emitter, ctx, *value)?;
                let value_ty = ctx
                    .ta
                    .try_expr(*value)
                    .map_err(crate::codegen::arena_failure)?
                    .ty
                    .clone();
                // If this assignment installs a narrowing, allocate a fresh
                // shadow local of the narrowed type. `local.tee` keeps the RHS
                // on the stack for the subsequent coerce+set while also storing
                // into the shadow. Installing it makes `LocalNarrowRef` reads
                // resolve there — no per-use cast needed. The narrowed type can
                // differ from the RHS's own: a function narrows to the declared
                // member it fits, which may take more parameters.
                let stored_ty = match narrowed_shadow_ty {
                    Some(narrowed_ty) => {
                        cast::emit_coerce_to_slot(emitter, ctx, &value_ty, narrowed_ty)?;
                        let narrowed_val = ctx.symbols.value_type(narrowed_ty)?;
                        let shadow_idx = emitter.add_anonymous_local(narrowed_val)?;
                        emitter.instruction(Instruction::LocalTee(shadow_idx));
                        emitter.install_narrow_shadow(&ident.name, shadow_idx, narrowed_val)?;
                        narrowed_ty
                    }
                    None => &value_ty,
                };
                cast::emit_coerce_to_slot(emitter, ctx, stored_ty, target_ty)?;
                emitter.instruction(Instruction::LocalSet(slot));
            }
        }
        TypedStmtKind::AssignGlobal {
            mangled,
            target_ty,
            value,
            ..
        } => {
            emit_expr(emitter, ctx, *value)?;
            let value_ty = ctx
                .ta
                .try_expr(*value)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            cast::emit_coerce_to_slot(emitter, ctx, &value_ty, target_ty)?;
            let idx = ctx.symbols.global_idx(mangled).ok_or_else(|| {
                crate::codegen::internal_failure("Inferer guarantees the binding exists")
            })?;
            emitter.instruction(Instruction::GlobalSet(idx));
        }
        TypedStmtKind::AssignField {
            receiver,
            name,
            value,
        } => {
            emit_assign_field(emitter, ctx, *receiver, name, *value)?;
        }
        TypedStmtKind::AssignIndex {
            receiver,
            index,
            value,
            elem_ty,
        } => {
            emit_assign_index(emitter, ctx, *receiver, *index, *value, elem_ty)?;
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond_ty = ctx
                .ta
                .try_expr(*condition)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            emit_expr(emitter, ctx, *condition)?;
            cast::emit_condition_to_i32(emitter, ctx, &cond_ty)?;
            emitter.emit_if(BlockType::Empty);
            emit_statement(emitter, ctx, *then_block)?;
            if let Some(else_id) = else_block {
                emitter.emit_else();
                emit_statement(emitter, ctx, *else_id)?;
            }
            emitter.emit_end();
        }
        TypedStmtKind::While { condition, body } => {
            // `block { loop { <cond>; i32.eqz; br_if 1; <body>; br 0; } }`
            // — break exits the outer block, continue restarts the loop.
            emitter.emit_while_open();
            let cond_ty = ctx
                .ta
                .try_expr(*condition)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .clone();
            emit_expr(emitter, ctx, *condition)?;
            cast::emit_condition_to_i32(emitter, ctx, &cond_ty)?;
            emitter.instruction(Instruction::I32Eqz);
            let break_label = emitter.break_label()?;
            emitter.instruction(Instruction::BrIf(break_label));
            emit_statement(emitter, ctx, *body)?;
            let continue_label = emitter.continue_label()?;
            emitter.instruction(Instruction::Br(continue_label));
            emitter.emit_while_close();
        }
        TypedStmtKind::Break => {
            let transfer = super::finally::Transfer::Branch {
                depth: emitter.wasm_block_depth - emitter.break_label()? - 1,
                finally_floor: emitter.break_finally_floor(),
            };
            super::finally::emit_transfer(emitter, transfer)?;
        }
        TypedStmtKind::Continue => {
            let transfer = super::finally::Transfer::Branch {
                depth: emitter.wasm_block_depth - emitter.continue_label()? - 1,
                finally_floor: emitter.continue_finally_floor(),
            };
            super::finally::emit_transfer(emitter, transfer)?;
        }
        TypedStmtKind::For { .. } | TypedStmtKind::ForOf { .. } | TypedStmtKind::DoWhile { .. } => {
            return Err(crate::codegen::internal_failure(
                "for / for-of / do-while must be lowered by the desugar pass before codegen",
            ));
        }
        TypedStmtKind::Switch {
            discriminant,
            discriminant_ty,
            cases,
            default,
        } => {
            emit_switch(
                emitter,
                ctx,
                *discriminant,
                discriminant_ty,
                cases,
                default.as_ref().copied(),
            )?;
        }
        TypedStmtKind::NarrowRegion {
            path,
            source,
            binding,
            cast_info,
            body,
        } => {
            emitter.push_scope();
            if path.chain.is_empty() {
                let shadow_val = ctx.symbols.value_type(&cast_info.to_ty)?;
                let shadow = emitter.add_anonymous_local(shadow_val)?;
                emit_expr(emitter, ctx, *source)?;
                cast::emit_narrowing_cast(emitter, ctx, cast_info)?;
                emitter.instruction(Instruction::LocalSet(shadow));
                emitter.install_narrow_shadow(&binding.name, shadow, shadow_val)?;
            } else {
                // The predicate proved a field-path value that may already have
                // changed while evaluating the rest of the condition. Defer its
                // first re-read and checked cast until an actual use in the body.
                emitter.register_narrow_source(&binding.name, *source)?;
            }
            emit_statement(emitter, ctx, *body)?;
            emitter.pop_scope()?;
        }
        TypedStmtKind::Throw { value } => {
            // cast is unconditional: typechecker guarantees Error type; Wasm-level nullability comes from (ref null $Object) lowering
            emit_expr(emitter, ctx, *value)?;
            crate::codegen::throw::emit_error_throw(emitter, ctx);
        }
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            emit_try(emitter, ctx, *body, catches, *finally)?;
        }
    }
    let _: () = if branches(&stmt.kind) {
        emitter.clear_all_narrow_shadows();
    };
    Ok(())
}

/// Evaluate `expr` into an anonymous local and register the node as already
/// evaluated, so a compound assignment's synthesized read — which shares the
/// write's own receiver and index nodes — reads the local instead of running
/// the expression a second time. Leaves the stack unchanged.
fn emit_operand_once(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    expr: ExprId,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let slot = emitter.add_anonymous_local(
        ctx.symbols.value_type(
            &ctx.ta
                .try_expr(expr)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?,
    )?;
    emit_expr(emitter, ctx, expr)?;
    emitter.instruction(Instruction::LocalSet(slot));
    emitter.record_single_evaluation(expr, slot)?;
    Ok(slot)
}

fn emit_assign_field(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    name: &Ident,
    value: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let mark = emitter.single_evaluation_mark();
    let receiver_ty = ctx
        .ta
        .source_type(receiver)
        .map_err(crate::codegen::arena_failure)?
        .clone();
    // The receiver runs before the value in both arms, which is both the
    // left-to-right order the language guarantees and the order
    // `emit_operand_once` needs: a read inside `value` can only reuse a slot
    // that is already filled.
    let recv = emit_operand_once(emitter, ctx, receiver)?;
    emit_operand_once(emitter, ctx, value)?;
    emitter.instruction(Instruction::LocalGet(recv));
    if ctx
        .ta
        .try_expr(receiver)
        .map_err(crate::codegen::arena_failure)?
        .ty
        != receiver_ty
    {
        crate::codegen::cast_check::emit_operation_cast_on_stack(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(receiver)
                .map_err(crate::codegen::arena_failure)?
                .ty,
            &receiver_ty,
        )?;
    }
    let recv = emitter.add_anonymous_local(ctx.symbols.value_type(&receiver_ty)?)?;
    emitter.instruction(Instruction::LocalSet(recv));
    match receiver_ty.peel() {
        // Class instance: nominal receiver, array-backed object payload.
        Type::ClassRef { mangled, .. } => {
            emit_class_field_store(emitter, ctx, receiver, recv, mangled, name, value)?;
        }
        _ => emit_object_field_store(emitter, ctx, &receiver_ty, recv, name, value)?,
    }
    emitter.end_single_evaluations(mark)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_class_field_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    recv: u32,
    mangled: &crate::MangledName,
    name: &Ident,
    value: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Accessor property (no data slot): dispatch the synthetic setter.
    let Some(slot) = ctx.symbols.class_field_slot(mangled, &name.name) else {
        let setter = crate::codegen::classes::accessor_setter_name(&name.name);
        emit_method_call(
            emitter,
            ctx,
            receiver,
            mangled,
            &setter,
            std::slice::from_ref(&value),
            None,
            None,
            &Type::Void,
        )?;
        return Ok(());
    };
    let struct_idx = ctx.symbols.class_struct_type_idx(mangled).ok_or_else(|| {
        crate::codegen::internal_failure("class struct type recorded in classes::emit")
    })?;
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let value_ty = ctx
        .ta
        .try_expr(value)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    emit_expr(emitter, ctx, value)?;
    cast::emit_box(emitter, ctx, &value_ty)?;
    let stored = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
    emitter.instruction(Instruction::LocalSet(stored));
    emitter.instruction(Instruction::LocalGet(recv));
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
    crate::codegen::field_names::emit_mark_present(emitter, ctx, recv, index)?;
    Ok(())
}

fn emit_object_field_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver_ty: &Type,
    recv: u32,
    name: &Ident,
    value: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let object_shape = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?
        .object_shape;
    emitter.instruction(Instruction::LocalGet(recv));
    let rcv_local = stash_receiver_as_object_shape(emitter, receiver_ty, object_shape)?;
    // Accessor-aware: a data field writes its slot; an accessor property
    // invokes its `set <prop>` method closure.
    emit_object_property_write(emitter, ctx, rcv_local, name, value)?;
    Ok(())
}

fn emit_assign_index(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    value: ExprId,
    elem_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let mark = emitter.single_evaluation_mark();
    if ctx
        .ta
        .source_type(receiver)
        .map_err(crate::codegen::arena_failure)?
        .is_structural_object()
    {
        let object = emit_operand_once(emitter, ctx, receiver)?;
        let key = emit_operand_once(emitter, ctx, index)?;
        let stored = emit_operand_once(emitter, ctx, value)?;
        emitter.instruction(Instruction::LocalGet(object));
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(receiver)
                .map_err(crate::codegen::arena_failure)?
                .ty,
        )?;
        emitter.instruction(Instruction::LocalGet(key));
        crate::codegen::cast_check::emit_operation_cast_on_stack(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(index)
                .map_err(crate::codegen::arena_failure)?
                .ty,
            &Type::String,
        )?;
        emitter.instruction(Instruction::LocalGet(stored));
        cast::emit_box(
            emitter,
            ctx,
            &ctx.ta
                .try_expr(value)
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
    } else if matches!(
        ctx.ta
            .source_type(receiver)
            .map_err(crate::codegen::arena_failure)?
            .peel(),
        Type::Uint8Array
    ) {
        emit_uint8_index_store(emitter, ctx, receiver, index, value)?;
    } else {
        emit_array_index_store(emitter, ctx, receiver, index, value, elem_ty)?;
    }
    emitter.end_single_evaluations(mark)?;
    Ok(())
}

/// `u8[i] = v`. Packed `i8` storage, so the RHS is truncated to its low 8 bits.
fn emit_uint8_index_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    value: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let raw_uint8_idx = ctx.symbols.raw_uint8_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Uint8Array requires intrinsic types declared")
    })?;
    let uint8_idx = ctx.symbols.uint8_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Uint8Array requires intrinsic types declared")
    })?;
    let recv_local = emit_operand_once(emitter, ctx, receiver)?;
    let key = emit_operand_once(emitter, ctx, index)?;
    // RHS evaluates before the bounds check throws, preserving left-to-right
    // evaluation order (`emit_array_index_store` does the same).
    let original_value = emit_operand_once(emitter, ctx, value)?;

    // `$Uint8Array`'s backing field is immutable — no `push` exists to swap the
    // buffer out — so this read could sit anywhere after the receiver. It goes
    // here to keep the two index stores the same shape.
    let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_uint8_idx),
    }))?;
    emitter.instruction(Instruction::LocalGet(recv_local));
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(receiver)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        &Type::Uint8Array,
    )?;
    emitter.instruction(Instruction::LocalGet(key));
    super::expr::emit_index_number(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(index)
            .map_err(crate::codegen::arena_failure)?
            .ty,
    )?;
    let idx_f64_local = stash_index_operand(emitter)?;
    let value_local = emitter.add_anonymous_local(ValType::I32)?;
    emitter.instruction(Instruction::LocalGet(original_value));
    cast::emit_coerce_to_slot(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(value)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        &Type::Number,
    )?;
    emitter.instruction(Instruction::I32TruncSatF64U);
    emitter.instruction(Instruction::I32Const(0xff));
    emitter.instruction(Instruction::I32And);
    emitter.instruction(Instruction::LocalSet(value_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: uint8_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(raw_local));
    let idx_local = emit_checked_index(emitter, ctx, raw_local, idx_f64_local)?;
    emitter.instruction(Instruction::LocalGet(raw_local));
    emitter.instruction(Instruction::LocalGet(idx_local));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::ArraySet(raw_uint8_idx));
    Ok(())
}

/// `a[i] = v` on a `$Array`.
fn emit_array_index_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    value: ExprId,
    _elem_ty: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let raw_array_idx = ctx.symbols.raw_array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let array_idx = ctx.symbols.array_type_idx().ok_or_else(|| {
        crate::codegen::internal_failure("Type::Array requires intrinsic types declared")
    })?;
    let object_idx = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?
        .object;
    let recv_local = emit_operand_once(emitter, ctx, receiver)?;
    let key = emit_operand_once(emitter, ctx, index)?;
    // RHS evaluates before the store (and so before the bounds check throws),
    // preserving left-to-right evaluation order.
    let value_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_idx),
    }))?;
    let value_ty = ctx
        .ta
        .try_expr(value)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    emit_expr(emitter, ctx, value)?;
    // Preserve the actual RHS in the erased element slot.
    cast::emit_box(emitter, ctx, &value_ty)?;
    emitter.instruction(Instruction::LocalSet(value_local));

    // Read the backing array *after* the RHS: `push` swaps in a fresh
    // `$rawArray`, so a RHS that grows this array leaves any buffer read
    // earlier detached — the store would land in one nobody holds, and the
    // bounds check would test the stale length.
    let raw_arr_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_array_idx),
    }))?;
    emitter.instruction(Instruction::LocalGet(recv_local));
    crate::codegen::cast_check::emit_operation_cast_on_stack(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(receiver)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        &Type::Array(Box::new(Type::Unknown)),
    )?;
    emitter.instruction(Instruction::LocalGet(key));
    super::expr::emit_index_number(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(index)
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
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::ArraySet(raw_array_idx));
    Ok(())
}

/// Catch handlers surround only the try body. A finally gets an outer
/// completion target shared by normal, exceptional, and early exits.
pub(super) fn emit_try(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    body: StmtId,
    catches: &[crate::TypedCatchClause],
    finally: Option<StmtId>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if let Some(finally) = finally {
        super::finally::emit_try_finally(emitter, ctx, body, catches, finally)?;
        return Ok(());
    }
    if catches.is_empty() {
        emit_statement(emitter, ctx, body)?;
        return Ok(());
    }
    let error_idx = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("error intrinsic registered"))?
        .error;
    let tag_idx = ctx
        .symbols
        .error_tag_idx()
        .ok_or_else(|| crate::codegen::internal_failure("error tag registered"))?;
    emitter.emit_block(BlockType::Empty);
    emitter.emit_block(BlockType::Result(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(error_idx),
    })));
    emitter.instruction(Instruction::TryTable(
        BlockType::Empty,
        std::borrow::Cow::Owned(vec![wasm_encoder::Catch::One {
            tag: tag_idx,
            label: 0,
        }]),
    ));
    emitter.bump_block_depth();
    emit_statement(emitter, ctx, body)?;
    emitter.emit_end();
    emitter.instruction(Instruction::Br(1));
    emitter.emit_end();
    let err_stash = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(error_idx),
    }))?;
    emitter.instruction(Instruction::LocalSet(err_stash));
    emit_catch_dispatch(emitter, ctx, catches, err_stash)?;
    emitter.emit_end();
    Ok(())
}

/// A `catch (e: MyError)` clause annotated with a proper `Error` subclass
/// filters by nominal identity — `(vtable global, struct type)` of the
/// annotation class. The root `Error` annotation (or none) binds everything.
fn catch_filter_class(
    ctx: &CodegenCtx,
    clause_ty: &Type,
) -> Result<Option<(u32, u32)>, crate::compiler_error::CompilerFailure> {
    let Type::ClassRef { mangled, .. } = clause_ty.peel() else {
        return Ok(None);
    };
    if *mangled == crate::mangle::prelude("Error") {
        return Ok(None);
    }
    let vtable_global = ctx
        .symbols
        .class_vtable_global_idx(mangled)
        .ok_or_else(|| {
            crate::codegen::internal_failure("catch-annotation class vtable global recorded")
        })?;
    let struct_idx = ctx.symbols.class_struct_type_idx(mangled).ok_or_else(|| {
        crate::codegen::internal_failure("catch-annotation class struct type recorded")
    })?;
    Ok(Some((vtable_global, struct_idx)))
}

/// Ordered dispatch over the catch arms for the caught error stashed in
/// `err_stash` (a nullable `(ref null $Error)` local, already set). Each
/// subclass arm runs the nominal brand test; the first match binds, runs its
/// body, and branches out. A root-`Error`/untyped arm is a catch-all and ends
/// the chain; without one, the chain ends by re-raising with `throw` (not
/// `throw_ref`) so an enclosing `catch_all_ref` wrapper still observes it and
/// runs `finally` first.
///
/// At the emission site, relative label 0 is `$end_try`, the matched-arm exit.
/// When cleanup is present, the whole try/catch sits inside its wrapper.
fn emit_catch_dispatch(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    catches: &[crate::TypedCatchClause],
    err_stash: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for clause in catches {
        let Some((vtable_global, struct_idx)) = catch_filter_class(ctx, &clause.ty)? else {
            emitter.push_scope();
            let slot =
                emitter.define_local(&clause.binding, ctx.symbols.value_type(&clause.ty)?)?;
            emitter.instruction(Instruction::LocalGet(err_stash));
            emitter.instruction(Instruction::RefAsNonNull);
            emitter.instruction(Instruction::LocalSet(slot));
            emit_statement(emitter, ctx, clause.body)?;
            emitter.pop_scope()?;
            // Falling through reaches the matched-arm exit in both topologies.
            return Ok(());
        };
        emitter.instruction(Instruction::LocalGet(err_stash));
        super::cast::emit_nominal_instance_test(emitter, ctx, vtable_global)?;
        emitter.emit_if(BlockType::Empty);
        emitter.push_scope();
        let slot = emitter.define_local(&clause.binding, ctx.symbols.value_type(&clause.ty)?)?;
        emitter.instruction(Instruction::LocalGet(err_stash));
        // Shape-only cast is sound here: the brand matched.
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(struct_idx)));
        emitter.instruction(Instruction::LocalSet(slot));
        emit_statement(emitter, ctx, clause.body)?;
        emitter.pop_scope()?;
        // Matched-arm exit is label 0 at chain level; +1 for the `if` frame.
        emitter.instruction(Instruction::Br(1));
        emitter.emit_end();
    }
    let tag_idx = ctx
        .symbols
        .error_tag_idx()
        .ok_or_else(|| crate::codegen::internal_failure("error tag registered"))?;
    emitter.instruction(Instruction::LocalGet(err_stash));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::Throw(tag_idx));
    Ok(())
}

/// Emit a `switch` statement as a block-stack of per-case targets.
///
/// Layout (with N cases and a `default`):
///
/// ```text
/// (block $end
///   (block $default
///     (block $case_{N-1})
///     …
///     (block $case_0
///       <dispatcher>            ;; br_if $case_i for each value-test
///       br $default             ;; (or $end when no default)
///     )                          ;; close $case_0
///     <case_0 body>; br $end
///     …                          ;; close + body for each case
///     <case_{N-1} body>; br $end
///   )                            ;; close $default
///   <default body>               ;; falls through to $end naturally
/// )
/// ```
///
/// The discriminant is evaluated once into a fresh anonymous local
/// so each per-case test reads it without re-running side effects.
/// `break` from any case body targets `$end` via the loop-contexts
/// stack (`emit_switch_open` pushed `is_switch: true`); `continue`
/// inside a case body walks past this frame to the enclosing loop.
fn emit_switch(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    discriminant: crate::ExprId,
    discriminant_ty: &Type,
    cases: &[TypedSwitchCase],
    default: Option<StmtId>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    // Stash discriminant in an anonymous local; numeric enums unboxed to f64
    // so per-case tests use f64.eq instead of a structural-equality call.
    emit_expr(emitter, ctx, discriminant)?;
    let disc_val = switch_disc_local_type(ctx, discriminant_ty)?;
    if matches!(discriminant_ty.peel(), Type::NumberEnum { .. }) {
        let boxed_idx = ctx.symbols.boxed_number_type_idx().ok_or_else(|| {
            crate::codegen::internal_failure("boxed_number type registered with intrinsics")
        })?;
        emitter.instruction(Instruction::StructGet {
            struct_type_index: boxed_idx,
            field_index: 1,
        });
    }
    let disc_local = emitter.add_anonymous_local(disc_val)?;
    emitter.instruction(Instruction::LocalSet(disc_local));

    emitter.emit_switch_open();
    // Open $default first (outermost), then $case_{N-1} … $case_0 (innermost),
    // so the dispatcher's br_if(source_idx) directly addresses each case block.
    if default.is_some() {
        emitter.emit_block(BlockType::Empty);
    }
    for _ in 0..cases.len() {
        emitter.emit_block(BlockType::Empty);
    }

    // Dispatcher inside $case_0: one br_if per case value, then br to $default (or $end).
    for (source_idx, case) in cases.iter().enumerate() {
        emitter.record_span(case.span);
        for value in &case.values {
            emit_case_comparison(emitter, ctx, disc_local, discriminant_ty, value)?;
            emitter.instruction(Instruction::BrIf(crate::codegen::wasm_u32(source_idx)?));
        }
    }
    // Both $default and $end sit at depth cases.len() from inside the dispatcher.
    emitter.instruction(Instruction::Br(crate::codegen::wasm_u32(cases.len())?));

    // Close each $case_i and emit its body. The trailing br $end is dead when
    // the body terminates via break/return, but Wasm's stack-polymorphic typing
    // validates unreachable code, so it's harmless.
    for case in cases {
        emitter.emit_end();
        emitter.push_scope();
        emit_statement(emitter, ctx, case.body)?;
        emitter.pop_scope()?;
        let end_label = emitter.break_label()?;
        emitter.instruction(Instruction::Br(end_label));
    }

    if let Some(default_body) = default {
        emitter.emit_end();
        emitter.push_scope();
        emit_statement(emitter, ctx, default_body)?;
        emitter.pop_scope()?;
        // Falls through to `$end` naturally — no explicit `br`.
    }

    emitter.emit_switch_close();
    Ok(())
}

/// Numeric-enum discriminants are unboxed to `f64` here so per-case tests use `f64.eq`
/// instead of a structural-equality call on every comparison.
fn switch_disc_local_type(
    ctx: &CodegenCtx,
    ty: &Type,
) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(if matches!(ty.peel(), Type::NumberEnum { .. }) {
        ValType::F64
    } else {
        ctx.symbols.value_type(ty)?
    })
}

/// Internal `if/else` uses `BlockType::Result(ValType::I32)` so the i32 result
/// flows out without adding a `br_if`-visible block depth.
fn emit_case_comparison(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    disc_local: u32,
    disc_ty: &Type,
    value: &TypedSwitchValue,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let disc_val = switch_disc_local_type(ctx, disc_ty)?;
    if let Some(case_ty) = switch_case_primitive_type(value)
        && matches!(disc_val, ValType::Ref(_))
        && disc_val != ctx.symbols.value_type(&case_ty)?
    {
        emit_boxed_case_comparison(emitter, ctx, disc_local, &case_ty, value)?;
        return Ok(());
    }
    match value {
        TypedSwitchValue::Expr { comparison, .. } => {
            emit_expr(emitter, ctx, *comparison)?;
        }
        TypedSwitchValue::Null { .. } => {
            // case null: accepted only when discriminant can hold null (nullable ref)
            emitter.instruction(Instruction::LocalGet(disc_local));
            emitter.instruction(Instruction::RefIsNull);
        }
        TypedSwitchValue::Number { value, .. } => {
            emitter.instruction(Instruction::LocalGet(disc_local));
            emitter.instruction(Instruction::F64Const((*value).into()));
            emitter.instruction(Instruction::F64Eq);
        }
        TypedSwitchValue::Boolean { value, .. } => {
            emitter.instruction(Instruction::LocalGet(disc_local));
            emitter.instruction(Instruction::I32Const(if *value { 1 } else { 0 }));
            emitter.instruction(Instruction::I32Eq);
        }
        TypedSwitchValue::String { value, .. } => {
            emit_string_compare(emitter, ctx, disc_local, disc_val, value)?;
        }
        TypedSwitchValue::Enum { value: payload, .. } => match payload {
            EnumVariantPayload::Number(n) => {
                emitter.instruction(Instruction::LocalGet(disc_local));
                emitter.instruction(Instruction::F64Const((*n).into()));
                emitter.instruction(Instruction::F64Eq);
            }
            EnumVariantPayload::String(s) => {
                emit_string_compare(emitter, ctx, disc_local, disc_val, s)?;
            }
        },
    };
    Ok(())
}

/// A union discriminant may carry a different primitive kind or null. Test
/// its boxed shape before unboxing; a nonmatching kind simply skips this case.
fn emit_boxed_case_comparison(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    disc_local: u32,
    case_ty: &Type,
    value: &TypedSwitchValue,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let heap_index = match case_ty {
        Type::Number => ctx.symbols.boxed_number_type_idx(),
        Type::Boolean => ctx.symbols.boxed_boolean_type_idx(),
        Type::String => ctx.symbols.string_type_idx(),
        _ => {
            return Err(crate::codegen::internal_failure(
                "switch case primitive classified above",
            ));
        }
    }
    .ok_or_else(|| {
        crate::codegen::internal_failure("primitive types registered before switch emission")
    })?;
    emitter.instruction(Instruction::LocalGet(disc_local));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(heap_index)));
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(disc_local));
    cast::emit_cast_to(emitter, ctx, case_ty)?;
    let unboxed = emitter.add_anonymous_local(ctx.symbols.value_type(case_ty)?)?;
    emitter.instruction(Instruction::LocalSet(unboxed));
    emit_case_comparison(emitter, ctx, unboxed, case_ty, value)?;
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end();

    Ok(())
}

fn switch_case_primitive_type(value: &TypedSwitchValue) -> Option<Type> {
    match value {
        TypedSwitchValue::Number { .. }
        | TypedSwitchValue::Enum {
            value: EnumVariantPayload::Number(_),
            ..
        } => Some(Type::Number),
        TypedSwitchValue::String { .. }
        | TypedSwitchValue::Enum {
            value: EnumVariantPayload::String(_),
            ..
        } => Some(Type::String),
        TypedSwitchValue::Boolean { .. } => Some(Type::Boolean),
        TypedSwitchValue::Null { .. } | TypedSwitchValue::Expr { .. } => None,
    }
}

/// Wraps in a `ref.is_null` guard when discriminant is nullable so a null
/// discriminant compares as not-equal. The `if/else` uses explicit i32 result
/// so no extra block depth is added to the dispatcher.
fn emit_string_compare(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    disc_local: u32,
    disc_val: ValType,
    literal: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let string_eq_idx = ctx.symbols.prelude_func_idx("string_eq").ok_or_else(|| {
        crate::codegen::internal_failure("string_eq is not imported from the prelude")
    })?;

    let disc_is_nullable = matches!(
        disc_val,
        ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(_),
        })
    );
    if disc_is_nullable {
        emitter.instruction(Instruction::LocalGet(disc_local));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::I32Const(0));
        emitter.emit_else();
        emitter.instruction(Instruction::LocalGet(disc_local));
        emitter.instruction(Instruction::RefAsNonNull);
        super::emit_const_string_by_text(emitter, ctx, literal)?;
        emitter.instruction(Instruction::Call(string_eq_idx));
        emitter.emit_end();
    } else {
        emitter.instruction(Instruction::LocalGet(disc_local));
        super::emit_const_string_by_text(emitter, ctx, literal)?;
        emitter.instruction(Instruction::Call(string_eq_idx));
    }
    Ok(())
}
