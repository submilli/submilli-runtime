//! Exception-safe depth accounting around generated structural vtable bodies.

use wasm_encoder::{BlockType, Catch, Function, Instruction, ValType};

use super::symbol_table::SymbolTable;

pub(super) fn guarded_body(
    body: u32,
    params: u32,
    result: ValType,
    symbols: &SymbolTable,
) -> Function {
    let enter = symbols
        .prelude_func_idx("vtable_walk_enter")
        .expect("walk guard imported");
    let leave = symbols
        .prelude_func_idx("vtable_walk_leave")
        .expect("walk guard imported");
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
    f
}
