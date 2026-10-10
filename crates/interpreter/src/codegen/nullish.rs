//! Inline tests for the language's absent values.
//!
//! `null` is a Wasm null reference and `undefined` is the store's single
//! `$undefined` struct, so both tests are plain instructions, with no host call.
//! Each builder takes the instructions that push the tested reference and runs
//! them once per alternative, so they must be a pure read such as `local.get`.
//! The sequences suit both a raw [`wasm_encoder::Function`] and the emitter.

use wasm_encoder::{HeapType, Instruction};

use super::intrinsics::IntrinsicTypeIndices;

/// `[] -> [i32]`: whether `load` pushes `null` or `undefined`.
pub(crate) fn is_nullish<'a>(
    load: &[Instruction<'a>],
    intrinsics: IntrinsicTypeIndices,
) -> Vec<Instruction<'a>> {
    either(
        load,
        Instruction::RefIsNull,
        Instruction::RefTestNonNull(HeapType::Concrete(intrinsics.undefined)),
    )
}

/// `[] -> [i32]`: whether `load` pushes `undefined` or a closure, the values
/// `JSON.stringify` omits.
pub(crate) fn is_undefined_or_closure<'a>(
    load: &[Instruction<'a>],
    intrinsics: IntrinsicTypeIndices,
) -> Vec<Instruction<'a>> {
    either(
        load,
        Instruction::RefTestNonNull(HeapType::Concrete(intrinsics.undefined)),
        Instruction::RefTestNonNull(HeapType::Concrete(intrinsics.closure)),
    )
}

fn either<'a>(
    load: &[Instruction<'a>],
    first: Instruction<'a>,
    second: Instruction<'a>,
) -> Vec<Instruction<'a>> {
    let mut sequence = Vec::with_capacity(load.len().saturating_mul(2).saturating_add(3));
    sequence.extend_from_slice(load);
    sequence.push(first);
    sequence.extend_from_slice(load);
    sequence.push(second);
    sequence.push(Instruction::I32Or);
    sequence
}
