//! ECMAScript Number::remainder using binary long division of significands.
//!
//! No rounded floating-point division is involved. Integer subtraction and
//! shifts keep the exact remainder, including subnormals; the original sign
//! bit also preserves negative zero. All loops execute as fuel-metered Wasm.
use wasm_encoder::{BlockType, Function, Instruction, ValType};

const NUMERATOR: u32 = 0;
const DENOMINATOR: u32 = 1;
const SIGN: u32 = 2;
const LEFT: u32 = 3;
const RIGHT: u32 = 4;
const LEFT_EXP: u32 = 5;
const RIGHT_EXP: u32 = 6;
const SHIFT: u32 = 7;
const HIDDEN_BIT: i64 = 1 << 52;
const FRACTION_MASK: i64 = HIDDEN_BIT - 1;
const INFINITY_BITS: i64 = 0x7ff0_0000_0000_0000;

pub(super) fn body() -> Function {
    use Instruction::*;
    let mut function = Function::new([(6, ValType::I64)]);
    emit(
        &mut function,
        [
            LocalGet(NUMERATOR),
            I64ReinterpretF64,
            I64Const(i64::MIN),
            I64And,
            LocalSet(SIGN),
            LocalGet(NUMERATOR),
            F64Abs,
            I64ReinterpretF64,
            LocalSet(LEFT),
            LocalGet(DENOMINATOR),
            F64Abs,
            I64ReinterpretF64,
            LocalSet(RIGHT),
            // NaN operands, an infinite numerator, or a zero denominator.
            LocalGet(LEFT),
            I64Const(INFINITY_BITS),
            I64GeU,
            LocalGet(RIGHT),
            I64Const(INFINITY_BITS),
            I64GtU,
            I32Or,
            LocalGet(RIGHT),
            I64Eqz,
            I32Or,
            If(BlockType::Empty),
            F64Const(f64::NAN.into()),
            Return,
            End,
            // Includes finite % infinity and either signed zero % nonzero.
            LocalGet(LEFT),
            LocalGet(RIGHT),
            I64LtU,
            If(BlockType::Empty),
            LocalGet(NUMERATOR),
            Return,
            End,
        ],
    );
    normalize(&mut function, LEFT, LEFT_EXP);
    normalize(&mut function, RIGHT, RIGHT_EXP);
    emit(
        &mut function,
        [
            Block(BlockType::Empty),
            Loop(BlockType::Empty),
            LocalGet(LEFT_EXP),
            LocalGet(RIGHT_EXP),
            I64LeS,
            BrIf(1),
        ],
    );
    subtract_divisor(&mut function);
    emit(
        &mut function,
        [
            LocalGet(LEFT),
            I64Const(1),
            I64Shl,
            LocalSet(LEFT),
            LocalGet(LEFT_EXP),
            I64Const(1),
            I64Sub,
            LocalSet(LEFT_EXP),
            Br(0),
            End,
            End,
        ],
    );
    subtract_divisor(&mut function);
    emit(
        &mut function,
        [
            LocalGet(LEFT),
            I64Eqz,
            If(BlockType::Empty),
            LocalGet(SIGN),
            F64ReinterpretI64,
            Return,
            End,
            // The remainder is at most 53 bits. Normalize it back to bit 52.
            LocalGet(LEFT),
            I64Clz,
            I64Const(11),
            I64Sub,
            LocalSet(SHIFT),
            LocalGet(LEFT),
            LocalGet(SHIFT),
            I64Shl,
            LocalSet(LEFT),
            LocalGet(LEFT_EXP),
            LocalGet(SHIFT),
            I64Sub,
            LocalSet(LEFT_EXP),
            LocalGet(LEFT_EXP),
            I64Const(0),
            I64GtS,
            If(BlockType::Result(ValType::I64)),
            LocalGet(LEFT),
            I64Const(FRACTION_MASK),
            I64And,
            LocalGet(LEFT_EXP),
            I64Const(52),
            I64Shl,
            I64Or,
            Else,
            // Exact subtraction of binary64 operands remains a multiple of 2^-1074.
            // Thus the subnormal shift is at most 52 and discards only zero bits.
            LocalGet(LEFT),
            I64Const(1),
            LocalGet(LEFT_EXP),
            I64Sub,
            I64ShrU,
            End,
            LocalGet(SIGN),
            I64Or,
            F64ReinterpretI64,
            End,
        ],
    );
    function
}

/// Represent magnitude as a 53-bit significand times 2^(exponent - 1075).
fn normalize(function: &mut Function, significand: u32, exponent: u32) {
    use Instruction::*;
    emit(
        function,
        [
            LocalGet(significand),
            I64Const(52),
            I64ShrU,
            LocalTee(exponent),
            I64Eqz,
            If(BlockType::Empty),
            LocalGet(significand),
            I64Clz,
            I64Const(11),
            I64Sub,
            LocalSet(SHIFT),
            LocalGet(significand),
            LocalGet(SHIFT),
            I64Shl,
            LocalSet(significand),
            I64Const(1),
            LocalGet(SHIFT),
            I64Sub,
            LocalSet(exponent),
            Else,
            LocalGet(significand),
            I64Const(FRACTION_MASK),
            I64And,
            I64Const(HIDDEN_BIT),
            I64Or,
            LocalSet(significand),
            End,
        ],
    );
}

fn subtract_divisor(function: &mut Function) {
    use Instruction::*;
    emit(
        function,
        [
            LocalGet(LEFT),
            LocalGet(RIGHT),
            I64GeU,
            If(BlockType::Empty),
            LocalGet(LEFT),
            LocalGet(RIGHT),
            I64Sub,
            LocalSet(LEFT),
            End,
        ],
    );
}

fn emit<const N: usize>(function: &mut Function, instructions: [Instruction<'_>; N]) {
    for instruction in instructions {
        function.instruction(&instruction);
    }
}
