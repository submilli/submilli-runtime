//! Codegen for `@mcp/<server>.<tool>(...)` calls.
//!
//! There is no per-tool Wasm import. Every call lowers to the single
//! `submilli:mcp.call(server, tool, argsJson) -> string` host fn: the server and
//! tool names (recovered from the mangled name) and the args (serialized via the
//! object's `toJson` vtable slot) are pushed as three `(ref $string)`s, then the
//! returned JSON text is parsed as `unknown`. The typechecker wraps known-return
//! MCP calls in a normal `Cast`, so this module never emits cast validation.

use wasm_encoder::Instruction;

use crate::ExprId;
use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::function_emitter::expr::{emit_expr, emit_vtable_dispatch_on_object_stack};
use crate::codegen::function_emitter::json::emit_inline_const_raw_string;

pub(super) fn emit_mcp_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    server: &str,
    tool: &str,
    args: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if args.len() > 1 {
        return Err(crate::codegen::internal_failure(
            "MCP calls require a single arguments object",
        ));
    }
    // Push the three `(ref $string)` args: server, tool, argsJson. A zero-arg
    // tool sends `{}`; otherwise the args object is serialized via its vtable
    // `toJson` slot.
    emit_inline_const_string(emitter, ctx, server);
    emit_inline_const_string(emitter, ctx, tool);
    if let Some(&arg) = args.first() {
        emit_expr(emitter, ctx, arg)?;
        emit_vtable_dispatch_on_object_stack(emitter, ctx, 1);
    } else {
        emit_inline_const_string(emitter, ctx, "{}");
    }

    let call_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::MCP_MODULE_NAME,
            "call",
        ))
        .ok_or_else(|| {
            crate::codegen::internal_failure("submilli:mcp.call imported during codegen")
        })?;
    emitter.instruction(Instruction::Call(call_idx));

    emit_parse_unknown(emitter, ctx);
    Ok(())
}

fn emit_parse_unknown(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsic type indices registered",
    ) else {
        return;
    };
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.string,
        field_index: 1,
    });
    let Some(parse_idx) = ctx.require(
        ctx.symbols.func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "parse",
        )),
        "submilli:json.parse imported during codegen",
    ) else {
        return;
    };
    emitter.instruction(Instruction::Call(parse_idx));
}

/// Push a real `(ref $string)` for an inline constant: the `string_vtable` over a
/// freshly built `$rawString`.
fn emit_inline_const_string(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, text: &str) {
    let Some(intrinsics) = ctx.require(
        ctx.symbols.intrinsic_type_indices(),
        "intrinsic type indices registered",
    ) else {
        return;
    };
    let Some(string_vtable_idx) = ctx.require(
        ctx.symbols.prelude_global_idx("string_vtable"),
        "string_vtable imported",
    ) else {
        return;
    };
    emitter.instruction(Instruction::GlobalGet(string_vtable_idx));
    emit_inline_const_raw_string(emitter, ctx, text);
    emitter.instruction(Instruction::StructNew(intrinsics.string));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypedAst;
    use crate::codegen::invariant_tests::{assert_internal, with_context};
    use crate::codegen::{SymbolTable, tests::mock_symbols_with_intrinsics};

    #[test]
    fn mcp_import_failures_are_compiler_failures() {
        with_context(&TypedAst::new(), &mock_symbols_with_intrinsics(), |ctx| {
            let mut emitter = FunctionEmitter::new(ctx, &[]).unwrap();
            assert_internal(emit_mcp_call(&mut emitter, ctx, "server", "tool", &[]).unwrap_err());
            assert_internal(ctx.check_failure().unwrap_err());
            emit_parse_unknown(&mut emitter, ctx);
            assert_internal(ctx.check_failure().unwrap_err());
        });
        with_context(&TypedAst::new(), &SymbolTable::default(), |ctx| {
            emit_parse_unknown(&mut FunctionEmitter::new(ctx, &[]).unwrap(), ctx);
            assert_internal(ctx.check_failure().unwrap_err());
            let invalid = crate::ExprId(u32::MAX);
            assert_internal(
                emit_mcp_call(
                    &mut FunctionEmitter::new(ctx, &[]).unwrap(),
                    ctx,
                    "server",
                    "tool",
                    &[invalid; 2],
                )
                .unwrap_err(),
            );
        });
    }
}
