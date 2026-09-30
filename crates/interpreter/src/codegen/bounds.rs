//! Bounds-checked array indexing.
//!
//! `arr[i]` lowers to a raw `array.get`/`array.set`, which traps uncatchably on
//! an out-of-range index. A trap halts the program and can't be intercepted by
//! user `try/catch`, turning a user-controlled index into a DoS vector. Instead
//! we range-check the index and `throw` a catchable `RangeError` on a miss,
//! matching how every other runtime fault surfaces (cast mismatch, etc.).

use wasm_encoder::{BlockType, Instruction, ValType};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::{FunctionEmitter, emit_const_string_by_text};

pub const INDEX_OOB_MESSAGE: &str = "index out of range";

/// Stashes the `f64` index sitting on the stack into a fresh local, ready for
/// [`emit_checked_index`]. Kept separate from the check because a write must
/// evaluate its RHS between the two — left-to-right evaluation order puts the
/// RHS's side effects before the check's throw.
pub fn stash_index_operand(emitter: &mut FunctionEmitter) -> u32 {
    let idx_f64_local = emitter.add_anonymous_local(ValType::F64);
    emitter.instruction(Instruction::LocalSet(idx_f64_local));
    idx_f64_local
}

/// Throws unless `idx >= 0 && idx < len && idx === Math.floor(idx)`, then returns
/// a fresh `i32` local holding the index truncated.
///
/// Checking the `f64` rather than the truncated `i32` is what makes the negative
/// and fractional halves of the domain reachable at all: `i32.trunc_sat_f64_u`
/// saturates `NaN`, `-1`, `-Infinity`, and `0.5` alike to `0`, an in-bounds slot,
/// so a post-truncation `>= len` test sees only indices past the end. Stack is
/// left untouched on the in-bounds path. NaN fails every comparison, so the
/// first conjunct alone rejects it; `-0` passes all three and truncates to `0`,
/// as in JS.
pub fn emit_checked_index(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    raw_arr_local: u32,
    idx_f64_local: u32,
) -> u32 {
    // idx >= 0
    emitter.instruction(Instruction::LocalGet(idx_f64_local));
    emitter.instruction(Instruction::F64Const(0.0.into()));
    emitter.instruction(Instruction::F64Ge);
    // && idx < len
    emitter.instruction(Instruction::LocalGet(idx_f64_local));
    emitter.instruction(Instruction::LocalGet(raw_arr_local));
    emitter.instruction(Instruction::ArrayLen);
    emitter.instruction(Instruction::F64ConvertI32U);
    emitter.instruction(Instruction::F64Lt);
    emitter.instruction(Instruction::I32And);
    // && idx === floor(idx)
    emitter.instruction(Instruction::LocalGet(idx_f64_local));
    emitter.instruction(Instruction::LocalGet(idx_f64_local));
    emitter.instruction(Instruction::F64Floor);
    emitter.instruction(Instruction::F64Eq);
    emitter.instruction(Instruction::I32And);
    // throw unless all three hold
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    emit_index_oob_throw(emitter, ctx);
    emitter.emit_end();

    let idx_local = emitter.add_anonymous_local(ValType::I32);
    emitter.instruction(Instruction::LocalGet(idx_f64_local));
    emitter.instruction(Instruction::I32TruncSatF64U);
    emitter.instruction(Instruction::LocalSet(idx_local));
    idx_local
}

fn emit_index_oob_throw(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    if ctx
        .latch(emit_const_string_by_text(emitter, ctx, INDEX_OOB_MESSAGE))
        .is_none()
    {
        return;
    }

    let new_idx = ctx
        .symbols
        .prelude_func_idx("RangeError#constructor")
        .expect("RangeError#constructor imported from prelude");
    emitter.instruction(Instruction::Call(new_idx));

    crate::codegen::throw::emit_error_throw(emitter, ctx);
}
