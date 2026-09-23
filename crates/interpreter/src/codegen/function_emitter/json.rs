//! Codegen for the `JSON.stringify` and `JSON.parse` compiler intrinsics.

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::function_emitter::expr::{emit_expr, emit_vtable_dispatch_on_object_stack};
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::may_hold_null;
use crate::{ExprId, Type};
use wasm_encoder::{BlockType, Function, HeapType, Instruction, RefType, ValType};

pub(super) fn emit_stringify(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, args: &[ExprId]) {
    debug_assert!(
        (1..=3).contains(&args.len()),
        "JSON.stringify arity is enforced by the typechecker"
    );
    let arg = args[0];
    let arg_ty = ctx.ta.expr(arg).ty.clone();
    if args.len() == 1 {
        emit_expr(emitter, ctx, arg);
        emit_stringify_value(emitter, ctx, &arg_ty);
        return;
    }

    emit_expr(emitter, ctx, arg);
    let arg_local = emitter.add_anonymous_local(ctx.symbols.value_type(&arg_ty));
    emitter.instruction(Instruction::LocalSet(arg_local));
    emit_stringify_optional_args(emitter, ctx, args, arg_local, &arg_ty);
}

/// Serializes a value of `arg_ty` already on the stack into a `(ref $string)`.
/// Shared by `JSON.stringify` codegen and `main`'s output shim (for the structured
/// returns it routes here — primitives take the `toString` path instead).
pub(crate) fn emit_stringify_value(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, arg_ty: &Type) {
    // Peel first: the scalar arms below lower unboxed, so an alias reaching the
    // `_` vtable arm would push an f64/i32 where a ref is expected.
    let peeled = arg_ty.peel();
    match peeled {
        Type::Null => {
            // The value is a null ref; discard it and emit the literal `null`.
            emitter.instruction(Instruction::Drop);
            super::emit_inline_string_literal(emitter, ctx, "null");
        }
        Type::Boolean => emit_to_json_direct(emitter, ctx, "Boolean"),
        Type::Number | Type::NumberLiteral(_) => {
            emit_to_json_direct(emitter, ctx, "Number");
        }
        Type::String | Type::StringLiteral(_) => {
            emit_stringify_string_host(emitter, ctx);
        }
        _ if may_hold_null(peeled) => {
            // The value lowers to `(ref null $Object)`; vtable dispatch on a
            // null receiver would trap at `struct.get $Object 0`. Branch on
            // `ref.is_null` first.
            emit_stringify_nullable(emitter, ctx);
        }
        _ => {
            emit_vtable_dispatch_on_object_stack(emitter, ctx, 1);
        }
    }
}

fn emit_stringify_nullable(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let obj_tmp = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }));
    emitter.instruction(Instruction::LocalTee(obj_tmp));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    })));
    super::emit_inline_string_literal(emitter, ctx, "null");
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(obj_tmp));
    emitter.instruction(Instruction::RefAsNonNull);
    emit_vtable_dispatch_on_object_stack(emitter, ctx, 1);
    emitter.emit_end();
}

/// Serialize a `$string` to its JSON form (`"…"`, escaped) via the
/// `submilli:json.stringify` host fn rather than the per-code-unit Wasm escape
/// loop, which costs ~64 fuel/byte. The host fn takes/returns a `$rawString`, so
/// unwrap the receiver's raw array, call it, and re-wrap with the string vtable.
fn emit_stringify_string_host(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsic type indices registered");
    let string_type_idx = ctx.symbols.string_type_idx().expect("$string registered");
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_type_idx,
        field_index: 1,
    });

    let stringify_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringify",
        ))
        .expect("submilli:json.stringify imported during codegen");
    emitter.instruction(Instruction::Call(stringify_idx));

    let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_string),
    }));
    emitter.instruction(Instruction::LocalSet(raw_local));
    let string_vtable_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable imported");
    emitter.instruction(Instruction::GlobalGet(string_vtable_idx));
    emitter.instruction(Instruction::LocalGet(raw_local));
    emitter.instruction(Instruction::StructNew(intrinsics.string));
}

fn emit_stringify_optional_args(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    args: &[ExprId],
    arg_local: u32,
    arg_ty: &Type,
) {
    emit_expr(emitter, ctx, args[1]);
    emitter.instruction(Instruction::Drop);

    let space = if args.len() == 3 {
        emit_stringify_space_arg(emitter, ctx, args[2])
    } else {
        StringifySpace::None
    };

    emitter.instruction(Instruction::LocalGet(arg_local));
    emit_stringify_value(emitter, ctx, arg_ty);
    match space {
        StringifySpace::None => {}
        StringifySpace::Number(local) => {
            emit_string_on_stack_raw(emitter, ctx);
            emitter.instruction(Instruction::LocalGet(local));
            emit_pretty_number_host(emitter, ctx);
            emit_wrap_raw_string(emitter, ctx);
        }
        StringifySpace::String(local) => {
            emit_string_on_stack_raw(emitter, ctx);
            emitter.instruction(Instruction::LocalGet(local));
            emit_string_on_stack_raw(emitter, ctx);
            emit_pretty_string_host(emitter, ctx);
            emit_wrap_raw_string(emitter, ctx);
        }
    }
}

enum StringifySpace {
    None,
    Number(u32),
    String(u32),
}

fn emit_stringify_space_arg(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    space: ExprId,
) -> StringifySpace {
    let space_ty = ctx.ta.expr(space).ty.clone();
    match space_ty.peel() {
        Type::Number | Type::NumberLiteral(_) => {
            emit_expr(emitter, ctx, space);
            let local = emitter.add_anonymous_local(ValType::F64);
            emitter.instruction(Instruction::LocalSet(local));
            StringifySpace::Number(local)
        }
        Type::String | Type::StringLiteral(_) => {
            emit_expr(emitter, ctx, space);
            let local = emitter.add_anonymous_local(ctx.symbols.value_type(&space_ty));
            emitter.instruction(Instruction::LocalSet(local));
            StringifySpace::String(local)
        }
        Type::Null => {
            emit_expr(emitter, ctx, space);
            emitter.instruction(Instruction::Drop);
            StringifySpace::None
        }
        Type::Error => StringifySpace::None,
        other => panic!("JSON.stringify space type should be checked, got {other:?}"),
    }
}

fn emit_string_on_stack_raw(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let string_type_idx = ctx.symbols.string_type_idx().expect("$string registered");
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_type_idx,
        field_index: 1,
    });
}

fn emit_pretty_number_host(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let pretty_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyPrettyNumber",
        ))
        .expect("submilli:json.stringifyPrettyNumber imported during codegen");
    emitter.instruction(Instruction::Call(pretty_idx));
}

fn emit_pretty_string_host(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let pretty_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyPrettyString",
        ))
        .expect("submilli:json.stringifyPrettyString imported during codegen");
    emitter.instruction(Instruction::Call(pretty_idx));
}

fn emit_wrap_raw_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsic type indices registered");
    let raw_local = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_string),
    }));
    emitter.instruction(Instruction::LocalSet(raw_local));
    let string_vtable_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable imported");
    emitter.instruction(Instruction::GlobalGet(string_vtable_idx));
    emitter.instruction(Instruction::LocalGet(raw_local));
    emitter.instruction(Instruction::StructNew(intrinsics.string));
}

fn emit_to_json_direct(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, iface: &str) {
    let mangled = crate::mangle::extend(&crate::mangle::prelude(iface), "toJson");
    let func_idx = ctx
        .symbols
        .func_idx(&mangled)
        .unwrap_or_else(|| panic!("{iface}#toJson imported from prelude"));
    emitter.instruction(Instruction::Call(func_idx));
}

/// Body of the exported `__main_output` shim: call `main`, then encode its result
/// to the `(ref $string)` the runtime emits as the program's output. Emitted for
/// every non-`void` return (`never`/`error` never produce a value). The host reads
/// the returned `$string` verbatim.
pub(crate) fn emit_main_output_shim(
    ctx: &CodegenCtx,
    main_func_idx: u32,
    return_ty: &Type,
) -> Function {
    let mut emitter = FunctionEmitter::new(ctx, &[]);
    emitter.instruction(Instruction::Call(main_func_idx));
    emit_main_output_value(&mut emitter, ctx, return_ty.peel());
    emitter.build()
}

/// Encodes `main`'s on-stack return value (`arg_ty`, already peeled) into the output
/// `(ref $string)`. Scalars render via `toString` — a `string` is its own output, so
/// it passes through verbatim and a large string return never pays a JSON-escape pass.
/// Everything else (objects, arrays, tuples, interfaces, unions, `null`) routes through
/// the shared JSON encoder, so structured returns match `JSON.stringify`.
fn emit_main_output_value(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, arg_ty: &Type) {
    match arg_ty {
        Type::String | Type::StringLiteral(_) => {}
        Type::Number | Type::NumberLiteral(_) => emit_number_to_string_radix10(emitter, ctx),
        Type::Boolean => emit_to_string_direct(emitter, ctx, "Boolean"),
        // A nullable primitive (`string | null`, the type of `readText` & friends)
        // follows the primitive rule, not JSON: the string flows through verbatim.
        Type::Union(members) if is_nullable_primitive(members) => {
            emit_nullable_primitive_to_string(emitter, ctx);
        }
        _ => emit_stringify_value(emitter, ctx, arg_ty),
    }
}

/// `true` for a union of `null` and one-or-more bare primitives (`string`/`number`/
/// `boolean` or their literals) — `string | null`, `number | null`, etc. Objects,
/// arrays, and mixed-primitive unions fall through to JSON.
fn is_nullable_primitive(members: &[Type]) -> bool {
    let mut has_null = false;
    let mut has_primitive = false;
    for member in members {
        match member.peel() {
            Type::Null => has_null = true,
            Type::String
            | Type::StringLiteral(_)
            | Type::Number
            | Type::NumberLiteral(_)
            | Type::Boolean => has_primitive = true,
            _ => return false,
        }
    }
    has_null && has_primitive
}

/// Renders a `(ref null $Object)`-lowered nullable primitive: the literal `null`
/// when null, else the boxed value's vtable `toString` (slot 0) — identity for a
/// string (verbatim), canonical `toString` for a boxed number/boolean.
fn emit_nullable_primitive_to_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let obj_tmp = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }));
    emitter.instruction(Instruction::LocalTee(obj_tmp));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    })));
    super::emit_inline_string_literal(emitter, ctx, "null");
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(obj_tmp));
    emitter.instruction(Instruction::RefAsNonNull);
    emit_vtable_dispatch_on_object_stack(emitter, ctx, 0);
    emitter.emit_end();
}

/// Calls `Number#toString` with the default radix 10 — the same import a no-arg
/// `(n).toString()` lowers to, so NaN/Infinity render as `"NaN"`/`"Infinity"`
/// (where JSON would emit `null`).
fn emit_number_to_string_radix10(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    emitter.instruction(Instruction::F64Const(10.0_f64.into()));
    emit_to_string_direct(emitter, ctx, "Number");
}

fn emit_to_string_direct(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, iface: &str) {
    let mangled = crate::mangle::extend(&crate::mangle::prelude(iface), "toString");
    let func_idx = ctx
        .symbols
        .func_idx(&mangled)
        .unwrap_or_else(|| panic!("{iface}#toString imported from prelude"));
    emitter.instruction(Instruction::Call(func_idx));
}

pub(super) fn emit_parse(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, args: &[ExprId]) {
    debug_assert_eq!(
        args.len(),
        1,
        "JSON.parse arity is enforced by the typechecker",
    );

    // Emit arg (the source `$string`), then extract field 1 — the host fn
    // signature takes `(ref $rawString)`, not the wrapped struct.
    emit_expr(emitter, ctx, args[0]);
    let string_type_idx = ctx.symbols.string_type_idx().expect("$string registered");
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_type_idx,
        field_index: 1,
    });

    let parse_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "parse",
        ))
        .expect("submilli:json.parse imported during codegen");
    // On invalid JSON the host fn raises a catchable Error directly. Valid JSON
    // returns the language's `unknown` representation: `(ref null $Object)`.
    emitter.instruction(Instruction::Call(parse_idx));
}

/// Builds a constant `(ref $rawString)` inline via `array.new_fixed` — the packed
/// UTF-16 data with no `$string` wrapper and no string-pool entry.
pub(crate) fn emit_inline_const_raw_string(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsic type indices registered");
    let units: Vec<u16> = text.encode_utf16().collect();
    for unit in &units {
        emitter.instruction(Instruction::I32Const(i32::from(*unit)));
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.raw_string,
        array_size: units.len() as u32,
    });
}
pub(crate) fn emit_raw_string_matches_literal(
    emitter: &mut FunctionEmitter,
    intrinsics: IntrinsicTypeIndices,
    key_raw_local: u32,
    expected: &str,
) {
    let result_local = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(result_local));

    let expected_units: Vec<u16> = expected.encode_utf16().collect();

    emitter.emit_block(BlockType::Empty);

    emitter.instruction(Instruction::LocalGet(key_raw_local));
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::I32Const(expected_units.len() as i32));
    emitter.instruction(Instruction::I32Ne);
    emitter.instruction(Instruction::BrIf(0));

    for (i, unit) in expected_units.iter().enumerate() {
        emitter.instruction(Instruction::LocalGet(key_raw_local));
        emitter.instruction(Instruction::I32Const(i as i32));
        emitter.instruction(Instruction::ArrayGetU(intrinsics.raw_string));
        emitter.instruction(Instruction::I32Const(*unit as i32));
        emitter.instruction(Instruction::I32Ne);
        emitter.instruction(Instruction::BrIf(0));
    }

    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::LocalSet(result_local));

    emitter.emit_end();

    emitter.instruction(Instruction::LocalGet(result_local));
}
