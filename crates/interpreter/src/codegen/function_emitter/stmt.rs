use crate::codegen::CodegenCtx;
use crate::codegen::bounds::{emit_checked_index, stash_index_operand};
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

pub fn emit_statement(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, id: StmtId) {
    let stmt = ctx.ta.stmt(id);
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
                emit_statement(emitter, ctx, child);
            }
            emitter.pop_scope();
        }
        TypedStmtKind::Return(value) => {
            if let Some(expr_id) = value {
                emit_expr(emitter, ctx, *expr_id);
                cast::emit_coerce_to_return_slot(emitter, ctx, &ctx.ta.expr(*expr_id).ty);
            }
            // JS semantics: finally runs on every exit path including return.
            if emitter.finally_count() > 0 {
                let stash = if value.is_some() {
                    emitter
                        .wasm_result_type(ctx)
                        .map(|val_ty| emitter.return_stash_local(val_ty))
                } else {
                    None
                };
                if let Some(slot) = stash {
                    emitter.instruction(Instruction::LocalSet(slot));
                }
                emit_finally_chain(emitter, ctx, 0);
                if let Some(slot) = stash {
                    emitter.instruction(Instruction::LocalGet(slot));
                }
            }
            emitter.instruction(Instruction::Return);
        }
        TypedStmtKind::Expr(expr_id) => {
            emit_expr(emitter, ctx, *expr_id);
            // Expression statements discard the result; `void` calls leave
            // nothing on the stack and need no Drop.
            let ty = &ctx.ta.expr(*expr_id).ty;
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
            emit_expr(emitter, ctx, *value);
            let value_ty = ctx.ta.expr(*value).ty.clone();
            cast::emit_coerce_to_slot(emitter, ctx, &value_ty, ty);
            if *boxed {
                let box_idx = ctx
                    .symbols
                    .box_type_idx(ty)
                    .expect("box type registered for every boxed Let");
                emitter.instruction(Instruction::StructNew(box_idx));
                let box_val = ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(box_idx),
                });
                let slot = emitter.define_local(name, box_val);
                emitter.instruction(Instruction::LocalSet(slot));
            } else {
                let slot = emitter.define_local(name, ctx.symbols.value_type(ty));
                emitter.instruction(Instruction::LocalSet(slot));
            }
        }
        TypedStmtKind::ReboxLocal { ident, ty } => {
            let slot = emitter
                .write_slot(&ident.name)
                .expect("Inferer guarantees the binding exists");
            let box_idx = ctx
                .symbols
                .box_type_idx(ty)
                .expect("box type registered for every boxed Let");
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
            emit_expr(emitter, ctx, *value);
            let value_ty = ctx.ta.expr(*value).ty.clone();
            cast::emit_coerce_to_slot(emitter, ctx, &value_ty, ty);
            let slot = emitter.define_local(name, ctx.symbols.value_type(ty));
            emitter.instruction(Instruction::LocalSet(slot));
        }
        TypedStmtKind::AssignLocal {
            ident,
            target_ty,
            value,
            boxed,
            narrowed_shadow_ty,
        } => {
            let slot = emitter
                .write_slot(&ident.name)
                .expect("Inferer guarantees the binding exists");
            if *boxed {
                // The box's inner field is keyed on the binding's
                // declared type (`target_ty`), not the RHS type, so
                // a primitive RHS into a wider boxed slot lands the
                // boxed primitive in the right box variant.
                let box_idx = ctx
                    .symbols
                    .box_type_idx(target_ty)
                    .expect("box type registered for every boxed AssignLocal");
                let value_ty = ctx.ta.expr(*value).ty.clone();
                emitter.instruction(Instruction::LocalGet(slot));
                emit_expr(emitter, ctx, *value);
                cast::emit_coerce_to_slot(emitter, ctx, &value_ty, target_ty);
                emitter.instruction(Instruction::StructSet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
            } else {
                emit_expr(emitter, ctx, *value);
                let value_ty = ctx.ta.expr(*value).ty.clone();
                // If this assignment installs a narrowing, allocate a fresh
                // shadow local of the narrowed type. `local.tee` keeps the RHS
                // on the stack for the subsequent coerce+set while also storing
                // into the shadow. Installing it makes `LocalNarrowRef` reads
                // resolve there — no per-use cast needed.
                if let Some(narrowed_ty) = narrowed_shadow_ty {
                    let narrowed_val = ctx.symbols.value_type(narrowed_ty);
                    let shadow_idx = emitter.add_anonymous_local(narrowed_val);
                    emitter.instruction(Instruction::LocalTee(shadow_idx));
                    emitter.install_narrow_shadow(&ident.name, shadow_idx, narrowed_val);
                }
                cast::emit_coerce_to_slot(emitter, ctx, &value_ty, target_ty);
                emitter.instruction(Instruction::LocalSet(slot));
            }
        }
        TypedStmtKind::AssignGlobal {
            mangled,
            target_ty,
            value,
            ..
        } => {
            emit_expr(emitter, ctx, *value);
            let value_ty = ctx.ta.expr(*value).ty.clone();
            cast::emit_coerce_to_slot(emitter, ctx, &value_ty, target_ty);
            let idx = ctx
                .symbols
                .global_idx(mangled)
                .expect("Inferer guarantees the binding exists");
            emitter.instruction(Instruction::GlobalSet(idx));
        }
        TypedStmtKind::AssignField {
            receiver,
            name,
            value,
        } => {
            emit_assign_field(emitter, ctx, *receiver, name, *value);
        }
        TypedStmtKind::AssignIndex {
            receiver,
            index,
            value,
            elem_ty,
        } => {
            emit_assign_index(emitter, ctx, *receiver, *index, *value, elem_ty);
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond_ty = ctx.ta.expr(*condition).ty.clone();
            emit_expr(emitter, ctx, *condition);
            cast::emit_condition_to_i32(emitter, ctx, &cond_ty);
            emitter.emit_if(BlockType::Empty);
            emit_statement(emitter, ctx, *then_block);
            if let Some(else_id) = else_block {
                emitter.emit_else();
                emit_statement(emitter, ctx, *else_id);
            }
            emitter.emit_end();
        }
        TypedStmtKind::While { condition, body } => {
            // `block { loop { <cond>; i32.eqz; br_if 1; <body>; br 0; } }`
            // — break exits the outer block, continue restarts the loop.
            emitter.emit_while_open();
            let cond_ty = ctx.ta.expr(*condition).ty.clone();
            emit_expr(emitter, ctx, *condition);
            cast::emit_condition_to_i32(emitter, ctx, &cond_ty);
            emitter.instruction(Instruction::I32Eqz);
            let break_label = emitter.break_label();
            emitter.instruction(Instruction::BrIf(break_label));
            emit_statement(emitter, ctx, *body);
            let continue_label = emitter.continue_label();
            emitter.instruction(Instruction::Br(continue_label));
            emitter.emit_while_close();
        }
        TypedStmtKind::Break => {
            // Inline every finally pushed inside the target loop / switch
            // (the ones the `break` will unwind out of).
            let floor = emitter.break_finally_floor();
            emit_finally_chain(emitter, ctx, floor);
            let label = emitter.break_label();
            emitter.instruction(Instruction::Br(label));
        }
        TypedStmtKind::Continue => {
            let floor = emitter.continue_finally_floor();
            emit_finally_chain(emitter, ctx, floor);
            let label = emitter.continue_label();
            emitter.instruction(Instruction::Br(label));
        }
        TypedStmtKind::For { .. } | TypedStmtKind::ForOf { .. } | TypedStmtKind::DoWhile { .. } => {
            unreachable!(
                "for / for-of / do-while must be lowered by the desugar pass before codegen"
            );
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
            );
        }
        TypedStmtKind::NarrowRegion {
            source,
            binding,
            cast_info,
            body,
            ..
        } => {
            // Region snapshots follow the same lifetime as assignment snapshots.
            emitter.push_scope();
            let shadow_val = ctx.symbols.value_type(&cast_info.to_ty);
            let shadow = emitter.add_anonymous_local(shadow_val);
            emit_expr(emitter, ctx, *source);
            cast::emit_narrowing_cast(emitter, ctx, cast_info);
            emitter.instruction(Instruction::LocalSet(shadow));
            emitter.install_narrow_shadow(&binding.name, shadow, shadow_val);
            emitter.register_narrow_source(&binding.name, *source);
            emit_statement(emitter, ctx, *body);
            emitter.pop_scope();
        }
        TypedStmtKind::Throw { value } => {
            // cast is unconditional: typechecker guarantees Error type; Wasm-level nullability comes from (ref null $Object) lowering
            emit_expr(emitter, ctx, *value);
            crate::codegen::throw::emit_error_throw(emitter, ctx);
        }
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            // finally body is emitted up to 3 times (normal / caught / uncaught) — Wasm has no native finally construct
            emit_try(emitter, ctx, *body, catches, *finally);
        }
    }
    if branches(&stmt.kind) {
        emitter.clear_all_narrow_shadows();
    }
}

/// Evaluate `expr` into an anonymous local and register the node as already
/// evaluated, so a compound assignment's synthesized read — which shares the
/// write's own receiver and index nodes — reads the local instead of running
/// the expression a second time. Leaves the stack unchanged.
fn emit_operand_once(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, expr: ExprId) -> u32 {
    let slot = emitter.add_anonymous_local(ctx.symbols.value_type(&ctx.ta.expr(expr).ty));
    emit_expr(emitter, ctx, expr);
    emitter.instruction(Instruction::LocalSet(slot));
    emitter.record_single_evaluation(expr, slot);
    slot
}

/// [`emit_operand_once`] for an index expression, which every indexing path
/// stashes as a bare `f64` rather than at its own value type.
fn emit_index_once(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, index: ExprId) -> u32 {
    emit_expr(emitter, ctx, index);
    let slot = stash_index_operand(emitter);
    emitter.record_single_evaluation(index, slot);
    slot
}

fn emit_assign_field(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    name: &Ident,
    value: ExprId,
) {
    let mark = emitter.single_evaluation_mark();
    let receiver_ty = ctx.ta.expr(receiver).ty.clone();
    // The receiver runs before the value in both arms, which is both the
    // left-to-right order the language guarantees and the order
    // `emit_operand_once` needs: a read inside `value` can only reuse a slot
    // that is already filled.
    let recv = emit_operand_once(emitter, ctx, receiver);
    match receiver_ty.peel() {
        // Class instance: nominal receiver, array-backed object payload.
        Type::ClassRef { mangled, .. } => {
            emit_class_field_store(emitter, ctx, receiver, recv, mangled, name, value);
        }
        _ => emit_object_field_store(emitter, ctx, &receiver_ty, recv, name, value),
    }
    emitter.end_single_evaluations(mark);
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
) {
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
        );
        return;
    };
    let struct_idx = ctx
        .symbols
        .class_struct_type_idx(mangled)
        .expect("class struct type recorded in classes::emit");
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    emitter.instruction(Instruction::LocalGet(recv));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: struct_idx,
        field_index: 2,
    });
    emitter.instruction(Instruction::I32Const(slot as i32));
    let value_ty = ctx.ta.expr(value).ty.clone();
    emit_expr(emitter, ctx, value);
    cast::emit_box(emitter, ctx, &value_ty);
    emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
}

fn emit_object_field_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver_ty: &Type,
    recv: u32,
    name: &Ident,
    value: ExprId,
) {
    let object_shape = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry")
        .object_shape;
    emitter.instruction(Instruction::LocalGet(recv));
    let rcv_local = stash_receiver_as_object_shape(emitter, receiver_ty, object_shape);
    // Accessor-aware: a data field writes its slot; an accessor property
    // invokes its `set <prop>` method closure.
    emit_object_property_write(emitter, ctx, rcv_local, name, value);
}

fn emit_assign_index(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    value: ExprId,
    elem_ty: &Type,
) {
    let mark = emitter.single_evaluation_mark();
    if matches!(ctx.ta.expr(receiver).ty.peel(), Type::Uint8Array) {
        emit_uint8_index_store(emitter, ctx, receiver, index, value);
    } else {
        emit_array_index_store(emitter, ctx, receiver, index, value, elem_ty);
    }
    emitter.end_single_evaluations(mark);
}

/// `u8[i] = v`. Packed `i8` storage, so the RHS is truncated to its low 8 bits.
fn emit_uint8_index_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    value: ExprId,
) {
    let raw_uint8_idx = ctx
        .symbols
        .raw_uint8_array_type_idx()
        .expect("Type::Uint8Array requires intrinsic types declared");
    let uint8_idx = ctx
        .symbols
        .uint8_array_type_idx()
        .expect("Type::Uint8Array requires intrinsic types declared");
    let recv_local = emit_operand_once(emitter, ctx, receiver);
    let idx_f64_local = emit_index_once(emitter, ctx, index);
    // RHS evaluates before the bounds check throws, preserving left-to-right
    // evaluation order (`emit_array_index_store` does the same).
    let value_local = emitter.add_anonymous_local(ValType::I32);
    emit_expr(emitter, ctx, value);
    emitter.instruction(Instruction::I32TruncSatF64U);
    emitter.instruction(Instruction::I32Const(0xff));
    emitter.instruction(Instruction::I32And);
    emitter.instruction(Instruction::LocalSet(value_local));

    // `$Uint8Array`'s backing field is immutable — no `push` exists to swap the
    // buffer out — so this read could sit anywhere after the receiver. It goes
    // here to keep the two index stores the same shape.
    let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_uint8_idx),
    }));
    emitter.instruction(Instruction::LocalGet(recv_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: uint8_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(raw_local));
    let idx_local = emit_checked_index(emitter, ctx, raw_local, idx_f64_local);
    emitter.instruction(Instruction::LocalGet(raw_local));
    emitter.instruction(Instruction::LocalGet(idx_local));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::ArraySet(raw_uint8_idx));
}

/// `a[i] = v` on a `$Array`.
fn emit_array_index_store(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    receiver: ExprId,
    index: ExprId,
    value: ExprId,
    elem_ty: &Type,
) {
    let raw_array_idx = ctx
        .symbols
        .raw_array_type_idx()
        .expect("Type::Array requires intrinsic types declared");
    let array_idx = ctx
        .symbols
        .array_type_idx()
        .expect("Type::Array requires intrinsic types declared");
    let object_idx = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared")
        .object;
    let recv_local = emit_operand_once(emitter, ctx, receiver);
    let idx_f64_local = emit_index_once(emitter, ctx, index);
    // RHS evaluates before the store (and so before the bounds check throws),
    // preserving left-to-right evaluation order.
    let value_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_idx),
    }));
    let value_ty = ctx.ta.expr(value).ty.clone();
    emit_expr(emitter, ctx, value);
    // Coerce the RHS to the element type (e.g. NumberLiteral → Number), then
    // box for the (ref null $Object) slot.
    cast::emit_coerce_to_slot(emitter, ctx, &value_ty, elem_ty);
    cast::emit_box(emitter, ctx, elem_ty);
    emitter.instruction(Instruction::LocalSet(value_local));

    // Read the backing array *after* the RHS: `push` swaps in a fresh
    // `$rawArray`, so a RHS that grows this array leaves any buffer read
    // earlier detached — the store would land in one nobody holds, and the
    // bounds check would test the stale length.
    let raw_arr_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_array_idx),
    }));
    emitter.instruction(Instruction::LocalGet(recv_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: array_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(raw_arr_local));
    let idx_local = emit_checked_index(emitter, ctx, raw_arr_local, idx_f64_local);
    emitter.instruction(Instruction::LocalGet(raw_arr_local));
    emitter.instruction(Instruction::LocalGet(idx_local));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::ArraySet(raw_array_idx));
}

/// Inline enclosing `finally` bodies above `stop_at`, innermost first,
/// before a control-transfer that unwinds out of those try scopes.
/// `stop_at = 0` = all finallys (return); non-zero = skip finallys outside the target loop/switch (break/continue).
///
/// Popped one at a time so a `return` inside a finally body still sees the
/// finallys outside it — JS runs every enclosing finally on the way out. A body
/// is off the stack while it runs, so it cannot re-enter itself.
fn emit_finally_chain(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, stop_at: usize) {
    let mut popped = Vec::new();
    while emitter.finally_count() > stop_at {
        let f = emitter
            .pop_finally()
            .expect("finally_count() > stop_at guarantees a body to pop");
        popped.push(f);
        emit_statement(emitter, ctx, f);
    }
    // Restore so later control transfers in the same function still see these finallys.
    for f in popped.into_iter().rev() {
        emitter.push_finally(f);
    }
}

/// Lowers try/catch/finally to a block/try_table topology (see interpreter.md §exception-handling).
/// Pushes `finally` onto the finally-stack before emitting bodies so break/return inside
/// them inlines it first; the three inline finally emissions each temporarily pop it.
fn emit_try(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    body: StmtId,
    catches: &[crate::TypedCatchClause],
    finally: Option<StmtId>,
) {
    use wasm_encoder::{Catch, HeapType};
    let error_idx = ctx
        .symbols
        .intrinsic_type_indices()
        .map(|i| i.error)
        .expect("error intrinsic registered");
    let tag_idx = ctx.symbols.error_tag_idx().expect("error tag registered");
    let error_ref = ValType::Ref(wasm_encoder::RefType {
        nullable: false,
        heap_type: HeapType::Concrete(error_idx),
    });

    // $end_try — outermost label; both normal and exceptional paths exit through here.
    emitter.emit_block(BlockType::Empty);

    // Push finally so return/break/continue inside the try or catch body inlines it.
    if let Some(f) = finally {
        emitter.push_finally(f);
    }

    match (catches.is_empty(), finally) {
        (false, None) => {
            // block $catch (result (ref $Error))
            emitter.emit_block(BlockType::Result(error_ref));
            // try_table (catch error_tag $catch=0); label 0 = $catch, label 1 = $end_try
            emitter.instruction(Instruction::TryTable(
                BlockType::Empty,
                std::borrow::Cow::Owned(vec![Catch::One {
                    tag: tag_idx,
                    label: 0,
                }]),
            ));
            // try_table's body opens a fresh block-like frame; bump
            // manually since `instruction()` doesn't track depth.
            emitter.bump_block_depth();
            emit_statement(emitter, ctx, body);
            emitter.emit_end();
            // From inside $catch, label 1 = $end_try.
            emitter.instruction(Instruction::Br(1));
            emitter.emit_end(); // $catch close — caught (ref $Error) on stack
            let err_stash = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(error_idx),
            }));
            emitter.instruction(Instruction::LocalSet(err_stash));
            // Depth-0 label here is $end_try — the matched-arm exit.
            emit_catch_dispatch(emitter, ctx, catches, err_stash);
        }
        (true, Some(finally_body)) => {
            // block $rethrow (result exnref)
            emitter.emit_block(BlockType::Result(ValType::EXNREF));
            // try_table (catch_all_ref $rethrow=0); label 0 = $rethrow, label 1 = $end_try
            emitter.instruction(Instruction::TryTable(
                BlockType::Empty,
                std::borrow::Cow::Owned(vec![Catch::AllRef { label: 0 }]),
            ));
            emitter.bump_block_depth();
            emit_statement(emitter, ctx, body);
            emitter.emit_end();
            emit_finally_inline(emitter, ctx, finally_body);
            emitter.instruction(Instruction::Br(1));
            emitter.emit_end(); // $rethrow close — exnref on stack
            emit_finally_inline(emitter, ctx, finally_body);
            emitter.instruction(Instruction::ThrowRef);
        }
        (false, Some(finally_body)) => {
            // block $rethrow (result exnref)
            emitter.emit_block(BlockType::Result(ValType::EXNREF));
            // block $catch (result (ref $Error))
            emitter.emit_block(BlockType::Result(error_ref));
            // try_table (catch error_tag $catch=0) (catch_all_ref $rethrow=1)
            // label 0 = $catch, label 1 = $rethrow, label 2 = $end_try
            emitter.instruction(Instruction::TryTable(
                BlockType::Empty,
                std::borrow::Cow::Owned(vec![
                    Catch::One {
                        tag: tag_idx,
                        label: 0,
                    },
                    Catch::AllRef { label: 1 },
                ]),
            ));
            emitter.bump_block_depth();
            emit_statement(emitter, ctx, body);
            emitter.emit_end();
            emit_finally_inline(emitter, ctx, finally_body);
            // From inside $catch, label 2 = $end_try.
            emitter.instruction(Instruction::Br(2));
            emitter.emit_end(); // $catch close — caught (ref $Error) on stack
            emit_catch_arms_with_trailing_finally(
                emitter,
                ctx,
                catches,
                finally_body,
                /* end_try_label */ 1,
            );
            emitter.emit_end(); // $rethrow close — exnref on stack
            emit_finally_inline(emitter, ctx, finally_body);
            emitter.instruction(Instruction::ThrowRef);
        }
        (true, None) => {
            unreachable!("parser guarantees at least one of catch/finally on a try statement",)
        }
    }

    if finally.is_some() {
        emitter.pop_finally();
    }

    emitter.emit_end(); // $end_try close
}

/// Temporarily pops the finally from the stack while emitting its body so a `return`
/// inside the finally doesn't re-enter it (which would be infinite recursion).
fn emit_finally_inline(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, finally_body: StmtId) {
    let popped = emitter.pop_finally();
    emitter.push_scope();
    emit_statement(emitter, ctx, finally_body);
    emitter.pop_scope();
    if let Some(f) = popped {
        emitter.push_finally(f);
    }
}

/// A `catch (e: MyError)` clause annotated with a proper `Error` subclass
/// filters by nominal identity — `(vtable global, struct type)` of the
/// annotation class. The root `Error` annotation (or none) binds everything.
fn catch_filter_class(ctx: &CodegenCtx, clause_ty: &Type) -> Option<(u32, u32)> {
    let Type::ClassRef { mangled, .. } = clause_ty.peel() else {
        return None;
    };
    if *mangled == crate::mangle::prelude("Error") {
        return None;
    }
    let vtable_global = ctx
        .symbols
        .class_vtable_global_idx(mangled)
        .expect("catch-annotation class vtable global recorded");
    let struct_idx = ctx
        .symbols
        .class_struct_type_idx(mangled)
        .expect("catch-annotation class struct type recorded");
    Some((vtable_global, struct_idx))
}

/// Ordered dispatch over the catch arms for the caught error stashed in
/// `err_stash` (a nullable `(ref null $Error)` local, already set). Each
/// subclass arm runs the nominal brand test; the first match binds, runs its
/// body, and branches out. A root-`Error`/untyped arm is a catch-all and ends
/// the chain; without one, the chain ends by re-raising with `throw` (not
/// `throw_ref`) so an enclosing `catch_all_ref` wrapper still observes it and
/// runs `finally` first.
///
/// Invariant: at the emission site, relative label 0 is the matched-arm exit —
/// `$end_try` when the try has no `finally`, or the finally-wrapper
/// `try_table`'s own label (a forward branch out of it lands on the shared
/// trailing finally).
fn emit_catch_dispatch(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    catches: &[crate::TypedCatchClause],
    err_stash: u32,
) {
    for clause in catches {
        let Some((vtable_global, struct_idx)) = catch_filter_class(ctx, &clause.ty) else {
            emitter.push_scope();
            let slot = emitter.define_local(&clause.binding, ctx.symbols.value_type(&clause.ty));
            emitter.instruction(Instruction::LocalGet(err_stash));
            emitter.instruction(Instruction::RefAsNonNull);
            emitter.instruction(Instruction::LocalSet(slot));
            emit_statement(emitter, ctx, clause.body);
            emitter.pop_scope();
            // Falling through reaches the matched-arm exit in both topologies.
            return;
        };
        emitter.instruction(Instruction::LocalGet(err_stash));
        super::cast::emit_nominal_instance_test(emitter, ctx, vtable_global);
        emitter.emit_if(BlockType::Empty);
        emitter.push_scope();
        let slot = emitter.define_local(&clause.binding, ctx.symbols.value_type(&clause.ty));
        emitter.instruction(Instruction::LocalGet(err_stash));
        // Shape-only cast is sound here: the brand matched.
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(struct_idx)));
        emitter.instruction(Instruction::LocalSet(slot));
        emit_statement(emitter, ctx, clause.body);
        emitter.pop_scope();
        // Matched-arm exit is label 0 at chain level; +1 for the `if` frame.
        emitter.instruction(Instruction::Br(1));
        emitter.emit_end();
    }
    let tag_idx = ctx.symbols.error_tag_idx().expect("error tag registered");
    emitter.instruction(Instruction::LocalGet(err_stash));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::Throw(tag_idx));
}

/// Catch dispatch wrapped in its own `try_table` so an exception a catch arm
/// throws (or an unmatched re-raise) still runs `finally` — and a `return` in
/// that finally suppresses it, per ECMA-262 §14.15.2. The finally remains on
/// the stack during the arm bodies so `return` inside an arm inlines it;
/// `emit_finally_inline` pops it temporarily for each trailing emission.
///
/// Mirrors the try-body rethrow topology: arm-matched runs the finally and
/// branches to `$end_try`; arm-throws (or no match) runs the finally and
/// rethrows (the rethrow is dead code if the finally returns).
fn emit_catch_arms_with_trailing_finally(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    catches: &[crate::TypedCatchClause],
    finally_body: StmtId,
    end_try_label: u32,
) {
    use wasm_encoder::Catch;
    let error_idx = ctx
        .symbols
        .intrinsic_type_indices()
        .map(|i| i.error)
        .expect("error intrinsic registered");
    // Stash the caught error; the dispatch chain runs *inside* the wrapper
    // below, so an unmatched re-raise is caught by the catch_all_ref and still
    // runs `finally`.
    let err_stash = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(error_idx),
    }));
    emitter.instruction(Instruction::LocalSet(err_stash));

    // block $catch_rethrow (result exnref)
    emitter.emit_block(BlockType::Result(ValType::EXNREF));
    // try_table (catch_all_ref $catch_rethrow=0)
    emitter.instruction(Instruction::TryTable(
        BlockType::Empty,
        std::borrow::Cow::Owned(vec![Catch::AllRef { label: 0 }]),
    ));
    emitter.bump_block_depth();
    // The try_table's own label is the matched-arm exit: branching to it lands
    // on the trailing finally below.
    emit_catch_dispatch(emitter, ctx, catches, err_stash);
    emitter.emit_end(); // try_table close — an arm matched and completed normally
    emit_finally_inline(emitter, ctx, finally_body);
    // $catch_rethrow adds one block between this `br` and $end_try.
    emitter.instruction(Instruction::Br(end_try_label + 1));
    emitter.emit_end(); // $catch_rethrow close — exnref on stack
    emit_finally_inline(emitter, ctx, finally_body);
    emitter.instruction(Instruction::ThrowRef);
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
) {
    // Stash discriminant in an anonymous local; numeric enums unboxed to f64
    // so per-case tests use f64.eq instead of a structural-equality call.
    emit_expr(emitter, ctx, discriminant);
    let disc_val = switch_disc_local_type(ctx, discriminant_ty);
    if matches!(discriminant_ty.peel(), Type::NumberEnum { .. }) {
        let boxed_idx = ctx
            .symbols
            .boxed_number_type_idx()
            .expect("boxed_number type registered with intrinsics");
        emitter.instruction(Instruction::StructGet {
            struct_type_index: boxed_idx,
            field_index: 1,
        });
    }
    let disc_local = emitter.add_anonymous_local(disc_val);
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
            emit_case_comparison(emitter, ctx, disc_local, discriminant_ty, value);
            emitter.instruction(Instruction::BrIf(source_idx as u32));
        }
    }
    // Both $default and $end sit at depth cases.len() from inside the dispatcher.
    emitter.instruction(Instruction::Br(cases.len() as u32));

    // Close each $case_i and emit its body. The trailing br $end is dead when
    // the body terminates via break/return, but Wasm's stack-polymorphic typing
    // validates unreachable code, so it's harmless.
    for case in cases {
        emitter.emit_end();
        emitter.push_scope();
        emit_statement(emitter, ctx, case.body);
        emitter.pop_scope();
        let end_label = emitter.break_label();
        emitter.instruction(Instruction::Br(end_label));
    }

    if let Some(default_body) = default {
        emitter.emit_end();
        emitter.push_scope();
        emit_statement(emitter, ctx, default_body);
        emitter.pop_scope();
        // Falls through to `$end` naturally — no explicit `br`.
    }

    emitter.emit_switch_close();
}

/// Numeric-enum discriminants are unboxed to `f64` here so per-case tests use `f64.eq`
/// instead of a structural-equality call on every comparison.
fn switch_disc_local_type(ctx: &CodegenCtx, ty: &Type) -> ValType {
    if matches!(ty.peel(), Type::NumberEnum { .. }) {
        ValType::F64
    } else {
        ctx.symbols.value_type(ty)
    }
}

/// Internal `if/else` uses `BlockType::Result(ValType::I32)` so the i32 result
/// flows out without adding a `br_if`-visible block depth.
fn emit_case_comparison(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    disc_local: u32,
    disc_ty: &Type,
    value: &TypedSwitchValue,
) {
    let disc_val = ctx.symbols.value_type(disc_ty);
    match value {
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
            emit_string_compare(emitter, ctx, disc_local, disc_val, value);
        }
        TypedSwitchValue::Enum { value: payload, .. } => match payload {
            EnumVariantPayload::Number(n) => {
                emitter.instruction(Instruction::LocalGet(disc_local));
                emitter.instruction(Instruction::F64Const((*n).into()));
                emitter.instruction(Instruction::F64Eq);
            }
            EnumVariantPayload::String(s) => {
                emit_string_compare(emitter, ctx, disc_local, disc_val, s);
            }
        },
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
) {
    let pool_idx = ctx
        .strings
        .lookup_text(literal)
        .expect("switch string case interned by CodegenAnalysis");
    let code_units = ctx.strings.code_units(pool_idx);
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .expect("string intrinsic type declared");
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .expect("string intrinsic type declared");
    let vtable_global_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("submilli:prelude.string_vtable imported");
    let string_eq_idx = ctx
        .symbols
        .prelude_func_idx("string_eq")
        .expect("submilli:prelude.string_eq imported");

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
        emitter.emit_const_string(
            string_type_idx,
            raw_string_type_idx,
            vtable_global_idx,
            pool_idx as u32,
            code_units,
        );
        emitter.instruction(Instruction::Call(string_eq_idx));
        emitter.emit_end();
    } else {
        emitter.instruction(Instruction::LocalGet(disc_local));
        emitter.emit_const_string(
            string_type_idx,
            raw_string_type_idx,
            vtable_global_idx,
            pool_idx as u32,
            code_units,
        );
        emitter.instruction(Instruction::Call(string_eq_idx));
    }
}
