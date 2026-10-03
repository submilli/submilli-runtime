//! Codegen for the `JSON.stringify` and `JSON.parse` compiler intrinsics.

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::function_emitter::expr::{emit_expr, emit_vtable_dispatch_on_object_stack};
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::may_hold_null;
use crate::{ExprId, Type};
use wasm_encoder::{BlockType, Function, HeapType, Instruction, RefType, ValType};

pub(super) fn emit_stringify(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    args: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let [arg, rest @ ..] = args else {
        return Err(crate::codegen::internal_failure(
            "JSON.stringify requires a value",
        ));
    };
    if rest.len() > 2 {
        return Err(crate::codegen::internal_failure(
            "JSON.stringify accepts at most three arguments",
        ));
    }
    let arg = *arg;
    let arg_ty = ctx
        .ta
        .try_expr(arg)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    if args.len() == 1 {
        emit_expr(emitter, ctx, arg)?;
        emit_stringify_value(emitter, ctx, &arg_ty);
        return Ok(());
    }

    emit_expr(emitter, ctx, arg)?;
    let arg_local = emitter.add_anonymous_local(ctx.symbols.value_type(&arg_ty)?)?;
    emitter.instruction(Instruction::LocalSet(arg_local));
    emit_stringify_optional_args(emitter, ctx, args, arg_local, &arg_ty)?;
    Ok(())
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
            ctx.latch(super::emit_inline_string_literal(emitter, ctx, "null"));
        }
        Type::Boolean | Type::BooleanLiteral(_) => emit_to_json_direct(emitter, ctx, "Boolean"),
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
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsics declared by codegen entry",
    ) else {
        return;
    };
    let Some(obj_tmp) = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))) else {
        return;
    };
    emitter.instruction(Instruction::LocalTee(obj_tmp));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    })));
    ctx.latch(super::emit_inline_string_literal(emitter, ctx, "null"));
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
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsic type indices registered",
    ) else {
        return;
    };
    let Some(string_type_idx) = ctx.require(ctx.symbols.string_type_idx(), "$string registered")
    else {
        return;
    };
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_type_idx,
        field_index: 1,
    });

    let Some(stringify_idx) = ctx.require(
        ctx.symbols.func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringify",
        )),
        "submilli:json.stringify imported during codegen",
    ) else {
        return;
    };
    emitter.instruction(Instruction::Call(stringify_idx));

    let Some(raw_local) = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_string),
    }))) else {
        return;
    };
    emitter.instruction(Instruction::LocalSet(raw_local));
    let Some(string_vtable_idx) = ctx.require(
        ctx.symbols.prelude_global_idx("string_vtable"),
        "string_vtable imported",
    ) else {
        return;
    };
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
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let [_, replacer, rest @ ..] = args else {
        return Err(crate::codegen::internal_failure(
            "JSON.stringify optional arguments are missing",
        ));
    };
    emit_expr(emitter, ctx, *replacer)?;
    emitter.instruction(Instruction::Drop);

    let space = match rest {
        [] => StringifySpace::None,
        [space] => emit_stringify_space_arg(emitter, ctx, *space)?,
        _ => {
            return Err(crate::codegen::internal_failure(
                "JSON.stringify has extra optional arguments",
            ));
        }
    };

    emitter.instruction(Instruction::LocalGet(arg_local));
    emit_stringify_value(emitter, ctx, arg_ty);
    let _: () = match space {
        StringifySpace::None => {}
        StringifySpace::Dynamic(local) => emit_dynamic_space(emitter, ctx, local)?,
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
    };
    Ok(())
}

enum StringifySpace {
    None,
    Number(u32),
    String(u32),
    Dynamic(u32),
}

fn emit_stringify_space_arg(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    space: ExprId,
) -> Result<StringifySpace, crate::compiler_error::CompilerFailure> {
    let space_ty = ctx
        .ta
        .try_expr(space)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    Ok(match space_ty.peel() {
        Type::Number | Type::NumberLiteral(_) => {
            emit_expr(emitter, ctx, space)?;
            let local = emitter.add_anonymous_local(ValType::F64)?;
            emitter.instruction(Instruction::LocalSet(local));
            StringifySpace::Number(local)
        }
        Type::String | Type::StringLiteral(_) => {
            emit_expr(emitter, ctx, space)?;
            let local = emitter.add_anonymous_local(ctx.symbols.value_type(&space_ty)?)?;
            emitter.instruction(Instruction::LocalSet(local));
            StringifySpace::String(local)
        }
        Type::Null => {
            emit_expr(emitter, ctx, space)?;
            emitter.instruction(Instruction::Drop);
            StringifySpace::None
        }
        Type::Unknown => {
            emit_expr(emitter, ctx, space)?;
            let local = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::Unknown)?)?;
            emitter.instruction(Instruction::LocalSet(local));
            StringifySpace::Dynamic(local)
        }
        other => {
            return Err(crate::codegen::internal_failure(format!(
                "JSON.stringify space type was not validated: {other}"
            )));
        }
    })
}

fn emit_dynamic_space(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    space: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let string_type = ctx.symbols.value_type(&Type::String)?;
    let json = emitter.add_anonymous_local(string_type)?;
    emitter.instruction(Instruction::LocalSet(json));
    let number = ctx
        .symbols
        .boxed_number_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("boxed number registered"))?;
    let string = ctx
        .symbols
        .string_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("string registered"))?;
    emitter.instruction(Instruction::LocalGet(space));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(number)));
    emitter.emit_if(BlockType::Result(string_type));
    emitter.instruction(Instruction::LocalGet(json));
    emit_string_on_stack_raw(emitter, ctx);
    emitter.instruction(Instruction::LocalGet(space));
    super::cast::emit_cast_to(emitter, ctx, &Type::Number)?;
    emit_pretty_number_host(emitter, ctx);
    emit_wrap_raw_string(emitter, ctx);
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(space));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(string)));
    emitter.emit_if(BlockType::Result(string_type));
    emitter.instruction(Instruction::LocalGet(json));
    emit_string_on_stack_raw(emitter, ctx);
    emitter.instruction(Instruction::LocalGet(space));
    super::cast::emit_cast_to(emitter, ctx, &Type::String)?;
    emit_string_on_stack_raw(emitter, ctx);
    emit_pretty_string_host(emitter, ctx);
    emit_wrap_raw_string(emitter, ctx);
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(json));
    emitter.emit_end();
    emitter.emit_end();

    Ok(())
}

fn emit_string_on_stack_raw(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(string_type_idx) = ctx.require(ctx.symbols.string_type_idx(), "$string registered")
    else {
        return;
    };
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_type_idx,
        field_index: 1,
    });
}

fn emit_pretty_number_host(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(pretty_idx) = ctx.require(
        ctx.symbols.func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyPrettyNumber",
        )),
        "submilli:json.stringifyPrettyNumber imported during codegen",
    ) else {
        return;
    };
    emitter.instruction(Instruction::Call(pretty_idx));
}

fn emit_pretty_string_host(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(pretty_idx) = ctx.require(
        ctx.symbols.func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyPrettyString",
        )),
        "submilli:json.stringifyPrettyString imported during codegen",
    ) else {
        return;
    };
    emitter.instruction(Instruction::Call(pretty_idx));
}

fn emit_wrap_raw_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsic type indices registered",
    ) else {
        return;
    };
    let Some(raw_local) = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_string),
    }))) else {
        return;
    };
    emitter.instruction(Instruction::LocalSet(raw_local));
    let Some(string_vtable_idx) = ctx.require(
        ctx.symbols.prelude_global_idx("string_vtable"),
        "string_vtable imported",
    ) else {
        return;
    };
    emitter.instruction(Instruction::GlobalGet(string_vtable_idx));
    emitter.instruction(Instruction::LocalGet(raw_local));
    emitter.instruction(Instruction::StructNew(intrinsics.string));
}

fn emit_to_json_direct(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, iface: &str) {
    let mangled = crate::mangle::extend(&crate::mangle::prelude(iface), "toJson");
    let Some(func_idx) = ctx.latch(ctx.symbols.func_idx(&mangled).ok_or_else(|| {
        crate::codegen::internal_failure(format!("{iface}#toJson imported from prelude"))
    })) else {
        return;
    };
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
    source_return_ty: &Type,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let mut emitter = FunctionEmitter::new(ctx, &[])?;
    emitter.instruction(Instruction::Call(main_func_idx));
    let scalar_output = match source_return_ty.peel() {
        Type::String
        | Type::StringLiteral(_)
        | Type::Number
        | Type::NumberLiteral(_)
        | Type::Boolean
        | Type::BooleanLiteral(_) => true,
        Type::Union(members) => is_nullable_primitive(members),
        _ => false,
    };
    if return_ty == &Type::Unknown && scalar_output {
        emit_nullable_primitive_to_string(&mut emitter, ctx);
    } else {
        emit_main_output_value(&mut emitter, ctx, return_ty.peel());
    }
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
        Type::Boolean | Type::BooleanLiteral(_) => emit_to_string_direct(emitter, ctx, "Boolean"),
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
            | Type::Boolean
            | Type::BooleanLiteral(_) => has_primitive = true,
            _ => return false,
        }
    }
    has_null && has_primitive
}

/// Renders a `(ref null $Object)`-lowered nullable primitive: the literal `null`
/// when null, else the boxed value's vtable `toString` (slot 0) — identity for a
/// string (verbatim), canonical `toString` for a boxed number/boolean.
fn emit_nullable_primitive_to_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsics declared by codegen entry",
    ) else {
        return;
    };
    let Some(obj_tmp) = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))) else {
        return;
    };
    emitter.instruction(Instruction::LocalTee(obj_tmp));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    })));
    ctx.latch(super::emit_inline_string_literal(emitter, ctx, "null"));
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
    let Some(func_idx) = ctx.latch(ctx.symbols.func_idx(&mangled).ok_or_else(|| {
        crate::codegen::internal_failure(format!("{iface}#toString imported from prelude"))
    })) else {
        return;
    };
    emitter.instruction(Instruction::Call(func_idx));
}

pub(super) fn emit_parse(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    args: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let [arg] = args else {
        return Err(crate::codegen::internal_failure(
            "JSON.parse requires exactly one argument",
        ));
    };

    // Emit arg (the source `$string`), then extract field 1 — the host fn
    // signature takes `(ref $rawString)`, not the wrapped struct.
    emit_expr(emitter, ctx, *arg)?;
    super::cast::emit_coerce_to_slot(
        emitter,
        ctx,
        &ctx.ta
            .try_expr(*arg)
            .map_err(crate::codegen::arena_failure)?
            .ty,
        &Type::String,
    )?;
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("$string registered"))?;
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
        .ok_or_else(|| {
            crate::codegen::internal_failure("submilli:json.parse imported during codegen")
        })?;
    // On invalid JSON the host fn raises a catchable Error directly. Valid JSON
    // returns the language's `unknown` representation: `(ref null $Object)`.
    emitter.instruction(Instruction::Call(parse_idx));
    Ok(())
}

/// Builds a constant `(ref $rawString)` inline via `array.new_fixed` — the packed
/// UTF-16 data with no `$string` wrapper and no string-pool entry.
pub(crate) fn emit_inline_const_raw_string(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) {
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsic type indices registered",
    ) else {
        return;
    };
    let Some(array_size) = ctx.latch(crate::codegen::wasm_u32(text.encode_utf16().count())) else {
        return;
    };
    for unit in text.encode_utf16() {
        emitter.instruction(Instruction::I32Const(i32::from(unit)));
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.raw_string,
        array_size,
    });
}
pub(crate) fn emit_raw_string_matches_literal(
    emitter: &mut FunctionEmitter,
    intrinsics: IntrinsicTypeIndices,
    key_raw_local: u32,
    expected: &str,
) {
    let Some(expected_length) = emitter
        .ctx
        .latch(crate::codegen::wasm_u32(expected.encode_utf16().count()))
    else {
        return;
    };
    let Some(result_local) = emitter.ctx.latch(emitter.add_anonymous_local(ValType::I32)) else {
        return;
    };
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(result_local));

    emitter.emit_block(BlockType::Empty);

    emitter.instruction(Instruction::LocalGet(key_raw_local));
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::I32Const(expected_length as i32));
    emitter.instruction(Instruction::I32Ne);
    emitter.instruction(Instruction::BrIf(0));

    for (unit, i) in expected.encode_utf16().zip(0..expected_length) {
        emitter.instruction(Instruction::LocalGet(key_raw_local));
        emitter.instruction(Instruction::I32Const(i as i32));
        emitter.instruction(Instruction::ArrayGetU(intrinsics.raw_string));
        emitter.instruction(Instruction::I32Const(i32::from(unit)));
        emitter.instruction(Instruction::I32Ne);
        emitter.instruction(Instruction::BrIf(0));
    }

    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::LocalSet(result_local));

    emitter.emit_end();

    emitter.instruction(Instruction::LocalGet(result_local));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::invariant_tests::{assert_internal, with_context};
    use crate::codegen::{SymbolTable, tests::mock_symbols_with_intrinsics};
    use crate::{Span, TypedAst, TypedExpr, TypedExprKind};

    #[test]
    fn invalid_json_arity_is_an_internal_failure_before_reading_arguments() {
        with_context(&TypedAst::new(), &SymbolTable::default(), |ctx| {
            let mut emitter = FunctionEmitter::new(ctx, &[]).unwrap();
            assert_internal(emit_stringify(&mut emitter, ctx, &[]).unwrap_err());
            assert_internal(emit_parse(&mut emitter, ctx, &[]).unwrap_err());
            let invalid = crate::ExprId(u32::MAX);
            assert_internal(emit_stringify(&mut emitter, ctx, &[invalid; 4]).unwrap_err());
            assert_internal(emit_parse(&mut emitter, ctx, &[invalid; 2]).unwrap_err());
            ctx.check_failure().unwrap();
        });
    }

    #[test]
    fn json_emitters_latch_missing_registrations() {
        type Emit = fn(&mut FunctionEmitter<'_>, &CodegenCtx<'_>);
        let emitters: &[Emit] = &[
            emit_stringify_nullable,
            emit_stringify_string_host,
            emit_string_on_stack_raw,
            emit_pretty_number_host,
            emit_pretty_string_host,
            emit_wrap_raw_string,
            emit_nullable_primitive_to_string,
            |emitter, ctx| emit_to_json_direct(emitter, ctx, "Number"),
            |emitter, ctx| emit_to_string_direct(emitter, ctx, "Boolean"),
            |emitter, ctx| emit_inline_const_raw_string(emitter, ctx, "\u{1f642}"),
        ];
        for emit in emitters {
            with_context(&TypedAst::new(), &SymbolTable::default(), |ctx| {
                emit(&mut FunctionEmitter::new(ctx, &[]).unwrap(), ctx);
                assert_internal(ctx.check_failure().unwrap_err());
            });
        }
        for emit in [emit_stringify_string_host as Emit, emit_wrap_raw_string] {
            with_context(&TypedAst::new(), &mock_symbols_with_intrinsics(), |ctx| {
                emit(&mut FunctionEmitter::new(ctx, &[]).unwrap(), ctx);
                assert_internal(ctx.check_failure().unwrap_err());
            });
        }
    }

    #[test]
    fn invalid_space_metadata_returns_internal_failure() {
        for ty in [Type::Boolean, Type::Error] {
            let mut ta = TypedAst::new();
            let space = ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Boolean(true),
                    ty,
                    span: Span::at(crate::FileId(0)),
                })
                .unwrap();
            with_context(&ta, &mock_symbols_with_intrinsics(), |ctx| {
                let mut emitter = FunctionEmitter::new(ctx, &[]).unwrap();
                match emit_stringify_space_arg(&mut emitter, ctx, space) {
                    Err(error) => assert_internal(error),
                    Ok(_) => panic!("invalid space metadata was accepted"),
                }
            });
        }
    }

    #[test]
    fn inline_raw_literals_keep_utf16_units() {
        with_context(&TypedAst::new(), &mock_symbols_with_intrinsics(), |ctx| {
            let mut emitter = FunctionEmitter::new(ctx, &[]).unwrap();
            emit_inline_const_raw_string(&mut emitter, ctx, "a\u{1f642}");
            ctx.check_failure().unwrap();
            let units: Vec<_> = emitter
                .instructions
                .iter()
                .filter_map(|inst| match inst {
                    Instruction::I32Const(unit) => Some(*unit),
                    _ => None,
                })
                .collect();
            assert_eq!(units, vec![97, 0xd83d, 0xde42]);
            assert!(matches!(
                emitter.instructions.last(),
                Some(Instruction::ArrayNewFixed { array_size: 3, .. })
            ));
        });
    }
}
