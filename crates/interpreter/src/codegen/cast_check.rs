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
/// Two other walks enumerate the same arms: `typechecker::infer::classes`'s
/// `narrowed_type_is_testable` decides which narrowed field reads get a guard,
/// and `codegen::analysis`'s `note_shape_member_names` registers the per-name
/// globals an arm's field scan reads — the property's own name and its
/// `get <prop>` accessor name. A new arm here reaches the guard only once
/// it is added to both — and only if every value the arm's type admits is one
/// this test accepts, which is why no interface is on that list.
fn emit_structural_test(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value_local: u32,
    ty: &Type,
) {
    let i32_block = BlockType::Result(ValType::I32);
    match ty.peel() {
        Type::Union(members) => {
            let mut first = true;
            for m in members {
                emit_structural_test(emitter, ctx, value_local, m);
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
            // Shallow is-closure (any signature); per-signature conformance is a static concern.
            emit_ref_test(
                emitter,
                value_local,
                ctx.symbols
                    .closure_type_idx()
                    .expect("$Closure intrinsic registered"),
            );
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
                emit_field_conformance(emitter, ctx, obj_local, field_local, fname, f);
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
            emit_structural_test(emitter, ctx, elem_local, elem);
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
                emit_structural_test(emitter, ctx, elem_local, et);
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
                .cast_validator_idx(ty.peel())
                .expect("recursive cast validator pre-allocated during discovery");
            emitter.instruction(Instruction::LocalGet(value_local));
            emitter.instruction(Instruction::Call(func_idx));
        }
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
        }
        // Rejected at typecheck (`unsupported_cast_target_reason`); defensive 0.
        _ => emitter.instruction(Instruction::I32Const(0)),
    }
}

/// The read of a class field whose declaration narrows an inherited one. The
/// slot is shared with the parent's declaration, so it can hold a value this
/// declaration does not admit (spec.md §Classes) — test before casting and throw
/// a named, catchable `TypeError` instead of letting `ref.cast` raise a bare
/// `cast failure`. The raw payload slot value is on the stack.
///
/// `result_ty` can be *narrower* than the declaration `check.test` describes — an
/// optional chain reading through a live field-path narrowing lands here that way
/// — and a value the declaration admits then passes the test only to trap in the
/// cast. Neither arm closes that in general; see the `TODO` below.
/// Pushes i32 1 if the object in `obj_local` carries property `fname` at a value
/// conforming to `field`, 0 otherwise.
///
/// A data slot is read and tested. No data slot is not the same as "absent": an
/// accessor-backed property has no slot, and the value it would answer with is
/// behind a getter this must not call — a conformance predicate that runs user
/// code could throw or have side effects. So the accessor slot's *existence* is
/// accepted as conformance, which is what a static structural check does with a
/// declared member. Neither slot means the property really is missing, and only
/// an optional field tolerates that.
///
/// The read counterpart is `expr::emit_object_property_read`, which makes the
/// same three-way distinction and calls the getter because it needs the value.
fn emit_field_conformance(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    obj_local: u32,
    field_local: u32,
    fname: &str,
    field: &crate::ObjectField,
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
        obj_local,
        name_global,
    );
    emitter.instruction(Instruction::LocalTee(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(i32_block);
    crate::codegen::function_emitter::expr::emit_field_slot_get(
        emitter,
        intrinsics,
        obj_local,
        index_local,
    );
    emitter.instruction(Instruction::LocalSet(field_local));
    emit_structural_test(emitter, ctx, field_local, &field.ty);
    if field.optional {
        emitter.instruction(Instruction::LocalGet(field_local));
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
            obj_local,
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
    let result_val = ctx.symbols.value_type(result_ty);
    let scratch = emitter.add_anonymous_local(scratch_object_ty(object_idx_of(ctx)));
    emitter.instruction(Instruction::LocalSet(scratch));
    // TODO: both arms test the *declaration*, so a `result_ty` that narrows it
    // further leaves a hole — a value the declaration admits passes the test and
    // then reaches the bare cast, the uncatchable `cast failure` this function
    // exists to replace. The `Shape` arm's conjunct closes the case where the
    // narrowing strips `null`; a narrowing that selects among non-null members is
    // still open, on either arm. Testing `result_ty` outright is not the fix:
    // unlike the recorded shape it carries unreduced interfaces
    // `emit_structural_test` cannot lower.
    match &check.test {
        // Presence is the check only where the read's type rejects `null`. On a
        // generic class the declaration cannot say: `Sub<T>`'s `v: T` is
        // `string | null` at `Sub<string | null>`, where a `null` is legal and
        // the cast lets it through anyway. `result_ty` is substituted, so it
        // answers what the declaration could not.
        crate::FieldNarrowingTest::NonNull => {
            if cast::target_allows_null(ctx, result_ty) {
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
            if !cast::target_allows_null(ctx, result_ty) {
                emit_is_non_null(emitter, scratch);
                emitter.instruction(Instruction::I32And);
            }
        }
    }
    emitter.emit_if(BlockType::Result(result_val));
    emitter.instruction(Instruction::LocalGet(scratch));
    cast::emit_cast_to(emitter, ctx, result_ty);
    emitter.emit_else();
    crate::codegen::throw::emit_type_error_throw(emitter, ctx, &check.message);
    emitter.emit_end();
}

/// Body of a per-alias `as`-cast validator: `(ref null $Object) -> i32` (1 = conforms).
/// Structurally tests the alias's expanded body; nested back-edges `call` their own
/// validators so recursion terminates on the value.
pub(crate) fn emit_cast_validator_body(ctx: &CodegenCtx, body: &Type) -> Function {
    let object_idx = object_idx_of(ctx);
    let object_ref_null = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_idx),
    });
    let param = Ident {
        name: "v".to_string(),
        span: Span::at(ctx.file),
    };
    let mut emitter = FunctionEmitter::new(ctx, &[(param, object_ref_null)]);
    emit_structural_test(&mut emitter, ctx, 0, body);
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
