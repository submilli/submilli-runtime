//! The structural conformance test and its consumers: `x as T`, the `!`
//! non-null assertion, and the read guard on a narrowed field redeclaration.

use wasm_encoder::{BlockType, Function, HeapType, Instruction, RefType, ValType};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::expr::emit_expr;
use crate::codegen::function_emitter::json::emit_inline_const_raw_string;
use crate::codegen::function_emitter::{FunctionEmitter, cast};
use crate::{ExprId, Ident, Span, Type};

pub const TYPE_TAG_STRINGS: &[&str] =
    &["string", "number", "boolean", "function", "object", "null"];
const RECURSIVE_VALIDATOR_CAPACITY: i32 = 1024;

#[derive(Clone, Copy)]
struct ValidatorState {
    visited: u32,
    validator_ids: u32,
    depth: u32,
}

#[derive(Clone, Copy)]
struct FieldConformanceLocals {
    object: u32,
    field: u32,
}

#[derive(Clone, Copy)]
struct CollectionStorage {
    backing: u32,
    backing_type: u32,
    order_len_field: u32,
    order: u32,
}

pub fn error_prefix(target_ty: &Type) -> String {
    format!("type mismatch: expected {target_ty}, got ")
}

/// `value as target_ty`. `check` is `Some(shape)` when a runtime structural check is needed
/// (the source isn't a static subtype of the target — `unknown`/downcasts); `None` for a
/// statically-proven upcast (repr-only narrow). `shape` has interfaces reduced to their
/// object shapes so this walks plain structural types.
pub fn emit_cast(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value: ExprId,
    target_ty: &Type,
    check: Option<&Type>,
) {
    let source_ty = ctx.ta.expr(value).ty.clone();

    // unknown accepts every value — box to $Object, no test.
    if matches!(target_ty.peel(), Type::Unknown) {
        emit_expr(emitter, ctx, value);
        cast::emit_box(emitter, ctx, &source_ty);
        return;
    }

    let Some(check_shape) = check else {
        if &source_ty != ctx.ta.source_type(value) {
            emit_expr(emitter, ctx, value);
            emit_checked_cast_on_stack(emitter, ctx, &source_ty, target_ty);
            return;
        }
        // Statically-proven upcast: box then narrow representation, no runtime test.
        emit_expr(emitter, ctx, value);
        cast::emit_box(emitter, ctx, &source_ty);
        cast::emit_cast_to(emitter, ctx, target_ty);
        return;
    };

    emit_expr(emitter, ctx, value);
    cast::emit_box(emitter, ctx, &source_ty);

    let object_idx = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared")
        .object;
    let scratch_ty = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_idx),
    });
    let scratch = emitter.add_anonymous_local(scratch_ty);
    emitter.instruction(Instruction::LocalSet(scratch));

    emit_structural_test(emitter, ctx, scratch, check_shape);

    let target_val = ctx.symbols.value_type(target_ty);
    let block_ty = BlockType::Result(target_val);
    emitter.emit_if(block_ty);
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, target_ty);
    emitter.emit_else();
    emit_cast_throw(emitter, ctx, scratch, target_ty);
    emitter.emit_end();
}

/// `value!`. Converts a runtime `null` into a catchable `TypeError` instead
/// of relying on `ref.as_non_null`, which would trap.
pub fn emit_non_null_assert(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value: ExprId,
    target_ty: &Type,
) {
    let source_ty = ctx.ta.expr(value).ty.clone();
    emit_expr(emitter, ctx, value);
    emit_non_null_assert_on_stack(emitter, ctx, &source_ty, target_ty);
}

/// `!` applied to a value already on the stack at `source_ty`'s slot — the
/// optional-chain form, where the operand is the step before rather than an
/// expression this can emit itself.
pub fn emit_non_null_assert_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source_ty: &Type,
    target_ty: &Type,
) {
    cast::emit_box(emitter, ctx, source_ty);

    let object_idx = object_idx_of(ctx);
    let scratch = emitter.add_anonymous_local(scratch_object_ty(object_idx));
    emitter.instruction(Instruction::LocalSet(scratch));

    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ctx.symbols.value_type(target_ty)));
    crate::codegen::throw::emit_type_error_throw(
        emitter,
        ctx,
        crate::codegen::throw::NON_NULL_ASSERT_MESSAGE,
    );
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, target_ty);
    emitter.emit_end();
}

/// Pushes i32 1 if the `(ref null $Object)` in `value_local` structurally conforms to
/// `ty`, 0 otherwise — no throw, so it composes inside unions/fields/elements. Recurses
/// into object fields, array elements, tuple slots, and union members; verifies number and
/// string literal values.
///
/// `typed_ast::runtime_type_is_testable` is the shared allowlist deciding which
/// types may reach this function. `codegen::analysis::note_shape_member_names`
/// separately registers the per-name globals an arm's field scan reads. A new
/// arm must update that registration when it reads names, and may enter the
/// allowlist only if every value the type admits is one this test accepts.
pub(super) fn emit_structural_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    ty: &Type,
) {
    emit_structural_test_inner(
        emitter,
        ctx,
        value_local,
        ty,
        &mut std::collections::BTreeSet::new(),
        None,
    );
}

fn emit_structural_test_inner(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    ty: &Type,
    interface_stack: &mut std::collections::BTreeSet<Type>,
    validator_state: Option<ValidatorState>,
) {
    let i32_block = BlockType::Result(ValType::I32);
    match ty.peel() {
        Type::Union(members) => {
            let mut first = true;
            for m in members {
                emit_structural_test_inner(
                    emitter,
                    ctx,
                    value_local,
                    m,
                    interface_stack,
                    validator_state,
                );
                if first {
                    first = false;
                } else {
                    emitter.instruction(Instruction::I32Or);
                }
            }
            if first {
                emitter.instruction(Instruction::I32Const(0));
            }
        }
        Type::Null => {
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefIsNull);
        }
        Type::Unknown => emitter.instruction(Instruction::I32Const(1)),
        Type::Number => emit_ref_test(emitter, value_local, boxed_number_idx(ctx)),
        Type::Boolean => emit_ref_test(emitter, value_local, boxed_boolean_idx(ctx)),
        Type::String => emit_ref_test(emitter, value_local, string_idx(ctx)),
        Type::NumberEnum { .. } | Type::StringEnum { .. } => {
            if let Some(crate::FieldNarrowingTest::Shape(members)) =
                ctx.ta.runtime_type_tests.get(ty.peel())
                && members.peel() != ty.peel()
            {
                emit_structural_test_inner(
                    emitter,
                    ctx,
                    value_local,
                    members,
                    interface_stack,
                    validator_state,
                );
            } else {
                emit_representation_test(emitter, ctx, value_local, ty);
            }
        }
        Type::BigInt => emit_ref_test(
            emitter,
            value_local,
            ctx.symbols
                .bigint_type_idx()
                .expect("$BigInt intrinsic registered"),
        ),
        Type::Uint8Array => emit_ref_test(
            emitter,
            value_local,
            ctx.symbols
                .uint8_array_type_idx()
                .expect("$Uint8Array intrinsic registered"),
        ),
        Type::Function { .. } => {
            let signature = crate::codegen::closures::classify(ty);
            emit_ref_test(
                emitter,
                value_local,
                ctx.symbols
                    .closure_struct_type_idx(signature)
                    .expect("closure signature registered during analysis"),
            );
        }
        // Generic leaves use the caller's concrete predicate when available.
        // Legacy entry points without descriptors retain their erased checks.
        Type::TypeVar(name) | Type::GenericParam { name, .. } => {
            super::runtime_descriptors::test_parameter(emitter, ctx, name, value_local);
        }
        Type::NumberLiteral(n) => {
            let boxed = boxed_number_idx(ctx);
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(boxed)));
            emitter.emit_if(i32_block);
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(boxed)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: boxed,
                field_index: 1,
            });
            emitter.instruction(Instruction::F64Const(n.0.into()));
            emitter.instruction(Instruction::F64Eq);
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
        }
        Type::BooleanLiteral(value) => {
            let boxed = boxed_boolean_idx(ctx);
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(boxed)));
            emitter.emit_if(i32_block);
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(boxed)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: boxed,
                field_index: 1,
            });
            emitter.instruction(Instruction::I32Const(i32::from(*value)));
            emitter.instruction(Instruction::I32Eq);
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
        }
        Type::StringLiteral(s) => {
            let str_idx = string_idx(ctx);
            let intr = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared");
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(str_idx)));
            emitter.emit_if(i32_block);
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(str_idx)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: str_idx,
                field_index: 1,
            });
            let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(intr.raw_string),
            }));
            emitter.instruction(Instruction::LocalSet(raw_local));
            crate::codegen::function_emitter::json::emit_raw_string_matches_literal(
                emitter, intr, raw_local, s,
            );
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
        }
        Type::Object { fields } => {
            let shape_idx = ctx
                .symbols
                .object_shape_type_idx()
                .expect("$ObjectShape intrinsic registered");
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(shape_idx)));
            emitter.emit_if(i32_block);
            let obj_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(shape_idx),
            }));
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(shape_idx)));
            emitter.instruction(Instruction::LocalSet(obj_local));
            let field_local = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
            emitter.instruction(Instruction::I32Const(1)); // accumulator
            for (fname, f) in fields {
                emit_field_conformance(
                    emitter,
                    ctx,
                    FieldConformanceLocals {
                        object: obj_local,
                        field: field_local,
                    },
                    fname,
                    f,
                    interface_stack,
                    validator_state,
                );
                emitter.instruction(Instruction::I32And);
            }
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
        }
        Type::Array(elem) => {
            let intr = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared");
            let array_idx = ctx
                .symbols
                .array_type_idx()
                .expect("$Array intrinsic registered");
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(array_idx)));
            emitter.emit_if(i32_block);
            let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(intr.raw_array),
            }));
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(array_idx)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: array_idx,
                field_index: 1,
            });
            emitter.instruction(Instruction::LocalSet(raw_local));
            let acc = emitter.add_anonymous_local(ValType::I32);
            let i = emitter.add_anonymous_local(ValType::I32);
            let len = emitter.add_anonymous_local(ValType::I32);
            let elem_local = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
            emitter.instruction(Instruction::I32Const(1));
            emitter.instruction(Instruction::LocalSet(acc));
            emitter.instruction(Instruction::I32Const(0));
            emitter.instruction(Instruction::LocalSet(i));
            emitter.instruction(Instruction::LocalGet(raw_local));
            emitter.instruction(Instruction::ArrayLen);
            emitter.instruction(Instruction::LocalSet(len));
            emitter.emit_block(BlockType::Empty);
            emitter.emit_loop(BlockType::Empty);
            emitter.instruction(Instruction::LocalGet(i));
            emitter.instruction(Instruction::LocalGet(len));
            emitter.instruction(Instruction::I32GeU);
            emitter.instruction(Instruction::BrIf(1));
            emitter.instruction(Instruction::LocalGet(raw_local));
            emitter.instruction(Instruction::LocalGet(i));
            emitter.instruction(Instruction::ArrayGet(intr.raw_array));
            emitter.instruction(Instruction::LocalSet(elem_local));
            emitter.instruction(Instruction::LocalGet(acc));
            emit_structural_test_inner(
                emitter,
                ctx,
                elem_local,
                elem,
                interface_stack,
                validator_state,
            );
            emitter.instruction(Instruction::I32And);
            emitter.instruction(Instruction::LocalSet(acc));
            emitter.instruction(Instruction::LocalGet(i));
            emitter.instruction(Instruction::I32Const(1));
            emitter.instruction(Instruction::I32Add);
            emitter.instruction(Instruction::LocalSet(i));
            emitter.instruction(Instruction::Br(0));
            emitter.emit_end();
            emitter.emit_end();
            emitter.instruction(Instruction::LocalGet(acc));
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
        }
        Type::Tuple(elems) => {
            let intr = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared");
            let array_idx = ctx
                .symbols
                .array_type_idx()
                .expect("$Array intrinsic registered");
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(array_idx)));
            emitter.emit_if(i32_block);
            let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(intr.raw_array),
            }));
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(array_idx)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: array_idx,
                field_index: 1,
            });
            emitter.instruction(Instruction::LocalSet(raw_local));
            // Length must match before indexing slots (else array.get would trap).
            emitter.instruction(Instruction::LocalGet(raw_local));
            emitter.instruction(Instruction::ArrayLen);
            emitter.instruction(Instruction::I32Const(elems.len() as i32));
            emitter.instruction(Instruction::I32Eq);
            emitter.emit_if(i32_block);
            let elem_local = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
            emitter.instruction(Instruction::I32Const(1));
            for (idx, et) in elems.iter().enumerate() {
                emitter.instruction(Instruction::LocalGet(raw_local));
                emitter.instruction(Instruction::I32Const(idx as i32));
                emitter.instruction(Instruction::ArrayGet(intr.raw_array));
                emitter.instruction(Instruction::LocalSet(elem_local));
                emit_structural_test_inner(
                    emitter,
                    ctx,
                    elem_local,
                    et,
                    interface_stack,
                    validator_state,
                );
                emitter.instruction(Instruction::I32And);
            }
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
            emitter.emit_else();
            emitter.instruction(Instruction::I32Const(0));
            emitter.emit_end();
        }
        // Recursion back-edge: delegate to the alias's generated validator, which
        // re-enters this test on the alias body. Terminates on the finite value.
        Type::AliasRef { .. } => {
            let func_idx = ctx
                .symbols
                .runtime_validator_idx(ty.peel())
                .expect("recursive runtime validator pre-allocated during discovery");
            emitter.instruction(Instruction::LocalGet(value_local));
            emit_validator_state(emitter, ctx, validator_state);
            super::runtime_descriptors::validator_environment(emitter, ctx, ty);
            emitter.instruction(Instruction::Call(func_idx));
        }
        Type::InterfaceRef { .. } => emit_interface_ref_test(
            emitter,
            ctx,
            value_local,
            ty,
            interface_stack,
            validator_state,
        ),
        // Nominal, not structural: same-shape sibling classes canonicalize to one
        // WasmGC type, so membership is the vtable-parent walk `instanceof` uses.
        // Unreachable from `as` (class targets are rejected at typecheck); reached
        // from the narrowed-field read guard.
        Type::ClassRef { mangled, .. } => {
            let vtable_global = ctx
                .symbols
                .class_vtable_global_idx(mangled)
                .expect("class vtable global recorded");
            emitter.instruction(Instruction::LocalGet(value_local));
            cast::emit_nominal_instance_test(emitter, ctx, vtable_global);
            if let Some(validator) = ctx.symbols.runtime_validator_idx(ty.peel()) {
                emitter.emit_if(BlockType::Result(ValType::I32));
                emitter.instruction(Instruction::LocalGet(value_local));
                emit_validator_state(emitter, ctx, validator_state);
                super::runtime_descriptors::validator_environment(emitter, ctx, ty);
                emitter.instruction(Instruction::Call(validator));
                emitter.emit_else();
                emitter.instruction(Instruction::I32Const(0));
                emitter.emit_end();
            }
        }
        // Rejected at typecheck (`unsupported_cast_target_reason`); defensive 0.
        _ => emitter.instruction(Instruction::I32Const(0)),
    }
}

fn emit_interface_ref_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    ty: &Type,
    interface_stack: &mut std::collections::BTreeSet<Type>,
    validator_state: Option<ValidatorState>,
) {
    let key = ty.peel().clone();
    if ctx.symbols.generic_runtime_validator(&key).is_some() {
        emit_validator_or_representation(emitter, ctx, value_local, ty, &key, validator_state);
        return;
    }
    match ctx.ta.runtime_type_tests.get(&key) {
        Some(crate::FieldNarrowingTest::Shape(shape)) => emit_structural_test_inner(
            emitter,
            ctx,
            value_local,
            shape,
            interface_stack,
            validator_state,
        ),
        Some(crate::FieldNarrowingTest::Interface(test)) => {
            if interface_stack.insert(key.clone()) {
                emit_interface_test_inner(
                    emitter,
                    ctx,
                    value_local,
                    test,
                    interface_stack,
                    validator_state,
                );
                interface_stack.remove(&key);
            } else {
                emit_validator_or_representation(
                    emitter,
                    ctx,
                    value_local,
                    ty,
                    &key,
                    validator_state,
                );
            }
        }
        _ => emit_validator_or_representation(emitter, ctx, value_local, ty, &key, validator_state),
    }
}

fn emit_validator_or_representation(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    ty: &Type,
    key: &Type,
    validator_state: Option<ValidatorState>,
) {
    if let Some(func_idx) = ctx.symbols.runtime_validator_idx(key) {
        emitter.instruction(Instruction::LocalGet(value_local));
        emit_validator_state(emitter, ctx, validator_state);
        super::runtime_descriptors::validator_environment(emitter, ctx, key);
        emitter.instruction(Instruction::Call(func_idx));
    } else {
        emit_representation_test(emitter, ctx, value_local, ty);
    }
}

/// Validate an erased value against its declared target before casting it.
/// Mismatches become a catchable `TypeError` instead of an unchecked Wasm trap.
pub(crate) fn emit_checked_cast_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source_ty: &Type,
    target_ty: &Type,
) {
    cast::emit_box(emitter, ctx, source_ty);
    let scratch = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
    emitter.instruction(Instruction::LocalSet(scratch));
    match ctx.ta.runtime_type_tests.get(target_ty.peel()) {
        Some(crate::FieldNarrowingTest::Interface(test)) => {
            emit_interface_test(emitter, ctx, scratch, test);
        }
        Some(crate::FieldNarrowingTest::Shape(shape)) => {
            emit_structural_test(emitter, ctx, scratch, shape);
        }
        _ => emit_representation_test(emitter, ctx, scratch, target_ty),
    }
    emitter.emit_if(BlockType::Result(ctx.symbols.value_type(target_ty)));
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, target_ty);
    emitter.emit_else();
    emit_cast_throw(emitter, ctx, scratch, target_ty);
    emitter.emit_end();
}

/// Validate an erased parameter before rebinding it to the declaration's
/// physical slot. Interface values may have host-owned carriers that cannot be
/// reconstructed structurally here (for example Temporal plain-date structs),
/// so parameters require representation safety; concrete value types retain
/// the stronger structural check.
pub(crate) fn emit_checked_parameter_cast_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source_ty: &Type,
    target_ty: &Type,
) {
    if !is_interface_parameter(target_ty) {
        emit_checked_cast_on_stack(emitter, ctx, source_ty, target_ty);
        return;
    }

    cast::emit_box(emitter, ctx, source_ty);
    let scratch = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
    emitter.instruction(Instruction::LocalSet(scratch));
    emit_representation_test(emitter, ctx, scratch, target_ty);
    emitter.emit_if(BlockType::Result(ctx.symbols.value_type(target_ty)));
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, target_ty);
    emitter.emit_else();
    emit_cast_throw(emitter, ctx, scratch, target_ty);
    emitter.emit_end();
}

fn is_interface_parameter(ty: &Type) -> bool {
    match ty.peel() {
        Type::InterfaceRef { .. } => true,
        Type::Union(members) => {
            let mut non_null = members
                .iter()
                .filter(|member| !matches!(member.peel(), Type::Null));
            non_null
                .next()
                .is_some_and(|member| matches!(member.peel(), Type::InterfaceRef { .. }))
                && non_null.next().is_none()
        }
        _ => false,
    }
}

/// Require the carrier an operation consumes, without validating its contents
/// or invoking getters. Reading a value itself does not perform this check.
pub(crate) fn emit_operation_cast_on_stack(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    source_ty: &Type,
    target_ty: &Type,
) {
    cast::emit_box(emitter, ctx, source_ty);
    let scratch = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
    emitter.instruction(Instruction::LocalSet(scratch));
    if matches!(target_ty.peel(), Type::Function { .. }) {
        let signature = crate::codegen::closures::classify(target_ty.peel());
        emit_representation_test(emitter, ctx, scratch, target_ty);
        let opposite = crate::codegen::closures::ClosureSig {
            is_void: !signature.is_void,
            ..signature
        };
        if let Some(structure) = ctx.symbols.closure_struct_type_idx(opposite) {
            emit_ref_test(emitter, scratch, structure);
            emitter.instruction(Instruction::I32Or);
        }
    } else if matches!(
        ctx.symbols.value_type(target_ty),
        ValType::F64 | ValType::I32
    ) {
        emit_structural_test(emitter, ctx, scratch, target_ty);
    } else {
        emit_representation_test(emitter, ctx, scratch, target_ty);
    }
    emitter.emit_if(BlockType::Result(ctx.symbols.value_type(target_ty)));
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, target_ty);
    emitter.emit_else();
    emit_cast_throw(emitter, ctx, scratch, target_ty);
    emitter.emit_end();
}

/// The conservative fallback for types without a full structural validator.
/// It cannot distinguish two interface values sharing `$Object`, but it proves
/// the following representation cast safe and turns representation mismatches
/// into a catchable `TypeError` rather than a Wasm trap.
fn emit_representation_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    target_ty: &Type,
) {
    let ValType::Ref(target) = ctx.symbols.value_type(target_ty) else {
        unreachable!("every non-structurally-testable narrowing target is a reference type")
    };
    let allows_null = cast::target_allows_null(ctx, target_ty);
    if allows_null {
        emitter.instruction(Instruction::LocalGet(value_local));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::I32Const(1));
        emitter.emit_else();
    }
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefTestNonNull(target.heap_type));
    if allows_null {
        emitter.emit_end();
    }
}

/// The read of a class field whose declaration narrows an inherited one. The
/// slot is shared with the parent's declaration, so it can hold a value this
/// declaration does not admit (spec.md §Classes) — test before casting and throw
/// a named, catchable `TypeError` instead of letting `ref.cast` raise a bare
/// `cast failure`. The raw payload slot value is on the stack.
///
/// Pushes i32 1 if the object in `obj_local` carries property `fname` at a value
/// conforming to `field`, 0 otherwise.
///
/// A data slot is read and tested. No data slot is not the same as "absent": an
/// accessor-backed property has no slot, and the value it would answer with is
/// behind a getter this must not call — a conformance predicate that runs user
/// code could throw or have side effects. The accessor slot's existence is
/// therefore accepted here; the accessor call validates its erased return at
/// the actual read. Neither slot means the property really is missing, and only
/// an optional field tolerates that.
///
/// The read counterpart is `expr::emit_object_property_read`, which makes the
/// same three-way distinction and calls the getter because it needs the value.
fn emit_field_conformance(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    locals: FieldConformanceLocals,
    fname: &str,
    field: &crate::ObjectField,
    interface_stack: &mut std::collections::BTreeSet<Type>,
    validator_state: Option<ValidatorState>,
) {
    let i32_block = BlockType::Result(ValType::I32);
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(fname)
        .expect("per-name string global recorded for cast-target field");
    let index_local = emitter.add_anonymous_local(ValType::I32);
    crate::codegen::function_emitter::expr::emit_object_field_index_by_name(
        emitter,
        ctx,
        locals.object,
        name_global,
    );
    emitter.instruction(Instruction::LocalTee(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(i32_block);
    crate::codegen::function_emitter::expr::emit_field_slot_get(
        emitter,
        intrinsics,
        locals.object,
        index_local,
    );
    emitter.instruction(Instruction::LocalSet(locals.field));
    emit_structural_test_inner(
        emitter,
        ctx,
        locals.field,
        &field.ty,
        interface_stack,
        validator_state,
    );
    if field.optional {
        emitter.instruction(Instruction::LocalGet(locals.field));
        emitter.instruction(Instruction::RefIsNull);
        emitter.instruction(Instruction::I32Or);
    }
    emitter.emit_else();
    let getter = crate::codegen::classes::accessor_getter_name(fname);
    if field.optional {
        // Absent and accessor-backed both conform, so the branch is constant.
        emitter.instruction(Instruction::I32Const(1));
    } else if crate::codegen::function_emitter::expr::accessor_branch_emittable(ctx, &getter) {
        let _ = crate::codegen::function_emitter::expr::emit_is_accessor_backed(
            emitter,
            ctx,
            locals.object,
            &getter,
            crate::AccessorKind::Get,
        );
    } else {
        emitter.instruction(Instruction::I32Const(0));
    }
    emitter.emit_end();
}

pub fn emit_narrowed_field_read(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    check: &crate::FieldNarrowingCheck,
    result_ty: &Type,
) {
    emit_narrowed_field_read_as(emitter, ctx, check, result_ty, result_ty);
}

pub(crate) fn emit_narrowed_field_read_as(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    check: &crate::FieldNarrowingCheck,
    result_ty: &Type,
    check_ty: &Type,
) {
    let result_val = ctx.symbols.value_type(result_ty);
    let scratch = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
    emitter.instruction(Instruction::LocalSet(scratch));
    // Prefer the concrete read-time descriptor. It includes generic
    // substitutions and nested interface metadata that the declaration could
    // not know; imported legacy declarations fall back to their recorded test.
    let runtime_test = ctx.ta.runtime_type_tests.get(check_ty.peel());
    let test = match runtime_test {
        Some(crate::FieldNarrowingTest::Representation)
            if !matches!(&check.test, crate::FieldNarrowingTest::Representation) =>
        {
            &check.test
        }
        Some(test) => test,
        None => &check.test,
    };
    match test {
        // Presence is the check only where the read's type rejects `null`. On a
        // generic class the declaration cannot say: `Sub<T>`'s `v: T` is
        // `string | null` at `Sub<string | null>`, where a `null` is legal and
        // the cast lets it through anyway. `result_ty` is substituted, so it
        // answers what the declaration could not.
        crate::FieldNarrowingTest::NonNull => {
            if cast::target_allows_null(ctx, check_ty) {
                emitter.instruction(Instruction::I32Const(1));
            } else {
                emit_is_non_null(emitter, scratch);
            }
        }
        crate::FieldNarrowingTest::Shape(shape) => {
            emit_structural_test(emitter, ctx, scratch, shape);
            // `emit_cast_to` lifts to non-null for every target it does not admit
            // a null for, so those are exactly the targets a `null` in the slot
            // would trap on.
            if !cast::target_allows_null(ctx, check_ty) {
                emit_is_non_null(emitter, scratch);
                emitter.instruction(Instruction::I32And);
            }
        }
        crate::FieldNarrowingTest::Interface(test) => {
            emit_interface_test(emitter, ctx, scratch, test);
        }
        crate::FieldNarrowingTest::Substituted => {
            emit_representation_test(emitter, ctx, scratch, check_ty);
        }
        crate::FieldNarrowingTest::Representation => {
            emit_representation_test(emitter, ctx, scratch, check_ty);
        }
    }
    emitter.emit_if(BlockType::Result(result_val));
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, result_ty);
    emitter.emit_else();
    crate::codegen::throw::emit_type_error_throw(emitter, ctx, &check.message);
    emitter.emit_end();
}

fn emit_interface_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    test: &crate::InterfaceNarrowingTest,
) {
    emit_interface_test_inner(
        emitter,
        ctx,
        value_local,
        test,
        &mut std::collections::BTreeSet::new(),
        None,
    );
}

fn emit_interface_test_inner(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    test: &crate::InterfaceNarrowingTest,
    interface_stack: &mut std::collections::BTreeSet<Type>,
    validator_state: Option<ValidatorState>,
) {
    if test.nullable {
        emitter.instruction(Instruction::LocalGet(value_local));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::I32Const(1));
        emitter.emit_else();
    }

    if !test.shape_allowed {
        emit_non_shape_interface_test(emitter, ctx, value_local, test, validator_state);
        if test.nullable {
            emitter.emit_end();
        }
        return;
    }

    let shape_idx = ctx
        .symbols
        .object_shape_type_idx()
        .expect("$ObjectShape intrinsic registered");
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(shape_idx)));
    emitter.emit_if(BlockType::Result(ValType::I32));
    let object_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(shape_idx),
    }));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(shape_idx)));
    emitter.instruction(Instruction::LocalSet(object_local));
    let field_local = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
    emitter.instruction(Instruction::I32Const(1));
    for (name, field) in &test.members {
        if test.methods.contains(name) {
            let name_global = ctx
                .symbols
                .field_name_string_global_idx(name)
                .expect("interface narrowing method name registered");
            crate::codegen::function_emitter::expr::emit_object_field_read_by_name(
                emitter,
                ctx,
                object_local,
                name_global,
            );
            emitter.instruction(Instruction::LocalSet(field_local));
            emit_structural_test_inner(
                emitter,
                ctx,
                field_local,
                &field.ty,
                interface_stack,
                validator_state,
            );
        } else {
            emit_field_conformance(
                emitter,
                ctx,
                FieldConformanceLocals {
                    object: object_local,
                    field: field_local,
                },
                name,
                field,
                interface_stack,
                validator_state,
            );
        }
        emitter.instruction(Instruction::I32And);
    }
    // Some direct-dispatch interfaces intentionally use `$ObjectShape` only as
    // an inert receiver carrier (for example TextEncoder/TextDecoder); their
    // methods are host imports, not payload slots. Admit those exact carriers
    // alongside ordinary structural conformance.
    emit_non_shape_interface_test(emitter, ctx, value_local, test, validator_state);
    emitter.instruction(Instruction::I32Or);
    emitter.emit_else();
    emit_non_shape_interface_test(emitter, ctx, value_local, test, validator_state);
    emitter.emit_end();

    if test.nullable {
        emitter.emit_end();
    }
}

fn emit_non_shape_interface_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    test: &crate::InterfaceNarrowingTest,
    validator_state: Option<ValidatorState>,
) {
    use crate::InterfaceCarrier;

    let matches_any = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(matches_any));
    for carrier in &test.non_shape_carriers {
        let intr = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared");
        match carrier {
            InterfaceCarrier::ArrayAny => emit_structural_test_inner(
                emitter,
                ctx,
                value_local,
                &Type::Array(Box::new(Type::Unknown)),
                &mut std::collections::BTreeSet::new(),
                validator_state,
            ),
            InterfaceCarrier::Array(ty) => {
                emit_structural_test_inner(
                    emitter,
                    ctx,
                    value_local,
                    ty,
                    &mut std::collections::BTreeSet::new(),
                    validator_state,
                );
            }
            InterfaceCarrier::Number => emit_ref_test(emitter, value_local, boxed_number_idx(ctx)),
            InterfaceCarrier::Boolean => {
                emit_ref_test(emitter, value_local, boxed_boolean_idx(ctx));
            }
            InterfaceCarrier::String => emit_ref_test(emitter, value_local, string_idx(ctx)),
            InterfaceCarrier::BigInt => emit_ref_test(emitter, value_local, intr.bigint),
            InterfaceCarrier::Uint8Array => emit_ref_test(emitter, value_local, intr.uint8_array),
            InterfaceCarrier::MapAny => emit_ref_test(emitter, value_local, intr.map),
            InterfaceCarrier::Map(key, value) => {
                emit_map_carrier_test(emitter, ctx, value_local, key, value, validator_state);
            }
            InterfaceCarrier::SetAny => emit_ref_test(emitter, value_local, intr.set),
            InterfaceCarrier::Set(element) => {
                emit_set_carrier_test(emitter, ctx, value_local, element, validator_state);
            }
            InterfaceCarrier::RegExp => emit_ref_test(emitter, value_local, intr.regex),
            InterfaceCarrier::RegExpMatch => {
                emit_ref_test(emitter, value_local, intr.regex_match_box);
            }
            InterfaceCarrier::TemporalInstant => {
                emit_ref_test(emitter, value_local, intr.temporal_instant);
            }
            InterfaceCarrier::TemporalDuration => {
                emit_ref_test(emitter, value_local, intr.temporal_duration);
            }
            InterfaceCarrier::TemporalZonedDateTime => {
                emit_ref_test(emitter, value_local, intr.temporal_zdt);
            }
            InterfaceCarrier::TemporalPlainDate => {
                emit_ref_test(emitter, value_local, intr.temporal_plain_date);
            }
            InterfaceCarrier::TemporalPlainTime => {
                emit_ref_test(emitter, value_local, intr.temporal_plain_time);
            }
            InterfaceCarrier::TemporalPlainDateTime => {
                emit_ref_test(emitter, value_local, intr.temporal_plain_date_time);
            }
            InterfaceCarrier::TemporalPlainYearMonth => {
                emit_ref_test(emitter, value_local, intr.temporal_plain_year_month);
            }
            InterfaceCarrier::TemporalPlainMonthDay => {
                emit_ref_test(emitter, value_local, intr.temporal_plain_month_day);
            }
            InterfaceCarrier::ObjectShape => {
                emit_ref_test(emitter, value_local, intr.object_shape);
            }
            InterfaceCarrier::Url => emit_ref_test(emitter, value_local, intr.url),
            InterfaceCarrier::FsStat => emit_ref_test(emitter, value_local, intr.fs_stat),
            InterfaceCarrier::FsPeek => emit_ref_test(emitter, value_local, intr.fs_peek),
            InterfaceCarrier::FsDirEntry => emit_ref_test(emitter, value_local, intr.fs_dir_entry),
            InterfaceCarrier::FsInfo => emit_ref_test(emitter, value_local, intr.fs_info),
            InterfaceCarrier::FsFileWriter => {
                emit_ref_test(emitter, value_local, intr.fs_file_writer);
            }
            InterfaceCarrier::HttpResponse => {
                emit_ref_test(emitter, value_local, intr.http_response);
            }
            InterfaceCarrier::HttpDownloadResult => {
                emit_ref_test(emitter, value_local, intr.http_download_result);
            }
            InterfaceCarrier::SessionEntry => {
                emit_ref_test(emitter, value_local, intr.session_entry);
            }
            InterfaceCarrier::SessionPage => emit_ref_test(emitter, value_local, intr.session_page),
        }
        emitter.instruction(Instruction::LocalGet(matches_any));
        emitter.instruction(Instruction::I32Or);
        emitter.instruction(Instruction::LocalSet(matches_any));
    }
    emitter.instruction(Instruction::LocalGet(matches_any));
}

fn emit_set_carrier_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    element: &Type,
    validator_state: Option<ValidatorState>,
) {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    emit_ref_test(emitter, value_local, intr.set);
    emitter.emit_if(BlockType::Result(ValType::I32));
    let backing = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.set),
    }));
    let elements = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_array),
    }));
    let order = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_index_array),
    }));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(intr.set)));
    emitter.instruction(Instruction::LocalTee(backing));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.set,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(elements));
    emitter.instruction(Instruction::LocalGet(backing));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.set,
        field_index: 3,
    });
    emitter.instruction(Instruction::LocalSet(order));
    emit_ordered_collection_members_test(
        emitter,
        ctx,
        CollectionStorage {
            backing,
            backing_type: intr.set,
            order_len_field: 4,
            order,
        },
        &[(elements, element)],
        validator_state,
    );
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end();
}

fn emit_map_carrier_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    key: &Type,
    value: &Type,
    validator_state: Option<ValidatorState>,
) {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    emit_ref_test(emitter, value_local, intr.map);
    emitter.emit_if(BlockType::Result(ValType::I32));
    let backing = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.map),
    }));
    let keys = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_array),
    }));
    let values = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_array),
    }));
    let order = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_index_array),
    }));
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(intr.map)));
    emitter.instruction(Instruction::LocalTee(backing));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.map,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(keys));
    emitter.instruction(Instruction::LocalGet(backing));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.map,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalSet(values));
    emitter.instruction(Instruction::LocalGet(backing));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.map,
        field_index: 4,
    });
    emitter.instruction(Instruction::LocalSet(order));
    emit_ordered_collection_members_test(
        emitter,
        ctx,
        CollectionStorage {
            backing,
            backing_type: intr.map,
            order_len_field: 5,
            order,
        },
        &[(keys, key), (values, value)],
        validator_state,
    );
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end();
}

fn emit_ordered_collection_members_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    storage: CollectionStorage,
    payloads: &[(u32, &Type)],
    validator_state: Option<ValidatorState>,
) {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    let acc = emitter.add_anonymous_local(ValType::I32);
    let cursor = emitter.add_anonymous_local(ValType::I32);
    let slot = emitter.add_anonymous_local(ValType::I32);
    let member = emitter.add_anonymous_local(scratch_object_ty(intr.object));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::LocalSet(acc));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(cursor));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(cursor));
    emitter.instruction(Instruction::LocalGet(storage.backing));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: storage.backing_type,
        field_index: storage.order_len_field,
    });
    emitter.instruction(Instruction::I32GeU);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(storage.order));
    emitter.instruction(Instruction::LocalGet(cursor));
    emitter.instruction(Instruction::ArrayGet(intr.raw_index_array));
    emitter.instruction(Instruction::LocalTee(slot));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(BlockType::Empty);
    for (payload, ty) in payloads {
        emitter.instruction(Instruction::LocalGet(*payload));
        emitter.instruction(Instruction::LocalGet(slot));
        emitter.instruction(Instruction::ArrayGet(intr.raw_array));
        emitter.instruction(Instruction::LocalSet(member));
        emitter.instruction(Instruction::LocalGet(acc));
        emit_structural_test_inner(
            emitter,
            ctx,
            member,
            ty,
            &mut std::collections::BTreeSet::new(),
            validator_state,
        );
        emitter.instruction(Instruction::I32And);
        emitter.instruction(Instruction::LocalSet(acc));
    }
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(cursor));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(cursor));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end();
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(acc));
}

fn emit_validator_state(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    validator_state: Option<ValidatorState>,
) {
    if let Some(state) = validator_state {
        emitter.instruction(Instruction::LocalGet(state.visited));
        emitter.instruction(Instruction::LocalGet(state.validator_ids));
        emitter.instruction(Instruction::LocalGet(state.depth));
        emitter.instruction(Instruction::I32Const(1));
        emitter.instruction(Instruction::I32Add);
    } else {
        let raw_array = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared")
            .raw_array;
        emitter.instruction(Instruction::I32Const(RECURSIVE_VALIDATOR_CAPACITY * 2));
        emitter.instruction(Instruction::ArrayNewDefault(raw_array));
        let raw_index_array = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared")
            .raw_index_array;
        emitter.instruction(Instruction::I32Const(RECURSIVE_VALIDATOR_CAPACITY));
        emitter.instruction(Instruction::ArrayNewDefault(raw_index_array));
        emitter.instruction(Instruction::I32Const(0));
    }
}

/// Body of a recursive runtime validator:
/// `(ref null $Object, ref $rawArray visited, ref $rawIndexArray ids, i32 depth, ref $ObjectFields types) -> i32`.
/// A `(value, validator)` pair already present on the active recursion path
/// closes a valid cycle without conflating mutually recursive shapes.
/// The fixed-capacity path rejects an implausibly deep acyclic graph rather than
/// overflowing the Wasm stack or accepting an unchecked tail.
pub(crate) fn emit_runtime_validator_body(
    ctx: &CodegenCtx,
    key: &Type,
    body: &Type,
    rejects_polymorphic_edge: bool,
    validator_id: i32,
) -> Function {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    let object_idx = intr.object;
    let object_ref_null = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_idx),
    });
    let raw_array_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_array),
    });
    let raw_index_array_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intr.raw_index_array),
    });
    let ident = |name: &str| Ident {
        name: name.to_string(),
        span: Span::at(ctx.file),
    };
    let mut emitter = FunctionEmitter::new(
        ctx,
        &[
            (ident("v"), object_ref_null),
            (ident("visited"), raw_array_ref),
            (ident("validator_ids"), raw_index_array_ref),
            (ident("depth"), ValType::I32),
            (
                ident("types"),
                super::runtime_descriptors::environment_type(ctx.symbols),
            ),
        ],
    );
    super::runtime_descriptors::bind(
        &mut emitter,
        &super::runtime_descriptors::parameters(key),
        4,
    );
    if rejects_polymorphic_edge {
        emitter.instruction(Instruction::I32Const(0));
        return emitter.build();
    }
    let parameters = super::runtime_descriptors::parameters(key);
    let seen = emitter.add_anonymous_local(ValType::I32);
    let index = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(seen));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(index));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::LocalGet(3));
    emitter.instruction(Instruction::I32GeU);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(1));
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::I32Const(2));
    emitter.instruction(Instruction::I32Mul);
    emitter.instruction(Instruction::ArrayGet(intr.raw_array));
    emitter.instruction(Instruction::LocalGet(0));
    emitter.instruction(Instruction::RefEq);
    emitter.instruction(Instruction::LocalGet(2));
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::ArrayGet(intr.raw_index_array));
    emitter.instruction(Instruction::I32Const(validator_id));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::I32And);
    emitter.emit_if(BlockType::Empty);
    emit_same_validator_environment(&mut emitter, ctx, index, parameters.len());
    emitter.instruction(Instruction::LocalSet(seen));
    emitter.instruction(Instruction::LocalGet(seen));
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::Br(3));
    emitter.emit_end();
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(index));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end();
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(seen));
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::I32Const(1));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(3));
    emitter.instruction(Instruction::I32Const(RECURSIVE_VALIDATOR_CAPACITY));
    emitter.instruction(Instruction::I32GeU);
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(1));
    emitter.instruction(Instruction::LocalGet(3));
    emitter.instruction(Instruction::I32Const(2));
    emitter.instruction(Instruction::I32Mul);
    emitter.instruction(Instruction::LocalGet(0));
    emitter.instruction(Instruction::ArraySet(intr.raw_array));
    if !parameters.is_empty() {
        emit_store_validator_environment(&mut emitter, ctx);
    }
    emitter.instruction(Instruction::LocalGet(2));
    emitter.instruction(Instruction::LocalGet(3));
    emitter.instruction(Instruction::I32Const(validator_id));
    emitter.instruction(Instruction::ArraySet(intr.raw_index_array));
    let mut interface_stack = std::collections::BTreeSet::new();
    if key.peel() == body.peel()
        && let Some(crate::FieldNarrowingTest::Interface(test)) =
            ctx.ta.runtime_type_tests.get(key.peel())
    {
        emit_interface_test_inner(
            &mut emitter,
            ctx,
            0,
            test,
            &mut interface_stack,
            Some(ValidatorState {
                visited: 1,
                validator_ids: 2,
                depth: 3,
            }),
        );
    } else {
        emit_structural_test_inner(
            &mut emitter,
            ctx,
            0,
            body,
            &mut interface_stack,
            Some(ValidatorState {
                visited: 1,
                validator_ids: 2,
                depth: 3,
            }),
        );
    }
    emitter.emit_end();
    emitter.emit_end();
    emitter.build()
}

fn emit_is_non_null(emitter: &mut FunctionEmitter, value_local: u32) {
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.instruction(Instruction::I32Eqz);
}

fn emit_ref_test(emitter: &mut FunctionEmitter, value_local: u32, type_idx: u32) {
    emitter.instruction(Instruction::LocalGet(value_local));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(type_idx)));
}

fn boxed_number_idx(ctx: &CodegenCtx) -> u32 {
    ctx.symbols
        .boxed_number_type_idx()
        .expect("$BoxedNumber intrinsic registered")
}

fn boxed_boolean_idx(ctx: &CodegenCtx) -> u32 {
    ctx.symbols
        .boxed_boolean_type_idx()
        .expect("$BoxedBoolean intrinsic registered")
}

fn string_idx(ctx: &CodegenCtx) -> u32 {
    ctx.symbols
        .string_type_idx()
        .expect("$string intrinsic registered")
}

fn object_idx_of(ctx: &CodegenCtx) -> u32 {
    ctx.symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared")
        .object
}

fn scratch_object_ty(object_idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_idx),
    })
}

fn emit_cast_throw(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    scratch: u32,
    target_ty: &Type,
) {
    let prefix = error_prefix(target_ty);
    emit_inline_string(emitter, ctx, &prefix);

    emit_runtime_type_string(emitter, ctx, scratch);

    let concat_idx = ctx
        .symbols
        .prelude_func_idx("string_concat")
        .expect("string_concat imported from prelude");
    emitter.instruction(Instruction::Call(concat_idx));

    let new_idx = ctx
        .symbols
        .prelude_func_idx("TypeError#constructor")
        .expect("TypeError#constructor imported from prelude");
    emitter.instruction(Instruction::Call(new_idx));

    // Constructor returns (ref null $Object); the throw helper narrows to
    // $Error so the throw carries the payload type the catch expects.
    crate::codegen::throw::emit_error_throw(emitter, ctx);
}

fn emit_inline_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, text: &str) {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    let string_vtable_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable imported");
    emitter.instruction(Instruction::GlobalGet(string_vtable_idx));
    emit_inline_const_raw_string(emitter, ctx, text);
    emitter.instruction(Instruction::StructNew(intr.string));
}

/// Cascades ref.test on scratch to produce a JS-typeof-style tag string;
/// closures first, then string/number/boolean, then null, then "object" fallthrough.
fn emit_runtime_type_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, scratch: u32) {
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .expect("$string intrinsic registered");
    let string_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(string_type_idx),
    });
    let block = BlockType::Result(string_ref);

    // null gets its own arm rather than JS's "object" tag — clearer cast error messages.
    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(block);
    emit_inline_string(emitter, ctx, "null");
    emitter.emit_else();

    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        string_type_idx,
    )));
    emitter.emit_if(block);
    emit_inline_string(emitter, ctx, "string");
    emitter.emit_else();

    let num_idx = ctx
        .symbols
        .boxed_number_type_idx()
        .expect("$BoxedNumber intrinsic registered");
    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(num_idx)));
    emitter.emit_if(block);
    emit_inline_string(emitter, ctx, "number");
    emitter.emit_else();

    let bool_idx = ctx
        .symbols
        .boxed_boolean_type_idx()
        .expect("$BoxedBoolean intrinsic registered");
    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(bool_idx)));
    emitter.emit_if(block);
    emit_inline_string(emitter, ctx, "boolean");
    emitter.emit_else();

    let closure_idx = ctx
        .symbols
        .closure_type_idx()
        .expect("$Closure intrinsic registered");
    emitter.instruction(Instruction::LocalGet(scratch));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(closure_idx)));
    emitter.emit_if(block);
    emit_inline_string(emitter, ctx, "function");
    emitter.emit_else();

    emit_inline_string(emitter, ctx, "object");

    for _ in 0..5 {
        emitter.emit_end();
    }
}

/// Visited entries pair the value with its effective type arguments. Forwarded
/// predicates preserve identity; growing arguments construct fresh predicates.
fn emit_same_validator_environment(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    index: u32,
    count: usize,
) {
    emitter.instruction(Instruction::I32Const(1));
    if count == 0 {
        return;
    }
    let intr = ctx.symbols.intrinsic_type_indices().expect("intrinsics");
    let closure = ctx
        .symbols
        .closure_struct_type_idx(super::field_guards::signature())
        .expect("descriptor closure");
    for slot in 0..count {
        emitter.instruction(Instruction::LocalGet(1));
        emitter.instruction(Instruction::LocalGet(index));
        emitter.instruction(Instruction::I32Const(2));
        emitter.instruction(Instruction::I32Mul);
        emitter.instruction(Instruction::I32Const(1));
        emitter.instruction(Instruction::I32Add);
        emitter.instruction(Instruction::ArrayGet(intr.raw_array));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(closure)));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: closure,
            field_index: 2,
        });
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            intr.object_fields,
        )));
        emitter.instruction(Instruction::I32Const(slot as i32));
        emitter.instruction(Instruction::ArrayGet(intr.object_fields));
        emitter.instruction(Instruction::LocalGet(4));
        emitter.instruction(Instruction::I32Const(slot as i32));
        emitter.instruction(Instruction::ArrayGet(intr.object_fields));
        emitter.instruction(Instruction::RefEq);
        emitter.instruction(Instruction::I32And);
    }
}

fn emit_store_validator_environment(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let intr = ctx.symbols.intrinsic_type_indices().expect("intrinsics");
    let closure = ctx
        .symbols
        .closure_struct_type_idx(super::field_guards::signature())
        .expect("descriptor closure");
    emitter.instruction(Instruction::LocalGet(1));
    emitter.instruction(Instruction::LocalGet(3));
    emitter.instruction(Instruction::I32Const(2));
    emitter.instruction(Instruction::I32Mul);
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::GlobalGet(
        ctx.symbols
            .closure_vtable_global_idx()
            .expect("closure vtable"),
    ));
    emitter.instruction(Instruction::RefFunc(
        ctx.symbols.type_descriptor_functions[&Type::Unknown],
    ));
    emitter.instruction(Instruction::LocalGet(4));
    emitter.instruction(Instruction::StructNew(closure));
    emitter.instruction(Instruction::ArraySet(intr.raw_array));
}
