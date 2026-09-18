//! Route completions through a single finally body outside its own handlers.

use wasm_encoder::{BlockType, Catch, Instruction, ValType};

use super::{FunctionEmitter, stmt};
use crate::codegen::CodegenCtx;
use crate::{StmtId, TypedCatchClause};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Transfer {
    Return,
    Branch { depth: u32, finally_floor: usize },
}

pub(super) struct FinallyFrame {
    entry_depth: u32,
    action: u32,
    result: Option<(u32, bool)>,
    transfers: Vec<Transfer>,
}

/// Return values arrive on the stack. Each frame owns its saved value, so a
/// nested try inside cleanup cannot overwrite a suspended outer return.
pub(super) fn emit_transfer(emitter: &mut FunctionEmitter, transfer: Transfer) {
    let floor = match transfer {
        Transfer::Return => 0,
        Transfer::Branch { finally_floor, .. } => finally_floor,
    };
    if emitter.finally_stack.len() > floor {
        let frame = emitter.finally_stack.last_mut().expect("pending finally");
        let index = if let Some(index) = frame.transfers.iter().position(|t| *t == transfer) {
            index
        } else {
            frame.transfers.push(transfer);
            frame.transfers.len() - 1
        };
        let (action, result, entry_depth) = (frame.action, frame.result, frame.entry_depth);
        if transfer == Transfer::Return
            && let Some((result, _)) = result
        {
            emitter.instruction(Instruction::LocalSet(result));
        }
        emitter.instruction(Instruction::I32Const(index as i32 + 1));
        emitter.instruction(Instruction::LocalSet(action));
        emitter.instruction(Instruction::Br(emitter.wasm_block_depth - entry_depth - 1));
        return;
    }
    match transfer {
        Transfer::Return => emitter.instruction(Instruction::Return),
        Transfer::Branch { depth, .. } => {
            emitter.instruction(Instruction::Br(emitter.wasm_block_depth - depth - 1));
        }
    }
}

pub(super) fn emit_try_finally(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    body: StmtId,
    catches: &[TypedCatchClause],
    finally: StmtId,
) {
    let action = emitter.add_anonymous_local(ValType::I32);
    let exception = emitter.add_anonymous_local(ValType::EXNREF);
    let result = emitter.wasm_result_type(ctx).map(|ty| {
        let (storage, non_null) = match ty {
            ValType::Ref(mut reference) => {
                let non_null = !reference.nullable;
                reference.nullable = true;
                (ValType::Ref(reference), non_null)
            }
            _ => (ty, false),
        };
        (emitter.add_anonymous_local(storage), non_null)
    });
    // This statement may execute repeatedly; normal entry resets its completion.
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalSet(action));
    emitter.emit_block(BlockType::Empty); // shared cleanup entry
    emitter.finally_stack.push(FinallyFrame {
        entry_depth: emitter.wasm_block_depth - 1,
        action,
        result,
        transfers: Vec::new(),
    });
    emitter.emit_block(BlockType::Result(ValType::EXNREF));
    emitter.instruction(Instruction::TryTable(
        BlockType::Empty,
        std::borrow::Cow::Owned(vec![Catch::AllRef { label: 0 }]),
    ));
    emitter.bump_block_depth();
    stmt::emit_try(emitter, ctx, body, catches, None);
    emitter.emit_end();
    emitter.instruction(Instruction::Br(1));
    emitter.emit_end();
    emitter.instruction(Instruction::LocalSet(exception));
    emitter.instruction(Instruction::I32Const(-1));
    emitter.instruction(Instruction::LocalSet(action));
    emitter.emit_end();
    let frame = emitter.finally_stack.pop().expect("finally frame");
    // Transfers or throws from cleanup supersede the pending completion and
    // can reach enclosing handlers, but never this try's own catch or finally.
    emitter.push_scope();
    stmt::emit_statement(emitter, ctx, finally);
    emitter.pop_scope();
    emit_completion_dispatch(emitter, frame, exception);
}

fn emit_completion_dispatch(emitter: &mut FunctionEmitter, frame: FinallyFrame, exception: u32) {
    emit_action_test(emitter, frame.action, -1);
    emitter.instruction(Instruction::LocalGet(exception));
    emitter.instruction(Instruction::ThrowRef);
    emitter.emit_end();
    for (index, transfer) in frame.transfers.into_iter().enumerate() {
        emit_action_test(emitter, frame.action, index as i32 + 1);
        if transfer == Transfer::Return
            && let Some((result, non_null)) = frame.result
        {
            emitter.instruction(Instruction::LocalGet(result));
            if non_null {
                emitter.instruction(Instruction::RefAsNonNull);
            }
        }
        emit_transfer(emitter, transfer);
        emitter.emit_end();
    }
}

fn emit_action_test(emitter: &mut FunctionEmitter, action: u32, expected: i32) {
    emitter.instruction(Instruction::LocalGet(action));
    emitter.instruction(Instruction::I32Const(expected));
    emitter.instruction(Instruction::I32Eq);
    emitter.emit_if(BlockType::Empty);
}
