//! `FunctionEmitter` — single-pass builder for the body of one Wasm function.

pub mod cast;
pub mod expr;
mod finally;
pub mod json;
pub mod mcp;
pub mod stmt;

use std::collections::BTreeMap;

use wasm_encoder::{BlockType, Encode, Function, Instruction, ValType};

use crate::codegen::CodegenCtx;
use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Ident, Span, Type};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExprContext {
    Value,
    Statement,
}

/// Where a `return` in a body coerces to. Exactly one applies to any body.
#[derive(Clone, Debug, Default)]
pub enum ReturnTarget {
    /// A void function, method, or constructor: no result at all.
    #[default]
    NoResult,
    /// A void closure has no result slot either, yet a diverging body still
    /// pushes the callee's value — Wasm can't see that the call never returns —
    /// so an expression body would fall off the end unbalanced without a drop.
    VoidClosure,
    /// A *recorded* Wasm result: a closure's funcref result, or a class
    /// method's vtable slot, which this body's own annotation may lower either
    /// wider or narrower than.
    Slot(ValType),
    /// A top-level function, whose declared return type *is* its signature.
    Declared(Type),
}

pub struct FunctionEmitter<'a> {
    pub(super) cast_diagnostic: Option<super::cast_diagnostics::Locals>,
    pub runtime_type_params: BTreeMap<String, (u32, u32)>,
    ctx: &'a CodegenCtx<'a>,

    next_local_index: u32,
    parameter_types: Vec<ValType>,
    locals: Vec<(u32, ValType)>,
    instructions: Vec<Instruction<'static>>,
    /// How many leading `instructions` [`Self::body_size`] has
    /// encoded so far.
    sized_instructions: usize,
    /// The encoded size of those instructions.
    instruction_bytes: usize,
    scopes: Vec<Scope>,

    wasm_block_depth: u32,
    loop_contexts: Vec<LoopLabels>,
    #[allow(dead_code)]
    return_block_depth: u32,

    return_target: ReturnTarget,

    /// Pending completion targets, innermost last. Bodies are emitted once.
    finally_stack: Vec<finally::FinallyFrame>,

    /// Local receiver for a class member, ordinary function expression, or
    /// arrow capturing an enclosing receiver.
    this_local: Option<u32>,
    pub(super) dynamic_this: bool,
    pub(super) call_receiver: Option<u32>,

    /// Set while emitting a constructor *init* fn: the class being constructed.
    /// `super(...)` reads it to emit the field-setup sequence (own initializers
    /// and parameter-property copies) right after the parent is initialized.
    ctor_class: Option<crate::MangledName>,

    source_mappings: Vec<(usize, Span)>,
    /// The source being emitted when the function first needed more locals
    /// than the Wasm engine accepts.
    locals_limit_span: Option<Span>,
    /// The encoded size of `locals`' entries.
    local_entry_bytes: usize,
    /// The first instruction [`Self::body_size`] found past the Wasm
    /// engine's body-size limit.
    body_limit_instruction: Option<usize>,

    /// Expression nodes already evaluated into a local, so re-emitting one
    /// reads the local instead of running it again. A compound assignment is
    /// the one place the typed AST reuses an `ExprId` to mean a *single*
    /// evaluation: `a[i] += v` synthesizes its read from the very receiver and
    /// index nodes the write uses.
    single_evaluations: Vec<(ExprId, u32)>,
}

/// A name's two slots. `declared` is the binding's storage — the slot every
/// write goes to. `shadow` is a snapshot of that storage taken at a narrowing,
/// held in a local of the narrowed value type so a narrowed read needs no cast.
/// Keeping them apart is what stops a widening write from landing in the
/// narrowed slot (`let x: string | null; x = "ab"; x = null`), and what lets a
/// read after such a write go back to the storage instead of a stale shadow.
#[derive(Clone, Copy, Default)]
struct Binding {
    /// `None` only while a narrowing snapshot sits in a scope inner to the one
    /// that declares the name — the shadow is visible here, the storage is not.
    declared: Option<(u32, ValType)>,
    shadow: Option<(u32, ValType)>,
}

#[derive(Default)]
struct Scope {
    bindings: BTreeMap<String, Binding>,
    narrow_sources: BTreeMap<String, crate::ExprId>,
}

impl Scope {
    fn new() -> Self {
        Self::default()
    }
    /// Declare (or re-declare) `name`'s storage. Re-declaring drops any
    /// snapshot: the storage moved, so the old one describes nothing.
    fn define(&mut self, name: String, index: u32, ty: ValType) {
        self.bindings.insert(
            name,
            Binding {
                declared: Some((index, ty)),
                shadow: None,
            },
        );
    }
    fn set_shadow(&mut self, name: String, index: u32, ty: ValType) {
        self.bindings.entry(name).or_default().shadow = Some((index, ty));
    }
    fn clear_shadow(&mut self, name: &str) {
        if let Some(binding) = self.bindings.get_mut(name) {
            binding.shadow = None;
        }
    }
    fn clear_all_shadows(&mut self) {
        for binding in self.bindings.values_mut() {
            binding.shadow = None;
        }
    }
    /// The slot a narrowed read resolves to: the shadow when one is live.
    fn lookup(&self, name: &str) -> Option<(u32, ValType)> {
        self.bindings
            .get(name)
            .and_then(|b| b.shadow.or(b.declared))
    }
    fn lookup_declared(&self, name: &str) -> Option<(u32, ValType)> {
        self.bindings.get(name).and_then(|b| b.declared)
    }
}

struct LoopLabels {
    break_depth: u32,
    continue_depth: u32,
    /// `true` for switch frames; `continue` skips these to find the enclosing loop.
    is_switch: bool,
    /// Finallys pushed after this loop opened must run before any break/continue out of it fires.
    finally_count_at_push: usize,
}

impl<'a> FunctionEmitter<'a> {
    /// Parameters are implicit Wasm locals (0..N) — not in the `locals` declaration section.
    pub fn new(
        ctx: &'a CodegenCtx<'a>,
        params: &[(Ident, ValType)],
    ) -> Result<Self, CompilerFailure> {
        let parameter_count = crate::codegen::wasm_u32(params.len())?;
        super::FunctionLimit::exceeded(0, parameter_count).map_or(Ok(()), |limit| {
            Err(limit.failure(params.last().map(|(name, _)| name.span)))
        })?;
        let mut emitter = Self {
            ctx,
            runtime_type_params: BTreeMap::new(),
            next_local_index: parameter_count,
            parameter_types: params.iter().map(|(_, ty)| *ty).collect(),
            locals: Vec::new(),
            instructions: Vec::new(),
            sized_instructions: 0,
            instruction_bytes: 0,
            scopes: vec![Scope::new()],
            wasm_block_depth: 0,
            loop_contexts: Vec::new(),
            return_block_depth: 0,
            return_target: ReturnTarget::NoResult,
            finally_stack: Vec::new(),
            this_local: None,
            dynamic_this: false,
            call_receiver: None,
            ctor_class: None,
            source_mappings: Vec::new(),
            locals_limit_span: None,
            body_limit_instruction: None,
            local_entry_bytes: 0,
            single_evaluations: Vec::new(),
            cast_diagnostic: None,
        };
        let mut scope = Scope::new();
        for (index, (name, ty)) in params.iter().enumerate() {
            scope.define(name.name.clone(), crate::codegen::wasm_u32(index)?, *ty);
        }
        emitter.scopes = vec![scope];
        Ok(emitter)
    }

    pub fn define_local(&mut self, name: &Ident, ty: ValType) -> Result<u32, CompilerFailure> {
        self.require_scope()?;
        let index = self.add_anonymous_local(ty)?;
        self.rebind_in_innermost_scope(&name.name, index, ty)?;
        Ok(index)
    }

    /// The local holding `id`'s already-computed value, if the statement being
    /// emitted registered one.
    pub fn evaluated_slot(&self, id: ExprId) -> Option<u32> {
        self.single_evaluations
            .iter()
            .find(|(recorded, _)| *recorded == id)
            .map(|(_, slot)| *slot)
    }

    /// Register `slot` as `id`'s value. Call *after* emitting `id`, so the
    /// emission that fills the slot isn't itself short-circuited.
    pub fn record_single_evaluation(
        &mut self,
        id: ExprId,
        slot: u32,
    ) -> Result<(), CompilerFailure> {
        self.local_type(slot)?;
        self.single_evaluations.push((id, slot));
        Ok(())
    }

    /// Registrations are statement-scoped: later emission of a shared expression
    /// must compute its value again. Take a mark before emitting a statement
    /// that registers, and [`Self::end_single_evaluations`] with it afterwards.
    pub fn single_evaluation_mark(&self) -> usize {
        self.single_evaluations.len()
    }

    /// Drop every registration taken since `mark`. See
    /// [`Self::single_evaluation_mark`].
    pub fn end_single_evaluations(&mut self, mark: usize) -> Result<(), CompilerFailure> {
        if mark > self.single_evaluations.len() {
            return Err(self.state_failure("invalid single evaluation mark"));
        }
        self.single_evaluations.truncate(mark);
        Ok(())
    }

    pub fn add_anonymous_local(&mut self, ty: ValType) -> Result<u32, CompilerFailure> {
        let index = self.next_local_index;
        let next = index
            .checked_add(1)
            .ok_or_else(|| self.state_failure("local index overflow"))?;
        if let Some(limit) = super::FunctionLimit::exceeded(0, next) {
            return Err(limit.failure(self.last_source_span()));
        }
        // Each local is its own entry: a count of one (one byte) and its type.
        let mut entry = vec![1u8];
        ty.encode(&mut entry);
        self.local_entry_bytes = self.local_entry_bytes.saturating_add(entry.len());
        self.locals.push((1, ty));
        self.next_local_index = next;
        Ok(index)
    }

    pub fn require_local_slot(&self, name: &str) -> Result<u32, CompilerFailure> {
        let index = self
            .local_slot(name)
            .ok_or_else(|| self.state_failure(format!("local `{name}` is not defined")))?;
        self.local_type(index)?;
        Ok(index)
    }

    /// The slot a narrowed read of `name` resolves to, and what it holds: the
    /// innermost shadow when one is live, otherwise the binding's storage,
    /// which the caller then casts down to the narrowed type. Only
    /// `LocalNarrowRef` wants this — an ordinary read wants
    /// [`Self::local_slot`], whose value is never stale.
    pub fn narrowed_read_slot(&self, name: &str) -> Option<(u32, ValType)> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.lookup(name))
    }

    /// The binding's storage — what every read at the declared type sees.
    pub fn local_slot(&self, name: &str) -> Option<u32> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.lookup_declared(name))
            .map(|(index, _)| index)
    }

    /// The storage slot to write `name` through, dropping every shadow of it on
    /// the way. Both halves are the point: writing through a shadow instead
    /// loses the write (the storage keeps its old value) and rejects any value
    /// wider than the narrowing — `x = null` into a `(ref $string)` slot is
    /// invalid Wasm — and leaving a shadow behind hands the next narrowed read
    /// the value from before the write.
    pub fn write_slot(&mut self, name: &str) -> Result<u32, CompilerFailure> {
        let index = self.require_local_slot(name)?;
        self.clear_narrow_shadows(name);
        Ok(index)
    }

    pub fn set_return_target(&mut self, target: ReturnTarget) {
        self.return_target = target;
    }

    pub fn return_target(&self) -> &ReturnTarget {
        &self.return_target
    }

    pub fn set_this_local(&mut self, index: u32) -> Result<(), CompilerFailure> {
        self.local_type(index)?;
        self.this_local = Some(index);
        Ok(())
    }

    pub fn this_local(&self) -> Option<u32> {
        self.this_local
    }

    pub fn set_ctor_class(&mut self, mangled: crate::MangledName) {
        self.ctor_class = Some(mangled);
    }

    pub fn ctor_class(&self) -> Option<&crate::MangledName> {
        self.ctor_class.as_ref()
    }

    /// Move `name`'s storage to `index` — for a param whose real home is the box
    /// or typed local the prologue builds. This is a re-declaration, not a
    /// narrowing: writes follow it.
    pub fn rebind_in_innermost_scope(
        &mut self,
        name: &str,
        index: u32,
        ty: ValType,
    ) -> Result<(), CompilerFailure> {
        self.check_local_type(index, ty)?;
        let failure = self.state_failure("no active emitter scope");
        let scope = self.scopes.last_mut().ok_or(failure)?;
        scope.define(name.to_string(), index, ty);
        Ok(())
    }

    /// Record the shadow an assignment's narrowing just took. Narrowed reads
    /// resolve here until the next write clears it; writes never do.
    pub fn install_narrow_shadow(
        &mut self,
        name: &str,
        index: u32,
        ty: ValType,
    ) -> Result<(), CompilerFailure> {
        self.check_local_type(index, ty)?;
        let failure = self.state_failure("no active emitter scope");
        let scope = self.scopes.last_mut().ok_or(failure)?;
        scope.set_shadow(name.to_string(), index, ty);
        Ok(())
    }

    /// Register the deferred live-read source for a field/index narrowing.
    /// Every narrowed use reloads this source and checks the current value.
    pub fn register_narrow_source(
        &mut self,
        name: &str,
        source: crate::ExprId,
    ) -> Result<(), CompilerFailure> {
        self.ctx
            .ta
            .try_expr(source)
            .map_err(crate::codegen::arena_failure)?;
        let failure = self.state_failure("no active emitter scope");
        let scope = self.scopes.last_mut().ok_or(failure)?;
        scope.narrow_sources.insert(name.to_string(), source);
        Ok(())
    }

    pub fn narrow_source(&self, name: &str) -> Option<crate::ExprId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.narrow_sources.get(name).copied())
    }

    /// Drop every shadow of `name` visible from here — a write to the storage
    /// slot makes all of them stale. Stops at the scope that declares the name;
    /// an outer binding of the same name is a different one.
    pub fn clear_narrow_shadows(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            scope.clear_shadow(name);
            if scope.lookup_declared(name).is_some() {
                return;
            }
        }
    }

    /// Drop every narrowing shadow in scope. A shadow is sound only for reads
    /// that follow its assignment both in the emitted code and at run time, and
    /// a branching statement is where those two stop agreeing — see
    /// `stmt::branches`, which is what calls this.
    pub fn clear_all_narrow_shadows(&mut self) {
        for scope in &mut self.scopes {
            scope.clear_all_shadows();
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// Wasm local indices remain reserved after pop — locals are function-scoped, not block-scoped.
    pub fn pop_scope(&mut self) -> Result<(), CompilerFailure> {
        if self.scopes.len() <= 1 {
            return Err(self.state_failure("cannot pop the root emitter scope"));
        }
        self.scopes.pop();
        Ok(())
    }

    /// `typed_slots[i]` is the Wasm slot holding the typed-form value of `params[i]` —
    /// index `i` for top-level functions, a prologue-allocated local for closure bodies.
    /// Boxing is body-internal; the Wasm signature is unchanged.
    pub fn emit_boxed_param_prologue(
        &mut self,
        params: &[crate::TypedParam],
        typed_slots: &[u32],
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if params.len() != typed_slots.len() {
            return Err(self.state_failure("typed slots must have one entry per parameter"));
        }
        self.require_scope()?;
        // Resolve every registration before mutating bindings or emitting a prologue.
        let mut boxes = Vec::new();
        for (p, &typed_slot) in params.iter().zip(typed_slots) {
            self.check_local_type(typed_slot, self.ctx.symbols.value_type(&p.ty)?)?;
            if self.require_local_slot(&p.name.name)? != typed_slot {
                return Err(self.state_failure("typed parameter slot does not match its binding"));
            }
            if !p.boxed {
                continue;
            }
            let box_idx =
                self.ctx.symbols.box_type_idx(&p.ty)?.ok_or_else(|| {
                    self.state_failure("box type registered for every boxed param")
                })?;
            boxes.push((p, typed_slot, box_idx));
        }
        let count = self
            .next_local_index
            .checked_add(crate::codegen::wasm_u32(boxes.len())?)
            .ok_or_else(|| self.state_failure("boxed parameter local count overflow"))?;
        if let Some(limit) = super::FunctionLimit::exceeded(0, count) {
            return Err(limit.failure(self.last_source_span()));
        }
        for (p, typed_slot, box_idx) in boxes {
            let box_val = ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(box_idx),
            });
            let shadow_idx = self.add_anonymous_local(box_val)?;
            self.instruction(Instruction::LocalGet(typed_slot));
            self.instruction(Instruction::StructNew(box_idx));
            self.instruction(Instruction::LocalSet(shadow_idx));
            self.rebind_in_innermost_scope(&p.name.name, shadow_idx, box_val)?;
        }
        Ok(())
    }

    /// Prefer the structured helpers (emit_block/if/loop) to keep depth tracking consistent.
    pub fn instruction(&mut self, inst: Instruction<'static>) {
        self.instructions.push(inst);
    }

    /// The encoded size the function body would have if it ended here: its
    /// local declarations, its instructions and the closing `end`, as
    /// [`Self::build`] encodes them. Instructions are only ever appended, so
    /// each is encoded once, the first time it is measured.
    pub fn body_size(&mut self) -> usize {
        let locals_count = u32::try_from(self.locals.len()).unwrap_or(u32::MAX);
        // The declarations and the one-byte `end`, around the instructions.
        let declarations_and_end_bytes = super::leb128_u32_size(locals_count)
            .saturating_add(self.local_entry_bytes)
            .saturating_add(1);
        let mut encoded = Vec::new();
        for (index, instruction) in self
            .instructions
            .iter()
            .enumerate()
            .skip(self.sized_instructions)
        {
            instruction.encode(&mut encoded);
            let body_bytes = self
                .instruction_bytes
                .saturating_add(encoded.len())
                .saturating_add(declarations_and_end_bytes);
            if self.body_limit_instruction.is_none() && !super::FunctionLimit::body_fits(body_bytes)
            {
                self.body_limit_instruction = Some(index);
            }
        }
        self.sized_instructions = self.instructions.len();
        self.instruction_bytes = self.instruction_bytes.saturating_add(encoded.len());
        self.instruction_bytes
            .saturating_add(declarations_and_end_bytes)
    }

    /// The source whose code took [`Self::body_size`] past the Wasm
    /// engine's body-size limit.
    pub fn body_limit_span(&self) -> Option<Span> {
        self.body_limit_instruction
            .and_then(|instruction| self.source_span_before(instruction))
    }

    /// The source being emitted when the function first needed more locals
    /// than the Wasm engine accepts.
    pub fn locals_limit_span(&self) -> Option<Span> {
        self.locals_limit_span
    }

    /// Locals the function has so far, parameters included.
    pub fn local_count(&self) -> u32 {
        self.next_local_index
    }

    /// The source span most recently mapped to emitted code.
    pub fn last_mapped_span(&self) -> Option<Span> {
        self.source_mappings.last().map(|(_, span)| *span)
    }

    /// The most recently mapped span that locates source, skipping the
    /// placeholders of desugared code.
    pub fn last_source_span(&self) -> Option<Span> {
        self.source_span_before(self.instructions.len())
    }

    /// The last span mapped at or before `instruction` that locates source.
    fn source_span_before(&self, instruction: usize) -> Option<Span> {
        let mapped = self
            .source_mappings
            .partition_point(|(index, _)| *index <= instruction);
        self.source_mappings
            .iter()
            .take(mapped)
            .rev()
            .map(|(_, span)| *span)
            .find(|span| !span.is_placeholder())
    }

    pub fn record_span(&mut self, span: Span) {
        self.source_mappings.push((self.instructions.len(), span));
    }

    pub fn emit_block(&mut self, ty: BlockType) {
        self.instructions.push(Instruction::Block(ty));
        if let Some(depth) = self.ctx.latch(
            self.wasm_block_depth
                .checked_add(1)
                .ok_or_else(|| self.state_failure("block depth overflow")),
        ) {
            self.wasm_block_depth = depth;
        }
    }

    pub fn emit_if(&mut self, ty: BlockType) {
        self.instructions.push(Instruction::If(ty));
        if let Some(depth) = self.ctx.latch(
            self.wasm_block_depth
                .checked_add(1)
                .ok_or_else(|| self.state_failure("block depth overflow")),
        ) {
            self.wasm_block_depth = depth;
        }
    }

    pub fn emit_loop(&mut self, ty: BlockType) {
        self.instructions.push(Instruction::Loop(ty));
        if let Some(depth) = self.ctx.latch(
            self.wasm_block_depth
                .checked_add(1)
                .ok_or_else(|| self.state_failure("block depth overflow")),
        ) {
            self.wasm_block_depth = depth;
        }
    }

    /// `else` doesn't change block depth — it's a continuation of the surrounding `if`.
    pub fn emit_else(&mut self) {
        self.instructions.push(Instruction::Else);
    }

    pub fn emit_end(&mut self) {
        let Some(depth) = self.ctx.latch(
            self.wasm_block_depth
                .checked_sub(1)
                .ok_or_else(|| self.state_failure("end without an active block")),
        ) else {
            return;
        };
        self.wasm_block_depth = depth;
        self.instructions.push(Instruction::End);
    }

    /// For instructions like `try_table` that open a block frame but are emitted via
    /// `instruction()` (which skips depth tracking); the matching `emit_end()` decrements.
    pub fn bump_block_depth(&mut self) {
        if let Some(depth) = self.ctx.latch(
            self.wasm_block_depth
                .checked_add(1)
                .ok_or_else(|| self.state_failure("block depth overflow")),
        ) {
            self.wasm_block_depth = depth;
        }
    }

    /// The Wasm result this body's signature declares, or `None` when it has none.
    pub fn wasm_result_type(
        &self,
        ctx: &CodegenCtx,
    ) -> Result<Option<ValType>, crate::compiler_error::CompilerFailure> {
        Ok(match &self.return_target {
            ReturnTarget::Slot(slot) => Some(*slot),
            ReturnTarget::Declared(ret) => Some(ctx.symbols.value_type(ret)?),
            ReturnTarget::VoidClosure | ReturnTarget::NoResult => None,
        })
    }

    pub fn break_finally_floor(&self) -> usize {
        self.loop_contexts
            .last()
            .map_or(0, |c| c.finally_count_at_push)
    }

    /// Like `break_finally_floor` but for `continue`; skips switch frames.
    pub fn continue_finally_floor(&self) -> usize {
        self.loop_contexts
            .iter()
            .rev()
            .find(|c| !c.is_switch)
            .map_or(0, |c| c.finally_count_at_push)
    }

    pub fn emit_while_open(&mut self) {
        if self.wasm_block_depth > u32::MAX - 2 {
            self.ctx
                .record_failure(self.state_failure("loop block depth overflow"));
            return;
        }
        self.emit_block(BlockType::Empty);
        self.emit_loop(BlockType::Empty);
        self.loop_contexts.push(LoopLabels {
            break_depth: self.wasm_block_depth - 2,
            continue_depth: self.wasm_block_depth - 1,
            is_switch: false,
            finally_count_at_push: self.finally_stack.len(),
        });
    }

    /// Pushes an `is_switch: true` break frame so `continue_label()` skips to the enclosing loop.
    pub fn emit_switch_open(&mut self) {
        if self.wasm_block_depth == u32::MAX {
            self.ctx
                .record_failure(self.state_failure("switch block depth overflow"));
            return;
        }
        self.emit_block(BlockType::Empty);
        self.loop_contexts.push(LoopLabels {
            break_depth: self.wasm_block_depth - 1,
            // unread for switch frames; 0 so any accidental read gives a deterministically bad label
            continue_depth: 0,
            is_switch: true,
            finally_count_at_push: self.finally_stack.len(),
        });
    }

    pub fn emit_switch_close(&mut self) {
        if !self.loop_contexts.last().is_some_and(|frame| {
            frame.is_switch && frame.break_depth.checked_add(1) == Some(self.wasm_block_depth)
        }) {
            self.ctx
                .record_failure(self.state_failure("mismatched switch close"));
            return;
        }
        self.loop_contexts.pop();
        self.emit_end();
    }

    pub fn emit_while_close(&mut self) {
        if !self.loop_contexts.last().is_some_and(|frame| {
            !frame.is_switch && frame.break_depth.checked_add(2) == Some(self.wasm_block_depth)
        }) {
            self.ctx
                .record_failure(self.state_failure("mismatched loop close"));
            return;
        }
        self.loop_contexts.pop();
        self.emit_end(); // end loop
        self.emit_end(); // end block
    }

    pub fn break_label(&self) -> Result<u32, CompilerFailure> {
        let ctx = self
            .loop_contexts
            .last()
            .ok_or_else(|| self.state_failure("break outside loop or switch"))?;
        self.branch_depth(ctx.break_depth)
    }

    /// Walks past switch frames — `continue` inside a switch targets the enclosing loop.
    pub fn continue_label(&self) -> Result<u32, CompilerFailure> {
        let ctx = self
            .loop_contexts
            .iter()
            .rev()
            .find(|c| !c.is_switch)
            .ok_or_else(|| self.state_failure("continue outside loop"))?;
        self.branch_depth(ctx.continue_depth)
    }

    pub fn return_label(&self) -> Result<u32, CompilerFailure> {
        self.branch_depth(self.return_block_depth)
    }

    pub fn branch_depth(&self, target_depth: u32) -> Result<u32, CompilerFailure> {
        self.wasm_block_depth
            .checked_sub(target_depth)
            .and_then(|depth| depth.checked_sub(1))
            .ok_or_else(|| self.state_failure("branch target is outside the active blocks"))
    }

    /// Segment offset is always 0 — one data segment per literal.
    pub fn emit_const_string(
        &mut self,
        string_type_idx: u32,
        raw_string_type_idx: u32,
        string_vtable_global_idx: u32,
        consts_data_idx: u32,
        code_units: u32,
    ) {
        self.instruction(Instruction::GlobalGet(string_vtable_global_idx));
        self.instruction(Instruction::I32Const(0));
        // Wasm takes the unsigned array length as an i32 bit pattern.
        self.instruction(Instruction::I32Const(code_units as i32));
        self.instruction(Instruction::ArrayNewData {
            array_type_index: raw_string_type_idx,
            array_data_index: consts_data_idx,
        });
        self.instruction(Instruction::I64Const(0));
        self.instruction(Instruction::StructNew(string_type_idx));
    }

    /// Consumes the emitter, returning bytes only after state and size checks pass.
    /// Any failure discards the owned instructions, scopes and completion frames.
    pub fn build(self) -> Result<Function, CompilerFailure> {
        Ok(self.build_with_lines()?.0)
    }

    /// Like `build`, but also returns DWARF byte offsets. Offsets are measured
    /// before each instruction is pushed, so they never decrease.
    pub fn build_with_lines(self) -> Result<(Function, Vec<(u64, Span)>), CompilerFailure> {
        self.ctx.check_failure()?;
        self.validate_state()?;
        let mut func = Function::new(self.locals.iter().copied());
        let mut byte_offsets: Vec<(u64, Span)> = Vec::with_capacity(self.source_mappings.len());
        let mut next_mapping = 0usize;
        let mut body_limit_instruction = None;

        for (i, inst) in self.instructions.iter().enumerate() {
            while next_mapping < self.source_mappings.len()
                && self.source_mappings[next_mapping].0 == i
            {
                let offset = func.byte_len() as u64;
                byte_offsets.push((offset, self.source_mappings[next_mapping].1));
                next_mapping += 1;
            }
            func.instruction(inst);
            // The closing `end` is one byte.
            if body_limit_instruction.is_none()
                && !super::FunctionLimit::body_fits(func.byte_len().saturating_add(1))
            {
                body_limit_instruction = Some(i);
            }
        }
        // Drain spans recorded past the last instruction; their offset is the function's end byte.
        let end_offset = func.byte_len() as u64;
        while next_mapping < self.source_mappings.len() {
            byte_offsets.push((end_offset, self.source_mappings[next_mapping].1));
            next_mapping += 1;
        }
        func.instruction(&Instruction::End);
        self.ctx.check_finished_function(super::FinishedFunction {
            body_bytes: func.byte_len(),
            body_limit_span: body_limit_instruction
                .and_then(|instruction| self.source_span_before(instruction)),
            locals: self.next_local_index,
            locals_limit_span: self.locals_limit_span,
        });
        self.ctx.check_failure()?;
        Ok((func, byte_offsets))
    }
    /// Validate recorded state before handing any bytes to a module builder.
    fn validate_state(&self) -> Result<(), CompilerFailure> {
        if self.scopes.len() != 1
            || self.wasm_block_depth != 0
            || !self.loop_contexts.is_empty()
            || !self.finally_stack.is_empty()
            || !self.single_evaluations.is_empty()
        {
            return Err(self.state_failure("unfinished emitter scope or control-flow state"));
        }
        let count = self
            .parameter_types
            .len()
            .checked_add(self.locals.len())
            .ok_or_else(|| self.state_failure("local count overflow"))?;
        if crate::codegen::wasm_u32(count)? != self.next_local_index
            || self.locals.iter().any(|(count, _)| *count != 1)
        {
            return Err(
                self.state_failure("local declaration count does not match allocated slots")
            );
        }
        for scope in &self.scopes {
            for binding in scope.bindings.values() {
                for (index, ty) in binding.declared.iter().chain(binding.shadow.iter()) {
                    self.check_local_type(*index, *ty)?;
                }
            }
        }
        if let Some(index) = self.this_local {
            self.local_type(index)?;
        }
        if let Some(index) = self.call_receiver {
            self.local_type(index)?;
        }
        for (index, _) in self.runtime_type_params.values() {
            self.local_type(*index)?;
        }
        self.validate_instructions()
    }

    fn validate_instructions(&self) -> Result<(), CompilerFailure> {
        let mut blocks = Vec::new();
        for (position, instruction) in self.instructions.iter().enumerate() {
            let failure = |message| {
                let error = crate::codegen::internal_failure(message);
                self.source_span_before(position)
                    .map_or(error.clone(), |span| error.with_span(span))
            };
            match instruction {
                Instruction::LocalGet(index)
                | Instruction::LocalSet(index)
                | Instruction::LocalTee(index) => {
                    self.local_type(*index)
                        .map_err(|_| failure("instruction uses an unallocated local"))?;
                }
                Instruction::Block(_) | Instruction::Loop(_) => blocks.push(false),
                Instruction::TryTable(_, catches) => {
                    for catch in catches.iter() {
                        let (wasm_encoder::Catch::One { label, .. }
                        | wasm_encoder::Catch::OneRef { label, .. }
                        | wasm_encoder::Catch::All { label }
                        | wasm_encoder::Catch::AllRef { label }) = catch;
                        if u64::from(*label) > blocks.len() as u64 {
                            return Err(failure("catch branch depth exceeds active blocks"));
                        }
                    }
                    blocks.push(false);
                }
                Instruction::If(_) => blocks.push(true),
                Instruction::Else => {
                    let block = blocks
                        .last_mut()
                        .ok_or_else(|| failure("else without an if block"))?;
                    if !*block {
                        return Err(failure("else without an unmatched if block"));
                    }
                    *block = false;
                }
                Instruction::End => {
                    blocks
                        .pop()
                        .ok_or_else(|| failure("end without an active block"))?;
                }
                Instruction::Br(depth)
                | Instruction::BrIf(depth)
                | Instruction::BrOnNull(depth)
                | Instruction::BrOnNonNull(depth)
                | Instruction::BrOnCast {
                    relative_depth: depth,
                    ..
                }
                | Instruction::BrOnCastFail {
                    relative_depth: depth,
                    ..
                }
                | Instruction::BrOnCastDescEq {
                    relative_depth: depth,
                    ..
                }
                | Instruction::BrOnCastDescEqFail {
                    relative_depth: depth,
                    ..
                } => {
                    // The function's implicit block is also a valid branch target.
                    if u64::from(*depth) > blocks.len() as u64 {
                        return Err(failure("branch depth exceeds active blocks"));
                    }
                }
                Instruction::BrTable(depths, default)
                    if depths
                        .iter()
                        .chain(std::iter::once(default))
                        .any(|depth| u64::from(*depth) > blocks.len() as u64) =>
                {
                    return Err(failure("branch table depth exceeds active blocks"));
                }
                _ => {}
            }
        }
        if !blocks.is_empty() {
            return Err(self.state_failure("unclosed Wasm blocks"));
        }
        Ok(())
    }

    fn state_failure(&self, message: impl Into<String>) -> CompilerFailure {
        let failure = crate::codegen::internal_failure(message);
        self.last_source_span()
            .map_or(failure.clone(), |span| failure.with_span(span))
    }

    fn require_scope(&self) -> Result<(), CompilerFailure> {
        if self.scopes.is_empty() {
            return Err(self.state_failure("no active emitter scope"));
        }
        Ok(())
    }

    fn local_type(&self, index: u32) -> Result<ValType, CompilerFailure> {
        let index = usize::try_from(index)
            .map_err(|_| self.state_failure("local index does not fit usize"))?;
        if let Some(ty) = self.parameter_types.get(index) {
            return Ok(*ty);
        }
        index
            .checked_sub(self.parameter_types.len())
            .and_then(|offset| self.locals.get(offset))
            .map(|(_, ty)| *ty)
            .ok_or_else(|| self.state_failure("local index is not allocated"))
    }

    fn check_local_type(&self, index: u32, ty: ValType) -> Result<(), CompilerFailure> {
        if self.local_type(index)? != ty {
            return Err(self.state_failure("local slot type does not match its metadata"));
        }
        Ok(())
    }
}

/// Push a constant `(ref $string)` for `text`, which must already be interned in
/// the [`StringPool`](crate::codegen::string_pool::StringPool). The ctx-aware
/// companion to [`FunctionEmitter::emit_const_string`]: it resolves the pool
/// slot and `$string`/`$rawString`/vtable indices, then emits the literal.
pub(crate) fn emit_const_string_by_text(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let pool_idx = ctx.strings.lookup_text(text).ok_or_else(|| {
        crate::codegen::internal_failure(format!("`{text}` was not interned by the string pool"))
    })?;
    emit_pooled_string(emitter, ctx, pool_idx)
}

/// Push the constant `(ref $string)` backed by string-pool entry `pool_idx`.
pub(crate) fn emit_pooled_string(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    pool_idx: usize,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    use crate::codegen::internal_failure;
    let code_units = ctx.strings.code_units(pool_idx)?;
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .ok_or_else(|| internal_failure("the $string intrinsic is not registered"))?;
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .ok_or_else(|| internal_failure("the $rawString intrinsic is not registered"))?;
    let vtable_global_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .ok_or_else(|| internal_failure("string_vtable is not imported from the prelude"))?;
    emitter.emit_const_string(
        string_type_idx,
        raw_string_type_idx,
        vtable_global_idx,
        crate::codegen::wasm_u32(pool_idx)?,
        code_units,
    );
    Ok(())
}

/// Push a fresh `(ref $string)` for a short codegen-owned literal (`"null"`,
/// `"[object Object]"`, …) — not pool-interned, built via `array.new_fixed`.
pub(crate) fn emit_inline_string_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    use crate::codegen::internal_failure;
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .ok_or_else(|| internal_failure("the $string intrinsic is not registered"))?;
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .ok_or_else(|| internal_failure("the $rawString intrinsic is not registered"))?;
    let vtable_global_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .ok_or_else(|| internal_failure("string_vtable is not imported from the prelude"))?;
    emitter.instruction(Instruction::GlobalGet(vtable_global_idx));
    let mut len = 0usize;
    for unit in text.encode_utf16() {
        emitter.instruction(Instruction::I32Const(i32::from(unit)));
        len = len.saturating_add(1);
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: raw_string_type_idx,
        array_size: crate::codegen::wasm_u32(len)?,
    });
    emitter.instruction(Instruction::I64Const(0));
    emitter.instruction(Instruction::StructNew(string_type_idx));
    Ok(())
}

pub fn emit_function(
    generics: &[String],
    ctx: &CodegenCtx<'_>,
    params: &[crate::TypedParam],
    body: crate::StmtId,
    return_type: &crate::Type,
) -> Result<(Function, Vec<(u64, Span)>), crate::compiler_error::CompilerFailure> {
    let mut wasm_params: Vec<(Ident, ValType)> = params
        .iter()
        .map(|p| Ok((p.name.clone(), ctx.symbols.value_type(&p.ty)?)))
        .collect::<Result<_, crate::compiler_error::CompilerFailure>>()?;
    if !generics.is_empty() {
        wasm_params.push((
            Ident {
                name: "$types".into(),
                span: Span::at(ctx.file),
            },
            crate::codegen::runtime_descriptors::environment_type(ctx.symbols)?,
        ));
    }
    let mut emitter = FunctionEmitter::new(ctx, &wasm_params)?;
    crate::codegen::runtime_descriptors::bind(
        &mut emitter,
        generics,
        crate::codegen::wasm_u32(params.len())?,
    )?;

    if !return_type.is_void() {
        emitter.set_return_target(ReturnTarget::Declared(return_type.clone()));
    }
    let typed_slots: Vec<u32> = (0..crate::codegen::wasm_u32(params.len())?).collect();
    emitter.emit_boxed_param_prologue(params, &typed_slots)?;
    stmt::emit_statement(&mut emitter, ctx, body)?;
    emit_body_end(&mut emitter, ctx, return_type)?;
    emitter.build_with_lines()
}

/// ABI: `(func (param (ref any) N×(ref $Object)) (result (ref $Object)?))` —
/// env at slot 0, every user param erased to `(ref $Object)`, result erased.
pub fn emit_closure_function(
    ctx: &CodegenCtx<'_>,
    meta: &crate::codegen::closures::ClosureMeta,
) -> Result<(Function, Vec<(u64, Span)>), crate::compiler_error::CompilerFailure> {
    let signature = crate::codegen::closures::classify(&meta.signature)?;
    if usize::from(signature.arity) != meta.params.len()
        || signature.is_void != meta.return_type.is_void()
    {
        return Err(crate::codegen::internal_failure(
            "closure body disagrees with its registered signature",
        ));
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let object_ref = ValType::Ref(wasm_encoder::RefType {
        nullable: true,
        heap_type: wasm_encoder::HeapType::Concrete(intrinsics.object),
    });
    let any_ref = ValType::Ref(wasm_encoder::RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::ANY,
    });

    let env_name = Ident {
        name: "$__env__".to_string(),
        span: Span::at(ctx.file),
    };
    let mut wasm_params: Vec<(Ident, ValType)> = vec![(env_name, any_ref)];
    for p in &meta.params {
        wasm_params.push((p.name.clone(), object_ref));
    }
    let mut emitter = FunctionEmitter::new(ctx, &wasm_params)?;

    let env_type_idx = ctx.symbols.env_type_idx(meta.expr_id).ok_or_else(|| {
        crate::codegen::internal_failure("env type registered for every closure expression")
    })?;
    let env_typed_val = ValType::Ref(wasm_encoder::RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(env_type_idx),
    });
    let env_typed_local = emitter.add_anonymous_local(env_typed_val)?;
    if meta.this_type.is_some() {
        crate::codegen::this_binding::load_receiver(&mut emitter, ctx)?;
    }
    emitter.instructions.push(Instruction::LocalGet(0));
    if meta.this_type.is_some() {
        emitter.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(
                ctx.symbols
                    .this_environment_type
                    .ok_or_else(|| crate::codegen::internal_failure("this environment"))?,
            ),
        ));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: ctx
                .symbols
                .this_environment_type
                .ok_or_else(|| crate::codegen::internal_failure("this environment"))?,
            field_index: 0,
        });
    }
    if crate::codegen::call_arguments::typed_metadata(&meta.params)?.is_some() {
        crate::codegen::call_arguments::unwrap(&mut emitter, ctx)?;
    }
    emitter.instructions.push(Instruction::RefCastNonNull(
        wasm_encoder::HeapType::Concrete(env_type_idx),
    ));
    emitter
        .instructions
        .push(Instruction::LocalSet(env_typed_local));

    if let Some(name) = &meta.self_name {
        let slot = emitter.define_local(name, ctx.symbols.value_type(&meta.signature)?)?;
        emitter.instruction(Instruction::LocalGet(env_typed_local));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: env_type_idx,
            field_index: crate::codegen::wasm_u32(
                meta.captured
                    .len()
                    .checked_add(usize::from(!meta.runtime_generics.is_empty()))
                    .ok_or_else(|| {
                        crate::codegen::internal_failure("capture field index overflow")
                    })?,
            )?,
        });
        cast::emit_cast_to(&mut emitter, ctx, &meta.signature)?;
        emitter.instruction(Instruction::LocalSet(slot));
    }
    if !meta.runtime_generics.is_empty() {
        let types = emitter.add_anonymous_local(
            crate::codegen::runtime_descriptors::environment_type(ctx.symbols)?,
        )?;
        emitter.instruction(Instruction::LocalGet(env_typed_local));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: env_type_idx,
            field_index: crate::codegen::wasm_u32(meta.captured.len())?,
        });
        emitter.instruction(Instruction::LocalSet(types));
        crate::codegen::runtime_descriptors::bind(&mut emitter, &meta.runtime_generics, types)?;
    }
    let mut typed_slots: Vec<u32> = Vec::with_capacity(meta.params.len());
    for (i, p) in meta.params.iter().enumerate() {
        let wasm_slot = crate::codegen::wasm_u32(i)?
            .checked_add(1)
            .ok_or_else(|| crate::codegen::internal_failure("parameter index overflow"))?;
        let typed_ty = ctx.symbols.value_type(&p.ty)?;
        let typed_local = emitter.add_anonymous_local(typed_ty)?;
        emitter.instructions.push(Instruction::LocalGet(wasm_slot));
        crate::codegen::cast_check::emit_checked_parameter_cast_on_stack(
            &mut emitter,
            ctx,
            &crate::Type::Unknown,
            &p.ty,
        )?;
        emitter
            .instructions
            .push(Instruction::LocalSet(typed_local));
        emitter.rebind_in_innermost_scope(&p.name.name, typed_local, typed_ty)?;
        typed_slots.push(typed_local);
    }

    // Boxed captures share the outer scope's box cell; unboxed captures copy the value directly.
    for (i, c) in meta.captured.iter().enumerate() {
        let local_ty = if c.boxed {
            let box_idx = ctx.symbols.box_type_idx(&c.ty)?.ok_or_else(|| {
                crate::codegen::internal_failure("box type registered for every boxed capture")
            })?;
            ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(box_idx),
            })
        } else {
            ctx.symbols.value_type(&c.ty)?
        };
        let captured_local = emitter.define_local(&c.name, local_ty)?;
        emitter
            .instructions
            .push(Instruction::LocalGet(env_typed_local));
        emitter.instructions.push(Instruction::StructGet {
            struct_type_index: env_type_idx,
            field_index: crate::codegen::wasm_u32(i)?,
        });
        // The env field may hold the erased `(ref null $Object)` form: env struct
        // types are emitted before class type indices are recorded, so a
        // class-typed capture's field widens while the local resolves concrete.
        if !c.boxed && matches!(c.ty.peel(), crate::Type::ClassRef { .. }) {
            cast::emit_cast_to(&mut emitter, ctx, &c.ty)?;
        } else if !c.boxed
            && let ValType::Ref(local_ref) = local_ty
            && !local_ref.nullable
        {
            // Same widening, one level up: a union of class types lowers to a
            // *nullable* `$Object` while the class indices are still unknown,
            // but to a non-null one once they are. `ref.as_non_null` closes the
            // repr gap and is a no-op when the field was already non-null.
            emitter.instructions.push(Instruction::RefAsNonNull);
        }
        emitter
            .instructions
            .push(Instruction::LocalSet(captured_local));
        // A closure that mentions `this` captures the enclosing member's
        // receiver; point the emitter's receiver slot at the materialized
        // local so `TypedExprKind::This` reads it like any method body.
        if c.name.name == crate::typechecker::capture::THIS_BINDING {
            emitter.set_this_local(captured_local)?;
            emitter.dynamic_this = c.ty == crate::Type::Unknown;
        }
    }

    emitter.emit_boxed_param_prologue(&meta.params, &typed_slots)?;

    emitter.set_return_target(closure_return_target(ctx, &meta.return_type)?);

    match meta.body {
        crate::ClosureBody::Expr(e) => {
            expr::emit_expr(&mut emitter, ctx, e)?;
            cast::emit_coerce_to_return_slot(
                &mut emitter,
                ctx,
                &ctx.ta
                    .try_expr(e)
                    .map_err(crate::codegen::arena_failure)?
                    .ty,
            )?;
        }
        crate::ClosureBody::Block(b) => {
            stmt::emit_statement(&mut emitter, ctx, b)?;
            emit_body_end(&mut emitter, ctx, &meta.return_type)?;
        }
    }

    emitter.build_with_lines()
}

/// A closure's result is fixed by its funcref signature: the erased
/// `(ref null $Object)`, or no slot at all when it returns void.
///
/// Must answer `void` the same way `closures::classify` does when it picks that
/// signature, or the target names a result slot the signature lacks.
fn closure_return_target(
    ctx: &CodegenCtx<'_>,
    ret: &Type,
) -> Result<ReturnTarget, CompilerFailure> {
    if ret.is_void() {
        return Ok(ReturnTarget::VoidClosure);
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?;
    Ok(ReturnTarget::Slot(ValType::Ref(wasm_encoder::RefType {
        nullable: true,
        heap_type: wasm_encoder::HeapType::Concrete(intrinsics.object),
    })))
}

/// Ends a block body that may produce a value. Control reaches the end of a
/// body returning `unknown` when it falls off without a `return`, which
/// yields `null` as JavaScript yields `undefined`. The missing-return rule
/// rejects such a body for every other value type, so there the end is
/// unreachable; the trap keeps the function statically total for Wasm
/// validation, which cannot prove that.
pub(crate) fn emit_body_end(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    return_type: &Type,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if return_type.is_void() {
        return Ok(());
    }
    if !matches!(return_type.peel(), Type::Unknown) {
        emitter.instruction(Instruction::Unreachable);
        return Ok(());
    }
    emitter.instruction(Instruction::RefNull(wasm_encoder::HeapType::Abstract {
        shared: false,
        ty: wasm_encoder::AbstractHeapType::None,
    }));
    cast::emit_coerce_to_return_slot(emitter, ctx, &Type::Null)?;
    emitter.instruction(Instruction::Return);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ExprContext, FunctionEmitter};
    use crate::codegen::{
        CodegenCtx, SymbolTable, bigint_pool::BigIntPool, string_pool::StringPool,
    };
    use crate::{Ident, LineIndex, Span, TypedAst};
    use wasm_encoder::{
        BlockType, ExportKind, ExportSection, Function, FunctionSection, Instruction, Module,
        TypeSection, ValType,
    };

    struct Fixture {
        ta: TypedAst,
        strings: StringPool,
        bigints: BigIntPool,
        symbols: SymbolTable,
        line_index: LineIndex,
        validator_bodies: crate::codegen::recursive_validators::ValidatorBodies,
        type_info: crate::TypeInfoTable,
    }

    fn fixture() -> Fixture {
        Fixture {
            ta: TypedAst::new(),
            strings: StringPool::default(),
            bigints: BigIntPool::default(),
            symbols: SymbolTable::default(),
            line_index: LineIndex::new("").unwrap(),
            validator_bodies: crate::codegen::recursive_validators::ValidatorBodies::collect(
                &TypedAst::new(),
                &[],
            ),
            type_info: crate::TypeInfoTable::default(),
        }
    }

    fn cx_of<'a>(f: &'a Fixture) -> CodegenCtx<'a> {
        CodegenCtx {
            ta: &f.ta,
            strings: &f.strings,
            bigints: &f.bigints,
            symbols: &f.symbols,
            source: "",
            line_index: &f.line_index,
            file: crate::FileId(0),
            validator_bodies: &f.validator_bodies,
            type_info: &f.type_info,
            package_string_global_idx: None,
            failure: std::cell::Cell::new(None),
            validator_steps_left: std::cell::Cell::new(
                crate::compiler_limits::MAX_INLINE_VALIDATOR_STEPS,
            ),
            validator_root: std::cell::Cell::new(None),
            check_is_standalone: std::cell::Cell::new(false),
        }
    }

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_string(),
            span: Span::at(crate::FileId(0)),
        }
    }

    fn ident_param(name: &str, ty: ValType) -> (Ident, ValType) {
        (ident(name), ty)
    }

    fn validate_in_module(f: wasm_encoder::Function, params: Vec<ValType>, results: Vec<ValType>) {
        let mut module = Module::new();
        let mut types = TypeSection::new();
        types.ty().function(params, results);
        module.section(&types);
        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);
        let mut exports = ExportSection::new();
        exports.export("f", ExportKind::Func, 0);
        module.section(&exports);
        let mut code = wasm_encoder::CodeSection::new();
        code.function(&f);
        module.section(&code);
        wasmparser::Validator::new()
            .validate_all(&module.finish())
            .expect("module validates");
    }

    #[test]
    fn body_size_is_the_encoded_body_size() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[ident_param("p", ValType::I32)]).unwrap();
        assert_eq!(emitter.body_size(), 2, "no locals, then `end`");
        let any = ValType::Ref(wasm_encoder::RefType {
            nullable: true,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        let concrete = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(300),
        });
        for i in 0..200 {
            emitter
                .add_anonymous_local([ValType::F64, any, concrete][i % 3])
                .unwrap();
            emitter.instruction(Instruction::I32Const(i as i32 * 1_000));
            emitter.instruction(Instruction::Drop);
            if i % 50 == 0 {
                let _ = emitter.body_size();
            }
        }
        let size = emitter.body_size();
        assert_eq!(size, emitter.build().unwrap().byte_len());
    }

    #[test]
    fn empty_function_validates() {
        let f = fixture();
        let cx = cx_of(&f);
        let emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        let func = emitter.build().unwrap();
        validate_in_module(func, vec![], vec![]);
    }

    #[test]
    fn params_become_named_locals_at_low_indices() {
        let f = fixture();
        let cx = cx_of(&f);
        let emitter = FunctionEmitter::new(
            &cx,
            &[
                ident_param("a", ValType::F64),
                ident_param("b", ValType::F64),
            ],
        )
        .unwrap();
        assert_eq!(emitter.local_slot("a"), Some(0));
        assert_eq!(emitter.local_slot("b"), Some(1));
    }

    #[test]
    fn define_local_returns_next_index_after_params() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(
            &cx,
            &[
                ident_param("a", ValType::F64),
                ident_param("b", ValType::F64),
            ],
        )
        .unwrap();
        let c = emitter.define_local(&ident("c"), ValType::I32).unwrap();
        assert_eq!(c, 2);
        assert_eq!(emitter.local_slot("c"), Some(2));
    }

    #[test]
    fn anonymous_local_is_unnamed() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        let idx = emitter.add_anonymous_local(ValType::I32).unwrap();
        assert_eq!(idx, 0);
        assert!(emitter.local_slot("anything").is_none());
    }

    #[test]
    fn nested_scopes_resolve_innermost_first() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        let outer = emitter.define_local(&ident("x"), ValType::I32).unwrap();
        emitter.push_scope();
        let inner = emitter.define_local(&ident("x"), ValType::I32).unwrap();
        assert_eq!(emitter.local_slot("x"), Some(inner));
        emitter.pop_scope().unwrap();
        assert_eq!(emitter.local_slot("x"), Some(outer));
    }

    #[test]
    fn pop_scope_drops_inner_locals() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.push_scope();
        emitter.define_local(&ident("temp"), ValType::I32).unwrap();
        assert!(emitter.local_slot("temp").is_some());
        emitter.pop_scope().unwrap();
        assert!(emitter.local_slot("temp").is_none());
    }

    #[test]
    fn block_depth_tracks_emit_end() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        assert_eq!(emitter.wasm_block_depth, 0);
        emitter.emit_block(BlockType::Empty);
        assert_eq!(emitter.wasm_block_depth, 1);
        emitter.emit_block(BlockType::Empty);
        assert_eq!(emitter.wasm_block_depth, 2);
        emitter.emit_end();
        assert_eq!(emitter.wasm_block_depth, 1);
        emitter.emit_end();
        assert_eq!(emitter.wasm_block_depth, 0);
    }

    #[test]
    fn break_label_for_innermost_loop() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_while_open();
        assert_eq!(emitter.break_label().unwrap(), 1);
    }

    #[test]
    fn continue_label_for_innermost_loop() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_while_open();
        assert_eq!(emitter.continue_label().unwrap(), 0);
    }

    #[test]
    fn nested_loops_break_label_targets_inner() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_while_open(); // outer
        emitter.emit_while_open(); // inner
        assert_eq!(emitter.break_label().unwrap(), 1);
        assert_eq!(emitter.continue_label().unwrap(), 0);
        emitter.emit_while_close();
        assert_eq!(emitter.break_label().unwrap(), 1);
        assert_eq!(emitter.continue_label().unwrap(), 0);
        emitter.emit_while_close();
    }

    #[test]
    fn build_produces_valid_function() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.instruction(Instruction::Unreachable);
        let func = emitter.build().unwrap();
        validate_in_module(func, vec![], vec![ValType::F64]);
    }

    #[test]
    fn build_emits_declared_locals_in_order() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.add_anonymous_local(ValType::I32).unwrap();
        emitter.add_anonymous_local(ValType::F64).unwrap();
        let func = emitter.build().unwrap();
        validate_in_module(func, vec![], vec![]);
    }

    #[test]
    fn emit_const_string_emits_vtable_loaded_struct_new() {
        use wasm_encoder::Encode;
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_const_string(
            /* string_type */ 3, /* raw_string_type */ 0,
            /* string_vtable_global */ 5, /* consts_data */ 3, /* code_units */ 5,
        );
        let actual = emitter.build().unwrap();
        let mut actual_bytes = Vec::new();
        actual.encode(&mut actual_bytes);

        let mut expected = Function::new(std::iter::empty());
        expected.instruction(&Instruction::GlobalGet(5));
        expected.instruction(&Instruction::I32Const(0));
        expected.instruction(&Instruction::I32Const(5));
        expected.instruction(&Instruction::ArrayNewData {
            array_type_index: 0,
            array_data_index: 3,
        });
        expected.instruction(&Instruction::I64Const(0));
        expected.instruction(&Instruction::StructNew(3));
        expected.instruction(&Instruction::End);
        let mut expected_bytes = Vec::new();
        expected.encode(&mut expected_bytes);

        assert_eq!(actual_bytes, expected_bytes);
    }

    #[test]
    fn record_span_keeps_mappings_in_order() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();

        emitter.record_span(Span::new(crate::FileId(0), 0, 5).unwrap());
        emitter.instruction(Instruction::I32Const(1));
        emitter.record_span(Span::new(crate::FileId(0), 7, 9).unwrap());
        emitter.instruction(Instruction::I32Const(2));
        emitter.record_span(Span::new(crate::FileId(0), 11, 15).unwrap());
        emitter.instruction(Instruction::I32Add);

        assert_eq!(
            emitter.source_mappings,
            vec![
                (0, Span::new(crate::FileId(0), 0, 5).unwrap()),
                (1, Span::new(crate::FileId(0), 7, 9).unwrap()),
                (2, Span::new(crate::FileId(0), 11, 15).unwrap()),
            ]
        );
    }

    #[test]
    fn expr_context_distinct_variants() {
        assert_ne!(ExprContext::Value, ExprContext::Statement);
    }
    fn param(name: &str, ty: crate::Type, boxed: bool) -> crate::TypedParam {
        crate::TypedParam {
            name: ident(name),
            ty,
            boxed,
            rest: false,
            default: None,
        }
    }

    fn assert_internal<T>(
        result: Result<T, crate::compiler_error::CompilerFailure>,
        message: &str,
    ) {
        match result {
            Err(crate::compiler_error::CompilerFailure::Internal {
                stage,
                message: actual,
                ..
            }) => {
                assert_eq!(stage, crate::compiler_error::CompilerStage::Codegen);
                assert!(actual.contains(message), "{actual}");
            }
            _ => panic!("expected internal failure containing {message}"),
        }
    }

    #[test]
    fn missing_scope_and_local_return_errors_without_mutation() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        assert_internal(emitter.require_local_slot("missing"), "not defined");
        assert_internal(emitter.write_slot("missing"), "not defined");
        assert_internal(emitter.pop_scope(), "root");
        assert_eq!(emitter.scopes.len(), 1);
        emitter.scopes.clear();
        assert_internal(emitter.define_local(&ident("x"), ValType::I32), "scope");
        assert_eq!(emitter.local_count(), 0);
        assert_internal(emitter.build(), "unfinished");
        validate_in_module(
            FunctionEmitter::new(&cx, &[]).unwrap().build().unwrap(),
            vec![],
            vec![],
        );
    }

    #[test]
    fn local_metadata_and_evaluation_marks_are_checked() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[ident_param("p", ValType::I32)]).unwrap();
        assert_internal(
            emitter.rebind_in_innermost_scope("p", 1, ValType::I32),
            "not allocated",
        );
        assert_internal(emitter.install_narrow_shadow("p", 0, ValType::F64), "type");
        assert_internal(emitter.set_this_local(1), "not allocated");
        assert_internal(emitter.end_single_evaluations(1), "mark");
        assert_eq!(emitter.local_slot("p"), Some(0));
        assert_eq!(emitter.narrowed_read_slot("p"), Some((0, ValType::I32)));
        emitter.scopes[0].define("bad".into(), 8, ValType::I32);
        assert_internal(emitter.build(), "not allocated");
    }

    #[test]
    fn parameter_prologue_rejects_lengths_types_and_wrong_binding_slots() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(
            &cx,
            &[
                ident_param("p", ValType::F64),
                ident_param("q", ValType::I32),
            ],
        )
        .unwrap();
        let params = [param("p", crate::Type::Number, false)];
        assert_internal(emitter.emit_boxed_param_prologue(&params, &[]), "one entry");
        assert_internal(
            emitter.emit_boxed_param_prologue(&params, &[9]),
            "not allocated",
        );
        assert_internal(emitter.emit_boxed_param_prologue(&params, &[1]), "type");
        let wrong_binding = [param("q", crate::Type::Number, false)];
        assert_internal(
            emitter.emit_boxed_param_prologue(&wrong_binding, &[0]),
            "binding",
        );
        emitter.emit_boxed_param_prologue(&params, &[0]).unwrap();
        assert!(emitter.instructions.is_empty());
        assert_eq!(emitter.local_count(), 2);
    }

    #[test]
    fn boxed_parameter_registration_failure_emits_no_prologue() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[ident_param("p", ValType::F64)]).unwrap();
        let span = Span::new(crate::FileId(0), 1, 3).unwrap();
        emitter.record_span(span);
        let error = emitter
            .emit_boxed_param_prologue(&[param("p", crate::Type::Number, true)], &[0])
            .unwrap_err();
        assert!(
            matches!(error, crate::compiler_error::CompilerFailure::Internal { span: Some(actual), .. } if actual == span)
        );
        assert!(emitter.instructions.is_empty());
        assert_eq!(emitter.local_count(), 1);
        assert_eq!(emitter.local_slot("p"), Some(0));
    }

    #[test]
    fn local_capacity_and_counter_overflow_fail_before_allocation() {
        let f = fixture();
        let cx = cx_of(&f);
        let params = vec![
            ident_param("p", ValType::I32);
            crate::compiler_limits::MAX_FUNCTION_LOCALS as usize + 1
        ];
        assert!(matches!(
            FunctionEmitter::new(&cx, &params),
            Err(crate::compiler_error::CompilerFailure::Limit { .. })
        ));
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.next_local_index = crate::compiler_limits::MAX_FUNCTION_LOCALS;
        assert!(matches!(
            emitter.add_anonymous_local(ValType::I32),
            Err(crate::compiler_error::CompilerFailure::Limit { .. })
        ));
        assert!(emitter.locals.is_empty());
        emitter.next_local_index = u32::MAX;
        assert_internal(emitter.add_anonymous_local(ValType::I32), "overflow");
        assert!(emitter.locals.is_empty());
    }

    #[test]
    fn invalid_block_operations_discard_the_body() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_end();
        assert_eq!(emitter.wasm_block_depth, 0);
        assert_internal(emitter.build(), "end without");
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.wasm_block_depth = u32::MAX;
        emitter.emit_while_open();
        assert_internal(emitter.build(), "overflow");
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_switch_open();
        emitter.emit_while_close();
        assert_internal(emitter.build(), "mismatched loop");
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.emit_if(BlockType::Empty);
        assert_internal(emitter.build(), "unfinished");
        validate_in_module(
            FunctionEmitter::new(&cx, &[]).unwrap().build().unwrap(),
            vec![],
            vec![],
        );
    }

    #[test]
    fn labels_reject_missing_or_stale_targets_and_continue_skips_switch() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        assert_internal(emitter.break_label(), "outside loop");
        assert_internal(emitter.continue_label(), "outside loop");
        assert_internal(emitter.return_label(), "outside the active");
        emitter.emit_while_open();
        emitter.emit_switch_open();
        assert_eq!(emitter.break_label().unwrap(), 0);
        assert_eq!(emitter.continue_label().unwrap(), 1);
        emitter.emit_switch_close();
        emitter.loop_contexts.last_mut().unwrap().continue_depth = 20;
        assert_internal(emitter.continue_label(), "outside the active");
    }

    #[test]
    fn malformed_raw_instructions_never_return_function_bytes() {
        let f = fixture();
        let cx = cx_of(&f);
        for instruction in [
            Instruction::LocalGet(0),
            Instruction::Br(1),
            Instruction::Else,
            Instruction::End,
            Instruction::BrTable(std::borrow::Cow::Owned(vec![1]), 0),
            Instruction::TryTable(
                BlockType::Empty,
                std::borrow::Cow::Owned(vec![wasm_encoder::Catch::AllRef { label: 1 }]),
            ),
        ] {
            let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
            emitter.instruction(instruction);
            assert!(matches!(
                emitter.build(),
                Err(crate::compiler_error::CompilerFailure::Internal { .. })
            ));
        }
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        emitter.instruction(Instruction::Block(BlockType::Empty));
        assert_internal(emitter.build(), "unclosed");
    }

    #[test]
    fn cast_branch_depths_are_checked_before_bytes_are_returned() {
        let f = fixture();
        let cx = cx_of(&f);
        let from_ref_type = wasm_encoder::RefType::ANYREF;
        let to_ref_type = wasm_encoder::RefType::EQREF;
        for instruction in [
            Instruction::BrOnCast {
                relative_depth: 1,
                from_ref_type,
                to_ref_type,
            },
            Instruction::BrOnCastFail {
                relative_depth: 1,
                from_ref_type,
                to_ref_type,
            },
            Instruction::BrOnCastDescEq {
                relative_depth: 1,
                from_ref_type,
                to_ref_type,
            },
            Instruction::BrOnCastDescEqFail {
                relative_depth: 1,
                from_ref_type,
                to_ref_type,
            },
        ] {
            let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
            emitter.instruction(instruction);
            assert_internal(emitter.build(), "branch depth");
        }
    }

    #[test]
    fn interned_string_and_closure_result_metadata_are_fallible() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        assert_internal(
            super::emit_const_string_by_text(&mut emitter, &cx, "absent"),
            "not interned",
        );
        assert_internal(
            super::emit_pooled_string(&mut emitter, &cx, usize::MAX),
            "pool",
        );
        assert!(emitter.instructions.is_empty());
        assert_internal(
            super::closure_return_target(&cx, &crate::Type::Number),
            "intrinsics",
        );
        assert!(matches!(
            super::closure_return_target(&cx, &crate::Type::Void).unwrap(),
            super::ReturnTarget::VoidClosure
        ));
    }

    #[test]
    fn latched_failure_prevents_finalization_and_retains_source() {
        let f = fixture();
        let cx = cx_of(&f);
        let emitter = FunctionEmitter::new(&cx, &[]).unwrap();
        let span = Span::new(crate::FileId(0), 2, 4).unwrap();
        cx.record_failure(
            crate::codegen::internal_failure("injected emitter failure").with_span(span),
        );
        let error = emitter.build_with_lines().unwrap_err();
        assert!(
            matches!(error, crate::compiler_error::CompilerFailure::Internal { span: Some(actual), .. } if actual == span)
        );
        validate_in_module(
            FunctionEmitter::new(&cx, &[]).unwrap().build().unwrap(),
            vec![],
            vec![],
        );
    }
}
