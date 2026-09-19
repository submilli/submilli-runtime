//! Recursive emitter for typed expressions.
//!
//! Walks a [`TypedExprKind`] tree and pushes Wasm instructions onto a
//! [`FunctionEmitter`]. Stateless — pure recursion over `&CodegenCtx` plus
//! mutation of the supplied emitter; both `_start`'s initializer code and
//! function bodies share this single dispatch.
//!
//! Each [`TypedExprKind`] variant lands as code generation work proceeds:
//! - `Number`, arithmetic `Binary`, `Unary::Neg/Pos`.
//! - `Binary::Add` on strings (dispatched via prelude).
//! - `Boolean`, comparisons, equality, `And`/`Or`, `Unary::Not`.
//! - `LocalRef` (function-local bindings).
//! - Later: user `Call`, `Null`.
//!
//! Until those land, unimplemented branches `unimplemented!` at codegen
//! time — the typed AST never reaches them in current fixtures.

use crate::codegen::CodegenCtx;
use crate::codegen::bounds::{emit_checked_index, stash_index_operand};
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::function_emitter::cast;
use crate::codegen::symbol_table::{MethodSlotAbi, may_hold_null};
use crate::typechecker::infer::narrowing::{BindingId, ReferencePath, cast_info_for};
use crate::{BinOp, ExprId, Ident, Intrinsic, Type, TypedExprKind, UnOp};
use wasm_encoder::{BlockType, HeapType, Ieee64, Instruction, RefType, ValType};

/// Emit code for the expression at `id`, leaving its result on the Wasm
/// stack. Records the expression's source span against the emitter so the
/// DWARF wiring task can recover instruction-offset → source-position
/// mappings later.
pub fn emit_expr(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, id: ExprId) {
    emit_expr_value(emitter, ctx, id);
    // Preserve concrete field validators while the expression still carries
    // its class arguments, before a surrounding cast or slot erases them.
    crate::codegen::field_guards::attach(emitter, ctx, &ctx.ta.expr(id).ty);
}

fn emit_expr_value(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, id: ExprId) {
    let expr = ctx.ta.expr(id);
    emitter.record_span(expr.span);
    // A node the enclosing statement already evaluated into a local — see
    // `FunctionEmitter::single_evaluations`.
    if let Some(slot) = emitter.evaluated_slot(id) {
        emitter.instruction(Instruction::LocalGet(slot));
        return;
    }
    match &expr.kind {
        TypedExprKind::Number(v) => {
            emitter.instruction(Instruction::F64Const(Ieee64::from(*v)));
        }
        TypedExprKind::BigInt(digits) => emit_bigint_literal(emitter, ctx, digits),
        TypedExprKind::Boolean(b) => {
            emitter.instruction(Instruction::I32Const(if *b { 1 } else { 0 }));
        }
        // String literals materialize via the prelude's vtable global +
        // `array.new_data` + `struct.new $string`, reading from the
        // per-literal passive data segment that StringPool assigned.
        TypedExprKind::String(_) => {
            let pool_idx = ctx
                .strings
                .locations
                .get(&id)
                .copied()
                .expect("CodegenAnalysis recorded string literals");
            let code_units = ctx.strings.code_units(pool_idx);
            let string_type_idx = ctx
                .symbols
                .string_type_idx()
                .expect("Type::String requires the intrinsic types to be declared");
            let raw_string_type_idx = ctx
                .symbols
                .raw_string_type_idx()
                .expect("Type::String requires the intrinsic types to be declared");
            let vtable_global_idx = ctx
                .symbols
                .prelude_global_idx("string_vtable")
                .expect("string_vtable global imported from prelude");
            emitter.emit_const_string(
                string_type_idx,
                raw_string_type_idx,
                vtable_global_idx,
                pool_idx as u32,
                code_units,
            );
        }
        TypedExprKind::Regex { source, flags } => {
            emit_regex_literal(emitter, ctx, source, flags);
        }
        TypedExprKind::EffectThen { effect, result } => {
            emit_expr(emitter, ctx, *effect);
            // A `void` call leaves nothing on the stack, so there is nothing to drop.
            if !ctx.ta.expr(*effect).ty.is_void() {
                emitter.instruction(Instruction::Drop);
            }
            emit_expr(emitter, ctx, *result);
        }
        TypedExprKind::Binary { op, lhs, rhs } => {
            emit_binary(emitter, ctx, *op, *lhs, *rhs, &expr.ty);
        }
        TypedExprKind::Unary { op, operand } => {
            emit_unary(emitter, ctx, *op, *operand);
        }
        TypedExprKind::LocalRef { ident, boxed } => {
            // Function-scope binding (parameter, function-local
            // `let`/`const`, or captured-by-closure — captures are
            // materialized as ordinary locals at the closure
            // prologue, see `emit_closure_function`). Boxed reads
            // load the `(ref $box_T)` slot, `struct.get` the value
            // field, then recover the declared type from the cell's
            // erased payload. Non-boxed reads are a plain `local.get`.
            let slot = emitter
                .local_slot(&ident.name)
                .expect("function-local binding has a Wasm local slot");
            emitter.instruction(Instruction::LocalGet(slot));
            if *boxed {
                let box_idx = ctx
                    .symbols
                    .box_type_idx(&expr.ty)
                    .expect("box type registered for every boxed LocalRef");
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
                cast::emit_unerase(emitter, ctx, &expr.ty);
            }
        }
        TypedExprKind::LocalNarrowRef { binding, path, .. } => {
            emit_local_narrow_ref(emitter, ctx, binding, path, &expr.ty);
        }
        TypedExprKind::GlobalRef { mangled, .. } => {
            // Read a top-level `let` / `const` binding. The typed AST
            // split means we no longer dispatch on
            // ValueKind here — `FunctionRef` handles the
            // function-as-value path separately.
            //
            // Static-interface receiver bindings (`console`, `Map`,
            // `Temporal.Instant`) are inert — every call site drops the
            // receiver — so they lower to a typed null with no global behind
            // them (see `static_interface_of` in codegen::mod).
            if let Type::InterfaceRef { mangled: iface, .. } = expr.ty.peel()
                && ctx.symbols.iface_dispatch(iface) == Some(crate::Dispatch::Static)
            {
                match ctx.symbols.value_type(&expr.ty) {
                    ValType::Ref(RefType { heap_type, .. }) => {
                        emitter.instruction(Instruction::RefNull(heap_type));
                    }
                    other => unreachable!("static-interface binding lowered to {other:?}"),
                }
                return;
            }
            let idx = ctx
                .symbols
                .global_idx(mangled)
                .expect("top-level let/const recorded during codegen");
            emitter.instruction(Instruction::GlobalGet(idx));
            if let ValType::Ref(RefType {
                nullable: false, ..
            }) = ctx.symbols.value_type(&expr.ty)
            {
                emitter.instruction(Instruction::RefAsNonNull);
            }
        }
        TypedExprKind::FunctionRef { mangled, .. } => {
            // top-level function used as a value. Wrap it in
            // a closure struct whose funcref points at a per-function
            // adapter (slot 0 = env, slot 1 = adapter funcref, slot 2
            // = env sentinel). The shared closure vtable reuses for
            // the env slot — adapter bodies ignore it, and the slot
            // just needs a non-null `(ref any)`.
            let closure_struct_idx = ctx
                .symbols
                .closure_struct_type_idx(crate::codegen::closures::classify(&expr.ty))
                .expect("closure struct type registered for every function-as-value");
            let adapter_idx = ctx
                .symbols
                .adapter_func_idx(mangled)
                .expect("adapter func recorded for every function-as-value");
            let vtable_idx = ctx
                .symbols
                .closure_vtable_global_idx()
                .expect("closure vtable global emitted whenever closures or adapters exist");
            emitter.instruction(Instruction::GlobalGet(vtable_idx));
            emitter.instruction(Instruction::RefFunc(adapter_idx));
            emitter.instruction(Instruction::GlobalGet(vtable_idx));
            emitter.instruction(Instruction::StructNew(closure_struct_idx));
        }
        TypedExprKind::Call { mangled, args, .. } => {
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
            let target = ctx.symbols.top_level_fn(mangled).unwrap_or_else(|| {
                panic!(
                    "Call references unknown top-level fn `{}`",
                    mangled.as_str()
                )
            });
            if crate::codegen::field_guards::guarded_constructor(ctx, mangled, &expr.ty) {
                let Type::ClassRef { mangled: class, .. } = expr.ty.peel() else {
                    unreachable!("constructor class");
                };
                emit_args_into_slots(emitter, ctx, args, ctx.symbols.class_ctor_abi(class));
                crate::codegen::field_guards::constructor_argument(emitter, ctx, &expr.ty);
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
                );
            }
        }
        TypedExprKind::SuperCtorCall { parent, args } => {
            // Direct call of the parent's constructor *init* fn on the current
            // `this` (self-first ABI), initializing the parent-declared fields.
            // A generic parent's declared ctor params erase to boxed slots.
            let this = emitter
                .this_local()
                .expect("super(...) only inside a constructor body");
            emitter.instruction(Instruction::LocalGet(this));
            let ctor_abi = ctx.symbols.class_ctor_abi(parent).map(<[ValType]>::to_vec);
            emit_args_into_slots(emitter, ctx, args, ctor_abi.as_deref());
            let init = ctx
                .symbols
                .class_ctor_init_func_idx(parent)
                .expect("parent ctor init fn allocated");
            emitter.instruction(Instruction::Call(init));
            // Own field initializers / parameter-property copies run right after
            // the parent is initialized, before the rest of the constructor body.
            if let Some(mangled) = emitter.ctor_class().cloned() {
                emit_class_field_setup(emitter, ctx, &mangled);
            }
        }
        TypedExprKind::SuperMethodCall { owner, name, args } => {
            // Direct call of the parent body that declares the method (skips
            // vtable dispatch, which would re-resolve to the override). The
            // owner's physical sig is the slot sig — same erasure handling as
            // the vtable path.
            let this = emitter
                .this_local()
                .expect("super.method() only inside a method body");
            emitter.instruction(Instruction::LocalGet(this));
            let abi = ctx.symbols.class_method_abi(owner, &name.name).cloned();
            emit_args_into_slots(
                emitter,
                ctx,
                args,
                abi.as_ref().map(|a| a.params.as_slice()),
            );
            let func = ctx
                .symbols
                .class_method_func_idx(owner, &name.name)
                .expect("parent method body fn allocated");
            emitter.instruction(Instruction::Call(func));
            emit_slot_return_cast(emitter, ctx, &expr.ty.clone(), abi.as_ref());
        }
        TypedExprKind::McpCall { server, tool, args } => {
            // No per-tool import: serialize the args to JSON, dispatch the single
            // `submilli:mcp.call` host fn, and parse the JSON result as `unknown`.
            // Known-return tools are wrapped in a normal `Cast` by typecheck.
            super::mcp::emit_mcp_call(emitter, ctx, server, tool, args);
        }
        TypedExprKind::CallClosure { callee, args } => {
            // Indirect dispatch via `call_ref` through a closure
            // struct: `LocalRef` to a function-typed binding, an
            // inline `Closure` expression, a function-typed field,
            // etc. The typechecker decided this isn't a static
            // top-level call.
            emit_indirect_closure_call(emitter, ctx, *callee, args);
        }
        TypedExprKind::GenericCall {
            mangled,
            type_args,
            args,
            return_cast,
            type_predicate: _,
        } => {
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
                None => panic!(
                    "GenericCall references unknown top-level fn `{}`",
                    mangled.as_str(),
                ),
            };
            for (i, arg) in args.iter().enumerate() {
                emit_expr(emitter, ctx, arg.expr);
                if arg.is_generic {
                    let arg_ty = ctx.ta.expr(arg.expr).ty.clone();
                    crate::codegen::function_emitter::cast::emit_box(emitter, ctx, &arg_ty);
                } else if let Some(param_ty) = target_params.get(i) {
                    // A non-generic arg can still land in a wider erased slot
                    // (`count: number | null`, a `T | null` param): coerce a
                    // primitive into the ref-typed param exactly like the
                    // plain-call path. No-op when the Wasm types line up.
                    let arg_ty = ctx.ta.expr(arg.expr).ty.clone();
                    cast::emit_coerce_to_slot(emitter, ctx, &arg_ty, param_ty);
                }
            }
            if crate::codegen::field_guards::guarded_constructor(ctx, mangled, &expr.ty) {
                crate::codegen::field_guards::constructor_argument(emitter, ctx, &expr.ty);
            }
            if ctx.symbols.runtime_generic_functions.contains(mangled) {
                crate::codegen::runtime_descriptors::environment(emitter, ctx, type_args);
            }
            emitter.instruction(Instruction::Call(wasm_idx));
            if let Some(ty) = return_cast {
                if ty.is_void() {
                    emitter.instruction(Instruction::Drop);
                } else {
                    cast::emit_cast_to(emitter, ctx, ty);
                }
            } else if !expr.ty.is_void() {
                cast::emit_coerce_to_slot(emitter, ctx, &target_return, &expr.ty);
            }
        }
        TypedExprKind::IntrinsicCall { kind, args } => {
            emit_intrinsic_call(emitter, ctx, *kind, args);
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
            );
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
            );
        }
        TypedExprKind::ObjectLiteral {
            spread_sources,
            fields,
        } => {
            // Push the object header, then the boxed user-field values in
            // canonical order. `array.new_fixed` leaves the payload array as
            // `$ObjectShape` slot 2.
            //
            // when the literal is flowing into an
            // `InterfaceRef` slot, `expr.ty` is the interface, not
            // the structural shape. Derive the structural Object
            // form from each output field origin — for `Literal`
            // origins use the value expression's static type, for
            // `Spread` origins read the source's declared field
            // type out of the recorded `source_ty`.
            //
            // each output field's origin (`Literal` or
            // `Spread`) carries everything codegen needs to emit one
            // slot. Spread sources are materialized once into a
            // local of the source's concrete arity-N struct type and
            // every spread-origin field reads from it via direct
            // `struct.get` — no vtable dispatch.
            let structural_ty: Type = match &expr.ty {
                Type::Object { .. } => expr.ty.clone(),
                Type::InterfaceRef { .. } => {
                    let field_map: std::collections::BTreeMap<String, crate::ObjectField> = fields
                        .iter()
                        .map(|f| {
                            (
                                f.name.name.clone(),
                                crate::ObjectField {
                                    ty: f.ty.clone(),
                                    optional: f.optional,
                                    readonly: false,
                                },
                            )
                        })
                        .collect();
                    Type::Object { fields: field_map }
                }
                other => {
                    panic!("ObjectLiteral expr.ty must be Object or InterfaceRef, got {other:?}",)
                }
            };
            let vtable_idx = ctx
                .symbols
                .vtable_global_idx(&structural_ty)
                .expect("vtable global recorded during object-emission pass");
            let object_shape_idx = ctx
                .symbols
                .object_subtype_idx(&structural_ty)
                .expect("object shape type recorded during object-emission pass");
            let Type::Object {
                fields: declared_fields,
            } = &structural_ty
            else {
                unreachable!("structural_ty is Type::Object by construction above");
            };
            let shape_key: Vec<String> = declared_fields.keys().cloned().collect();
            let field_names_idx = ctx
                .symbols
                .field_names_global_idx(&shape_key)
                .expect("field-names global recorded during field-names emission pass");
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared by codegen entry");

            // materialize spread sources before the
            // header-globals push. Each source is ref-cast to the
            // arity-N concrete struct type so subsequent
            // `struct.get` instructions know the slot layout.
            // Locals stay alive for the rest of the literal's
            // emission — no scoping needed since the literal's
            // code emits sequentially with no nested binding.
            let mut spread_locals: Vec<(u32, u32, Vec<String>)> =
                Vec::with_capacity(spread_sources.len());
            for &source_id in spread_sources {
                let source_ty = ctx.ta.expr(source_id).ty.clone();
                let source_struct_ty = source_ty.peel().clone();
                let source_object_shape_idx = ctx
                    .symbols
                    .object_subtype_idx(&source_struct_ty)
                    .expect("object shape type recorded for spread source's structural type");
                let source_fields: Vec<String> = match &source_struct_ty {
                    Type::Object { fields: src_fields } => src_fields.keys().cloned().collect(),
                    other => panic!("spread source must peel to Type::Object, got {other:?}",),
                };
                let source_val = ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(source_object_shape_idx),
                });
                let local = emitter.add_anonymous_local(source_val);
                emit_expr(emitter, ctx, source_id);
                emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
                    source_object_shape_idx,
                )));
                emitter.instruction(Instruction::LocalSet(local));
                spread_locals.push((local, source_object_shape_idx, source_fields));
            }

            emitter.instruction(Instruction::GlobalGet(vtable_idx));
            emitter.instruction(Instruction::GlobalGet(field_names_idx));

            // Walk the declared shape's field set in BTreeMap order
            // and emit one value per slot. The typed-AST `fields`
            // list is already in the same BTreeMap order — pair them
            // by name to stay robust against ordering drift.
            let by_name: std::collections::BTreeMap<&str, &crate::TypedObjectFieldOrigin> =
                fields.iter().map(|f| (f.name.name.as_str(), f)).collect();
            for (name, field) in declared_fields {
                if let Some(origin) = by_name.get(name.as_str()) {
                    match &origin.source {
                        crate::TypedObjectFieldSource::Literal(vid) => {
                            // emit_box dispatches on the
                            // value expression's static type, not
                            // the declared field type — see the
                            // long-form comment that previously sat
                            // here for the boxed-union argument.
                            let value_ty = ctx.ta.expr(*vid).ty.clone();
                            emit_expr(emitter, ctx, *vid);
                            crate::codegen::function_emitter::cast::emit_box(
                                emitter, ctx, &value_ty,
                            );
                        }
                        crate::TypedObjectFieldSource::Spread {
                            source_index,
                            field_name,
                            ..
                        } => {
                            let (local, source_object_shape_idx, source_keys) =
                                &spread_locals[*source_index];
                            let slot = source_keys
                                .iter()
                                .position(|k| k == field_name)
                                .expect("spread origin's field_name is in source's field set");
                            emitter.instruction(Instruction::LocalGet(*local));
                            emitter.instruction(Instruction::StructGet {
                                struct_type_index: *source_object_shape_idx,
                                field_index: 2,
                            });
                            emitter.instruction(Instruction::I32Const(slot as i32));
                            emitter.instruction(Instruction::ArrayGet(intrinsics.object_fields));
                        }
                    }
                } else {
                    debug_assert!(
                        field.optional,
                        "ObjectLiteral missing field `{name}` in declared shape {:?}; \
                         typechecker should reject missing non-optional fields",
                        expr.ty,
                    );
                    emitter
                        .instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
                }
            }
            emitter.instruction(Instruction::ArrayNewFixed {
                array_type_index: intrinsics.object_fields,
                array_size: declared_fields.len() as u32,
            });
            emitter.instruction(Instruction::StructNew(object_shape_idx));
        }
        TypedExprKind::FieldAccess { receiver, name } => {
            let receiver_ty = ctx.ta.expr(*receiver).ty.clone();
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
                        &ctx.ta.expr(id).ty,
                    );
                    return;
                }
                let struct_idx = ctx
                    .symbols
                    .class_struct_type_idx(mangled)
                    .expect("class struct type recorded in classes::emit");
                let slot = ctx
                    .symbols
                    .class_field_slot(mangled, &name.name)
                    .expect("class field slot recorded in classes::emit");
                let intrinsics = ctx
                    .symbols
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry");
                emit_expr(emitter, ctx, *receiver);
                let object = emitter.add_anonymous_local(ctx.symbols.value_type(&receiver_ty));
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
                    crate::codegen::field_guards::check(emitter, ctx, object, mangled, &name.name);
                }
                emit_class_field_slot_cast(emitter, ctx, mangled, &name.name, &ctx.ta.expr(id).ty);
                return;
            }
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared by codegen entry");
            // The field's static type — single object: lookup; union
            // of objects: union of each member's field type. Both
            // shapes are guaranteed valid by `infer_field_access`.
            //
            // the `for-of` desugar reads optional properties
            // on an InterfaceRef-typed receiver (`__it.close` where
            // `__it: Iterator<T>`). The interface's properties aren't
            // reachable from the codegen ctx, so use the typed-expr's
            // own `ty` field — the desugar stamps it with the
            // already-widened `(() => void) | null` type that
            // `field_type_for_access` would have produced via
            // `interface_structural_form`.
            let field_ty = if let Type::InterfaceRef { .. } = receiver_ty.peel() {
                ctx.ta.expr(id).ty.clone()
            } else {
                // a recursive-alias field carries a back-edge
                // (`AliasRef`) in the shape body, which lowers to the
                // universal `$Object` and mismatches the slot's
                // rehydrated `$ObjectShape`. The node's own type was
                // rehydrated at infer time — it's the authoritative
                // lowering, so prefer it here.
                match field_type_for_access(&receiver_ty, &name.name) {
                    Some(computed) if !type_mentions_alias_ref(&computed) => computed,
                    _ => ctx.ta.expr(id).ty.clone(),
                }
            };
            // Stashed so we can re-push it (as the call's self argument) and
            // then read its slot 2 to get the getter funcref.
            emit_expr(emitter, ctx, *receiver);
            let rcv_local =
                stash_receiver_as_object_shape(emitter, &receiver_ty, intrinsics.object_shape);
            emit_object_property_read(emitter, ctx, rcv_local, &name.name, &field_ty);
        }
        TypedExprKind::InterfacePropertyAccess {
            receiver,
            iface,
            name,
        } => {
            // property dispatch. The typechecker resolved this
            // to an interface property at infer time and stamped the
            // interface's mangled name on the node, so codegen looks
            // up the getter under `<iface>#<name>` directly — no
            // receiver-type inspection needed.
            let key = crate::mangle::extend(iface, &name.name);
            // Static-interface properties (`Number.EPSILON`) import as
            // constant globals — no receiver, no getter call.
            if ctx.symbols.iface_dispatch(iface) == Some(crate::Dispatch::Static) {
                let global_idx = ctx
                    .symbols
                    .global_idx(&key)
                    .expect("static interface property recorded as a global import");
                emitter.instruction(Instruction::GlobalGet(global_idx));
                return;
            }
            if let Some(struct_idx) = inline_length_struct_idx(ctx, iface, &name.name) {
                emit_expr(emitter, ctx, *receiver);
                emit_inline_length(emitter, struct_idx);
                return;
            }
            let func_idx = ctx
                .symbols
                .func_idx(&key)
                .expect("interface property getter recorded during the dependency-import pass");
            emit_expr(emitter, ctx, *receiver);
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
            if matches!(ctx.ta.expr(*receiver).ty.peel(), Type::InterfaceRef { .. }) {
                emitter.instruction(Instruction::RefAsNonNull);
            }
            emitter.instruction(Instruction::Call(func_idx));
        }
        TypedExprKind::ArrayLiteral {
            elements,
            element_ty: _,
        } => {
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
            let array_idx = ctx
                .symbols
                .array_type_idx()
                .expect("Type::Array requires intrinsic types declared");
            let raw_array_idx = ctx
                .symbols
                .raw_array_type_idx()
                .expect("Type::Array requires intrinsic types declared");
            let array_vtable_global = ctx
                .symbols
                .prelude_global_idx("array_vtable")
                .expect("array_vtable imported from prelude");

            let has_spread = elements
                .iter()
                .any(|e| matches!(e, crate::TypedArrayElement::Spread(_)));

            if has_spread {
                // Locals:
                //   - `total`:  i32, running sum of element count
                //   - `dst`:    (ref $rawArray), allocated storage
                //   - `offset`: i32, write cursor
                // Per-spread:
                //   - `<spread_raw>`: (ref $rawArray), the source's
                //                     raw storage extracted once
                let raw_array_val = ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(raw_array_idx),
                });
                let total_local = emitter.add_anonymous_local(ValType::I32);
                let dst_local = emitter.add_anonymous_local(raw_array_val);
                let offset_local = emitter.add_anonymous_local(ValType::I32);

                // Pass 1: extract each spread's raw storage into a
                // local, summing lengths into `total`. Fixed
                // positions contribute +1 each (folded into the
                // initial `total` constant below).
                let fixed_count: u32 = elements
                    .iter()
                    .filter(|e| matches!(e, crate::TypedArrayElement::Value(_)))
                    .count() as u32;

                // Reserve per-spread raw-storage locals up front so
                // the pass-2 walk can read them by index.
                let mut spread_raw_locals: Vec<u32> = Vec::new();
                for el in elements {
                    if let crate::TypedArrayElement::Spread(source_id) = el {
                        let raw_local = emitter.add_anonymous_local(raw_array_val);
                        emit_expr(emitter, ctx, *source_id);
                        emitter.instruction(Instruction::StructGet {
                            struct_type_index: array_idx,
                            field_index: 1,
                        });
                        emitter.instruction(Instruction::LocalSet(raw_local));
                        spread_raw_locals.push(raw_local);
                    }
                }

                // total = fixed_count + sum(spread_raw[i].len)
                emitter.instruction(Instruction::I32Const(fixed_count as i32));
                emitter.instruction(Instruction::LocalSet(total_local));
                for &raw_local in &spread_raw_locals {
                    emitter.instruction(Instruction::LocalGet(total_local));
                    emitter.instruction(Instruction::LocalGet(raw_local));
                    emitter.instruction(Instruction::ArrayLen);
                    emitter.instruction(Instruction::I32Add);
                    emitter.instruction(Instruction::LocalSet(total_local));
                }

                // dst = array.new_default $rawArray total
                emitter.instruction(Instruction::LocalGet(total_local));
                emitter.instruction(Instruction::ArrayNewDefault(raw_array_idx));
                emitter.instruction(Instruction::LocalSet(dst_local));

                // offset = 0
                emitter.instruction(Instruction::I32Const(0));
                emitter.instruction(Instruction::LocalSet(offset_local));

                // Pass 2: walk in source order. For each fixed
                // element: array.set dst[offset] <- boxed_value;
                // offset += 1. For each spread: array.copy
                // dst[offset..] <- spread_raw[0..sub_len]; offset
                // += sub_len.
                let mut spread_cursor: usize = 0;
                for el in elements {
                    match el {
                        crate::TypedArrayElement::Value(vid) => {
                            let elem_ty = ctx.ta.expr(*vid).ty.clone();
                            // dst, offset, value
                            emitter.instruction(Instruction::LocalGet(dst_local));
                            emitter.instruction(Instruction::LocalGet(offset_local));
                            emit_expr(emitter, ctx, *vid);
                            crate::codegen::function_emitter::cast::emit_box(
                                emitter, ctx, &elem_ty,
                            );
                            emitter.instruction(Instruction::ArraySet(raw_array_idx));
                            // offset += 1
                            emitter.instruction(Instruction::LocalGet(offset_local));
                            emitter.instruction(Instruction::I32Const(1));
                            emitter.instruction(Instruction::I32Add);
                            emitter.instruction(Instruction::LocalSet(offset_local));
                        }
                        crate::TypedArrayElement::Spread(_) => {
                            let raw_local = spread_raw_locals[spread_cursor];
                            spread_cursor += 1;
                            // array.copy dst dst_offset src src_offset len
                            emitter.instruction(Instruction::LocalGet(dst_local));
                            emitter.instruction(Instruction::LocalGet(offset_local));
                            emitter.instruction(Instruction::LocalGet(raw_local));
                            emitter.instruction(Instruction::I32Const(0));
                            emitter.instruction(Instruction::LocalGet(raw_local));
                            emitter.instruction(Instruction::ArrayLen);
                            emitter.instruction(Instruction::ArrayCopy {
                                array_type_index_dst: raw_array_idx,
                                array_type_index_src: raw_array_idx,
                            });
                            // offset += raw_local.length
                            emitter.instruction(Instruction::LocalGet(offset_local));
                            emitter.instruction(Instruction::LocalGet(raw_local));
                            emitter.instruction(Instruction::ArrayLen);
                            emitter.instruction(Instruction::I32Add);
                            emitter.instruction(Instruction::LocalSet(offset_local));
                        }
                    }
                }

                // Wrap into $Array: global.get vtable; local.get dst;
                // struct.new $Array.
                emitter.instruction(Instruction::GlobalGet(array_vtable_global));
                emitter.instruction(Instruction::LocalGet(dst_local));
                emitter.instruction(Instruction::StructNew(array_idx));
            } else {
                emitter.instruction(Instruction::GlobalGet(array_vtable_global));
                for el in elements {
                    let elem_id = el.expr_id();
                    emit_expr(emitter, ctx, elem_id);
                    let elem_ty = ctx.ta.expr(elem_id).ty.clone();
                    crate::codegen::function_emitter::cast::emit_box(emitter, ctx, &elem_ty);
                }
                emitter.instruction(Instruction::ArrayNewFixed {
                    array_type_index: raw_array_idx,
                    array_size: elements.len() as u32,
                });
                emitter.instruction(Instruction::StructNew(array_idx));
            }
        }
        TypedExprKind::TupleLiteral {
            elements,
            element_types,
        } => {
            // Tuples lower to `$Array` at the Wasm level (
            // follow-up). Same recipe as ArrayLiteral, but each slot
            // is boxed by its *declared* position type — the
            // typechecker tracks per-position element types so
            // `emit_box` produces the right boxed shape regardless of
            // the homogeneous-array element-type generalization.
            let array_idx = ctx
                .symbols
                .array_type_idx()
                .expect("Type::Array requires intrinsic types declared");
            let raw_array_idx = ctx
                .symbols
                .raw_array_type_idx()
                .expect("Type::Array requires intrinsic types declared");
            let array_vtable_global = ctx
                .symbols
                .prelude_global_idx("array_vtable")
                .expect("array_vtable imported from prelude");
            emitter.instruction(Instruction::GlobalGet(array_vtable_global));
            for (&elem_id, elem_ty) in elements.iter().zip(element_types.iter()) {
                emit_expr(emitter, ctx, elem_id);
                crate::codegen::function_emitter::cast::emit_box(emitter, ctx, elem_ty);
            }
            emitter.instruction(Instruction::ArrayNewFixed {
                array_type_index: raw_array_idx,
                array_size: elements.len() as u32,
            });
            emitter.instruction(Instruction::StructNew(array_idx));
        }
        TypedExprKind::IndexAccess { receiver, index } => {
            // Uint8Array uses a different storage shape than
            // Array (packed i8 vs boxed anyref slots), so the index
            // recipe forks on the static receiver type. Tuples lower
            // to `$Array` and fall through to the default arm.
            let recv_ty = ctx.ta.expr(*receiver).ty.clone();
            if recv_ty.peel() == &Type::Uint8Array {
                // bytes[i] →
                //   <push receiver: (ref $Uint8Array)>
                //   struct.get $Uint8Array 1            ;; (ref $rawUint8Array)
                //   <push index: f64>; i32.trunc_sat_f64_u
                //   bounds-check                        ;; throws RangeError on a miss
                //   array.get_u $rawUint8Array          ;; i32 (unsigned 0–255)
                //   f64.convert_i32_u                   ;; number
                let uint8_idx = ctx
                    .symbols
                    .uint8_array_type_idx()
                    .expect("Type::Uint8Array requires intrinsic types declared");
                let raw_uint8_idx = ctx
                    .symbols
                    .raw_uint8_array_type_idx()
                    .expect("Type::Uint8Array requires intrinsic types declared");
                let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(raw_uint8_idx),
                }));
                emit_expr(emitter, ctx, *receiver);
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: uint8_idx,
                    field_index: 1,
                });
                emitter.instruction(Instruction::LocalSet(raw_local));
                emit_expr(emitter, ctx, *index);
                let idx_f64_local = stash_index_operand(emitter);
                let idx_local = emit_checked_index(emitter, ctx, raw_local, idx_f64_local);
                emitter.instruction(Instruction::LocalGet(raw_local));
                emitter.instruction(Instruction::LocalGet(idx_local));
                emitter.instruction(Instruction::ArrayGetU(raw_uint8_idx));
                emitter.instruction(Instruction::F64ConvertI32U);
            } else {
                emit_expr(emitter, ctx, *receiver);
                emit_bounds_checked_index_with_receiver_on_stack(emitter, ctx, *index, &expr.ty);
            }
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
            let slot = emitter
                .this_local()
                .expect("`this` is only emitted inside a class method/constructor body");
            emitter.instruction(Instruction::LocalGet(slot));
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
                .expect("boxed_number type registered");
            let vtable_global = ctx
                .symbols
                .prelude_global_idx("boxed_number_vtable")
                .expect("boxed_number_vtable imported from prelude");
            emitter.instruction(Instruction::GlobalGet(vtable_global));
            emitter.instruction(Instruction::F64Const(Ieee64::from(*value)));
            emitter.instruction(Instruction::StructNew(boxed_idx));
        }
        TypedExprKind::StringEnumMember { .. } => {
            // String enum values share the `$string` representation.
            // The StringPool collector records each `StringEnumMember`
            // by `ExprId` (see string_pool.rs), so the materialisation
            // path is identical to a regular string literal.
            let pool_idx = ctx
                .strings
                .locations
                .get(&id)
                .copied()
                .expect("CodegenAnalysis recorded the variant value");
            let code_units = ctx.strings.code_units(pool_idx);
            let string_type_idx = ctx
                .symbols
                .string_type_idx()
                .expect("Type::String requires the intrinsic types to be declared");
            let raw_string_type_idx = ctx
                .symbols
                .raw_string_type_idx()
                .expect("Type::String requires the intrinsic types to be declared");
            let vtable_global_idx = ctx
                .symbols
                .prelude_global_idx("string_vtable")
                .expect("string_vtable global imported from prelude");
            emitter.emit_const_string(
                string_type_idx,
                raw_string_type_idx,
                vtable_global_idx,
                pool_idx as u32,
                code_units,
            );
        }
        TypedExprKind::TypeofTag { value, tag } => {
            emit_typeof_tag(emitter, ctx, *value, *tag);
        }
        TypedExprKind::InstanceOf { value, class } => match class.peel() {
            Type::ClassRef { mangled, .. } => {
                let vtable_global = ctx
                    .symbols
                    .class_vtable_global_idx(mangled)
                    .expect("class vtable global recorded");
                emit_expr(emitter, ctx, *value);
                cast::emit_nominal_instance_test(emitter, ctx, vtable_global);
            }
            // Not a class, so there is no vtable to walk. `$Uint8Array` is
            // canonically unique, which makes the structural test the same
            // answer the nominal walk would give.
            Type::Uint8Array => {
                let idx = ctx
                    .symbols
                    .uint8_array_type_idx()
                    .expect("$Uint8Array intrinsic registered");
                emit_expr(emitter, ctx, *value);
                emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(idx)));
            }
            _ => unreachable!("instanceof codegen with a non-class RHS: {class:?}"),
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
                let shadow_val = ctx.symbols.value_type(&cast_info.to_ty);
                let shadow = emitter.define_local(binding, shadow_val);
                emit_expr(emitter, ctx, *source);
                crate::codegen::function_emitter::cast::emit_narrowing_cast(
                    emitter, ctx, cast_info,
                );
                emitter.instruction(Instruction::LocalSet(shadow));
            } else {
                emitter.register_narrow_source(&binding.name, *source);
            }
            emit_expr(emitter, ctx, *inner);
            emitter.pop_scope();
        }
        TypedExprKind::Closure {
            captured,
            runtime_generics,
            ..
        } => {
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
            // closure expression's `expr.ty` (a `Type::Function`);
            // per-arrow `$env_N` and the body's func index are keyed
            // on `id` (the closure's `ExprId`). The closure vtable
            // global is shared across every closure.
            let closure_struct_idx = ctx
                .symbols
                .closure_struct_type_idx(crate::codegen::closures::classify(&expr.ty))
                .expect("closure struct type registered for every closure-typed expression");
            let env_type_idx = ctx
                .symbols
                .env_type_idx(id)
                .expect("env type registered for every closure expression");
            let closure_func_idx = ctx
                .symbols
                .closure_func_idx(id)
                .expect("closure body function index allocated");
            let closure_vtable_idx = ctx
                .symbols
                .closure_vtable_global_idx()
                .expect("closure vtable global emitted whenever closures exist");

            // Field 0: shared closure vtable (inherited from $Object).
            emitter.instruction(Instruction::GlobalGet(closure_vtable_idx));

            // Field 1: typed funcref to the closure body.
            emitter.instruction(Instruction::RefFunc(closure_func_idx));

            // Field 2: env. Build it inline by pushing each captured
            // value in order, then `struct.new`.
            for c in captured {
                emit_captured_load(emitter, ctx, c);
            }
            if !runtime_generics.is_empty() {
                let types: Vec<_> = runtime_generics
                    .iter()
                    .cloned()
                    .map(Type::TypeVar)
                    .collect();
                crate::codegen::runtime_descriptors::environment(emitter, ctx, &types);
            }
            emitter.instruction(Instruction::StructNew(env_type_idx));

            // Stack: vtable, funcref, env — `(ref $env_N)` subtypes
            // `(ref any)` so the env flows into field 2 implicitly.
            // Closure struct allocation consumes all three.
            emitter.instruction(Instruction::StructNew(closure_struct_idx));
        }
        // `cond ? then_: else_`. Wasm `if`-with-result lifts
        // the branch values onto the parent stack. Each branch's value
        // is coerced to the chain's result Wasm slot so both arms
        // agree on the stack type.
        TypedExprKind::Ternary { cond, then_, else_ } => {
            let result_ty = expr.ty.clone();
            let result_val = ctx.symbols.value_type(&result_ty);
            let cond_ty = ctx.ta.expr(*cond).ty.clone();
            emit_expr(emitter, ctx, *cond);
            crate::codegen::function_emitter::cast::emit_condition_to_i32(emitter, ctx, &cond_ty);
            emitter.emit_if(BlockType::Result(result_val));
            let then_ty = ctx.ta.expr(*then_).ty.clone();
            emit_expr(emitter, ctx, *then_);
            crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                emitter, ctx, &then_ty, &result_ty,
            );
            emitter.emit_else();
            let else_ty = ctx.ta.expr(*else_).ty.clone();
            emit_expr(emitter, ctx, *else_);
            crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                emitter, ctx, &else_ty, &result_ty,
            );
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
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            let result_ty = expr.ty.clone();
            let result_val = ctx.symbols.value_type(&result_ty);
            let lhs_ty = ctx.ta.expr(*lhs).ty.clone();
            let lhs_val = ctx.symbols.value_type(&lhs_ty);
            let lhs_is_ref = matches!(lhs_val, ValType::Ref(_));
            if lhs_is_ref {
                let tmp = emitter.add_anonymous_local(lhs_val);
                emit_expr(emitter, ctx, *lhs);
                emitter.instruction(Instruction::LocalTee(tmp));
                emitter.instruction(Instruction::RefIsNull);
                emitter.emit_if(BlockType::Result(result_val));
                let rhs_ty = ctx.ta.expr(*rhs).ty.clone();
                emit_expr(emitter, ctx, *rhs);
                crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                    emitter, ctx, &rhs_ty, &result_ty,
                );
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
                crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, &result_ty);
                emitter.emit_end();
            } else {
                // Non-nullable, primitive-typed lhs — just emit it.
                emit_expr(emitter, ctx, *lhs);
                crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                    emitter, ctx, &lhs_ty, &result_ty,
                );
            }
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
            let base_ty = ctx.ta.expr(*base).ty.clone();
            emit_expr(emitter, ctx, *base);
            emit_chain_parts(emitter, ctx, parts, 0, &base_ty, &result_ty);
        }
        TypedExprKind::PostfixUnary { op, target } => {
            emit_postfix_unary(emitter, ctx, *op, target, &expr.ty);
        }
        TypedExprKind::NonNullAssert { value } => {
            crate::codegen::cast_check::emit_non_null_assert(emitter, ctx, *value, &expr.ty);
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
            );
        }
    }
}

/// Read a narrowing-ref binding, bridging its slot to the narrowed type.
///
/// A non-empty field or index path always re-evaluates the region's saved
/// source and applies a checked cast. The type fact may survive a call, but the
/// referenced slot may have changed; reading the live value prevents a stale
/// shadow from escaping and turns an incompatible mutation into `TypeError`.
///
/// Root-binding resolution covers three remaining shapes:
///
/// - **Predicate-narrowing inside a branch body**: the synthetic
///   `#narrow_<N>` shadow materialized by `NarrowRegion` / `Narrowed`.
///   Its slot type matches the expression's `ty`, so this is a bare `local.get`.
/// - **Assignment-narrowing**: the `AssignLocal` codegen allocates a fresh
///   narrowed-type shadow + rebinds the original ident name. Slot type matches
///   `ty` → bare `local.get`.
/// - **Post-`if` join-installed narrowing**: the inferer rebinds the joined
///   narrowings to the original ident; no shadow allocated. Slot Wasm-type is
///   the binding's declared type (wider than the narrowed `ty`), so a
///   `ref.cast` / `struct.get` has to bridge.
///
/// Comparing what the slot actually leaves on the stack against the narrowed
/// value-type uniformly dispatches between the three cases — no
/// flag-on-the-AST needed.
fn emit_local_narrow_ref(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    binding: &Ident,
    path: &ReferencePath,
    narrowed_ty: &Type,
) {
    if !path.chain.is_empty()
        && let Some(source) = emitter.narrow_source(&binding.name)
    {
        // Calls preserve this typecheck-time fact but may mutate the referenced
        // slot, so re-read and validate every use instead of caching a snapshot.
        emit_expr(emitter, ctx, source);
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &ctx.ta.expr(source).ty,
            narrowed_ty,
        );
        return;
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
            if stack_ty != ctx.symbols.value_type(narrowed_ty) {
                cast::emit_cast_to(emitter, ctx, narrowed_ty);
            }
            return;
        }
        if let BindingId::Global(mangled) = &path.root {
            let idx = ctx
                .symbols
                .global_idx(mangled)
                .expect("Inferer guarantees the binding exists");
            emitter.instruction(Instruction::GlobalGet(idx));
            cast::emit_cast_to(emitter, ctx, narrowed_ty);
            return;
        }
        if let Some(source) = emitter.narrow_source(&binding.name) {
            emit_expr(emitter, ctx, source);
            let source_ty = &ctx.ta.expr(source).ty;
            let cast_info = cast_info_for(source_ty.clone(), narrowed_ty.clone());
            cast::emit_narrowing_cast(emitter, ctx, &cast_info);
            return;
        }
        panic!(
            "narrow binding `{}` (path {path:?}) not registered in codegen scope — \
             the region's source names a shadow from a scope that already closed",
            binding.name,
        )
    };
    emitter.instruction(Instruction::LocalGet(slot));
    let stack_ty = unbox_if_boxed(emitter, ctx, slot_ty);
    if stack_ty != ctx.symbols.value_type(narrowed_ty) {
        cast::emit_cast_to(emitter, ctx, narrowed_ty);
    }
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

/// Read the original value, write its increment/decrement, and return the
/// original. Binding operands are read at the narrowed result type but written
/// back through their declared slot, including nullable and captured bindings.
fn emit_postfix_unary(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    op: crate::PostfixOp,
    target: &crate::PostfixTarget,
    result_ty: &Type,
) {
    let delta_op = match op {
        crate::PostfixOp::Inc => Instruction::F64Add,
        crate::PostfixOp::Dec => Instruction::F64Sub,
        crate::PostfixOp::NonNullAssert => unreachable!("non-null assertion is not PostfixUnary"),
    };
    match target {
        crate::PostfixTarget::Local {
            ident,
            boxed,
            target_ty,
        } => {
            // A read-modify-write, so the write decides the slot.
            let slot = emitter
                .write_slot(&ident.name)
                .expect("Inferer guarantees the binding exists");
            let old = emitter.add_anonymous_local(ctx.symbols.value_type(result_ty));
            let box_idx = boxed.then(|| {
                ctx.symbols
                    .box_type_idx(target_ty)
                    .expect("box type registered for postfix operand")
            });
            emitter.instruction(Instruction::LocalGet(slot));
            if let Some(box_idx) = box_idx {
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
            }
            let cast_info = cast_info_for(target_ty.clone(), result_ty.clone());
            cast::emit_narrowing_cast(emitter, ctx, &cast_info);
            emitter.instruction(Instruction::LocalSet(old));
            if box_idx.is_some() {
                emitter.instruction(Instruction::LocalGet(slot));
            }
            emitter.instruction(Instruction::LocalGet(old));
            emit_postfix_delta(emitter, ctx, op, result_ty);
            cast::emit_coerce_to_slot(emitter, ctx, result_ty, target_ty);
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
            let idx = ctx
                .symbols
                .global_idx(mangled)
                .expect("Inferer guarantees the binding exists");
            let old = emitter.add_anonymous_local(ctx.symbols.value_type(result_ty));
            emitter.instruction(Instruction::GlobalGet(idx));
            // Reference globals start as null before module initialization.
            if let ValType::Ref(RefType {
                nullable: false, ..
            }) = ctx.symbols.value_type(target_ty)
            {
                emitter.instruction(Instruction::RefAsNonNull);
            }
            let cast_info = cast_info_for(target_ty.clone(), result_ty.clone());
            cast::emit_narrowing_cast(emitter, ctx, &cast_info);
            emitter.instruction(Instruction::LocalSet(old));
            emitter.instruction(Instruction::LocalGet(old));
            emit_postfix_delta(emitter, ctx, op, result_ty);
            cast::emit_coerce_to_slot(emitter, ctx, result_ty, target_ty);
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
            let receiver_ty = ctx.ta.expr(*receiver).ty.clone();
            if let Type::ClassRef { mangled, .. } = receiver_ty.peel() {
                emit_class_field_postfix(
                    emitter, ctx, *receiver, mangled, &name.name, op, delta_op, target_ty,
                );
                return;
            }
            // Mirror AssignField + FieldAccess's vtable-dispatched calls.
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared by codegen entry");
            let name_global = ctx
                .symbols
                .field_name_string_global_idx(&name.name)
                .expect("per-name string global recorded during field-name-strings emission");
            let is_bigint = matches!(target_ty.peel(), Type::BigInt);
            let tmp = emitter.add_anonymous_local(ctx.symbols.value_type(target_ty));
            emit_expr(emitter, ctx, *receiver);
            let rcv_local =
                stash_receiver_as_object_shape(emitter, &receiver_ty, intrinsics.object_shape);
            emit_object_field_read_by_name(emitter, ctx, rcv_local, name_global);
            crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, target_ty);
            emitter.instruction(Instruction::LocalTee(tmp));
            emitter.instruction(Instruction::LocalGet(tmp));
            if is_bigint {
                emit_bigint_pm_one(emitter, ctx, op);
            } else {
                emitter.instruction(Instruction::F64Const(Ieee64::from(1.0)));
                emitter.instruction(delta_op);
            }
            crate::codegen::function_emitter::cast::emit_box(emitter, ctx, target_ty);
            emit_object_field_write_by_name(emitter, ctx, rcv_local, name_global);
        }
        crate::PostfixTarget::Index {
            receiver,
            index,
            elem_ty,
        } => {
            let raw_array_idx = ctx
                .symbols
                .raw_array_type_idx()
                .expect("Type::Array requires intrinsic types declared");
            let array_idx = ctx
                .symbols
                .array_type_idx()
                .expect("Type::Array requires intrinsic types declared");
            let is_bigint = matches!(elem_ty.peel(), Type::BigInt);
            let raw_arr_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(raw_array_idx),
            }));
            let tmp = emitter.add_anonymous_local(ctx.symbols.value_type(elem_ty));
            // Stash raw array and index.
            emit_expr(emitter, ctx, *receiver);
            emitter.instruction(Instruction::StructGet {
                struct_type_index: array_idx,
                field_index: 1,
            });
            emitter.instruction(Instruction::LocalSet(raw_arr_local));
            emit_expr(emitter, ctx, *index);
            let idx_f64_local = stash_index_operand(emitter);
            let idx_local = emit_checked_index(emitter, ctx, raw_arr_local, idx_f64_local);
            // Read original.
            emitter.instruction(Instruction::LocalGet(raw_arr_local));
            emitter.instruction(Instruction::LocalGet(idx_local));
            emitter.instruction(Instruction::ArrayGet(raw_array_idx));
            crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, elem_ty);
            //   stack: [orig]
            emitter.instruction(Instruction::LocalTee(tmp));
            // Write new value via array.set.
            emitter.instruction(Instruction::LocalGet(raw_arr_local));
            emitter.instruction(Instruction::LocalGet(idx_local));
            emitter.instruction(Instruction::LocalGet(tmp));
            if is_bigint {
                emit_bigint_pm_one(emitter, ctx, op);
            } else {
                emitter.instruction(Instruction::F64Const(Ieee64::from(1.0)));
                emitter.instruction(delta_op);
            }
            crate::codegen::function_emitter::cast::emit_box(emitter, ctx, elem_ty);
            emitter.instruction(Instruction::ArraySet(raw_array_idx));
            //   stack: [orig]
        }
    }
}

fn emit_postfix_delta(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    op: crate::PostfixOp,
    ty: &Type,
) {
    if matches!(ty.peel(), Type::BigInt) {
        emit_bigint_pm_one(emitter, ctx, op);
        return;
    }
    emitter.instruction(Instruction::F64Const(Ieee64::from(1.0)));
    emitter.instruction(match op {
        crate::PostfixOp::Inc => Instruction::F64Add,
        crate::PostfixOp::Dec => Instruction::F64Sub,
        crate::PostfixOp::NonNullAssert => unreachable!("non-null assertion is not PostfixUnary"),
    });
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
    delta_op: Instruction<'static>,
    target_ty: &Type,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let struct_idx = ctx
        .symbols
        .class_struct_type_idx(mangled)
        .expect("class struct type recorded in classes::emit");
    // A property with no slot is an accessor, which has no payload to
    // read-modify-write; `class_postfix_target` rejects those before codegen.
    let slot = ctx
        .symbols
        .class_field_slot(mangled, field)
        .expect("data-field slot recorded in classes::emit — accessors are rejected in infer");
    let fields_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object_fields),
    }));
    let old = emitter.add_anonymous_local(ctx.symbols.value_type(target_ty));
    let object = emitter.add_anonymous_local(ctx.symbols.value_type(&ctx.ta.expr(receiver).ty));
    emit_expr(emitter, ctx, receiver);
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
        crate::codegen::field_guards::check(emitter, ctx, object, mangled, field);
    }
    emit_class_field_slot_cast(emitter, ctx, mangled, field, target_ty);
    emitter.instruction(Instruction::LocalSet(old));
    emitter.instruction(Instruction::LocalGet(fields_local));
    emitter.instruction(Instruction::I32Const(slot as i32));
    emitter.instruction(Instruction::LocalGet(old));
    if matches!(target_ty.peel(), Type::BigInt) {
        emit_bigint_pm_one(emitter, ctx, op);
    } else {
        emitter.instruction(Instruction::F64Const(Ieee64::from(1.0)));
        emitter.instruction(delta_op);
    }
    crate::codegen::function_emitter::cast::emit_box(emitter, ctx, target_ty);
    emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
    emitter.instruction(Instruction::LocalGet(old));
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
) {
    if idx == parts.len() {
        // A void-tailed chain left nothing on the stack — no value to
        // coerce, and `value_type(void)` has no lowering.
        if result_ty.is_void() {
            return;
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
        );
        return;
    }
    let part = &parts[idx];
    let part_result_ty = part.result_ty().clone();
    // Only an optional step needs the receiver's slot, so don't ask for a
    // lowering a straight-line step never uses.
    let recv_val = part
        .is_optional()
        .then(|| ctx.symbols.value_type(receiver_ty));
    // A receiver in a primitive slot cannot hold null, and `ref.is_null` does
    // not accept an f64/i32, so a `?.` on a non-nullable `number`/`boolean`
    // lowers as the straight-line access. The redundant-`?.` warning does not
    // gate this: it only fires on the chain's base, so the same shape mid-chain
    // arrives here undiagnosed.
    if let Some(recv_val @ ValType::Ref(_)) = recv_val {
        // A void-tailed chain yields no value — empty block, and a no-op
        // null branch (and `value_type(void)` has no lowering).
        let is_void = result_ty.is_void();
        let block_ty = if is_void {
            BlockType::Empty
        } else {
            BlockType::Result(ctx.symbols.value_type(result_ty))
        };
        let tmp = emitter.add_anonymous_local(recv_val);
        emitter.instruction(Instruction::LocalTee(tmp));
        emitter.instruction(Instruction::RefIsNull);
        emitter.emit_if(block_ty);
        // Null branch: produce a `null` cast to the chain's result
        // type. `emit_null_for_result_ty` handles the per-Wasm-shape
        // null literal — for ref-typed results that's `ref.null T`;
        // for an `unknown`-typed result it's `ref.null $Object`.
        if !is_void {
            emit_null_for_chain_result(emitter, ctx, result_ty);
        }
        emitter.emit_else();
        emitter.instruction(Instruction::LocalGet(tmp));
        emitter.instruction(Instruction::RefAsNonNull);
        // Normalise the stack value to the non-null type's Wasm shape
        // *before* dispatching to the per-part access. After
        // `ref.as_non_null`, we're holding whatever heap type the
        // union lowered to (often `(ref $Object)` for mixed unions);
        // the field/index access codegen expects the access-specific
        // subtype (`(ref $ObjectShape)`, `(ref $Array)`), or — for a
        // primitive-slotted receiver like `number | null`, which is boxed
        // inside the union's ref slot — the raw f64/i32 the access takes.
        let non_null_ty =
            super::super::super::typechecker::infer::narrowing::strip_null(receiver_ty);
        let non_null_val = ctx.symbols.value_type(&non_null_ty);
        if let ValType::Ref(RefType { heap_type, .. }) = non_null_val
            && !matches!(non_null_ty.peel(), Type::Function { .. })
        {
            emitter.instruction(Instruction::RefCastNonNull(heap_type));
        } else {
            cast::emit_cast_to(emitter, ctx, &non_null_ty);
        }
        emit_chain_access(emitter, ctx, part, &non_null_ty);
        emit_chain_parts(emitter, ctx, parts, idx + 1, &part_result_ty, result_ty);
        emitter.emit_end();
    } else {
        emit_chain_access(emitter, ctx, part, receiver_ty);
        emit_chain_parts(emitter, ctx, parts, idx + 1, &part_result_ty, result_ty);
    }
}

/// The inline lowering table for `intrinsic`-declared properties: members the
/// declaration marks as codegen-owned (see `PropertySig::intrinsic`) have no
/// getter import; this maps each to its receiver struct type for the payload
/// `struct.get` + `array.len` sequence. Declaring a member intrinsic without a
/// row here is a bug — the lookup panics rather than silently importing.
fn inline_length_struct_idx(
    ctx: &CodegenCtx<'_>,
    iface: &crate::MangledName,
    prop: &str,
) -> Option<u32> {
    let key = crate::mangle::extend(iface, prop);
    if !ctx.symbols.is_intrinsic_member(&key) {
        return None;
    }
    if *iface == crate::mangle::prelude("String") && prop == "length" {
        Some(
            ctx.symbols
                .string_type_idx()
                .expect("String#length requires intrinsic types declared"),
        )
    } else if *iface == crate::mangle::prelude("Array") && prop == "length" {
        Some(
            ctx.symbols
                .array_type_idx()
                .expect("Array#length requires intrinsic types declared"),
        )
    } else {
        unreachable!("no inline lowering for intrinsic member `{}`", key.as_str())
    }
}

/// Receiver (concrete `$string`/`$Array`) on stack → its element count as f64.
fn emit_inline_length(emitter: &mut FunctionEmitter<'_>, struct_idx: u32) {
    emitter.instruction(Instruction::StructGet {
        struct_type_index: struct_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::ArrayLen);
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
) {
    let val = ctx.symbols.value_type(result_ty);
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
        _ => unreachable!(
            "optional chain result_ty `{result_ty}` lowers to a primitive — \
             chain must be ref-typed"
        ),
    }
}

/// Narrows a class field's raw payload slot value, on the stack, to the type the
/// read is typed at. Ordinarily that is the plain representation cast; a field
/// carrying a [`crate::FieldNarrowingCheck`] is read through its guard instead.
fn emit_class_field_slot_cast(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    class: &crate::MangledName,
    field: &str,
    result_ty: &Type,
) {
    match ctx.symbols.class_field_narrowing_check(class, field) {
        Some(check) => {
            crate::codegen::cast_check::emit_narrowed_field_read(emitter, ctx, check, result_ty);
        }
        None => crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, result_ty),
    }
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
) -> u32 {
    coerce_field_receiver_to_object_shape(emitter, receiver_ty, object_shape);
    let rcv_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(object_shape),
    }));
    emitter.instruction(Instruction::LocalSet(rcv_local));
    rcv_local
}

fn coerce_field_receiver_to_object_shape(
    emitter: &mut FunctionEmitter<'_>,
    receiver_ty: &Type,
    object_shape: u32,
) {
    let universal = match receiver_ty.peel() {
        Type::InterfaceRef { .. } => true,
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
) {
    match part {
        crate::TypedChainPart::Field {
            name, result_ty, ..
        } => {
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared by codegen entry");
            // Class receiver: resolve the slot statically, mirroring the
            // non-optional `FieldAccess` path. The name scan below would read a
            // null for an accessor property, which backs no payload slot.
            if let Type::ClassRef { mangled, .. } = receiver_ty.peel() {
                let Some(slot) = ctx.symbols.class_field_slot(mangled, &name.name) else {
                    let getter = crate::codegen::classes::accessor_getter_name(&name.name);
                    emit_class_vtable_dispatch(emitter, ctx, mangled, &getter, &[], result_ty);
                    return;
                };
                let struct_idx = ctx
                    .symbols
                    .class_struct_type_idx(mangled)
                    .expect("class struct type recorded in classes::emit");
                let object = emitter.add_anonymous_local(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(struct_idx),
                }));
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
                    crate::codegen::field_guards::check(emitter, ctx, object, mangled, &name.name);
                }
                emit_class_field_slot_cast(emitter, ctx, mangled, &name.name, result_ty);
                return;
            }
            let rcv_local =
                stash_receiver_as_object_shape(emitter, receiver_ty, intrinsics.object_shape);
            emit_object_property_read(emitter, ctx, rcv_local, &name.name, result_ty);
        }
        crate::TypedChainPart::Index { idx, result_ty, .. } => {
            emit_bounds_checked_index_with_receiver_on_stack(emitter, ctx, *idx, result_ty);
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
            if let Some(struct_idx) = inline_length_struct_idx(ctx, iface, &name.name) {
                emit_inline_length(emitter, struct_idx);
                return;
            }
            let key = crate::mangle::extend(iface, &name.name);
            let func_idx = ctx
                .symbols
                .func_idx(&key)
                .expect("interface property getter recorded during the dependency-import pass");
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
            emit_indirect_closure_call_with_receiver_on_stack(emitter, ctx, receiver_ty, args);
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
            );
        }
        crate::TypedChainPart::NonNull { result_ty, .. } => {
            crate::codegen::cast_check::emit_non_null_assert_on_stack(
                emitter,
                ctx,
                receiver_ty,
                result_ty,
            );
        }
    }
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
fn emit_captured_load(emitter: &mut FunctionEmitter, _ctx: &CodegenCtx, c: &crate::CapturedVar) {
    let slot = emitter
        .local_slot(&c.name.name)
        .expect("captured binding resolves in the outer scope");
    emitter.instruction(Instruction::LocalGet(slot));
}

fn emit_binary(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    op: BinOp,
    lhs: ExprId,
    rhs: ExprId,
    result_ty: &Type,
) {
    match op {
        // `+` dispatches on the result type the typechecker chose: numeric
        // operands → `f64.add`, string operands → `string_concat` from the
        // prelude. Mixed-type or other operand combinations were rejected
        // by the inferer.
        BinOp::Add => match result_ty {
            Type::Number => {
                emit_expr(emitter, ctx, lhs);
                emit_expr(emitter, ctx, rhs);
                emitter.instruction(Instruction::F64Add);
            }
            Type::String => {
                emit_expr(emitter, ctx, lhs);
                emit_expr(emitter, ctx, rhs);
                let idx = ctx
                    .symbols
                    .prelude_func_idx("string_concat")
                    .expect("string_concat imported from prelude");
                emitter.instruction(Instruction::Call(idx));
            }
            // `bigint + bigint` → inline host call.
            Type::BigInt => emit_bigint_binop_inline(emitter, ctx, lhs, rhs, "add"),
            other => unreachable!("typechecker rejects `+` for {other:?}"),
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div => {
            // bigint operands route to inline host calls.
            if matches!(result_ty, Type::BigInt) {
                let name = match op {
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    BinOp::Div => "div",
                    _ => unreachable!(),
                };
                emit_bigint_binop_inline(emitter, ctx, lhs, rhs, name);
                return;
            }
            debug_assert!(matches!(result_ty, Type::Number));
            emit_expr(emitter, ctx, lhs);
            emit_expr(emitter, ctx, rhs);
            let inst = match op {
                BinOp::Sub => Instruction::F64Sub,
                BinOp::Mul => Instruction::F64Mul,
                BinOp::Div => Instruction::F64Div,
                _ => unreachable!(),
            };
            emitter.instruction(inst);
        }
        BinOp::Pow => {
            // exponentiation. BigInt routes to the
            // `submilli:bigint.pow` host call (which validates the
            // exponent is non-negative and fits in u32, else traps).
            // Number routes to the prelude-host `Math#pow` function.
            if matches!(result_ty, Type::BigInt) {
                emit_bigint_binop_inline(emitter, ctx, lhs, rhs, "pow");
                return;
            }
            debug_assert!(matches!(result_ty, Type::Number));
            emit_expr(emitter, ctx, lhs);
            emit_expr(emitter, ctx, rhs);
            let idx = ctx
                .symbols
                .func_idx(&crate::runtime::prelude::math::math_key("pow"))
                .expect("Math#pow host import recorded");
            emitter.instruction(Instruction::Call(idx));
        }
        BinOp::Rem => {
            // `bigint % bigint` → inline host call.
            if matches!(result_ty, Type::BigInt) {
                emit_bigint_binop_inline(emitter, ctx, lhs, rhs, "mod");
                return;
            }
            // Wasm has no `f64.rem`; lower to JS-truncation modulo using
            // `a - trunc(a / b) * b`. Two scratch f64 locals so each operand
            // is evaluated exactly once (preserves side-effect order).
            let a = emitter.add_anonymous_local(ValType::F64);
            let b = emitter.add_anonymous_local(ValType::F64);
            emit_expr(emitter, ctx, lhs);
            emitter.instruction(Instruction::LocalSet(a));
            emit_expr(emitter, ctx, rhs);
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
            let operand_ty = ctx.ta.expr(lhs).ty.peel().clone();
            if matches!(operand_ty, Type::BigInt) {
                emit_bigint_cmp_inline(emitter, ctx, lhs, rhs, op);
                return;
            }
            emit_expr(emitter, ctx, lhs);
            emit_expr(emitter, ctx, rhs);
            if matches!(operand_ty, Type::String | Type::StringLiteral(_)) {
                let idx = ctx
                    .symbols
                    .prelude_func_idx("string_cmp")
                    .expect("string_cmp imported from prelude");
                emitter.instruction(Instruction::Call(idx));
                emitter.instruction(Instruction::I32Const(0));
                emitter.instruction(match op {
                    BinOp::Lt => Instruction::I32LtS,
                    BinOp::Gt => Instruction::I32GtS,
                    BinOp::Le => Instruction::I32LeS,
                    BinOp::Ge => Instruction::I32GeS,
                    _ => unreachable!(),
                });
            } else {
                emitter.instruction(match op {
                    BinOp::Lt => Instruction::F64Lt,
                    BinOp::Gt => Instruction::F64Gt,
                    BinOp::Le => Instruction::F64Le,
                    BinOp::Ge => Instruction::F64Ge,
                    _ => unreachable!(),
                });
            }
        }
        BinOp::Eq | BinOp::NotEq => {
            // The inferer's Eq rule infers the RHS using the LHS's
            // type as the hint, so both operands share a Wasm
            // `value_type`. Dispatch off that value-type — the
            // language `Type` might be a literal-refined primitive
            // or a literal-only union; at the Wasm level those
            // collapse to the same compare path as their base
            // primitive (equality is value-level — the literal
            // refinement is a typecheck-time restriction, not a
            // runtime distinction).
            // Plan 75.8: when either operand is the `null` literal,
            // emit the non-null side followed by `ref.is_null`
            // (negated for `NotEq`). This covers `x === null` /
            // `x !== null` against any nullable value, the primary
            // null-narrowing predicate. Both-null collapses to a
            // constant.
            let lhs_is_null_lit = matches!(ctx.ta.expr(lhs).kind, TypedExprKind::Null);
            let rhs_is_null_lit = matches!(ctx.ta.expr(rhs).kind, TypedExprKind::Null);
            if lhs_is_null_lit && rhs_is_null_lit {
                let v: i32 = if matches!(op, BinOp::Eq) { 1 } else { 0 };
                emitter.instruction(Instruction::I32Const(v));
                return;
            }
            if lhs_is_null_lit || rhs_is_null_lit {
                let non_null_side = if lhs_is_null_lit { rhs } else { lhs };
                let non_null_ty = ctx.ta.expr(non_null_side).ty.clone();
                let non_null_val = ctx.symbols.value_type(&non_null_ty);
                // Plan 75.10 PR 2: when the non-null side has a
                // primitive Wasm type (f64 / i32 — e.g., a
                // narrowed-to-`Number` after assignment), `ref.is_null`
                // would be a validation error. The answer is
                // statically known — a primitive can never be null —
                // so emit the operand for side effects, drop, then
                // push the constant result.
                if matches!(non_null_val, ValType::F64 | ValType::I32) {
                    emit_expr(emitter, ctx, non_null_side);
                    emitter.instruction(Instruction::Drop);
                    let v: i32 = if matches!(op, BinOp::NotEq) { 1 } else { 0 };
                    emitter.instruction(Instruction::I32Const(v));
                    return;
                }
                emit_expr(emitter, ctx, non_null_side);
                emitter.instruction(Instruction::RefIsNull);
                if matches!(op, BinOp::NotEq) {
                    emitter.instruction(Instruction::I32Eqz);
                }
                return;
            }
            // When either operand admits null the two may differ in Wasm shape
            // (one `(ref null $Object)`, the other f64 / `(ref $string)` / …),
            // and the `operand_val` dispatch below assumes a shared value-type.
            // Route through a null-aware dispatch that boxes both sides to
            // `(ref [null] $Object)` and calls `vtable.equals`.
            let lhs_admits_null = may_hold_null(&ctx.ta.expr(lhs).ty);
            let rhs_admits_null = may_hold_null(&ctx.ta.expr(rhs).ty);
            if lhs_admits_null || rhs_admits_null {
                emit_nullable_eq(emitter, ctx, lhs, rhs, op);
                return;
            }
            let operand_ty = ctx.ta.expr(lhs).ty.clone();
            // bigint equality short-circuits to a direct
            // `submilli:bigint.cmp == 0` call — a faster path than the
            // generic `ValType::Ref(_)` arm's vtable `equals` dispatch.
            if matches!(operand_ty.peel(), Type::BigInt) {
                emit_bigint_cmp_eq_inline(emitter, ctx, lhs, rhs, op);
                return;
            }
            let operand_val = ctx.symbols.value_type(&operand_ty);
            let string_idx = ctx
                .symbols
                .string_type_idx()
                .expect("string type registered with intrinsics");
            match operand_val {
                ValType::F64 => {
                    emit_expr(emitter, ctx, lhs);
                    emit_expr(emitter, ctx, rhs);
                    emitter.instruction(if matches!(op, BinOp::Eq) {
                        Instruction::F64Eq
                    } else {
                        Instruction::F64Ne
                    });
                }
                ValType::I32 => {
                    emit_expr(emitter, ctx, lhs);
                    emit_expr(emitter, ctx, rhs);
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
                    emit_expr(emitter, ctx, lhs);
                    emit_expr(emitter, ctx, rhs);
                    let func = ctx
                        .symbols
                        .prelude_func_idx("string_eq")
                        .expect("string_eq imported from prelude");
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
                    //
                    // The Eq rule infers the RHS with the LHS type as
                    // hint, but a union LHS (`$Object` repr) can pair
                    // with an RHS that lands on a concrete primitive
                    // member (`first === 1`, RHS `f64`). Box both sides
                    // to `$Object` so they match the `equals` slot's
                    // signature.
                    emit_expr(emitter, ctx, lhs);
                    cast::emit_box(emitter, ctx, &operand_ty);
                    emit_expr(emitter, ctx, rhs);
                    let rhs_ty = ctx.ta.expr(rhs).ty.clone();
                    cast::emit_box(emitter, ctx, &rhs_ty);
                    emit_object_or_array_equality(emitter, ctx, &operand_ty, op);
                }
                other => {
                    unreachable!("typechecker rejects equality on Wasm value-type `{other:?}`")
                }
            }
        }
        BinOp::And | BinOp::Or => {
            // JS value-returning short-circuit, mirroring the `??` lowering:
            // stash the LHS in an anonymous local, truthiness-test the teed
            // copy, then either evaluate the RHS or replay the kept LHS —
            // each branch coerced into the result union's slot. `&&` keeps
            // the LHS when falsy, `||` when truthy.
            let result_val = ctx.symbols.value_type(result_ty);
            let lhs_ty = ctx.ta.expr(lhs).ty.clone();
            let lhs_val = ctx.symbols.value_type(&lhs_ty);
            let lhs_is_ref = matches!(lhs_val, ValType::Ref(_));
            let tmp = emitter.add_anonymous_local(lhs_val);
            emit_expr(emitter, ctx, lhs);
            emitter.instruction(Instruction::LocalTee(tmp));
            crate::codegen::function_emitter::cast::emit_condition_to_i32(emitter, ctx, &lhs_ty);
            let emit_rhs_branch = |emitter: &mut FunctionEmitter| {
                let rhs_ty = ctx.ta.expr(rhs).ty.clone();
                emit_expr(emitter, ctx, rhs);
                crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                    emitter, ctx, &rhs_ty, result_ty,
                );
            };
            let emit_kept_lhs_branch = |emitter: &mut FunctionEmitter| {
                emitter.instruction(Instruction::LocalGet(tmp));
                if lhs_is_ref {
                    // Ref-repr LHS: cast into the result slot's form —
                    // `ref.as_non_null` lift, downcast, or unbox as needed
                    // (same contract as the `??` kept branch).
                    crate::codegen::function_emitter::cast::emit_cast_to(emitter, ctx, result_ty);
                } else {
                    crate::codegen::function_emitter::cast::emit_coerce_to_slot(
                        emitter, ctx, &lhs_ty, result_ty,
                    );
                }
            };
            emitter.emit_if(BlockType::Result(result_val));
            match op {
                BinOp::And => emit_rhs_branch(emitter),
                _ => emit_kept_lhs_branch(emitter),
            }
            emitter.emit_else();
            match op {
                BinOp::And => emit_kept_lhs_branch(emitter),
                _ => emit_rhs_branch(emitter),
            }
            emitter.emit_end();
        }
        BinOp::In => emit_in_operator(emitter, ctx, lhs, rhs),
        // infer lifts `Binary { op: NullishCoalesce,.. }`
        // into the dedicated `TypedExprKind::NullishCoalesce` node, so
        // this arm is unreachable in well-typed input.
        BinOp::NullishCoalesce => {
            unreachable!(
                "BinOp::NullishCoalesce should be lifted into TypedExprKind::NullishCoalesce by infer"
            );
        }
    }
}

/// `"field" in obj` codegen. Walks the receiver's
/// `$ObjectShape.field_names` array twice — once with `ref.eq` (the
/// same-module fast path, sharing the per-name string global) and
/// once with the prelude `$string_eq` (the cross-module slow path).
/// Returns an i32 boolean (0/1) on the stack.
///
/// The check is inlined at every callsite rather than threaded
/// through a prelude helper, keeping the surface small while matching
/// the inline field-name scan used by object field access.
fn emit_in_operator(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, lhs: ExprId, rhs: ExprId) {
    let lhs_name = match &ctx.ta.expr(lhs).kind {
        crate::TypedExprKind::String(s) => s.clone(),
        other => unreachable!("typechecker enforces string-literal LHS for `in`, got {other:?}"),
    };
    // Push receiver; cast to (ref $ObjectShape) so `struct.get
    // $ObjectShape 1` is well-typed when the slot is wider
    // (`unknown`).
    emit_expr(emitter, ctx, rhs);
    crate::codegen::function_emitter::cast::emit_cast_to(
        emitter,
        ctx,
        &Type::Object {
            fields: std::collections::BTreeMap::new(),
        },
    );
    let object_shape_idx = ctx
        .symbols
        .object_shape_type_idx()
        .expect("ObjectShape type registered");
    let field_names_type_idx = ctx
        .symbols
        .field_names_type_idx()
        .expect("field_names type registered");
    // struct.get $ObjectShape 1 → (ref $field_names).
    emitter.instruction(Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 1,
    });
    // Cache the array in a fresh anonymous local; allocate i / len
    // scratch slots for the loop.
    let field_names_ref_ty = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(field_names_type_idx),
    });
    let names_local = emitter.add_anonymous_local(field_names_ref_ty);
    let i_local = emitter.add_anonymous_local(ValType::I32);
    let len_local = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::LocalSet(names_local));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::LocalSet(len_local));
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(&lhs_name)
        .expect("field-name string global allocated before in-operator codegen");
    let string_eq_idx = ctx
        .symbols
        .prelude_func_idx("string_eq")
        .expect("submilli:prelude.string_eq imported");
    // Result block — `br <n>` from inside the loops carries an i32
    // back to this frame; the fall-through at the bottom pushes 0.
    emitter.emit_block(BlockType::Result(ValType::I32));
    // Pass 1 — ref.eq fast path. Same-module callers share the
    // per-name string global with the shape's array entries, so
    // ref.eq matches in one instruction.
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::LocalGet(len_local));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::ArrayGet(field_names_type_idx));
    emitter.instruction(Instruction::GlobalGet(name_global));
    emitter.instruction(Instruction::RefEq);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::Br(3));
    emitter.emit_end(); // end if
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end(); // end loop
    emitter.emit_end(); // end pass-1 outer block
    // Pass 2 — string_eq slow path.
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::LocalGet(len_local));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::ArrayGet(field_names_type_idx));
    emitter.instruction(Instruction::GlobalGet(name_global));
    emitter.instruction(Instruction::Call(string_eq_idx));
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::Br(3));
    emitter.emit_end(); // end if
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end(); // end loop
    emitter.emit_end(); // end pass-2 outer block
    // No match in either pass.
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end(); // end result block
}

/// Emit the constructor field-setup sequence for `mangled`: each own-field
/// initializer (and, later, parameter-property copy) as `this.field = value`
/// into the object-fields payload, in recorded order. Runs after `super(...)`
/// (or at the top of a base class's init fn), so `this`/parent fields are live.
pub(crate) fn emit_class_field_setup(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    mangled: &crate::MangledName,
) {
    use crate::codegen::symbol_table::FieldSetup;
    let setup: Vec<FieldSetup> = match ctx.symbols.class_field_setup(mangled) {
        Some(s) if !s.is_empty() => s.to_vec(),
        _ => return,
    };
    let struct_idx = ctx
        .symbols
        .class_struct_type_idx(mangled)
        .expect("class struct type recorded in classes::emit");
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let this = emitter
        .this_local()
        .expect("field setup runs inside a constructor init fn");
    for step in &setup {
        let field = match step {
            FieldSetup::Init { field, .. }
            | FieldSetup::ParamCopy { field, .. }
            | FieldSetup::Reset { field } => field,
        };
        let slot = ctx
            .symbols
            .class_field_slot(mangled, field)
            .expect("class field slot recorded in classes::emit");
        emitter.instruction(Instruction::LocalGet(this));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: struct_idx,
            field_index: 2,
        });
        emitter.instruction(Instruction::I32Const(slot as i32));
        match step {
            FieldSetup::Init { value, .. } => {
                let value_ty = ctx.ta.expr(*value).ty.clone();
                emit_expr(emitter, ctx, *value);
                crate::codegen::function_emitter::cast::emit_box(emitter, ctx, &value_ty);
            }
            FieldSetup::ParamCopy {
                param_local, ty, ..
            } => {
                emitter.instruction(Instruction::LocalGet(*param_local));
                crate::codegen::function_emitter::cast::emit_box(emitter, ctx, ty);
            }
            FieldSetup::Reset { .. } => {
                emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
            }
        }
        emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
    }
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
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let index_local = emitter.add_anonymous_local(ValType::I32);
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
}

pub(crate) fn emit_object_field_write_by_name(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let value_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }));
    let index_local = emitter.add_anonymous_local(ValType::I32);
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
    emitter.emit_end();
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
) -> u32 {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let accessor_global = ctx
        .symbols
        .field_name_string_global_idx(accessor_name)
        .expect("caller checked the accessor name has a string global");
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(crate::codegen::classes::accessor_closure_sig(kind))
        .expect("accessor closure struct collected by analysis::note_shaped_property_access");
    let index_local = emitter.add_anonymous_local(ValType::I32);
    emit_object_field_index_by_name(emitter, ctx, object_local, accessor_global);
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
    emit_field_slot_get(emitter, intrinsics, object_local, index_local);
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
        closure_struct_idx,
    )));
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end();
    index_local
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
) {
    let getter = crate::codegen::classes::accessor_getter_name(prop_name);
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(prop_name)
        .expect("per-name string global recorded during field-name-strings emission");
    if !accessor_branch_emittable(ctx, &getter) {
        emit_object_field_read_by_name(emitter, ctx, object_local, name_global);
        emit_dynamic_narrowed_property_read(emitter, ctx, object_local, prop_name, field_ty);
        return;
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let result_vt = ctx.symbols.value_type(field_ty);
    let index_local = emitter.add_anonymous_local(ValType::I32);
    emit_object_field_index_by_name(emitter, ctx, object_local, name_global);
    emitter.instruction(Instruction::LocalSet(index_local));
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(BlockType::Result(result_vt));
    // data slot
    emit_field_slot_get(emitter, intrinsics, object_local, index_local);
    emit_dynamic_narrowed_property_read(emitter, ctx, object_local, prop_name, field_ty);
    emitter.emit_else();
    let getter_slot_local = emit_is_accessor_backed(
        emitter,
        ctx,
        object_local,
        &getter,
        crate::AccessorKind::Get,
    );
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
    );
    emitter.emit_else();
    // neither: absent — an optional member the value never materialised, so
    // `field_ty` admits null and the cast is a widen. An accessor-backed value
    // does not reach here even when the accessor is declared in another module:
    // the `get <prop>` name is interned off the *access*, not off any accessor
    // declaration this module can see (`analysis::note_shaped_property_access`).
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(intrinsics.object)));
    cast::emit_cast_to(emitter, ctx, field_ty);
    emitter.emit_end();
    emitter.emit_end();
}

fn emit_dynamic_narrowed_property_read(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop_name: &str,
    field_ty: &Type,
) {
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
        cast::emit_cast_to(emitter, ctx, field_ty);
        return;
    }

    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let raw = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }));
    emitter.instruction(Instruction::LocalSet(raw));
    emit_dynamic_narrowing_check(emitter, ctx, object_local, raw, field_ty, &checks, 0);
}

fn emit_dynamic_narrowing_check(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    raw: u32,
    field_ty: &Type,
    checks: &[(u32, &crate::FieldNarrowingCheck)],
    index: usize,
) {
    let Some((vtable, check)) = checks.get(index) else {
        emitter.instruction(Instruction::LocalGet(raw));
        cast::emit_cast_to(emitter, ctx, field_ty);
        return;
    };
    emitter.instruction(Instruction::LocalGet(object_local));
    crate::codegen::function_emitter::cast::emit_nominal_instance_test(emitter, ctx, *vtable);
    emitter.emit_if(BlockType::Result(ctx.symbols.value_type(field_ty)));
    emitter.instruction(Instruction::LocalGet(raw));
    crate::codegen::cast_check::emit_narrowed_field_read(emitter, ctx, check, field_ty);
    emitter.emit_else();
    emit_dynamic_narrowing_check(emitter, ctx, object_local, raw, field_ty, checks, index + 1);
    emitter.emit_end();
}

/// Write `value` to property `prop` on an `$ObjectShape`-typed receiver,
/// accessor-aware: a data field writes its payload slot; an accessor property
/// invokes its `set <prop>` method closure; a property backed by a getter with no
/// setter throws, since the target exists but is read-only; a property with none
/// of the three is absent and the write is discarded, matching
/// `emit_object_field_write_by_name`. `value` is emitted in whichever branch runs
/// (only one executes at runtime), so its side effects happen exactly once even
/// where the store goes nowhere.
pub(crate) fn emit_object_property_write(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    prop: &Ident,
    value: ExprId,
) {
    let prop_name = prop.name.as_str();
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let setter = crate::codegen::classes::accessor_setter_name(prop_name);
    let getter = crate::codegen::classes::accessor_getter_name(prop_name);
    let has_setter_name = ctx.symbols.field_name_string_global_idx(&setter).is_some();
    let has_getter_name = ctx.symbols.field_name_string_global_idx(&getter).is_some();
    let name_global = ctx
        .symbols
        .field_name_string_global_idx(prop_name)
        .expect("per-name string global recorded during field-name-strings emission");
    // Fail-soft twin of [`accessor_branch_emittable`], asked of both halves:
    // reaching here means analysis classified the access, which interns both
    // accessor names — so neither name can actually be missing.
    if !has_setter_name && !has_getter_name {
        let value_ty = ctx.ta.expr(value).ty.clone();
        emit_expr(emitter, ctx, value);
        cast::emit_box(emitter, ctx, &value_ty);
        emit_object_field_write_by_name(emitter, ctx, object_local, name_global);
        return;
    }
    let index_local = emitter.add_anonymous_local(ValType::I32);
    emit_object_field_index_by_name(emitter, ctx, object_local, name_global);
    emitter.instruction(Instruction::LocalSet(index_local));
    emitter.instruction(Instruction::LocalGet(index_local));
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::I32GeS);
    emitter.emit_if(BlockType::Empty);
    // data slot
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    emitter.instruction(Instruction::LocalGet(index_local));
    let value_ty = ctx.ta.expr(value).ty.clone();
    emit_expr(emitter, ctx, value);
    cast::emit_box(emitter, ctx, &value_ty);
    emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
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
            emit_is_accessor_backed(emitter, ctx, object_local, accessor, arm.scanned_kind());
        emitter.emit_if(BlockType::Empty);
        match arm {
            AccessorWrite::Setter => {
                emitter.instruction(Instruction::LocalGet(object_local));
                emit_interface_method_via_shape_with_receiver_on_stack(
                    emitter,
                    ctx,
                    accessor,
                    std::slice::from_ref(&value),
                    &Type::Void,
                    Some(slot_index_local),
                );
            }
            // A getter with no setter: the property is there and is read-only.
            // Discarding the write here would lose it silently.
            AccessorWrite::ReadOnly => {
                emit_value_for_effect(emitter, ctx, value);
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
    // No slot of any kind: an absent optional member, so the store has nowhere
    // to go.
    emit_value_for_effect(emitter, ctx, value);
    for _ in &arms {
        emitter.emit_end();
    }
    emitter.emit_end();
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
) {
    let array_idx = ctx
        .symbols
        .array_type_idx()
        .expect("Type::Array requires intrinsic types declared");
    let raw_array_idx = ctx
        .symbols
        .raw_array_type_idx()
        .expect("Type::Array requires intrinsic types declared");
    let raw_arr_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(raw_array_idx),
    }));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: array_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(raw_arr_local));
    emit_expr(emitter, ctx, idx);
    let idx_f64_local = stash_index_operand(emitter);
    let idx_local = emit_checked_index(emitter, ctx, raw_arr_local, idx_f64_local);
    emitter.instruction(Instruction::LocalGet(raw_arr_local));
    emitter.instruction(Instruction::LocalGet(idx_local));
    emitter.instruction(Instruction::ArrayGet(raw_array_idx));
    cast::emit_cast_to(emitter, ctx, result_ty);
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

/// Run `value` for its side effects and discard the result. A write that never
/// reaches a slot still has to evaluate its right-hand side, so that every
/// branch of a property write evaluates it exactly once.
fn emit_value_for_effect(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, value: ExprId) {
    emit_expr(emitter, ctx, value);
    emitter.instruction(Instruction::Drop);
}

pub(crate) fn emit_object_field_index_by_name(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object_local: u32,
    name_global: u32,
) {
    let object_shape_idx = ctx
        .symbols
        .object_shape_type_idx()
        .expect("ObjectShape type registered");
    let field_names_type_idx = ctx
        .symbols
        .field_names_type_idx()
        .expect("field_names type registered");
    let names_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(field_names_type_idx),
    }));
    let i_local = emitter.add_anonymous_local(ValType::I32);
    let len_local = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::LocalGet(object_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalSet(names_local));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::LocalSet(len_local));
    let string_eq_idx = ctx
        .symbols
        .prelude_func_idx("string_eq")
        .expect("submilli:prelude.string_eq imported");

    emitter.emit_block(BlockType::Result(ValType::I32));
    emit_field_name_scan_pass(
        emitter,
        field_names_type_idx,
        names_local,
        i_local,
        len_local,
        name_global,
        None,
    );
    emit_field_name_scan_pass(
        emitter,
        field_names_type_idx,
        names_local,
        i_local,
        len_local,
        name_global,
        Some(string_eq_idx),
    );
    emitter.instruction(Instruction::I32Const(-1));
    emitter.emit_end();
}

fn emit_field_name_scan_pass(
    emitter: &mut FunctionEmitter,
    field_names_type_idx: u32,
    names_local: u32,
    i_local: u32,
    len_local: u32,
    name_global: u32,
    string_eq_idx: Option<u32>,
) {
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.emit_block(BlockType::Empty);
    emitter.emit_loop(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::LocalGet(len_local));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::BrIf(1));
    emitter.instruction(Instruction::LocalGet(names_local));
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::ArrayGet(field_names_type_idx));
    emitter.instruction(Instruction::GlobalGet(name_global));
    if let Some(string_eq_idx) = string_eq_idx {
        emitter.instruction(Instruction::Call(string_eq_idx));
    } else {
        emitter.instruction(Instruction::RefEq);
    }
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::Br(3));
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(i_local));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32Add);
    emitter.instruction(Instruction::LocalSet(i_local));
    emitter.instruction(Instruction::Br(0));
    emitter.emit_end();
    emitter.emit_end();
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
) {
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .expect("$string type registered before call codegen");
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .expect("$rawString type registered before call codegen");
    for (i, &arg_id) in args.iter().enumerate() {
        emit_expr(emitter, ctx, arg_id);
        // Plan 75.8: coerce a primitive arg into a wider ref-typed
        // param slot (e.g., calling `f(x: number | null)` with `5`
        // requires boxing the f64 before the call). No-op when the
        // value's Wasm type already satisfies the param slot via
        // subtyping.
        if let Some(param_ty) = params.get(i) {
            let arg_ty = ctx.ta.expr(arg_id).ty.clone();
            cast::emit_coerce_to_slot(emitter, ctx, &arg_ty, param_ty);
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
    if is_host && matches!(ret.peel(), Type::String) {
        let scratch = emitter.add_anonymous_local(ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(raw_string_type_idx),
        }));
        emitter.instruction(Instruction::LocalSet(scratch));
        let vtable_global = ctx
            .symbols
            .prelude_global_idx("string_vtable")
            .expect("string_vtable imported from prelude");
        emitter.instruction(Instruction::GlobalGet(vtable_global));
        emitter.instruction(Instruction::LocalGet(scratch));
        emitter.instruction(Instruction::StructNew(string_type_idx));
    }
}

/// Emit an indirect call via `call_ref` through a closure value
/// (`LocalRef` / `GlobalRef` to a function-typed binding, an inline
/// `Closure`, etc.).
///
/// Under uniform closure ABI, every closure param and the
/// return are erased to `(ref $Object)` at the Wasm boundary. This
/// call site boxes each typed arg via `cast::emit_box` before
/// `call_ref` and validates the erased return before its declared-type cast.
/// A mismatched implementation therefore throws a catchable `TypeError`.
fn emit_indirect_closure_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    callee: ExprId,
    args: &[ExprId],
) {
    let callee_ty = ctx.ta.expr(callee).ty.clone();
    emit_expr(emitter, ctx, callee);
    emit_indirect_closure_call_with_receiver_on_stack(emitter, ctx, &callee_ty, args);
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
) {
    // chain receivers can arrive aliased
    // (`type NumberFn = (x: number) => number; let f: NumberFn | null`
    // → after `strip_null`, the type is the alias, not the underlying
    // function). `classify` panics on non-Function inputs, so peel.
    let callee_ty = callee_ty.peel();
    let closure_sig = crate::codegen::closures::classify(callee_ty);
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(closure_sig)
        .expect("closure struct type registered for every closure-typed callee");
    let fn_type_idx = ctx
        .symbols
        .closure_func_type_idx(closure_sig)
        .expect("closure funcref type registered for every closure-typed callee");
    let closure_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_struct_idx),
    }));
    emitter.instruction(Instruction::LocalTee(closure_local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 2,
    });
    // note: variadic closures arrive with their trailing args
    // already packed into a synthesized `ArrayLiteral` filling the
    // rest slot, so `args.len()` matches the closure's funcref
    // signature arity. The per-arg `emit_box` below is still correct
    // — the synthesized array's static type is a concrete
    // `Type::Array(_)`, so `emit_box` upcasts it to `(ref $Object)`
    // via the same ref-extension that any array arg uses.
    for &arg_id in args {
        emit_expr(emitter, ctx, arg_id);
        let arg_ty = ctx.ta.expr(arg_id).ty.clone();
        cast::emit_box(emitter, ctx, &arg_ty);
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
    let Type::Function { ret, .. } = callee_ty else {
        unreachable!("indirect call callee must be Type::Function");
    };
    if !ret.is_void() {
        crate::codegen::cast_check::emit_checked_cast_on_stack(emitter, ctx, &Type::Unknown, ret);
    }
}

/// Runtime check for `typeof x === "object"` / `"function"`.
///
/// The typechecker's `fold_typeof_tag` (in `infer/expr.rs`) folds
/// the predicate to `Boolean(true)` / `Boolean(false)` whenever the
/// operand's type makes the answer statically determinable — so by
/// the time codegen sees a `TypeofTag` node, a real runtime
/// classification is needed. Both tags rest on intrinsic base
/// supertypes:
///
/// - Function tag → `ref.test (ref $Closure)`. Every per-signature
///   closure struct extends `$Closure`, so one instruction
///   classifies any closure regardless of arity / signature.
/// - Object tag → `ref.test (ref $Object)` minus the `$Object`
///   subtypes that answer a primitive tag instead (`$string`,
///   `$BoxedNumber`, `$BoxedBoolean`, `$bigint`, `$Closure`), with an
///   OR-ed `ref.is_null` for the JS `typeof null === "object"` quirk
///   when the operand's Wasm type admits null.
fn emit_typeof_tag(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    value: ExprId,
    tag: crate::TypeofTagKind,
) {
    match tag {
        crate::TypeofTagKind::Number => {
            // `typeof x === "number"` lowers directly to
            // TypeofTag(Number). `ref.test (ref $BoxedNumber)` —
            // returns 1 iff the value is a `$BoxedNumber`. Handles
            // nullable LHS (returns 0 for null) without an explicit
            // null check.
            let boxed = ctx
                .symbols
                .boxed_number_type_idx()
                .expect("$BoxedNumber intrinsic type registered");
            emit_expr(emitter, ctx, value);
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(boxed)));
        }
        crate::TypeofTagKind::String => {
            let string = ctx
                .symbols
                .string_type_idx()
                .expect("$string intrinsic type registered");
            emit_expr(emitter, ctx, value);
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(string)));
        }
        crate::TypeofTagKind::Boolean => {
            let boxed = ctx
                .symbols
                .boxed_boolean_type_idx()
                .expect("$BoxedBoolean intrinsic type registered");
            emit_expr(emitter, ctx, value);
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(boxed)));
        }
        crate::TypeofTagKind::Function => {
            let closure_idx = ctx
                .symbols
                .closure_type_idx()
                .expect("$Closure intrinsic type registered");
            emit_expr(emitter, ctx, value);
            emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(closure_idx)));
        }
        crate::TypeofTagKind::Object => {
            let lhs_ty = ctx.ta.expr(value).ty.clone();
            let scratch_ty = ctx.symbols.value_type(&lhs_ty);
            let nullable = matches!(scratch_ty, ValType::Ref(rt) if rt.nullable);
            let scratch = emitter.add_anonymous_local(scratch_ty);
            emit_expr(emitter, ctx, value);
            emitter.instruction(Instruction::LocalSet(scratch));

            // Stated as the complement of the primitive tags rather than a list
            // of object-like types: every runtime value is an `$Object`
            // subtype, so a positive list silently answers "not an object" for
            // any struct nobody remembered to add. The exclusions below are
            // exhaustive by construction — they are the `$Object` subtypes that
            // carry a primitive tag of their own.
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared by codegen entry");
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
    }
}

fn emit_unary(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, op: UnOp, operand: ExprId) {
    match op {
        UnOp::Neg => {
            // bigint negation routes to inline host call.
            if matches!(ctx.ta.expr(operand).ty.peel(), Type::BigInt) {
                emit_expr(emitter, ctx, operand);
                emit_bigint_extract_to_stack(emitter, ctx);
                let host_idx = ctx
                    .symbols
                    .func_idx(&crate::mangle::host(
                        crate::runtime::BIGINT_MODULE_NAME,
                        "neg",
                    ))
                    .expect("submilli:bigint.neg import recorded by codegen bootstrap");
                emit_bigint_wrap_host_result(emitter, ctx, host_idx);
                return;
            }
            emit_expr(emitter, ctx, operand);
            emitter.instruction(Instruction::F64Neg);
        }
        UnOp::Pos => {
            // On a string, `+` is the explicit numeric coercion and lowers to
            // the same host parse `Number(s)` calls. On a number it is the
            // identity, so the operand's value is already what we want.
            emit_expr(emitter, ctx, operand);
            if ctx.ta.expr(operand).ty.is_string_shaped() {
                let host_idx = ctx
                    .symbols
                    .func_idx(&crate::mangle::host(
                        crate::runtime::NUMBER_MODULE_NAME,
                        "toNumber",
                    ))
                    .expect("submilli:number.toNumber import recorded by codegen analysis");
                emitter.instruction(Instruction::Call(host_idx));
            }
        }
        UnOp::Not => {
            // `!x` on a boolean is `i32.eqz` — 0 → 1, anything else → 0.
            // Plan 75.9: the inferer also accepts nullable
            // operands (e.g., `!s` with `s: string | null`); for those
            // we first coerce to the i32 truthiness convention
            // (`ref.is_null; i32.eqz`) before negating.
            let operand_ty = ctx.ta.expr(operand).ty.clone();
            emit_expr(emitter, ctx, operand);
            crate::codegen::function_emitter::cast::emit_condition_to_i32(
                emitter,
                ctx,
                &operand_ty,
            );
            emitter.instruction(Instruction::I32Eqz);
        }
    }
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
) {
    let recv_ty = ctx.ta.expr(receiver).ty.clone();
    emit_expr(emitter, ctx, receiver);
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
    );
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
) {
    let concrete_receiver = crate::typechecker::infer::narrowing::strip_null(recv_ty);
    crate::codegen::field_guards::attach(emitter, ctx, &concrete_receiver);
    let direct_key = crate::mangle::extend(iface, method);
    if let Some(func_idx) = ctx.symbols.func_idx(&direct_key) {
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
        );
        emit_args_into_slots(emitter, ctx, args, Some(&abi.params));
        emitter.instruction(Instruction::Call(func_idx));
        emit_slot_return_cast(emitter, ctx, call_ret_ty, Some(&abi));
        return;
    }
    // Class method (static path): the receiver is `(ref $Foo)` on the stack.
    // Dispatch through the class's own vtable slot (4+); `class_method_slot` is
    // only populated for classes, so a `Some` here means a genuine class method
    // (a user method named `toString` etc. also resolves here, not slot 0–3).
    if ctx.symbols.class_method_slot(iface, method).is_some() {
        emit_class_vtable_dispatch(emitter, ctx, iface, method, args, call_ret_ty);
        return;
    }
    if let Some(slot) = vtable_slot_for_method(method) {
        emit_vtable_dispatch_on_object_stack(emitter, ctx, slot);
        return;
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
        );
        return;
    }
    // Fallthrough: no direct-dispatch wrapper, no vtable slot,
    // not an interface-shape dispatch. The typechecker accepted
    // this call, so an emit path landed here in error — trap with
    // a clear failure at the source span.
    emitter.instruction(Instruction::Unreachable);
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
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");

    // Build the closure signature from the arg types and the
    // call's return type. `classify` reduces to (arity, is_void).
    let arg_tys: Vec<Type> = args.iter().map(|a| ctx.ta.expr(*a).ty.clone()).collect();
    let fn_ty = Type::Function {
        params: arg_tys,
        ret: Box::new(call_ret_ty.clone()),
        predicate: None,
        // call-site arg shape — variadic packing has already
        // happened upstream by this point, so this signature is the
        // fixed-arity post-pack form.
        has_rest: false,
    };
    let closure_sig = crate::codegen::closures::classify(&fn_ty);
    let closure_struct_idx = ctx
        .symbols
        .closure_struct_type_idx(closure_sig)
        .expect("closure struct registered for the method's signature (SUB-166)");
    let fn_type_idx = ctx
        .symbols
        .closure_func_type_idx(closure_sig)
        .expect("closure funcref type registered for the method's signature");

    let rcv_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object_shape),
    }));
    let closure_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(closure_struct_idx),
    }));

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
        let name_global = ctx.symbols.field_name_string_global_idx(method).expect(
            "per-name string global recorded during field-name-strings emission \
             for interface method names",
        );
        emit_object_field_read_by_name(emitter, ctx, rcv_local, name_global);
    }

    // 3. Cast to the matching closure struct type. closure
    // ABI: every Function-typed value stored in an `$Object` slot is
    // a `(ref $closure_<sig>)`.
    crate::codegen::closure_coercions::emit_erased_cast(
        emitter,
        ctx,
        crate::codegen::closures::ClosureSig::of(args.len(), call_ret_ty),
    );
    emitter.instruction(Instruction::LocalTee(closure_local));

    // 4. Push the closure env (slot 2) as the implicit first arg.
    emitter.instruction(Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 2,
    });

    // 5. Emit each user arg, boxing to `(ref $Object)` per the
    // closure ABI's erasure.
    for &arg_id in args {
        emit_expr(emitter, ctx, arg_id);
        let arg_ty = ctx.ta.expr(arg_id).ty.clone();
        cast::emit_box(emitter, ctx, &arg_ty);
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
    if !call_ret_ty.is_void() {
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            call_ret_ty,
        );
    }
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
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    // Per-slot funcref type. Slots 0 (toString) and 1 (toJson) share
    // operational signature `(ref $Object) -> (ref $string)` but are
    // declared as distinct sub-funcrefs inside the `$Object` rec
    // group, so `call_ref` needs the slot's own type index.
    let fn_type_idx = match slot {
        0 => intrinsics.to_string_fn,
        1 => intrinsics.to_json_fn,
        _ => panic!(
            "emit_vtable_dispatch: slot {slot} not yet routed through this helper; \
             equals (2) goes through `emit_object_or_array_equality`, hash (3) \
             has no method-call shape yet",
        ),
    };
    let obj_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object),
    }));
    let fn_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(fn_type_idx),
    }));
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
) {
    for (i, &arg) in args.iter().enumerate() {
        emit_expr(emitter, ctx, arg);
        if let Some(slot) = slots.and_then(|s| s.get(i).copied()) {
            let arg_ty = ctx.ta.expr(arg).ty.clone();
            cast::emit_coerce_to_wasm_slot(emitter, ctx, &arg_ty, slot);
        }
    }
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
) -> MethodSlotAbi {
    if let Some(recorded) = ctx.symbols.iface_method_abi(direct_key) {
        return recorded.clone();
    }
    let erased = ctx.symbols.value_type(&Type::Unknown);
    let slot_is_erased =
        |i: usize| generic_args.is_some_and(|flags| flags.get(i).is_some_and(|g| g.is_generic));
    MethodSlotAbi {
        params: args
            .iter()
            .enumerate()
            .map(|(i, &a)| {
                if slot_is_erased(i) {
                    erased
                } else {
                    ctx.symbols.value_type(&ctx.ta.expr(a).ty)
                }
            })
            .collect(),
        ret: match return_cast {
            Some(_) => Some(erased),
            None => (!call_ret_ty.is_void()).then(|| ctx.symbols.value_type(call_ret_ty)),
        },
    }
}

/// Cast an erased slot result back to the call-site type. A slot whose result
/// already matches needs nothing.
fn emit_slot_return_cast(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    call_ret_ty: &Type,
    abi: Option<&MethodSlotAbi>,
) {
    if matches!(call_ret_ty.peel(), Type::Never) {
        emitter.instruction(Instruction::Unreachable);
        return;
    }
    if call_ret_ty.is_void() {
        return;
    }
    if abi.and_then(|a| a.ret) != Some(ctx.symbols.value_type(call_ret_ty)) {
        crate::codegen::cast_check::emit_checked_cast_on_stack(
            emitter,
            ctx,
            &Type::Unknown,
            call_ret_ty,
        );
    }
    crate::codegen::field_guards::attach(emitter, ctx, call_ret_ty);
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
) {
    let struct_idx = ctx
        .symbols
        .class_struct_type_idx(class)
        .expect("class struct type recorded");
    let vtable_idx = ctx
        .symbols
        .class_vtable_type_idx(class)
        .expect("class vtable type recorded");
    let slot = ctx
        .symbols
        .class_method_slot(class, method)
        .expect("class method slot recorded");
    let sig_idx = ctx
        .symbols
        .class_method_sig(class, method)
        .expect("class method sig recorded");

    let rcv_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(struct_idx),
    }));
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
    }));
    emitter.instruction(Instruction::LocalSet(fn_local));

    // self (`(ref $Foo)` <: the sig's `(ref $Object)`), then args, then funcref.
    let abi = ctx.symbols.class_method_abi(class, method).cloned();
    emitter.instruction(Instruction::LocalGet(rcv_local));
    emit_args_into_slots(
        emitter,
        ctx,
        args,
        abi.as_ref().map(|a| a.params.as_slice()),
    );
    emitter.instruction(Instruction::LocalGet(fn_local));
    emitter.instruction(Instruction::CallRef(sig_idx));
    emit_slot_return_cast(emitter, ctx, call_ret_ty, abi.as_ref());
}

fn emit_intrinsic_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    kind: Intrinsic,
    args: &[ExprId],
) {
    match kind {
        Intrinsic::Assert => {
            // `assert(cond, msg)` — throw `new Error(msg)` on false.
            // JS-style left-to-right argument evaluation: cond first,
            // then msg (stashed in a local for the throw path).
            debug_assert_eq!(args.len(), 2, "assert arity is enforced by the typechecker");
            emit_expr(emitter, ctx, args[0]);
            emit_expr(emitter, ctx, args[1]);
            let string_idx = ctx.symbols.string_type_idx().expect("$string registered");
            let msg_local = emitter.add_anonymous_local(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(string_idx),
            }));
            emitter.instruction(Instruction::LocalSet(msg_local));
            emitter.instruction(Instruction::I32Eqz);
            emitter.emit_if(BlockType::Empty);
            emitter.instruction(Instruction::LocalGet(msg_local));
            let ctor_idx = ctx
                .symbols
                .func_idx(&crate::mangle::prelude("Error#constructor"))
                .expect("Error#constructor exported from prelude");
            emitter.instruction(Instruction::Call(ctor_idx));
            crate::codegen::throw::emit_error_throw(emitter, ctx);
            emitter.emit_end();
        }
        Intrinsic::JsonStringify => {
            super::json::emit_stringify(emitter, ctx, args);
        }
        Intrinsic::JsonParse => {
            super::json::emit_parse(emitter, ctx, args);
        }
        // `BigInt.fromString(s)` — the remaining
        // bigint-producing intrinsic after retired
        // `BigIntFromNumber` / `NumberFromBigInt` in favour of the
        // call-signature wrappers on `BigIntConstructor` /
        // `NumberConstructor`. Wraps the host result `(i32 sign, ref
        // $rawBigInt limbs)` into a `$bigint` struct.
        Intrinsic::BigIntFromString => {
            debug_assert_eq!(
                args.len(),
                1,
                "BigInt.fromString arity enforced by typechecker",
            );
            emit_expr(emitter, ctx, args[0]);
            // The arg is `(ref $string)`; the host fn takes
            // `(ref $rawString)`. Pull the raw array out via slot 1.
            let string_type_idx = ctx.symbols.string_type_idx().expect("$string registered");
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
                .expect("submilli:bigint.fromString import recorded by codegen bootstrap");
            emit_bigint_wrap_host_result(emitter, ctx, host_idx);
        }
    }
}

/// Emit `lhs.vtable.equals(lhs, rhs)` for two same-typed operands
/// of `Type::Object` or `Type::Array`. Stack at entry: `[lhs, rhs]`.
/// Stack at exit: `[i32]` (1 if equal, 0 otherwise — flipped via
/// `I32Eqz` for `BinOp::NotEq`).
fn emit_object_or_array_equality(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    operand_ty: &Type,
    op: BinOp,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let operand_ref = ctx.symbols.value_type(operand_ty);
    let object_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let equals_fn_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.equals_fn),
    });

    // Stack: [lhs, rhs]. Stash both, load equals fn, restack args.
    let lhs_local = emitter.add_anonymous_local(operand_ref);
    let rhs_local = emitter.add_anonymous_local(operand_ref);
    let lhs_object = emitter.add_anonymous_local(object_ref);
    let rhs_object = emitter.add_anonymous_local(object_ref);
    let eq_fn_local = emitter.add_anonymous_local(equals_fn_ref);

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
}

/// null-aware `===` / `!==` dispatch. Used when either
/// operand's static type admits null. Bridges the operands to a
/// common Wasm shape (`(ref null $Object)`) via `emit_box`, then
/// emits a 4-way condition:
///
/// - both null → equal (1).
/// - lhs null, rhs non-null → unequal (0).
/// - lhs non-null, rhs null → unequal (0).
/// - both non-null → `lhs.vtable.equals(lhs, rhs)`.
///
/// Final `I32Eqz` flips the result for `NotEq`.
fn emit_nullable_eq(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op: BinOp,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
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

    let lhs_ty = ctx.ta.expr(lhs).ty.clone();
    let rhs_ty = ctx.ta.expr(rhs).ty.clone();

    // Materialize lhs as (ref null $Object). `emit_box` wraps
    // primitive Wasm values into their `$Boxed*` shape and is a
    // no-op for ref types / unions, so both sides land on the
    // stack at the universal nullable shape.
    let lhs_local = emitter.add_anonymous_local(object_null_ref);
    let rhs_local = emitter.add_anonymous_local(object_null_ref);
    let lhs_nonnull_local = emitter.add_anonymous_local(object_ref);
    let eq_fn_local = emitter.add_anonymous_local(equals_fn_ref);

    emit_expr(emitter, ctx, lhs);
    cast::emit_box(emitter, ctx, &lhs_ty);
    emitter.instruction(Instruction::LocalSet(lhs_local));

    emit_expr(emitter, ctx, rhs);
    cast::emit_box(emitter, ctx, &rhs_ty);
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

    if matches!(op, BinOp::NotEq) {
        emitter.instruction(Instruction::I32Eqz);
    }
}

/// Static type of a field access `receiver.name`. For a `Type::Object`
/// receiver, look up the field on its declared shape. For a
/// `Type::Union` of objects return the canonical union of
/// each member's field type — the typechecker has already verified
/// that every member exposes `name`. Both shapes are pre-validated by
/// `infer_field_access`; this helper is just the codegen-side
/// resolver used by the `FieldAccess` virtual-call cast step.
/// does `ty` reach a recursion back-edge (`AliasRef`) without
/// descending into another alias's stored body? Codegen-local mirror of
/// the typechecker's check; used to decide when a recomputed field type
/// must defer to the node's rehydrated type for a consistent lowering.
fn type_mentions_alias_ref(ty: &Type) -> bool {
    match ty {
        Type::AliasRef { .. } => true,
        Type::Union(ms) => ms.iter().any(type_mentions_alias_ref),
        Type::Array(e) => type_mentions_alias_ref(e),
        Type::Tuple(es) => es.iter().any(type_mentions_alias_ref),
        Type::Object { fields } => fields.values().any(|f| type_mentions_alias_ref(&f.ty)),
        Type::Function { params, ret, .. } => {
            params.iter().any(type_mentions_alias_ref) || type_mentions_alias_ref(ret)
        }
        Type::InterfaceRef { args, .. } | Type::Alias { args, .. } => {
            args.iter().any(type_mentions_alias_ref)
        }
        _ => false,
    }
}

pub(super) fn field_type_for_access(receiver_ty: &Type, name: &str) -> Option<Type> {
    // peel through aliases so a `type Point = { x: number }`
    // receiver reaches the Object arm.
    match receiver_ty.peel() {
        Type::Object { fields } => Some(
            fields
                .get(name)
                .expect("field validated by infer")
                .read_ty(),
        ),
        // A nominal member (interface, class) has no inline field map to read
        // here. `None` sends the caller to the node's own stamped type, which is
        // what the typechecker computed across all members.
        Type::Union(members) => members
            .iter()
            .map(|m| match m.peel() {
                Type::Object { fields } => fields.get(name).map(crate::ObjectField::read_ty),
                _ => None,
            })
            .collect::<Option<Vec<Type>>>()
            .map(Type::union),
        _ => {
            panic!("FieldAccess receiver must be Type::Object or Type::Union, got {receiver_ty:?}")
        }
    }
}

/// emit a bigint literal. Small literals (≤ ±2^53 - 1)
/// fast-path through `submilli:bigint.fromNumber`; larger literals
/// are looked up in the bigint constants pool and materialized via
/// `array.new_data` over a per-program data segment, then wrapped
/// directly into `$bigint` with the prelude's vtable.
fn emit_bigint_literal(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, digits: &str) {
    if let Some(entry) = ctx.bigints.lookup(digits) {
        // Large literal — pack limbs from data segment, wrap with
        // bigint vtable directly (no host fn involved). Data-segment
        // index is offset by the string-pool count because string
        // segments come first in the module's data section.
        let raw_bigint_idx = ctx
            .symbols
            .raw_bigint_type_idx()
            .expect("$rawBigInt registered");
        let bigint_idx = ctx.symbols.bigint_type_idx().expect("$bigint registered");
        let vtable_global = ctx
            .symbols
            .prelude_global_idx("bigint_vtable")
            .expect("bigint_vtable imported from prelude bootstrap");
        let data_idx = ctx.strings.strings.len() as u32 + entry.data_idx;
        emitter.instruction(Instruction::GlobalGet(vtable_global));
        emitter.instruction(Instruction::I32Const(entry.sign as i32));
        emitter.instruction(Instruction::I32Const(0));
        emitter.instruction(Instruction::I32Const(entry.limb_count as i32));
        emitter.instruction(Instruction::ArrayNewData {
            array_type_index: raw_bigint_idx,
            array_data_index: data_idx,
        });
        emitter.instruction(Instruction::StructNew(bigint_idx));
        return;
    }
    // Fits in f64's safe-integer range (pool guarantees it).
    // Construct via `submilli:bigint.fromNumber` + standard wrap.
    let v: i64 = digits
        .parse()
        .expect("small-bigint literals parse as i64 by construction of the pool");
    let host_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            "fromNumber",
        ))
        .expect("submilli:bigint.fromNumber import recorded by codegen bootstrap");
    emitter.instruction(Instruction::F64Const(Ieee64::from(v as f64)));
    emit_bigint_wrap_host_result(emitter, ctx, host_idx);
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
fn emit_regex_literal(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, source: &str, flags: &str) {
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .expect("Type::String requires the intrinsic types to be declared");
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .expect("Type::String requires the intrinsic types to be declared");
    let string_vtable_global_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable global imported from prelude");

    let source_idx = ctx
        .strings
        .lookup_text(source)
        .expect("regex source string interned by CodegenAnalysis");
    let flags_idx = ctx
        .strings
        .lookup_text(flags)
        .expect("regex flags string interned by CodegenAnalysis");

    emitter.emit_const_string(
        string_type_idx,
        raw_string_type_idx,
        string_vtable_global_idx,
        source_idx as u32,
        ctx.strings.code_units(source_idx),
    );
    emitter.emit_const_string(
        string_type_idx,
        raw_string_type_idx,
        string_vtable_global_idx,
        flags_idx as u32,
        ctx.strings.code_units(flags_idx),
    );

    let ctor_idx = ctx
        .symbols
        .func_idx(&crate::mangle::prelude("RegExpConstructor#new"))
        .expect("RegExpConstructor#new exported from prelude");
    emitter.instruction(Instruction::Call(ctor_idx));
}

/// helper — given a `(ref $bigint)` on top of the stack,
/// destructure it into `(i32 sign, ref $rawBigInt limbs)` on the
/// stack. Used by every codegen site that hands a bigint to a host
/// fn (`bigint.add` / `bigint.cmp` / `bigint.neg` /
/// `number.fromBigInt`).
fn emit_bigint_extract_to_stack(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let bigint_idx = ctx.symbols.bigint_type_idx().expect("$bigint registered");
    // Stash the receiver in a scratch local since we need two
    // struct.get reads.
    let scratch = emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(bigint_idx),
    }));
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
    emitter.instruction(Instruction::Call(host_func_idx));
    let bigint_idx = ctx.symbols.bigint_type_idx().expect("$bigint registered");
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .expect("$rawBigInt registered");
    let vtable_global = ctx
        .symbols
        .prelude_global_idx("bigint_vtable")
        .expect("bigint_vtable imported from prelude bootstrap");
    // Stack: [sign, limbs]. Need [vtable, sign, limbs] for
    // struct.new $bigint. Use two scratch locals to reorder.
    let scratch_limbs =
        emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        }));
    let scratch_sign = emitter.add_anonymous_local(wasm_encoder::ValType::I32);
    emitter.instruction(Instruction::LocalSet(scratch_limbs));
    emitter.instruction(Instruction::LocalSet(scratch_sign));
    emitter.instruction(Instruction::GlobalGet(vtable_global));
    emitter.instruction(Instruction::LocalGet(scratch_sign));
    emitter.instruction(Instruction::LocalGet(scratch_limbs));
    emitter.instruction(Instruction::StructNew(bigint_idx));
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
) {
    emit_expr(emitter, ctx, lhs);
    emit_bigint_extract_to_stack(emitter, ctx);
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .expect("$rawBigInt registered");
    let lhs_limbs =
        emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        }));
    let lhs_sign = emitter.add_anonymous_local(wasm_encoder::ValType::I32);
    emitter.instruction(Instruction::LocalSet(lhs_limbs));
    emitter.instruction(Instruction::LocalSet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_limbs));
    emit_expr(emitter, ctx, rhs);
    emit_bigint_extract_to_stack(emitter, ctx);
    let host_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            op_name,
        ))
        .unwrap_or_else(|| {
            panic!("submilli:bigint.{op_name} import recorded by codegen bootstrap")
        });
    emit_bigint_wrap_host_result(emitter, ctx, host_idx);
}

/// postfix `++` / `--` compute step for a bigint operand —
/// stack `[orig (ref $bigint)]` → `[new (ref $bigint)]`. Mirrors
/// [`emit_bigint_binop_inline`] but skips the lhs `emit_expr` (it's
/// already on the stack) and emits the rhs as a synthesized `1n`
/// literal via [`emit_bigint_literal`] (small-literal path:
/// `f64.const 1` + `submilli:bigint.fromNumber`).
fn emit_bigint_pm_one(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, op: crate::PostfixOp) {
    emit_bigint_extract_to_stack(emitter, ctx);
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .expect("$rawBigInt registered");
    let lhs_limbs =
        emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        }));
    let lhs_sign = emitter.add_anonymous_local(wasm_encoder::ValType::I32);
    emitter.instruction(Instruction::LocalSet(lhs_limbs));
    emitter.instruction(Instruction::LocalSet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_limbs));
    emit_bigint_literal(emitter, ctx, "1");
    emit_bigint_extract_to_stack(emitter, ctx);
    let op_name = match op {
        crate::PostfixOp::Inc => "add",
        crate::PostfixOp::Dec => "sub",
        crate::PostfixOp::NonNullAssert => unreachable!("non-null assertion is not PostfixUnary"),
    };
    let host_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            op_name,
        ))
        .unwrap_or_else(|| {
            panic!("submilli:bigint.{op_name} import recorded by codegen bootstrap")
        });
    emit_bigint_wrap_host_result(emitter, ctx, host_idx);
}

/// inline `cmp` + signed compare against 0 for
/// `<` / `>` / `<=` / `>=`.
fn emit_bigint_cmp_inline(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op: BinOp,
) {
    emit_bigint_cmp_call(emitter, ctx, lhs, rhs);
    emitter.instruction(Instruction::I32Const(0));
    let inst = match op {
        BinOp::Lt => Instruction::I32LtS,
        BinOp::Gt => Instruction::I32GtS,
        BinOp::Le => Instruction::I32LeS,
        BinOp::Ge => Instruction::I32GeS,
        _ => unreachable!("emit_bigint_cmp_inline called with non-cmp op"),
    };
    emitter.instruction(inst);
}

/// `===` / `!==` on bigint — equal iff `cmp == 0`.
fn emit_bigint_cmp_eq_inline(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    lhs: ExprId,
    rhs: ExprId,
    op: BinOp,
) {
    emit_bigint_cmp_call(emitter, ctx, lhs, rhs);
    emitter.instruction(Instruction::I32Eqz);
    if matches!(op, BinOp::NotEq) {
        emitter.instruction(Instruction::I32Eqz);
    }
}

/// shared "two bigint operands → submilli:bigint.cmp"
/// setup. Stack at exit: `i32` (the raw cmp result, -1/0/1).
fn emit_bigint_cmp_call(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, lhs: ExprId, rhs: ExprId) {
    emit_expr(emitter, ctx, lhs);
    emit_bigint_extract_to_stack(emitter, ctx);
    let raw_bigint_idx = ctx
        .symbols
        .raw_bigint_type_idx()
        .expect("$rawBigInt registered");
    let lhs_limbs =
        emitter.add_anonymous_local(wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(raw_bigint_idx),
        }));
    let lhs_sign = emitter.add_anonymous_local(wasm_encoder::ValType::I32);
    emitter.instruction(Instruction::LocalSet(lhs_limbs));
    emitter.instruction(Instruction::LocalSet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_sign));
    emitter.instruction(Instruction::LocalGet(lhs_limbs));
    emit_expr(emitter, ctx, rhs);
    emit_bigint_extract_to_stack(emitter, ctx);
    let cmp_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            "cmp",
        ))
        .expect("submilli:bigint.cmp import recorded by codegen bootstrap");
    emitter.instruction(Instruction::Call(cmp_idx));
}
