//! Box / unbox helpers — the unified mechanism for moving values
//! between language types and the universal `(ref $Object)` slot
//! type used by arrays, `unknown`, and erased generics.

use wasm_encoder::{BlockType, HeapType, Ieee64, Instruction, RefType, ValType};

use crate::Type;
use crate::codegen::CodegenCtx;
use crate::codegen::classes::VTABLE_PARENT_SLOT;
use crate::codegen::function_emitter::{FunctionEmitter, ReturnTarget};
use crate::codegen::symbol_table::is_nullable_ref;
use crate::typechecker::infer::narrowing::{TruthinessClass, truthiness_class};

/// Emit the nominal class-membership test for the value on top of the stack
/// (any `(ref null $Object)`-compatible ref), leaving an i32 (0/1): walk the
/// value's vtable parent chain, comparing each link by `ref.eq` against the
/// target class's vtable-singleton global. Value identity, not `ref.test` —
/// same-shape sibling classes canonicalize to one WasmGC type, so shape can't
/// carry `instanceof`. Non-class values (strings, boxed primitives, structural
/// objects, closures) exit false at the `$ClassVTable` test — their vtables are
/// plain `$VTable` subtypes.
pub fn emit_nominal_instance_test(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    target_vtable_global_idx: u32,
) {
    let intr = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsic types declared before body emission");
    let ref_null_to = |idx: u32| {
        ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(idx),
        })
    };
    let obj_local = emitter.add_anonymous_local(ref_null_to(intr.object));
    let raw_vt_local = emitter.add_anonymous_local(ref_null_to(intr.vtable));
    let vt_local = emitter.add_anonymous_local(ref_null_to(intr.class_vtable));

    emitter.instruction(Instruction::LocalSet(obj_local));
    emitter.emit_block(BlockType::Result(ValType::I32)); // $out
    emitter.emit_block(BlockType::Empty); // $false
    emitter.instruction(Instruction::LocalGet(obj_local));
    emitter.instruction(Instruction::BrOnNull(0));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.object,
        field_index: 0,
    });
    emitter.instruction(Instruction::LocalTee(raw_vt_local));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        intr.class_vtable,
    )));
    emitter.instruction(Instruction::I32Eqz);
    emitter.instruction(Instruction::BrIf(0));
    emitter.instruction(Instruction::LocalGet(raw_vt_local));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        intr.class_vtable,
    )));
    emitter.instruction(Instruction::LocalSet(vt_local));
    emitter.emit_loop(BlockType::Empty); // $walk
    emitter.instruction(Instruction::LocalGet(vt_local));
    emitter.instruction(Instruction::GlobalGet(target_vtable_global_idx));
    emitter.instruction(Instruction::RefEq);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::Br(3)); // -> $out
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(vt_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intr.class_vtable,
        field_index: VTABLE_PARENT_SLOT,
    });
    emitter.instruction(Instruction::BrOnNull(1)); // chain root -> $false
    emitter.instruction(Instruction::LocalSet(vt_local));
    emitter.instruction(Instruction::Br(0)); // continue $walk
    emitter.emit_end(); // end $walk
    emitter.emit_end(); // end $false
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end(); // end $out
}

/// Emit Wasm instructions that wrap the value currently on top of
/// the stack (of language type `ty`) into a `(ref $Object)`.
pub fn emit_box(emitter: &mut FunctionEmitter<'_>, ctx: &CodegenCtx<'_>, ty: &Type) {
    let ty = ty.peel();
    match ty {
        Type::Number | Type::NumberLiteral(_) => {
            let boxed_idx = ctx
                .symbols
                .boxed_number_type_idx()
                .expect("boxed_number type registered");
            let vtable_global = ctx
                .symbols
                .prelude_global_idx("boxed_number_vtable")
                .expect("boxed_number_vtable imported");
            // Stack: [n_f64]. Need [vtable_ref, n_f64] for struct.new.
            // Stash via scratch local.
            let scratch = emitter.add_anonymous_local(wasm_encoder::ValType::F64);
            emitter.instruction(Instruction::LocalSet(scratch));
            emitter.instruction(Instruction::GlobalGet(vtable_global));
            emitter.instruction(Instruction::LocalGet(scratch));
            emitter.instruction(Instruction::StructNew(boxed_idx));
        }
        Type::Boolean | Type::BooleanLiteral(_) => {
            let boxed_idx = ctx
                .symbols
                .boxed_boolean_type_idx()
                .expect("boxed_boolean type registered");
            let vtable_global = ctx
                .symbols
                .prelude_global_idx("boxed_boolean_vtable")
                .expect("boxed_boolean_vtable imported");
            let scratch = emitter.add_anonymous_local(wasm_encoder::ValType::I32);
            emitter.instruction(Instruction::LocalSet(scratch));
            emitter.instruction(Instruction::GlobalGet(vtable_global));
            emitter.instruction(Instruction::LocalGet(scratch));
            emitter.instruction(Instruction::StructNew(boxed_idx));
        }
        Type::String
        | Type::StringLiteral(_)
        | Type::BigInt
        | Type::Object { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Uint8Array
        | Type::Function { .. } => {
            // Already an $Object subtype — upcast is implicit.
        }
        Type::TypeVar(_) | Type::GenericParam { .. } | Type::Unknown | Type::Never => {
            // Never: the call always throws before producing a value, but
            // the wasm sig still has the slot — lowered same as Unknown.
        }
        Type::Null => {
            // null lowers to (ref null none) — the bottom of the WasmGC ref
            // hierarchy — a subtype of every nullable ref; no instruction needed.
        }
        Type::Void | Type::Error => {
            unimplemented!("emit_box for {ty:?} arrives in a later codegen task");
        }
        Type::NumberEnum { .. } | Type::StringEnum { .. } => {}
        Type::Union(_) => {
            // value_type is authoritative: literal-only unions can collapse to
            // primitive slots (mirrors emit_cast_to's Union arm); ref-lowered
            // unions are already $Object subtypes.
            match ctx.symbols.value_type(ty) {
                ValType::F64 => emit_box(emitter, ctx, &Type::Number),
                ValType::I32 => emit_box(emitter, ctx, &Type::Boolean),
                ValType::Ref(_) => {}
                other => {
                    unreachable!("Type::Union lowers to unsupported Wasm value type: {other:?}")
                }
            }
        }
        // A class instance is an `$Object` subtype; the upcast is implicit.
        Type::InterfaceRef { .. } | Type::ClassRef { .. } => {}
        // A recursion back-edge lowers to `(ref null $Object)` — already
        // an `$Object` subtype, so the upcast is implicit. No-op.
        Type::AliasRef { .. } => {}
        Type::Alias { .. } | Type::Refined { .. } | Type::Readonly(_) => {
            unreachable!("peel guarantees no alias here (SUB-242)")
        }
    }
}

/// Coerce a `source_ty`-typed value into the Wasm slot type for `target_ty`.
/// Boxes primitives (f64/i32 → ref); WasmGC subtyping handles ref→ref cases.
///
/// Caller guarantees `source_ty` is assignable to `target_ty` (typechecker-proven);
/// narrowing-direction casts use `emit_narrowing_cast` instead.
pub fn emit_coerce_to_slot(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    source_ty: &Type,
    target_ty: &Type,
) {
    let source_val = ctx.symbols.value_type(source_ty);
    let target_val = ctx.symbols.value_type(target_ty);
    if source_val == target_val {
        return;
    }
    // Never: the call already threw, but the wasm validator still needs a
    // well-typed cast sequence; emit_cast_to covers this. Traps here are fine.
    if matches!(source_ty.peel(), Type::Never) {
        emit_cast_to(emitter, ctx, target_ty);
        return;
    }
    emit_coerce_to_wasm_slot(emitter, ctx, source_ty, target_val);
}

/// Coerce a `source_ty`-typed value into an already-known Wasm slot type.
///
/// Used where the target's Wasm type was *recorded* when the signature was
/// emitted rather than re-derived from a language type — class-method vtable
/// slots and closure funcref result slots. A vtable signature deliberately
/// erases class types and type variables (see `SymbolTable::slot_value_type`);
/// re-deriving would be unsound because `value_type`'s `ClassRef` lowering
/// changes once the class's struct type is recorded.
pub fn emit_coerce_to_wasm_slot(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    source_ty: &Type,
    target_val: ValType,
) {
    let source_val = ctx.symbols.value_type(source_ty);
    if source_val == target_val {
        return;
    }
    // A `never`-typed argument comes from a call that always throws, so this
    // point is unreachable. The validator still has to see a well-typed stack
    // and the value's slot may be a primitive it can't be cast into, so make
    // the rest of the sequence stack-polymorphic instead.
    if matches!(source_ty.peel(), Type::Never) {
        emitter.instruction(Instruction::Unreachable);
        return;
    }
    if crate::codegen::closure_coercions::emit_coercion(emitter, ctx, source_ty, target_val) {
        return;
    }
    match (&source_val, &target_val) {
        (ValType::Ref(_), ValType::F64) => {
            emit_cast_to(emitter, ctx, &Type::Number);
        }
        // Primitive into a boxed slot.
        (ValType::F64 | ValType::I32, ValType::Ref(_)) => emit_box(emitter, ctx, source_ty),
        // Ref subtypes — WasmGC subtyping carries the value; no instruction needed.
        (ValType::Ref(source), ValType::Ref(target))
            if ctx.symbols.ref_fits_slot(*source, *target) => {}
        // Any other ref pairing needs the value pinned to the slot's own heap
        // type: a nullable value into a non-null slot, or a supertype-lowered
        // value (an `InterfaceRef` is `(ref null $Object)`) into a narrower
        // structural slot. The typechecker proved the value fits, so neither
        // trap is reachable.
        (ValType::Ref(_), ValType::Ref(target)) => {
            if target.nullable {
                emitter.instruction(Instruction::RefCastNullable(target.heap_type));
            } else {
                emitter.instruction(Instruction::RefCastNonNull(target.heap_type));
            }
        }
        _ => unreachable!(
            "emit_coerce_to_wasm_slot: {source_ty:?} ({source_val:?}) does not fit \
             recorded slot type {target_val:?}"
        ),
    }
}

/// Coerce the returned value on top of the stack (static type `source_ty`) into
/// the enclosing body's [`ReturnTarget`].
///
/// A recorded slot is coerced to as a Wasm type, not through the declaration it
/// came from: coercing `(x: number): number => x + 2` against its declaration
/// would leave a bare f64 in the closure ABI's ref slot.
pub fn emit_coerce_to_return_slot(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    source_ty: &Type,
) {
    match emitter.return_target().clone() {
        ReturnTarget::Slot(slot) => emit_coerce_to_wasm_slot(emitter, ctx, source_ty, slot),
        ReturnTarget::Declared(ret) => emit_coerce_to_slot(emitter, ctx, source_ty, &ret),
        // On the `return` route the drop is redundant, `return` being
        // polymorphic; it is the expression body that would otherwise fall off
        // the end unbalanced.
        ReturnTarget::VoidClosure => {
            if !source_ty.is_void() {
                emitter.instruction(Instruction::Drop);
            }
        }
        ReturnTarget::NoResult => {}
    }
}

/// Consume the condition value (static type `cond_ty`) on the stack and leave
/// an i32 where nonzero = truthy, per JS semantics: `null`, `""`, `0`, `-0`,
/// `NaN`, `false`, and `0n` are falsy; everything else is truthy.
pub fn emit_condition_to_i32(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    cond_ty: &Type,
) {
    let val = ctx.symbols.value_type(cond_ty);
    match val {
        // Number slot: `|x| > 0` folds `0`, `-0`, and `NaN` (abs(NaN) > 0 is
        // false) into falsy in three instructions.
        ValType::F64 => {
            emitter.instruction(Instruction::F64Abs);
            emitter.instruction(Instruction::F64Const(Ieee64::from(0.0)));
            emitter.instruction(Instruction::F64Gt);
        }
        ValType::Ref(rt) => emit_ref_truthiness(emitter, ctx, cond_ty, rt.nullable),
        // Boolean slot (i32) is already the condition convention.
        _ => {}
    }
}

/// Ref-slot truthiness. Fast paths: a type whose only falsy value is `null`
/// needs just a null test; a bare `$string` needs just its length. Everything
/// else (falsy-bearing unions, erased generics) takes a `ref.test` dispatch
/// over only the falsy kinds the static type admits, defaulting to truthy —
/// mirrors `emit_typeof_tag`.
fn emit_ref_truthiness(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    cond_ty: &Type,
    nullable: bool,
) {
    use TruthinessClass::*;
    let peeled = cond_ty.peel();
    let members: Vec<&Type> = match peeled {
        Type::Union(members) => members.iter().collect(),
        single => vec![single],
    };
    let classes: Vec<TruthinessClass> = members
        .iter()
        .filter(|m| !matches!(m.peel(), Type::Null))
        .map(|m| truthiness_class(m))
        .collect();

    if classes.iter().all(|c| *c == AlwaysTruthy) {
        if nullable {
            emitter.instruction(Instruction::RefIsNull);
            emitter.instruction(Instruction::I32Eqz);
        } else {
            emitter.instruction(Instruction::Drop);
            emitter.instruction(Instruction::I32Const(1));
        }
        return;
    }
    if classes.iter().all(|c| *c == AlwaysFalsy) {
        emitter.instruction(Instruction::Drop);
        emitter.instruction(Instruction::I32Const(0));
        return;
    }

    let needs = |class: TruthinessClass, literal: fn(&Type) -> bool| {
        classes.contains(&class)
            || classes.contains(&Dynamic)
            || members.iter().any(|m| literal(m.peel()))
    };
    let test_string = needs(StringLike, |m| matches!(m, Type::StringLiteral(_)));
    let test_number = needs(NumberLike, |m| matches!(m, Type::NumberLiteral(_)));
    let test_boolean = needs(BooleanLike, |m| matches!(m, Type::BooleanLiteral(_)));
    let test_bigint = needs(BigIntLike, |_| false);

    let slot = ctx.symbols.value_type(cond_ty);
    let tmp = emitter.add_anonymous_local(slot);
    emitter.instruction(Instruction::LocalSet(tmp));

    // Nested if/else chain, innermost default = truthy. Each falsy-capable
    // kind gets a `ref.test` arm that extracts its payload's own truthiness.
    let mut open_blocks = 0u32;
    if nullable {
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::I32Const(0));
        emitter.emit_else();
        open_blocks += 1;
    }
    if test_string {
        let idx = ctx
            .symbols
            .string_type_idx()
            .expect("$string intrinsic type registered");
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(idx)));
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: idx,
            field_index: 1,
        });
        emitter.instruction(Instruction::ArrayLen);
        emitter.emit_else();
        open_blocks += 1;
    }
    if test_number {
        let idx = ctx
            .symbols
            .boxed_number_type_idx()
            .expect("$BoxedNumber intrinsic type registered");
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(idx)));
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: idx,
            field_index: 1,
        });
        emitter.instruction(Instruction::F64Abs);
        emitter.instruction(Instruction::F64Const(0.0.into()));
        emitter.instruction(Instruction::F64Gt);
        emitter.emit_else();
        open_blocks += 1;
    }
    if test_boolean {
        let idx = ctx
            .symbols
            .boxed_boolean_type_idx()
            .expect("$BoxedBoolean intrinsic type registered");
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(idx)));
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: idx,
            field_index: 1,
        });
        emitter.emit_else();
        open_blocks += 1;
    }
    if test_bigint {
        let idx = ctx
            .symbols
            .bigint_type_idx()
            .expect("$bigint intrinsic type registered");
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(idx)));
        emitter.emit_if(BlockType::Result(ValType::I32));
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        // Field 1 is the sign (−1/0/1) — nonzero iff the bigint is nonzero.
        emitter.instruction(Instruction::StructGet {
            struct_type_index: idx,
            field_index: 1,
        });
        emitter.emit_else();
        open_blocks += 1;
    }
    emitter.instruction(Instruction::I32Const(1));
    for _ in 0..open_blocks {
        emitter.emit_end();
    }
}

/// Emit the Wasm cast from `cast_info.from_ty` to `cast_info.to_ty`.
/// The cast is **unchecked** — codegen trusts the typechecker's narrowing
/// decision; the Wasm-level cast traps if the predicate's runtime check was unsound.
pub fn emit_narrowing_cast(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    cast_info: &crate::typechecker::infer::narrowing::CastInfo,
) {
    use crate::typechecker::infer::narrowing::CastKind;
    if matches!(cast_info.to_ty.peel(), Type::Never) {
        emitter.instruction(Instruction::Drop);
        emitter.instruction(Instruction::Unreachable);
        return;
    }
    let from_val = ctx.symbols.value_type(&cast_info.from_ty);
    let to_val = ctx.symbols.value_type(&cast_info.to_ty);
    // Same Wasm repr → no instructions. Happens when the inferer
    // tightened a multi-member union into another multi-member union
    // (e.g., `number | string | null` → `string | null` after
    // `typeof x === "number"` else-branch) — both lower to
    // `(ref null $Object)`.
    if from_val == to_val {
        return;
    }
    // Repr changes only in the nullable bit (same heap type, source nullable,
    // target non-null) — e.g. the false-branch of `!== null` on a union like
    // `number | string | null` narrows to a non-null union still lowered to
    // `(ref $Object)`. A bare `ref.as_non_null` covers the repr gap; no
    // `ref.cast` is meaningful since the heap type is unchanged.
    if let (ValType::Ref(s), ValType::Ref(t)) = (&from_val, &to_val)
        && s.heap_type == t.heap_type
        && s.nullable
        && !t.nullable
    {
        emitter.instruction(Instruction::RefAsNonNull);
        return;
    }
    match cast_info.cast_kind {
        CastKind::NonNull | CastKind::Unbox => {
            // Both reduce to emit_cast_to: source is (ref null $Object),
            // target is the narrowed concrete type.
            emit_cast_to(emitter, ctx, &cast_info.to_ty);
        }
        CastKind::RefSubtype => {
            // Same-repr cases were handled above. ref.cast trusts the
            // typechecker — traps only if the narrowing decision was unsound.
            let ValType::Ref(target_ref) = to_val else {
                unreachable!(
                    "RefSubtype target lowers to a non-ref Wasm type: from={:?} to={:?}",
                    cast_info.from_ty, cast_info.to_ty,
                );
            };
            let cast = if target_ref.nullable {
                Instruction::RefCastNullable(target_ref.heap_type)
            } else {
                Instruction::RefCastNonNull(target_ref.heap_type)
            };
            emitter.instruction(cast);
        }
    }
}

/// Recover `ty`'s value type from a slot that stores it erased — a box cell's
/// payload, keyed on `slot_value_type`.
///
/// Skips the cast when the two lowerings already agree — which is not the same
/// as `ty` not being erased. An erased type often lowers to exactly the
/// `(ref null $Object)` the cell holds: a `TypeVar` or `GenericParam` does, and
/// so does a union mixing one with anything else. Gating on erasure instead
/// would cast a value already in its own lowering — and for an unrecorded
/// `ClassRef` that cast is a `ref.as_non_null` plus a `ref.cast` to a class
/// struct this module never recorded.
pub fn emit_unerase(emitter: &mut FunctionEmitter<'_>, ctx: &CodegenCtx<'_>, ty: &Type) {
    if ctx.symbols.slot_value_type(ty) != ctx.symbols.value_type(ty) {
        emit_cast_to(emitter, ctx, ty);
    }
}

/// Emit Wasm instructions that pop a `(ref null $Object)` from the
/// stack and produce a `ty`-typed value.
///
/// Never leaves the stack *wider* than `value_type(ty)` — the arms that emit no
/// `ref.cast` narrow with `ref.as_non_null` or nothing at all. Several callers
/// gate on `ValType` equality alone and would emit an ill-typed sequence if a
/// cast target could come back wider than its own lowering.
pub fn emit_cast_to(emitter: &mut FunctionEmitter<'_>, ctx: &CodegenCtx<'_>, ty: &Type) {
    // codegen casts on the structural shape — peel through
    // aliases. The alias label is purely a typechecker/display thing.
    let ty = ty.peel();
    // Every per-type cast below assumes non-null input, so lift unless the target
    // admits a null of its own.
    if !target_allows_null(ctx, ty) {
        emitter.instruction(Instruction::RefAsNonNull);
    }
    match ty {
        Type::Number | Type::NumberLiteral(_) => {
            let boxed_idx = ctx
                .symbols
                .boxed_number_type_idx()
                .expect("boxed_number type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(boxed_idx)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: boxed_idx,
                field_index: 1,
            });
        }
        Type::Boolean | Type::BooleanLiteral(_) => {
            let boxed_idx = ctx
                .symbols
                .boxed_boolean_type_idx()
                .expect("boxed_boolean type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(boxed_idx)));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: boxed_idx,
                field_index: 1,
            });
        }
        Type::String | Type::StringLiteral(_) => {
            let idx = ctx
                .symbols
                .string_type_idx()
                .expect("string type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::BigInt => {
            let idx = ctx
                .symbols
                .bigint_type_idx()
                .expect("bigint type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::Object { .. } => {
            // `Type::Object` lowers to `(ref $ObjectShape)`,
            // not the arity-specific subtype. The static arity does
            // not pin runtime arity once width subtyping has erased
            // it, so casting to a per-arity subtype here would trap
            // for any widened value. Field reads/writes go through
            // the per-shape getter/setter funcrefs on slots 2/3.
            let idx = ctx
                .symbols
                .object_shape_type_idx()
                .expect("$ObjectShape type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::Array(_) | Type::Tuple(_) => {
            // Tuples and arrays share the `$Array` Wasm representation
            // — the typechecker keeps `Type::Tuple` distinct for
            // positional assignability + per-position `emit_cast_to`
            // at the element level.
            let idx = ctx.symbols.array_type_idx().expect("array type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::Uint8Array => {
            let idx = ctx
                .symbols
                .uint8_array_type_idx()
                .expect("uint8_array type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::TypeVar(_) | Type::GenericParam { .. } | Type::Unknown | Type::Never => {
            // Never: the call always throws before producing a value; reuses
            // Unknown's (ref null $Object) lowering — see value_type.
        }
        Type::Function { .. } => {
            crate::codegen::closure_coercions::emit_erased_cast(
                emitter,
                ctx,
                crate::codegen::closures::classify(ty),
            );
        }
        Type::Null => {
            // Null lowers to (ref null $Object) — already a subtype of any nullable ref.
        }
        Type::Void | Type::Error => {
            unimplemented!("emit_cast_to for {ty:?} arrives in a later codegen task");
        }
        Type::Union(_) => {
            // value_type is authoritative: literal-only unions can collapse to
            // primitive slots, while object/mixed unions remain references.
            match ctx.symbols.value_type(ty) {
                ValType::F64 => emit_cast_to(emitter, ctx, &Type::Number),
                ValType::I32 => emit_cast_to(emitter, ctx, &Type::Boolean),
                ValType::Ref(target_ref) => {
                    let cast = if target_ref.nullable {
                        Instruction::RefCastNullable(target_ref.heap_type)
                    } else {
                        Instruction::RefCastNonNull(target_ref.heap_type)
                    };
                    emitter.instruction(cast);
                }
                other => {
                    unreachable!("Type::Union lowers to unsupported Wasm value type: {other:?}")
                }
            }
        }
        Type::NumberEnum { .. } => {
            // Numeric enum values are `$BoxedNumber` instances —
            // cast the universal slot back to that subtype. Unlike
            // `Type::Number`, do NOT unbox to f64: the static type
            // is the boxed form.
            let boxed_idx = ctx
                .symbols
                .boxed_number_type_idx()
                .expect("boxed_number type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(boxed_idx)));
        }
        Type::StringEnum { .. } => {
            let idx = ctx
                .symbols
                .string_type_idx()
                .expect("string type registered");
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::InterfaceRef { .. } => {
            // InterfaceRef: RefAsNonNull above is enough — method dispatch uses
            // mangled names, not ref.cast to a per-interface struct.
        }
        Type::ClassRef { mangled, .. } => {
            // Body emission runs after `imported_classes::reconstruct` and the
            // `ClassPlan` reservation, so every class a body can reference has a
            // recorded struct type; a miss here is a dependency-usage collection
            // gap, not the erased pre-recording representation `value_type`
            // tolerates for env/box emission.
            let idx = ctx
                .symbols
                .class_struct_type_idx(mangled)
                .unwrap_or_else(|| panic!("class struct type recorded for `{mangled}`"));
            emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(idx)));
        }
        Type::AliasRef { .. } => {
            // A recursion back-edge lowers to `(ref null $Object)` and the
            // `RefAsNonNull` above is skipped for it, so the null the alias body
            // may list flows through. Field reads go through the shape
            // field-name scan, same as `InterfaceRef`; no per-shape cast.
        }
        Type::Alias { .. } | Type::Refined { .. } | Type::Readonly(_) => {
            unreachable!("peel guarantees no alias here (SUB-242)")
        }
    }
}

/// Whether [`emit_cast_to`] lets a `null` reach the target rather than lifting to
/// non-null first. True for the targets whose lowering is a nullable
/// `(ref null $Object)`: bare `Null`, nullable unions, and the no-op
/// erased/dynamic types — `unknown`, `never`, an erased generic (which can hold
/// null after instantiation, e.g. `T = string | null`), and a recursion back-edge
/// whose body may list `null`. `emit_cast_to`'s `Union` arm uses `RefCastNullable`
/// for the same reason. A union's nullability is its *lowering*, not whether a
/// member is spelled `null`: `T | number` lowers nullable because `T` does.
///
/// A caller that must not hand [`emit_cast_to`] a `null` has to ask this, not
/// `is_nullable_ref`: the two disagree — an `InterfaceRef` lowers to a nullable
/// slot and is still `ref.as_non_null`ed there, which traps.
pub fn target_allows_null(ctx: &CodegenCtx<'_>, ty: &Type) -> bool {
    let ty = ty.peel();
    matches!(
        ty,
        Type::Null
            | Type::Unknown
            | Type::Never
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::AliasRef { .. }
    ) || (matches!(ty, Type::Union(_)) && is_nullable_ref(ctx.symbols.value_type(ty)))
}
