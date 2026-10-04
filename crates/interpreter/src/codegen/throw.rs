//! Shared emission for raising a catchable `Error`: the native-`throw` tail
//! used by every wasm throw site (user `throw`, array bounds, `as` cast
//! mismatch, JSON shape mismatch), plus the `TypeError` raise built on it and
//! its messages.
//!
//! A throw whose message belongs to one subsystem keeps its constructor and
//! constant there — `bounds::emit_index_oob_throw` does. What lands here is
//! what has no better home: the shared tail, and messages raised from emitters
//! that don't own a module of their own.
//!
//! Every constant here must also be interned by `codegen::analysis`. The throw
//! reads its message out of the string pool; a missing entry is an internal
//! compile failure rather than a guest exception.

use wasm_encoder::{HeapType, Instruction};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::{FunctionEmitter, emit_const_string_by_text};

/// Thrown by `x!` when the value turns out to be `null`.
pub const NON_NULL_ASSERT_MESSAGE: &str = "non-null assertion failed: value is null";

/// Thrown when a property write resolves, at runtime, to a getter with no
/// setter — reachable only through a receiver whose static type does not say
/// which implementation backs the property.
pub const READ_ONLY_PROPERTY_MESSAGE: &str =
    "cannot assign to a property backed by a getter with no setter";

/// Raise a `TypeError` carrying `message`. Every message thrown this way must
/// also be interned by the analysis pass, or the string pool has no entry to
/// point at. Diverges, like [`emit_error_throw`].
pub(crate) fn emit_type_error_throw(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    message: &str,
) {
    if ctx
        .latch(emit_const_string_by_text(emitter, ctx, message))
        .is_none()
    {
        return;
    }
    let Some(new_idx) = ctx.require(
        ctx.symbols.prelude_func_idx("TypeError#constructor"),
        "TypeError#constructor imported from prelude",
    ) else {
        return;
    };
    emitter.instruction(Instruction::Call(new_idx));
    emit_error_throw(emitter, ctx);
}

/// Emit the throw tail: with an `Error` value already on the stack, narrow it
/// to `$Error` and raise the module's exception tag. The engine captures the
/// throw-site backtrace at the `throw` op. The `Throw` diverges; a caller
/// inside a typed block must emit a trailing `Unreachable` itself.
pub(crate) fn emit_error_throw(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    let Some(error_idx) = ctx.require(
        ctx.symbols.intrinsic_type_indices().map(|i| i.error),
        "error intrinsic registered",
    ) else {
        return;
    };
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(error_idx)));
    let Some(tag_idx) = ctx.require(ctx.symbols.error_tag_idx(), "error tag registered") else {
        return;
    };
    emitter.instruction(Instruction::Throw(tag_idx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypedAst;
    use crate::codegen::invariant_tests::{assert_internal, with_context};
    use crate::codegen::{SymbolTable, tests::mock_symbols_with_intrinsics};

    #[test]
    fn missing_exception_metadata_is_a_compile_failure() {
        for symbols in [SymbolTable::default(), mock_symbols_with_intrinsics()] {
            with_context(&TypedAst::new(), &symbols, |ctx| {
                emit_error_throw(&mut FunctionEmitter::new(ctx, &[]).unwrap(), ctx);
                assert_internal(ctx.check_failure().unwrap_err());
            });
        }
    }

    #[test]
    fn missing_type_error_constructor_is_a_compile_failure() {
        let mut symbols = mock_symbols_with_intrinsics();
        symbols.record_global(crate::mangle::prelude("string_vtable"), 0);
        let mut strings = crate::codegen::StringPool::default();
        strings.intern_text("test message");
        with_context(&TypedAst::new(), &symbols, |ctx| {
            let ctx = CodegenCtx {
                strings: &strings,
                failure: std::cell::Cell::new(None),
                validator_steps_left: std::cell::Cell::new(ctx.validator_steps_left.get()),
                validator_root: std::cell::Cell::new(None),
                check_is_standalone: std::cell::Cell::new(false),
                ..*ctx
            };
            emit_type_error_throw(
                &mut FunctionEmitter::new(&ctx, &[]).unwrap(),
                &ctx,
                "test message",
            );
            assert_internal(ctx.check_failure().unwrap_err());
        });
    }
}
