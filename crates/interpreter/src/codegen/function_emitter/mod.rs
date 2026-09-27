//! `FunctionEmitter` — single-pass builder for the body of one Wasm function.

pub mod cast;
pub mod expr;
mod finally;
pub mod json;
pub mod mcp;
pub mod stmt;

use std::collections::BTreeMap;

use wasm_encoder::{BlockType, Function, Instruction, ValType};

use crate::codegen::CodegenCtx;
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
    #[allow(dead_code)]
    ctx: &'a CodegenCtx<'a>,

    next_local_index: u32,
    locals: Vec<(u32, ValType)>,
    instructions: Vec<Instruction<'static>>,
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
    pub fn new(ctx: &'a CodegenCtx<'a>, params: &[(Ident, ValType)]) -> Self {
        let mut emitter = Self {
            ctx,
            runtime_type_params: BTreeMap::new(),
            next_local_index: 0,
            locals: Vec::new(),
            instructions: Vec::new(),
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
            single_evaluations: Vec::new(),
            cast_diagnostic: None,
        };
        for (name, ty) in params {
            let index = emitter.next_local_index;
            emitter.next_local_index += 1;
            emitter
                .scopes
                .last_mut()
                .expect("scopes was seeded with one frame")
                .define(name.name.clone(), index, *ty);
        }
        emitter
    }

    pub fn define_local(&mut self, name: &Ident, ty: ValType) -> u32 {
        let index = self.add_anonymous_local(ty);
        self.scopes
            .last_mut()
            .expect("define_local called with no active scope")
            .define(name.name.clone(), index, ty);
        index
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
    pub fn record_single_evaluation(&mut self, id: ExprId, slot: u32) {
        self.single_evaluations.push((id, slot));
    }

    /// Registrations are statement-scoped: later emission of a shared expression
    /// must compute its value again. Take a mark before emitting a statement
    /// that registers, and [`Self::end_single_evaluations`] with it afterwards.
    pub fn single_evaluation_mark(&self) -> usize {
        self.single_evaluations.len()
    }

    /// Drop every registration taken since `mark`. See
    /// [`Self::single_evaluation_mark`].
    pub fn end_single_evaluations(&mut self, mark: usize) {
        self.single_evaluations.truncate(mark);
    }

    pub fn add_anonymous_local(&mut self, ty: ValType) -> u32 {
        let index = self.next_local_index;
        self.next_local_index += 1;
        self.locals.push((1, ty));
        index
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
    pub fn write_slot(&mut self, name: &str) -> Option<u32> {
        self.clear_narrow_shadows(name);
        self.local_slot(name)
    }

    pub fn set_return_target(&mut self, target: ReturnTarget) {
        self.return_target = target;
    }

    pub fn return_target(&self) -> &ReturnTarget {
        &self.return_target
    }

    pub fn set_this_local(&mut self, index: u32) {
        self.this_local = Some(index);
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
    pub fn rebind_in_innermost_scope(&mut self, name: &str, index: u32, ty: ValType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.define(name.to_string(), index, ty);
        }
    }

    /// Record the shadow an assignment's narrowing just took. Narrowed reads
    /// resolve here until the next write clears it; writes never do.
    pub fn install_narrow_shadow(&mut self, name: &str, index: u32, ty: ValType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.set_shadow(name.to_string(), index, ty);
        }
    }

    /// Register the deferred live-read source for a field/index narrowing.
    /// Every narrowed use reloads this source and checks the current value.
    pub fn register_narrow_source(&mut self, name: &str, source: crate::ExprId) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.narrow_sources.insert(name.to_string(), source);
        }
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
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// `typed_slots[i]` is the Wasm slot holding the typed-form value of `params[i]` —
    /// index `i` for top-level functions, a prologue-allocated local for closure bodies.
    /// Boxing is body-internal; the Wasm signature is unchanged.
    pub fn emit_boxed_param_prologue(&mut self, params: &[crate::TypedParam], typed_slots: &[u32]) {
        assert_eq!(
            params.len(),
            typed_slots.len(),
            "typed_slots must have one entry per param"
        );
        for (param_idx, p) in params.iter().enumerate() {
            if !p.boxed {
                continue;
            }
            let box_idx = self
                .ctx
                .symbols
                .box_type_idx(&p.ty)
                .expect("box type registered for every boxed param");
            let box_val = ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(box_idx),
            });
            let shadow_idx = self.add_anonymous_local(box_val);
            self.instructions
                .push(Instruction::LocalGet(typed_slots[param_idx]));
            self.instructions.push(Instruction::StructNew(box_idx));
            self.instructions.push(Instruction::LocalSet(shadow_idx));
            self.rebind_in_innermost_scope(&p.name.name, shadow_idx, box_val);
        }
    }

    /// Prefer the structured helpers (emit_block/if/loop) to keep depth tracking consistent.
    pub fn instruction(&mut self, inst: Instruction<'static>) {
        self.instructions.push(inst);
    }

    pub fn record_span(&mut self, span: Span) {
        self.source_mappings.push((self.instructions.len(), span));
    }

    pub fn emit_block(&mut self, ty: BlockType) {
        self.instructions.push(Instruction::Block(ty));
        self.wasm_block_depth += 1;
    }

    pub fn emit_if(&mut self, ty: BlockType) {
        self.instructions.push(Instruction::If(ty));
        self.wasm_block_depth += 1;
    }

    pub fn emit_loop(&mut self, ty: BlockType) {
        self.instructions.push(Instruction::Loop(ty));
        self.wasm_block_depth += 1;
    }

    /// `else` doesn't change block depth — it's a continuation of the surrounding `if`.
    pub fn emit_else(&mut self) {
        self.instructions.push(Instruction::Else);
    }

    pub fn emit_end(&mut self) {
        self.wasm_block_depth -= 1;
        self.instructions.push(Instruction::End);
    }

    /// For instructions like `try_table` that open a block frame but are emitted via
    /// `instruction()` (which skips depth tracking); the matching `emit_end()` decrements.
    pub fn bump_block_depth(&mut self) {
        self.wasm_block_depth += 1;
    }

    /// The Wasm result this body's signature declares, or `None` when it has none.
    pub fn wasm_result_type(&self, ctx: &CodegenCtx) -> Option<ValType> {
        match &self.return_target {
            ReturnTarget::Slot(slot) => Some(*slot),
            ReturnTarget::Declared(ret) => Some(ctx.symbols.value_type(ret)),
            ReturnTarget::VoidClosure | ReturnTarget::NoResult => None,
        }
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
        self.loop_contexts.pop();
        self.emit_end();
    }

    pub fn emit_while_close(&mut self) {
        self.loop_contexts.pop();
        self.emit_end(); // end loop
        self.emit_end(); // end block
    }

    pub fn break_label(&self) -> u32 {
        let ctx = self
            .loop_contexts
            .last()
            .expect("break outside loop or switch");
        self.wasm_block_depth - ctx.break_depth - 1
    }

    /// Walks past switch frames — `continue` inside a switch targets the enclosing loop.
    pub fn continue_label(&self) -> u32 {
        let ctx = self
            .loop_contexts
            .iter()
            .rev()
            .find(|c| !c.is_switch)
            .expect("continue outside loop");
        self.wasm_block_depth - ctx.continue_depth - 1
    }

    pub fn return_label(&self) -> u32 {
        self.wasm_block_depth - self.return_block_depth - 1
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
        self.instruction(Instruction::I32Const(code_units as i32));
        self.instruction(Instruction::ArrayNewData {
            array_type_index: raw_string_type_idx,
            array_data_index: consts_data_idx,
        });
        self.instruction(Instruction::StructNew(string_type_idx));
    }

    pub fn build(self) -> Function {
        self.build_with_lines().0
    }

    /// Like `build`, but also returns DWARF byte offsets. Offsets are measured before each instruction is pushed.
    pub fn build_with_lines(self) -> (Function, Vec<(u64, Span)>) {
        let mut func = Function::new(self.locals);
        let mut byte_offsets: Vec<(u64, Span)> = Vec::with_capacity(self.source_mappings.len());
        let mut next_mapping = 0usize;

        for (i, inst) in self.instructions.iter().enumerate() {
            while next_mapping < self.source_mappings.len()
                && self.source_mappings[next_mapping].0 == i
            {
                let offset = func.byte_len() as u64;
                debug_assert!(
                    byte_offsets.last().is_none_or(|(prev, _)| *prev <= offset),
                    "line-program offsets must be monotonic; got {offset} after {:?}",
                    byte_offsets.last(),
                );
                byte_offsets.push((offset, self.source_mappings[next_mapping].1));
                next_mapping += 1;
            }
            func.instruction(inst);
        }
        // Drain spans recorded past the last instruction; their offset is the function's end byte.
        let end_offset = func.byte_len() as u64;
        while next_mapping < self.source_mappings.len() {
            byte_offsets.push((end_offset, self.source_mappings[next_mapping].1));
            next_mapping += 1;
        }
        func.instruction(&Instruction::End);
        (func, byte_offsets)
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
) {
    let pool_idx = ctx
        .strings
        .lookup_text(text)
        .unwrap_or_else(|| panic!("`{text}` was not interned by the StringPool"));
    let code_units = ctx.strings.code_units(pool_idx);
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .expect("$string intrinsic registered");
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .expect("$rawString intrinsic registered");
    let vtable_global_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable global imported from prelude");
    emitter.emit_const_string(
        string_type_idx,
        raw_string_type_idx,
        vtable_global_idx,
        pool_idx as u32,
        code_units,
    );
}

/// Push a fresh `(ref $string)` for a short codegen-owned literal (`"null"`,
/// `"[object Object]"`, …) — not pool-interned, built via `array.new_fixed`.
pub(crate) fn emit_inline_string_literal(
    emitter: &mut FunctionEmitter,
    ctx: &CodegenCtx,
    text: &str,
) {
    let string_type_idx = ctx
        .symbols
        .string_type_idx()
        .expect("$string intrinsic registered");
    let raw_string_type_idx = ctx
        .symbols
        .raw_string_type_idx()
        .expect("$rawString intrinsic registered");
    let vtable_global_idx = ctx
        .symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable global imported from prelude");
    emitter.instruction(Instruction::GlobalGet(vtable_global_idx));
    let mut len = 0u32;
    for unit in text.encode_utf16() {
        emitter.instruction(Instruction::I32Const(i32::from(unit)));
        len += 1;
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: raw_string_type_idx,
        array_size: len,
    });
    emitter.instruction(Instruction::StructNew(string_type_idx));
}

pub fn emit_function(
    generics: &[String],
    ctx: &CodegenCtx<'_>,
    params: &[crate::TypedParam],
    body: crate::StmtId,
    return_type: &crate::Type,
) -> (Function, Vec<(u64, Span)>) {
    let mut wasm_params: Vec<(Ident, ValType)> = params
        .iter()
        .map(|p| (p.name.clone(), ctx.symbols.value_type(&p.ty)))
        .collect();
    if !generics.is_empty() {
        wasm_params.push((
            Ident {
                name: "$types".into(),
                span: Span::at(ctx.file),
            },
            crate::codegen::runtime_descriptors::environment_type(ctx.symbols),
        ));
    }
    let mut emitter = FunctionEmitter::new(ctx, &wasm_params);
    crate::codegen::runtime_descriptors::bind(&mut emitter, generics, params.len() as u32);

    if !return_type.is_void() {
        emitter.set_return_target(ReturnTarget::Declared(return_type.clone()));
    }
    let typed_slots: Vec<u32> = (0..params.len() as u32).collect();
    emitter.emit_boxed_param_prologue(params, &typed_slots);
    stmt::emit_statement(&mut emitter, ctx, body);
    if !return_type.is_void() {
        // Keeps the function statically total even when Wasm validation can't prove all paths terminate.
        emitter.instruction(Instruction::Unreachable);
    }
    emitter.build_with_lines()
}

/// ABI: `(func (param (ref any) N×(ref $Object)) (result (ref $Object)?))` —
/// env at slot 0, every user param erased to `(ref $Object)`, result erased.
pub fn emit_closure_function(
    ctx: &CodegenCtx<'_>,
    meta: &crate::codegen::closures::ClosureMeta,
) -> (Function, Vec<(u64, Span)>) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
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
    let mut emitter = FunctionEmitter::new(ctx, &wasm_params);

    let env_type_idx = ctx
        .symbols
        .env_type_idx(meta.expr_id)
        .expect("env type registered for every closure expression");
    let env_typed_val = ValType::Ref(wasm_encoder::RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(env_type_idx),
    });
    let env_typed_local = emitter.add_anonymous_local(env_typed_val);
    if meta.this_type.is_some() {
        crate::codegen::this_binding::load_receiver(&mut emitter, ctx);
    }
    emitter.instructions.push(Instruction::LocalGet(0));
    if meta.this_type.is_some() {
        emitter.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(
                ctx.symbols.this_environment_type.expect("this environment"),
            ),
        ));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: ctx.symbols.this_environment_type.expect("this environment"),
            field_index: 0,
        });
    }
    if crate::codegen::call_arguments::typed_metadata(&meta.params).is_some() {
        crate::codegen::call_arguments::unwrap(&mut emitter, ctx);
    }
    emitter.instructions.push(Instruction::RefCastNonNull(
        wasm_encoder::HeapType::Concrete(env_type_idx),
    ));
    emitter
        .instructions
        .push(Instruction::LocalSet(env_typed_local));

    if let Some(name) = &meta.self_name {
        let slot = emitter.define_local(name, ctx.symbols.value_type(&meta.signature));
        emitter.instruction(Instruction::LocalGet(env_typed_local));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: env_type_idx,
            field_index: (meta.captured.len() + usize::from(!meta.runtime_generics.is_empty()))
                as u32,
        });
        cast::emit_cast_to(&mut emitter, ctx, &meta.signature);
        emitter.instruction(Instruction::LocalSet(slot));
    }
    if !meta.runtime_generics.is_empty() {
        let types = emitter.add_anonymous_local(
            crate::codegen::runtime_descriptors::environment_type(ctx.symbols),
        );
        emitter.instruction(Instruction::LocalGet(env_typed_local));
        emitter.instruction(Instruction::StructGet {
            struct_type_index: env_type_idx,
            field_index: meta.captured.len() as u32,
        });
        emitter.instruction(Instruction::LocalSet(types));
        crate::codegen::runtime_descriptors::bind(&mut emitter, &meta.runtime_generics, types);
    }
    let mut typed_slots: Vec<u32> = Vec::with_capacity(meta.params.len());
    for (i, p) in meta.params.iter().enumerate() {
        let wasm_slot = (i + 1) as u32;
        let typed_ty = ctx.symbols.value_type(&p.ty);
        let typed_local = emitter.add_anonymous_local(typed_ty);
        emitter.instructions.push(Instruction::LocalGet(wasm_slot));
        crate::codegen::cast_check::emit_checked_parameter_cast_on_stack(
            &mut emitter,
            ctx,
            &crate::Type::Unknown,
            &p.ty,
        );
        emitter
            .instructions
            .push(Instruction::LocalSet(typed_local));
        emitter.rebind_in_innermost_scope(&p.name.name, typed_local, typed_ty);
        typed_slots.push(typed_local);
    }

    // Boxed captures share the outer scope's box cell; unboxed captures copy the value directly.
    for (i, c) in meta.captured.iter().enumerate() {
        let local_ty = if c.boxed {
            let box_idx = ctx
                .symbols
                .box_type_idx(&c.ty)
                .expect("box type registered for every boxed capture");
            ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(box_idx),
            })
        } else {
            ctx.symbols.value_type(&c.ty)
        };
        let captured_local = emitter.define_local(&c.name, local_ty);
        emitter
            .instructions
            .push(Instruction::LocalGet(env_typed_local));
        emitter.instructions.push(Instruction::StructGet {
            struct_type_index: env_type_idx,
            field_index: i as u32,
        });
        // The env field may hold the erased `(ref null $Object)` form: env struct
        // types are emitted before class type indices are recorded, so a
        // class-typed capture's field widens while the local resolves concrete.
        if !c.boxed && matches!(c.ty.peel(), crate::Type::ClassRef { .. }) {
            cast::emit_cast_to(&mut emitter, ctx, &c.ty);
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
            emitter.set_this_local(captured_local);
            emitter.dynamic_this = c.ty == crate::Type::Unknown;
        }
    }

    emitter.emit_boxed_param_prologue(&meta.params, &typed_slots);

    emitter.set_return_target(closure_return_target(ctx, &meta.return_type));

    match meta.body {
        crate::ClosureBody::Expr(e) => {
            expr::emit_expr(&mut emitter, ctx, e);
            cast::emit_coerce_to_return_slot(&mut emitter, ctx, &ctx.ta.expr(e).ty);
        }
        crate::ClosureBody::Block(b) => {
            stmt::emit_statement(&mut emitter, ctx, b);
            if !meta.return_type.is_void() {
                emitter.instruction(Instruction::Unreachable);
            }
        }
    }

    emitter.build_with_lines()
}

/// A closure's result is fixed by its funcref signature: the erased
/// `(ref null $Object)`, or no slot at all when it returns void.
///
/// Must answer `void` the same way `closures::classify` does when it picks that
/// signature, or the target names a result slot the signature lacks.
fn closure_return_target(ctx: &CodegenCtx<'_>, ret: &Type) -> ReturnTarget {
    if ret.is_void() {
        return ReturnTarget::VoidClosure;
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    ReturnTarget::Slot(ValType::Ref(wasm_encoder::RefType {
        nullable: true,
        heap_type: wasm_encoder::HeapType::Concrete(intrinsics.object),
    }))
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
            line_index: LineIndex::new(""),
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
    fn empty_function_validates() {
        let f = fixture();
        let cx = cx_of(&f);
        let emitter = FunctionEmitter::new(&cx, &[]);
        let func = emitter.build();
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
        );
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
        );
        let c = emitter.define_local(&ident("c"), ValType::I32);
        assert_eq!(c, 2);
        assert_eq!(emitter.local_slot("c"), Some(2));
    }

    #[test]
    fn anonymous_local_is_unnamed() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        let idx = emitter.add_anonymous_local(ValType::I32);
        assert_eq!(idx, 0);
        assert!(emitter.local_slot("anything").is_none());
    }

    #[test]
    fn nested_scopes_resolve_innermost_first() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        let outer = emitter.define_local(&ident("x"), ValType::I32);
        emitter.push_scope();
        let inner = emitter.define_local(&ident("x"), ValType::I32);
        assert_eq!(emitter.local_slot("x"), Some(inner));
        emitter.pop_scope();
        assert_eq!(emitter.local_slot("x"), Some(outer));
    }

    #[test]
    fn pop_scope_drops_inner_locals() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.push_scope();
        emitter.define_local(&ident("temp"), ValType::I32);
        assert!(emitter.local_slot("temp").is_some());
        emitter.pop_scope();
        assert!(emitter.local_slot("temp").is_none());
    }

    #[test]
    fn block_depth_tracks_emit_end() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
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
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.emit_while_open();
        assert_eq!(emitter.break_label(), 1);
    }

    #[test]
    fn continue_label_for_innermost_loop() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.emit_while_open();
        assert_eq!(emitter.continue_label(), 0);
    }

    #[test]
    fn nested_loops_break_label_targets_inner() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.emit_while_open(); // outer
        emitter.emit_while_open(); // inner
        assert_eq!(emitter.break_label(), 1);
        assert_eq!(emitter.continue_label(), 0);
        emitter.emit_while_close();
        assert_eq!(emitter.break_label(), 1);
        assert_eq!(emitter.continue_label(), 0);
        emitter.emit_while_close();
    }

    #[test]
    fn build_produces_valid_function() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.instruction(Instruction::Unreachable);
        let func = emitter.build();
        validate_in_module(func, vec![], vec![ValType::F64]);
    }

    #[test]
    fn build_emits_declared_locals_in_order() {
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.add_anonymous_local(ValType::I32);
        emitter.add_anonymous_local(ValType::F64);
        let func = emitter.build();
        validate_in_module(func, vec![], vec![]);
    }

    #[test]
    fn emit_const_string_emits_vtable_loaded_struct_new() {
        use wasm_encoder::Encode;
        let f = fixture();
        let cx = cx_of(&f);
        let mut emitter = FunctionEmitter::new(&cx, &[]);
        emitter.emit_const_string(
            /* string_type */ 3, /* raw_string_type */ 0,
            /* string_vtable_global */ 5, /* consts_data */ 3, /* code_units */ 5,
        );
        let actual = emitter.build();
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
        let mut emitter = FunctionEmitter::new(&cx, &[]);

        emitter.record_span(Span::new(crate::FileId(0), 0, 5));
        emitter.instruction(Instruction::I32Const(1));
        emitter.record_span(Span::new(crate::FileId(0), 7, 9));
        emitter.instruction(Instruction::I32Const(2));
        emitter.record_span(Span::new(crate::FileId(0), 11, 15));
        emitter.instruction(Instruction::I32Add);

        assert_eq!(
            emitter.source_mappings,
            vec![
                (0, Span::new(crate::FileId(0), 0, 5)),
                (1, Span::new(crate::FileId(0), 7, 9)),
                (2, Span::new(crate::FileId(0), 11, 15)),
            ]
        );
    }

    #[test]
    fn expr_context_distinct_variants() {
        assert_ne!(ExprContext::Value, ExprContext::Statement);
    }
}
