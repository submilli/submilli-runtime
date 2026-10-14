//! Codegen for virtual-package calls: `@mcp/<server>.<tool>(...)` and
//! `@tools.<tool>(...)`.
//!
//! There is no per-tool Wasm import. Every call lowers to the package's single
//! host fn: `submilli:mcp.call(server, tool, argsJson) -> unknown` or
//! `submilli:tools.call(tool, argsJson) -> unknown`. The names and the args
//! (serialized via the object's `toJson` vtable slot) are pushed as
//! `(ref $string)`s, and the host returns guest objects directly. The typechecker
//! wraps known-return calls in a normal `Cast`, so this module never emits cast
//! validation.

use wasm_encoder::{BlockType, Instruction};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::function_emitter::expr::{emit_expr, emit_vtable_dispatch_on_object_stack};
use crate::codegen::function_emitter::json::emit_inline_const_raw_string;
use crate::{ExprId, VirtualDispatch};

pub(super) fn emit_virtual_call(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    target: &VirtualDispatch,
    tool: &str,
    args: &[ExprId],
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if args.len() > 1 {
        return Err(crate::codegen::internal_failure(
            "virtual-package calls require a single arguments object",
        ));
    }
    // Push the `(ref $string)` args: the server (MCP only), tool, argsJson. A
    // zero-arg tool sends `{}`; otherwise the args object is serialized via its
    // vtable `toJson` slot.
    if let VirtualDispatch::Mcp { server } = target {
        emit_inline_const_string(emitter, ctx, server)?;
    }
    emit_inline_const_string(emitter, ctx, tool)?;
    if let Some(&arg) = args.first() {
        emit_expr(emitter, ctx, arg)?;
        let ty = &ctx
            .ta
            .try_expr(arg)
            .map_err(crate::codegen::arena_failure)?
            .ty;
        let local = emitter.add_anonymous_local(ctx.symbols.value_type(ty)?)?;
        emitter.instruction(Instruction::LocalTee(local));
        super::expr::emit_is_undefined(emitter, ctx)?;
        emitter.emit_if(BlockType::Result(
            ctx.symbols.value_type(&crate::Type::String)?,
        ));
        emit_inline_const_string(emitter, ctx, "{}")?;
        emitter.emit_else();
        emitter.instruction(Instruction::LocalGet(local));
        emit_vtable_dispatch_on_object_stack(emitter, ctx, 1);
        emitter.emit_end();
    } else {
        emit_inline_const_string(emitter, ctx, "{}")?;
    }

    let call_idx = ctx
        .symbols
        .func_idx(&crate::mangle::host(target.host_module(), "call"))
        .ok_or_else(|| {
            crate::codegen::internal_failure("virtual-package call imported during codegen")
        })?;
    emitter.instruction(Instruction::Call(call_idx));

    Ok(())
}

/// Push a real `(ref $string)` for an inline constant: the `string_vtable` over a
/// freshly built `$rawString`.
fn emit_inline_const_string(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("MCP intrinsic types are missing"))?;
    let string_vtable_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .ok_or_else(|| crate::codegen::internal_failure("MCP string vtable is missing"))?;
    emitter.instruction(Instruction::GlobalGet(string_vtable_idx));
    emit_inline_const_raw_string(emitter, ctx, text)?;
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(intrinsics.string));
    Ok(())
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
            let server = VirtualDispatch::Mcp {
                server: "server".to_string(),
            };
            assert_internal(
                emit_virtual_call(&mut emitter, ctx, &server, "tool", &[]).unwrap_err(),
            );
            assert_internal(
                emit_virtual_call(&mut emitter, ctx, &VirtualDispatch::Tools, "tool", &[])
                    .unwrap_err(),
            );
            assert_internal(emit_inline_const_string(&mut emitter, ctx, "server").unwrap_err());
        });
        with_context(&TypedAst::new(), &SymbolTable::default(), |ctx| {
            assert_internal(
                emit_inline_const_string(
                    &mut FunctionEmitter::new(ctx, &[]).unwrap(),
                    ctx,
                    "server",
                )
                .unwrap_err(),
            );
            let invalid = crate::ExprId(u32::MAX);
            assert_internal(
                emit_virtual_call(
                    &mut FunctionEmitter::new(ctx, &[]).unwrap(),
                    ctx,
                    &VirtualDispatch::Tools,
                    "tool",
                    &[invalid; 2],
                )
                .unwrap_err(),
            );
        });
    }
}
