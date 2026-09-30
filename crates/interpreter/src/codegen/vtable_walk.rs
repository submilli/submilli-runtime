//! Exception-safe depth accounting around generated structural vtable bodies.

use wasm_encoder::{BlockType, Catch, Function, Instruction, ValType};

use super::internal_failure;
use super::symbol_table::SymbolTable;
use crate::compiler_error::CompilerFailure;

pub(super) fn guarded_body(
    body: u32,
    params: u32,
    result: ValType,
    symbols: &SymbolTable,
) -> Result<Function, CompilerFailure> {
    let enter = symbols
        .prelude_func_idx("vtable_walk_enter")
        .ok_or_else(|| internal_failure("the vtable walk guard is not imported"))?;
    let leave = symbols
        .prelude_func_idx("vtable_walk_leave")
        .ok_or_else(|| internal_failure("the vtable walk guard is not imported"))?;
    let mut f = Function::new([]);
    // An entry failure belongs to the caller's frame: do not decrement it here.
    f.instruction(&Instruction::Call(enter));
    f.instruction(&Instruction::Block(BlockType::Result(ValType::EXNREF)));
    f.instruction(&Instruction::TryTable(
        BlockType::Result(result),
        std::borrow::Cow::Owned(vec![Catch::AllRef { label: 0 }]),
    ));
    for param in 0..params {
        f.instruction(&Instruction::LocalGet(param));
    }
    f.instruction(&Instruction::Call(body));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::Call(leave));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::Call(leave));
    f.instruction(&Instruction::ThrowRef);
    f.instruction(&Instruction::End);
    Ok(f)
}
