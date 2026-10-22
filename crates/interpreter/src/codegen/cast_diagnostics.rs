//! Tracks failing locations during the existing validation walk.
//! No value is re-read to explain a failure, and diagnostics never copy payloads:
//! the innermost failing test keeps a reference to the value it tested and the
//! name of the type it expected there, and only a thrown failure renders them.

use super::{CodegenCtx, function_emitter::FunctionEmitter};
use crate::compiler_error::CompilerFailure;
use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

// The recursive validator's visited array holds two cells per active frame.
pub(super) const PATH_SLOT: i32 = super::cast_check::RECURSIVE_VALIDATOR_CAPACITY * 2;
pub(super) const FAILURE_SLOT: i32 = PATH_SLOT + 1;
const VALUE_SLOT: i32 = PATH_SLOT + 2;
const EXPECTED_SLOT: i32 = PATH_SLOT + 3;
/// Non-null when the failure's tier is [`TAG_MISS`] or [`IN_MEMBER`]: array
/// cells hold references, so each tier bit is a null or non-null cell.
const TAG_MISS_SLOT: i32 = PATH_SLOT + 4;
const IN_MEMBER_SLOT: i32 = PATH_SLOT + 5;
/// Cells after the recursion frames that carry the diagnostic across a call.
pub(super) const SLOTS: i32 = 6;

/// How well a failure explains why a union member rejected a value, lowest
/// first. A member that rejects the value itself, as `null` rejects an object,
/// says least; one whose own tag field mismatched is most likely another member;
/// any other failure inside a member is the one to report.
const AT_UNION: i32 = 0;
const TAG_MISS: i32 = 1;
const IN_MEMBER: i32 = 2;
/// The tier sits above any path length in a failure's rank.
const TIER_SHIFT: i32 = 29;

/// Longer type names, such as an inline object type, are named by kind, so a
/// check site does not carry a long literal for each nested value it tests.
const MAX_EXPECTED_LEN: usize = 48;

/// UTF-16 units of a field name in an emitted path charged as one more step.
const PATH_UNITS_PER_STEP: usize = 64;

#[derive(Clone)]
pub(super) struct Locals {
    current: u32,
    failure: Failure,
    key: Option<u32>,
    path: Vec<Segment>,
    /// Union locals by nesting depth: sibling unions reuse them, so a large
    /// type does not cost locals for each union it contains.
    unions: Vec<UnionLocals>,
    /// The path length of each union being checked, innermost last.
    open_unions: Vec<usize>,
}

/// What a test that just failed was checking.
#[derive(Clone, Copy)]
pub(super) enum Tested<'a> {
    /// The value in a local, against `ty`.
    Value { ty: &'a crate::Type, local: u32 },
    /// A required field that is not there.
    Absent { ty: &'a crate::Type },
}

/// Where a check failed and what its innermost failing test saw. Only `path`
/// says whether there is a failure; the rest is meaningful once it is set.
#[derive(Clone, Copy)]
struct Failure {
    path: u32,
    /// The value the test read.
    value: u32,
    /// The name of the type the test expected.
    expected: u32,
    /// An i32 tier: [`AT_UNION`], [`TAG_MISS`] or [`IN_MEMBER`].
    tier: u32,
}

/// A union's best failure so far, and the enclosing failure, which the union's
/// alternatives overwrite while each starts over.
#[derive(Clone, Copy)]
pub(super) struct UnionLocals {
    result: u32,
    best: Failure,
    outer: Failure,
}

#[derive(Clone)]
enum Segment {
    Field(String),
    Index(u32),
    /// A record's entry, by its position among the record's values.
    RecordKey {
        keys: RecordKeys,
        position: u32,
    },
}

/// Where a record check finds the keys that name its entries: the record, its
/// values array, and a cache the first failure fills with the quoted keys, so
/// later failures in the same record do not walk it again.
#[derive(Clone, Copy)]
pub(super) struct RecordKeys {
    object: u32,
    values: u32,
    cache: u32,
}

/// Scalar checks retain their established root-only message and need no paths.
pub(super) fn has_nested_paths(ty: &crate::Type) -> bool {
    use crate::Type;
    match ty.peel() {
        Type::Union(members) => members.iter().any(has_nested_paths),
        Type::Number
        | Type::NumberLiteral(_)
        | Type::NumberEnum { .. }
        | Type::String
        | Type::StringLiteral(_)
        | Type::StringEnum { .. }
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::BigInt
        | Type::BigIntLiteral(_)
        | Type::Uint8Array
        | Type::Null
        | Type::Undefined
        | Type::Void
        | Type::Unknown
        | Type::Never
        | Type::Error
        | Type::Function { .. } => false,
        _ => true,
    }
}

pub(super) fn begin(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) -> Option<Locals> {
    let previous = emitter.cast_diagnostic.take();
    let current = ctx.latch(emitter.add_anonymous_local(ctx.latch(string_slot(ctx))?))?;
    let failure = ctx.latch(add_failure(emitter, ctx))?;
    super::cast_check::emit_inline_string(emitter, ctx, "$");
    emitter.instruction(Instruction::LocalSet(current));
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(
        ctx.latch(string_index(ctx))?,
    )));
    emitter.instruction(Instruction::LocalSet(failure.path));
    emitter.cast_diagnostic = Some(Locals {
        current,
        failure,
        key: None,
        path: Vec::new(),
        unions: Vec::new(),
        open_unions: Vec::new(),
    });
    previous
}

fn add_failure(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<Failure, CompilerFailure> {
    Ok(Failure {
        path: emitter.add_anonymous_local(string_slot(ctx)?)?,
        value: emitter.add_anonymous_local(object_slot(ctx)?)?,
        expected: emitter.add_anonymous_local(string_slot(ctx)?)?,
        tier: emitter.add_anonymous_local(ValType::I32)?,
    })
}

fn copy_failure(emitter: &mut FunctionEmitter, from: Failure, to: Failure) {
    for (from, to) in [
        (from.path, to.path),
        (from.value, to.value),
        (from.expected, to.expected),
        (from.tier, to.tier),
    ] {
        emitter.instruction(Instruction::LocalGet(from));
        emitter.instruction(Instruction::LocalSet(to));
    }
}

pub(super) fn session_key(emitter: &mut FunctionEmitter, key: Option<u32>) {
    if let Some(state) = &mut emitter.cast_diagnostic {
        state.key = key;
    }
}

pub(super) fn checkpoint(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) -> Option<u32> {
    let state = emitter.cast_diagnostic.clone()?;
    let saved = ctx.latch(emitter.add_anonymous_local(ctx.latch(string_slot(ctx))?))?;
    emitter.instruction(Instruction::LocalGet(state.failure.path));
    emitter.instruction(Instruction::LocalSet(saved));
    Some(saved)
}

/// Consume and reproduce a conformance bit. Successful alternatives discard
/// their speculative failures; a failing parent preserves its child's location
/// and what the child expected there.
pub(super) fn finish(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    saved: Option<u32>,
    tested: Tested<'_>,
) -> Result<(), CompilerFailure> {
    let (Some(state), Some(saved)) = (emitter.cast_diagnostic.clone(), saved) else {
        return Ok(());
    };
    let result = emitter.add_anonymous_local(ValType::I32)?;
    emitter.instruction(Instruction::LocalTee(result));
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(saved));
    emitter.instruction(Instruction::LocalSet(state.failure.path));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(state.failure.path));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emit_path(emitter, ctx, &state)?;
    emitter.instruction(Instruction::LocalSet(state.failure.path));
    record_tested(emitter, ctx, &state, tested)?;
    emitter.emit_end();
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(result));
    Ok(())
}

fn record_tested(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    state: &Locals,
    tested: Tested<'_>,
) -> Result<(), CompilerFailure> {
    let ty = match tested {
        Tested::Value { ty, local } => {
            emitter.instruction(Instruction::LocalGet(local));
            ty
        }
        Tested::Absent { ty } => {
            crate::codegen::function_emitter::expr::emit_undefined(emitter, ctx)?;
            ty
        }
    };
    emitter.instruction(Instruction::LocalSet(state.failure.value));
    super::cast_check::emit_inline_string(emitter, ctx, &expected_name(ty));
    emitter.instruction(Instruction::LocalSet(state.failure.expected));
    emitter.instruction(Instruction::I32Const(tier(state, ty)));
    emitter.instruction(Instruction::LocalSet(state.failure.tier));
    Ok(())
}

/// The tier of a failure at the current path, relative to the innermost union
/// being checked. Without one, a recursive validator's own value is taken as a
/// union member, and [`load_recursive`] corrects the tier where it is deeper.
fn tier(state: &Locals, ty: &crate::Type) -> i32 {
    match state.path.len().checked_sub(innermost_union_path(state)) {
        None | Some(0) => AT_UNION,
        Some(1) if is_discriminant(ty) => TAG_MISS,
        Some(_) => IN_MEMBER,
    }
}

/// The path length of the innermost union being checked, or of the validated
/// value itself when there is none.
fn innermost_union_path(state: &Locals) -> usize {
    state.open_unions.last().copied().unwrap_or(0)
}

/// Whether the current path lies below the innermost union's value, so a
/// failure recorded deeper, by a nested union or a callee, is inside one of
/// that union's members.
fn inside_member(state: &Locals) -> bool {
    state.path.len() > innermost_union_path(state)
}

/// Fields in check order: a value of another discriminated union member then
/// fails at its tag, a [`TAG_MISS`], rather than at another of its fields.
/// Conformance tests every field either way, so the order changes no result.
pub(super) fn discriminants_first(
    fields: &std::collections::BTreeMap<String, crate::ObjectField>,
) -> impl Iterator<Item = (&String, &crate::ObjectField)> {
    let (tags, rest): (Vec<_>, Vec<_>) = fields
        .iter()
        .partition(|(_, field)| is_discriminant(&field.ty));
    tags.into_iter().chain(rest)
}

/// A literal, or a union of literals, as a discriminated union's tag field is.
fn is_discriminant(ty: &crate::Type) -> bool {
    use crate::Type;
    match ty.peel() {
        Type::Union(members) => members.iter().all(is_discriminant),
        Type::StringLiteral(_)
        | Type::NumberLiteral(_)
        | Type::BooleanLiteral(_)
        | Type::BigIntLiteral(_) => true,
        Type::StringEnum { member, .. } => member.is_some(),
        Type::NumberEnum { member, .. } => member.is_some(),
        _ => false,
    }
}

/// The type a failure names as expected: as written when short, else its kind.
fn expected_name(ty: &crate::Type) -> String {
    use crate::Type;
    let written = ty.to_string();
    if written.chars().count() <= MAX_EXPECTED_LEN {
        return written;
    }
    if is_discriminant(ty) {
        return "a literal".to_string();
    }
    match ty.peel() {
        Type::Array(_) | Type::Tuple(_) => "an array",
        Type::Union(_) => "a union",
        Type::Function { .. } => "a function",
        _ => "an object",
    }
    .to_string()
}

/// Each union alternative starts with an independent failure. When all
/// alternatives fail, prefer the highest tier, then a longer path, so a tag or
/// null mismatch does not hide a nested field failure. Ties retain the first
/// alternative's explanation.
pub(super) fn union_start(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) -> Option<UnionLocals> {
    let state = emitter.cast_diagnostic.clone()?;
    let union = if let Some(union) = state.unions.get(state.open_unions.len()) {
        *union
    } else {
        let union = UnionLocals {
            result: ctx.latch(emitter.add_anonymous_local(ValType::I32))?,
            best: ctx.latch(add_failure(emitter, ctx))?,
            outer: ctx.latch(add_failure(emitter, ctx))?,
        };
        emitter.cast_diagnostic.as_mut()?.unions.push(union);
        union
    };
    emitter
        .cast_diagnostic
        .as_mut()?
        .open_unions
        .push(state.path.len());
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(
        ctx.latch(string_index(ctx))?,
    )));
    emitter.instruction(Instruction::LocalSet(union.best.path));
    copy_failure(emitter, state.failure, union.outer);
    Some(union)
}

pub(super) fn union_next(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(union_next_checked(emitter, ctx));
}

fn union_next_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(string_index(ctx)?)));
    emitter.instruction(Instruction::LocalSet(state.failure.path));

    Ok(())
}

pub(super) fn union_keep(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    union: Option<UnionLocals>,
) {
    let (Some(state), Some(union)) = (emitter.cast_diagnostic.clone(), union) else {
        return;
    };
    failure_rank(emitter, ctx, state.failure);
    failure_rank(emitter, ctx, union.best);
    emitter.instruction(Instruction::I32GtU);
    emitter.emit_if(BlockType::Empty);
    copy_failure(emitter, state.failure, union.best);
    emitter.emit_end();
}

/// How well a failure explains a union's rejection: none ranks lowest, then
/// by tier, then by path length. Paths stay far below `1 << TIER_SHIFT` units.
fn failure_rank(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, failure: Failure) {
    path_length(emitter, ctx, failure.path);
    emitter.instruction(Instruction::I32Const(0));
    emitter.instruction(Instruction::LocalGet(failure.tier));
    emitter.instruction(Instruction::I32Const(TIER_SHIFT));
    emitter.instruction(Instruction::I32Shl);
    emitter.instruction(Instruction::LocalGet(failure.path));
    emitter.instruction(Instruction::RefIsNull);
    emitter.instruction(Instruction::Select);
    emitter.instruction(Instruction::I32Or);
}

/// Settle the union's failure, consuming and reproducing its conformance bit.
/// When every alternative rejected the union's own value, the union as a
/// whole is what was expected there, `string | null` rather than its first
/// member: clearing the failure lets the union's own `finish` record it. A
/// failure kept from a union nested below the enclosing union's value lies
/// inside one of that union's members, and ranks there as such.
pub(super) fn union_end(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    union: Option<UnionLocals>,
) -> Result<(), CompilerFailure> {
    let (Some(state), Some(union)) = (emitter.cast_diagnostic.as_mut(), union) else {
        return Ok(());
    };
    state
        .open_unions
        .pop()
        .ok_or_else(|| super::internal_failure("a union check ended without starting"))?;
    let state = state.clone();
    emitter.instruction(Instruction::LocalSet(union.result));
    emitter.instruction(Instruction::LocalGet(union.outer.path));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    copy_failure(emitter, union.best, state.failure);
    emitter.instruction(Instruction::LocalGet(union.result));
    emitter.instruction(Instruction::I32Eqz);
    emitter.instruction(Instruction::LocalGet(state.failure.tier));
    emitter.instruction(Instruction::I32Const(AT_UNION));
    emitter.instruction(Instruction::I32Eq);
    emitter.instruction(Instruction::I32And);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(string_index(ctx)?)));
    emitter.instruction(Instruction::LocalSet(state.failure.path));
    if inside_member(&state) {
        emitter.emit_else();
        emitter.instruction(Instruction::I32Const(IN_MEMBER));
        emitter.instruction(Instruction::LocalSet(state.failure.tier));
    }
    emitter.emit_end();
    emitter.emit_else();
    copy_failure(emitter, union.outer, state.failure);
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(union.result));
    Ok(())
}

fn path_length(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, local: u32) {
    ctx.latch(path_length_checked(emitter, ctx, local));
}

fn path_length_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    local: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::StructGet {
        struct_type_index: string_index(ctx)?,
        field_index: 1,
    });
    emitter.instruction(Instruction::ArrayLen);
    emitter.emit_end();

    Ok(())
}

pub(super) fn field(emitter: &mut FunctionEmitter, _ctx: &CodegenCtx, name: &str) -> Option<usize> {
    let state = emitter.cast_diagnostic.as_mut()?;
    let mark = state.path.len();
    state.path.push(Segment::Field(name.into()));
    Some(mark)
}

pub(super) fn index(emitter: &mut FunctionEmitter, _ctx: &CodegenCtx, index: u32) -> Option<usize> {
    let state = emitter.cast_diagnostic.as_mut()?;
    let mark = state.path.len();
    state.path.push(Segment::Index(index));
    Some(mark)
}

/// Starts the key cache of the record in `object`, whose values are in
/// `values`. Each check of a record starts empty.
pub(super) fn record_keys(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    object: u32,
    values: u32,
) -> Option<RecordKeys> {
    emitter.cast_diagnostic.as_ref()?;
    let array = ctx.require(ctx.symbols.array_type_idx(), "$Array intrinsic registered")?;
    let cache = ctx.latch(emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(array),
    })))?;
    emitter.instruction(Instruction::RefNull(HeapType::Concrete(array)));
    emitter.instruction(Instruction::LocalSet(cache));
    Some(RecordKeys {
        object,
        values,
        cache,
    })
}

pub(super) fn record_key(
    emitter: &mut FunctionEmitter,
    keys: RecordKeys,
    position: u32,
) -> Option<usize> {
    let state = emitter.cast_diagnostic.as_mut()?;
    let mark = state.path.len();
    state.path.push(Segment::RecordKey { keys, position });
    Some(mark)
}

pub(super) fn pop(emitter: &mut FunctionEmitter, mark: Option<usize>) {
    if let (Some(state), Some(mark)) = (emitter.cast_diagnostic.as_mut(), mark) {
        state.path.truncate(mark);
    }
}

/// Inline paths are assembled only when a check fails. Index locals still
/// contain the failing iteration's index when this code runs.
///
/// Every test emits the path to the value it tests, so a check emits its
/// nesting depth in segments per test. Each segment is charged to the check as
/// a step, and a field name as one more per [`PATH_UNITS_PER_STEP`] UTF-16
/// units, since each unit is an instruction.
fn emit_path(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    state: &Locals,
) -> Result<(), CompilerFailure> {
    emitter.instruction(Instruction::LocalGet(state.current));
    for segment in &state.path {
        match segment {
            Segment::Field(name) => {
                let units = name.encode_utf16().count() / PATH_UNITS_PER_STEP;
                let name_steps = u64::try_from(units).unwrap_or(u64::MAX);
                ctx.charge_validator_steps(emitter, name_steps.saturating_add(1))?;
                let quoted = serde_json::to_string(name).map_err(|error| {
                    super::internal_failure(format!("cannot quote a field name: {error}"))
                })?;
                super::cast_check::emit_inline_string(emitter, ctx, &format!("[{quoted}]"));
                concat(emitter, ctx);
            }
            Segment::Index(index) => {
                ctx.charge_validator_step(emitter)?;
                super::cast_check::emit_inline_string(emitter, ctx, "[");
                concat(emitter, ctx);
                emitter.instruction(Instruction::LocalGet(*index));
                emitter.instruction(Instruction::F64ConvertI32U);
                emitter.instruction(Instruction::F64Const(10.0_f64.into()));
                let number_to_string =
                    ctx.symbols
                        .prelude_func_idx("Number#toString")
                        .ok_or_else(|| {
                            super::internal_failure(
                                "Number#toString is not imported from the prelude",
                            )
                        })?;
                emitter.instruction(Instruction::Call(number_to_string));
                concat(emitter, ctx);
                super::cast_check::emit_inline_string(emitter, ctx, "]");
                concat(emitter, ctx);
            }
            Segment::RecordKey { keys, position } => {
                ctx.charge_validator_step(emitter)?;
                super::cast_check::emit_inline_string(emitter, ctx, "[");
                concat(emitter, ctx);
                emit_record_key(emitter, ctx, *keys, *position)?;
                concat(emitter, ctx);
                super::cast_check::emit_inline_string(emitter, ctx, "]");
                concat(emitter, ctx);
            }
        }
    }
    Ok(())
}

/// Pushes the quoted key at `position`, filling the record's key cache first
/// if this is the record's first failure.
fn emit_record_key(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    keys: RecordKeys,
    position: u32,
) -> Result<(), CompilerFailure> {
    let symbols = &ctx.symbols;
    let array = symbols
        .array_type_idx()
        .ok_or_else(|| super::internal_failure("$Array intrinsic registered"))?;
    let raw_array = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| super::internal_failure("intrinsic types are not declared"))?
        .raw_array;
    let quoted_keys = symbols
        .prelude_func_idx("ObjectConstructor##quotedRecordKeys")
        .ok_or_else(|| super::internal_failure("the record keys helper is not imported"))?;
    emitter.instruction(Instruction::LocalGet(keys.cache));
    emitter.instruction(Instruction::RefIsNull);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(keys.object));
    emitter.instruction(Instruction::LocalGet(keys.values));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(array)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: array,
        field_index: 2,
    });
    emitter.instruction(Instruction::F64ConvertI32U);
    emitter.instruction(Instruction::Call(quoted_keys));
    emitter.instruction(Instruction::RefCastNullable(HeapType::Concrete(array)));
    emitter.instruction(Instruction::LocalSet(keys.cache));
    emitter.emit_end();
    emitter.instruction(Instruction::LocalGet(keys.cache));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::StructGet {
        struct_type_index: array,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalGet(position));
    emitter.instruction(Instruction::ArrayGet(raw_array));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
        string_index(ctx)?,
    )));
    Ok(())
}

pub(super) fn append_failure(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(append_failure_checked(emitter, ctx));
}

fn append_failure_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    let Some(message) = ctx.latch(emitter.add_anonymous_local(string_slot(ctx)?)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(message));
    // A root mismatch keeps the established scalar diagnostic unchanged.
    path_length(emitter, ctx, state.failure.path);
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::I32GtU);
    emitter.emit_if(BlockType::Result(string_slot(ctx)?));
    emitter.instruction(Instruction::LocalGet(message));
    super::cast_check::emit_inline_string(emitter, ctx, " at ");
    concat(emitter, ctx);
    emitter.instruction(Instruction::LocalGet(state.failure.path));
    concat(emitter, ctx);
    super::cast_check::emit_inline_string(emitter, ctx, " (expected ");
    concat(emitter, ctx);
    emitter.instruction(Instruction::LocalGet(state.failure.expected));
    concat(emitter, ctx);
    super::cast_check::emit_inline_string(emitter, ctx, ", got ");
    concat(emitter, ctx);
    super::cast_check::emit_runtime_type_string(emitter, ctx, state.failure.value);
    concat(emitter, ctx);
    super::cast_check::emit_inline_string(emitter, ctx, ")");
    concat(emitter, ctx);
    emitter.emit_else();
    emitter.instruction(Instruction::LocalGet(message));
    emitter.emit_end();
    emitter.instruction(Instruction::RefAsNonNull);
    if let Some(key) = state.key {
        super::cast_check::emit_inline_string(emitter, ctx, " for session key ");
        concat(emitter, ctx);
        emitter.instruction(Instruction::LocalGet(key));
        concat(emitter, ctx);
    }

    Ok(())
}

pub(super) fn save_recursive(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    visited: u32,
) -> Result<(), CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    let array = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| super::internal_failure("intrinsic types are not declared"))?
        .raw_array;
    for (index, local) in [
        (PATH_SLOT, state.current),
        (FAILURE_SLOT, state.failure.path),
        (VALUE_SLOT, state.failure.value),
        (EXPECTED_SLOT, state.failure.expected),
    ] {
        emitter.instruction(Instruction::LocalGet(visited));
        emitter.instruction(Instruction::I32Const(index));
        if index == PATH_SLOT {
            emit_path(emitter, ctx, &state)?;
        } else {
            emitter.instruction(Instruction::LocalGet(local));
        }
        emitter.instruction(Instruction::ArraySet(array));
    }
    for (index, tier) in [(TAG_MISS_SLOT, TAG_MISS), (IN_MEMBER_SLOT, IN_MEMBER)] {
        emitter.instruction(Instruction::LocalGet(visited));
        emitter.instruction(Instruction::I32Const(index));
        emitter.instruction(Instruction::LocalGet(state.failure.path));
        emitter.instruction(Instruction::RefNull(HeapType::Concrete(string_index(ctx)?)));
        emitter.instruction(Instruction::LocalGet(state.failure.tier));
        emitter.instruction(Instruction::I32Const(tier));
        emitter.instruction(Instruction::I32Eq);
        emitter.instruction(Instruction::TypedSelect(string_slot(ctx)?));
        emitter.instruction(Instruction::ArraySet(array));
    }
    Ok(())
}

pub(super) fn load_recursive(emitter: &mut FunctionEmitter, ctx: &CodegenCtx, visited: u32) {
    ctx.latch(load_recursive_checked(emitter, ctx, visited));
}

fn load_recursive_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    visited: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(state) = emitter.cast_diagnostic.clone() else {
        return Ok(());
    };
    let array = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .raw_array;
    // Only a failure the callee recorded takes a new tier; one this frame
    // already had comes back as the same path and keeps its own.
    emitter.instruction(Instruction::LocalGet(visited));
    emitter.instruction(Instruction::I32Const(FAILURE_SLOT));
    emitter.instruction(Instruction::ArrayGet(array));
    emitter.instruction(Instruction::LocalGet(state.failure.path));
    emitter.instruction(Instruction::RefEq);
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    if inside_member(&state) {
        emitter.instruction(Instruction::I32Const(IN_MEMBER));
    } else {
        emitter.instruction(Instruction::I32Const(AT_UNION));
        for (index, tier) in [(TAG_MISS_SLOT, TAG_MISS), (IN_MEMBER_SLOT, IN_MEMBER)] {
            emitter.instruction(Instruction::I32Const(tier));
            emitter.instruction(Instruction::LocalGet(visited));
            emitter.instruction(Instruction::I32Const(index));
            emitter.instruction(Instruction::ArrayGet(array));
            emitter.instruction(Instruction::RefIsNull);
            emitter.instruction(Instruction::Select);
        }
    }
    emitter.instruction(Instruction::LocalSet(state.failure.tier));
    emitter.emit_end();
    for (index, local, heap) in [
        (FAILURE_SLOT, state.failure.path, string_index(ctx)?),
        (VALUE_SLOT, state.failure.value, object_index(ctx)?),
        (EXPECTED_SLOT, state.failure.expected, string_index(ctx)?),
    ] {
        emitter.instruction(Instruction::LocalGet(visited));
        emitter.instruction(Instruction::I32Const(index));
        emitter.instruction(Instruction::ArrayGet(array));
        emitter.instruction(Instruction::RefCastNullable(HeapType::Concrete(heap)));
        emitter.instruction(Instruction::LocalSet(local));
    }

    Ok(())
}

pub(super) fn enter_recursive(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(enter_recursive_checked(emitter, ctx));
}

fn enter_recursive_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    begin(emitter, ctx);
    let state = emitter
        .cast_diagnostic
        .clone()
        .ok_or_else(|| crate::codegen::internal_failure("validator diagnostic"))?;
    let array = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .raw_array;
    emitter.instruction(Instruction::LocalGet(1));
    emitter.instruction(Instruction::I32Const(PATH_SLOT));
    emitter.instruction(Instruction::ArrayGet(array));
    emitter.instruction(Instruction::RefCastNullable(HeapType::Concrete(
        string_index(ctx)?,
    )));
    let Some(incoming) = ctx.latch(emitter.add_anonymous_local(string_slot(ctx)?)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalTee(incoming));
    emitter.instruction(Instruction::RefIsNull);
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(incoming));
    emitter.instruction(Instruction::LocalSet(state.current));
    emitter.emit_end();
    load_recursive(emitter, ctx, 1);

    Ok(())
}

pub(super) fn concat(emitter: &mut FunctionEmitter, ctx: &CodegenCtx) {
    ctx.latch(concat_checked(emitter, ctx));
}

fn concat_checked(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let Some(right) = ctx.latch(emitter.add_anonymous_local(string_slot(ctx)?)) else {
        return Ok(());
    };
    emitter.instruction(Instruction::LocalSet(right));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::LocalGet(right));
    emitter.instruction(Instruction::RefAsNonNull);
    emitter.instruction(Instruction::Call(
        ctx.symbols
            .prelude_func_idx("string_concat")
            .ok_or_else(|| crate::codegen::internal_failure("string concat"))?,
    ));

    Ok(())
}

fn string_index(ctx: &CodegenCtx) -> Result<u32, crate::compiler_error::CompilerFailure> {
    ctx.symbols
        .string_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("string"))
}
fn object_index(ctx: &CodegenCtx) -> Result<u32, crate::compiler_error::CompilerFailure> {
    Ok(ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .object)
}
fn object_slot(ctx: &CodegenCtx) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(object_index(ctx)?),
    }))
}
fn string_slot(ctx: &CodegenCtx) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(string_index(ctx)?),
    }))
}
