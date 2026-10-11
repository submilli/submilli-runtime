//! Captured bindings keep initialization state separate from their payload.

use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

use super::{FunctionEmitter, emit_const_string_by_text};
use crate::codegen::{CodegenCtx, internal_failure};
use crate::compiler_error::CompilerFailure;
use crate::{StmtId, TypedStmtKind};

pub(in crate::codegen) const UNINITIALIZED_PARAMETER_MESSAGE: &str =
    "Cannot access parameter before initialization";

impl FunctionEmitter<'_> {
    /// Reserve captured pattern bindings before an earlier default can capture
    /// them. Their declarations later initialize these same cells.
    pub(crate) fn prepare_parameter_initialization(
        &mut self,
        body: Option<StmtId>,
    ) -> Result<(), CompilerFailure> {
        let Some(body) = body else {
            return Ok(());
        };
        let Some(&count) = self.ctx.ta.parameter_default_prologues.get(&body) else {
            return Ok(());
        };
        let TypedStmtKind::Block(statements) = &self
            .ctx
            .ta
            .try_stmt(body)
            .map_err(crate::codegen::arena_failure)?
            .kind
        else {
            return Err(internal_failure("parameter prologue must be a block"));
        };
        let statements = statements
            .get(..count)
            .ok_or_else(|| internal_failure("parameter prologue is incomplete"))?
            .to_vec();
        for statement in statements {
            if !self.ctx.ta.parameter_initializations.contains(&statement) {
                continue;
            }
            let kind = self
                .ctx
                .ta
                .try_stmt(statement)
                .map_err(crate::codegen::arena_failure)?
                .kind
                .clone();
            match kind {
                TypedStmtKind::AssignLocal { ident, .. } => {
                    self.uninitialized_parameters.insert(ident);
                }
                TypedStmtKind::Let {
                    name,
                    ty,
                    boxed: true,
                    ..
                } => {
                    let box_type = self.ctx.symbols.box_type_idx(&ty)?.ok_or_else(|| {
                        internal_failure("parameter binding cell is not registered")
                    })?;
                    let slot = self.define_local(
                        &name,
                        ValType::Ref(RefType {
                            nullable: false,
                            heap_type: HeapType::Concrete(box_type),
                        }),
                    )?;
                    self.instruction(Instruction::StructNewDefault(box_type));
                    self.instruction(Instruction::LocalSet(slot));
                    self.uninitialized_cells.insert(slot);
                    self.parameter_binding_cells.insert(name, slot);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub(super) fn check_initialized(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    local: u32,
    box_type: u32,
) -> Result<(), CompilerFailure> {
    if !emitter.uninitialized_cells.contains(&local) {
        return Ok(());
    }
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: box_type,
        field_index: 1,
    });
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(BlockType::Empty);
    emit_const_string_by_text(emitter, ctx, UNINITIALIZED_PARAMETER_MESSAGE)?;
    let constructor = ctx
        .symbols
        .prelude_func_idx("ReferenceError#constructor")
        .ok_or_else(|| internal_failure("ReferenceError constructor was not imported"))?;
    emitter.instruction(Instruction::Call(constructor));
    crate::codegen::throw::emit_error_throw(emitter, ctx);
    emitter.emit_end();
    Ok(())
}

pub(super) fn read(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    local: u32,
    box_type: u32,
) -> Result<ValType, CompilerFailure> {
    let payload = ctx
        .symbols
        .box_payload_type(box_type)
        .ok_or_else(|| internal_failure("binding cell payload is not registered"))?;
    check_initialized(emitter, ctx, local, box_type)?;
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: box_type,
        field_index: 0,
    });
    if matches!(payload, ValType::Ref(reference) if !reference.nullable) {
        emitter.instruction(Instruction::RefAsNonNull);
    }
    Ok(payload)
}

pub(super) fn mark_initialized(emitter: &mut FunctionEmitter<'_>, local: u32, box_type: u32) {
    emitter.instruction(Instruction::LocalGet(local));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::StructSet {
        struct_type_index: box_type,
        field_index: 1,
    });
}
