use crate::compiler_error::CompilerFailure;

use std::collections::BTreeMap;

use crate::{
    ExprId, Ident, MangledName, ObjectField, Span, StmtId, Type, TypedExpr, TypedStmt,
    TypedStmtKind, ValueKind,
};

use super::{Inferer, narrowing};

/// Drops `null` from a reconstructed narrow-source type. Every prefix of a
/// narrowed path is provably non-null wherever a synthesized source is
/// evaluated (the guard has passed), and codegen's `FieldAccess` needs the bare
/// shape — it cannot lower a read through a nullable union. `None` when nothing
/// but `null` remains, which means the path can't carry a narrowing at all.
pub(super) fn non_null_form(ty: Type) -> Option<Type> {
    match ty.peel() {
        Type::Null => None,
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !matches!(m.peel(), Type::Null))
                .cloned()
                .collect();
            (!kept.is_empty()).then(|| Type::union(kept))
        }
        _ => Some(ty),
    }
}

fn field_member_ty(fields: &BTreeMap<String, ObjectField>, field_name: &str) -> Option<Type> {
    Some(fields.get(field_name)?.read_ty())
}

/// Enclosing narrowing state parked for the duration of a closure body.
/// Suspended rather than dropped so diagnostics can explain why a path that is
/// narrowed outside reads as its declared type inside.
pub(in crate::typechecker) struct SuspendedNarrowing {
    narrow_scopes: Vec<narrowing::NarrowEnv>,
    clause_write_scopes: Vec<std::collections::BTreeSet<narrowing::ReferencePath>>,
    assigned_scopes: Vec<std::collections::BTreeSet<narrowing::ReferencePath>>,
    tombstone_scopes:
        Vec<std::collections::BTreeMap<narrowing::ReferencePath, narrowing::InvalidationReason>>,
    pending_materializations: Vec<narrowing::PendingPostIfMaterialization>,
}

impl<'a> Inferer<'a> {
    /// A closure starts with fresh narrowing state, retaining bare bindings
    /// whose last assignment precedes its creation. Nested-function writes
    /// disqualify mutable bindings because their execution order is unknown.
    ///
    /// Surviving views are re-minted onto a seed frame and the caller re-emits
    /// the region *inside* the body, so the closure captures the ordinary
    /// binding and re-checks the cast per call — narrowing is never smuggled
    /// across the frame as a shadow local. The seeded sources are plain
    /// `LocalRef`/`GlobalRef` reads, so they name no narrow binding at all.
    ///
    /// A closure invoked where it is created starts with every narrowing at
    /// its call, as in TypeScript: nothing runs between the two.
    ///
    /// Call after the closure's params are in scope.
    pub(super) fn enter_closure_narrow_boundary(
        &mut self,
        span: Span,
        immediately_invoked: bool,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        let (active, _assigned) = self.snapshot_active_narrowings(0);
        self.suspend_narrow_scopes();
        let mut seed = narrowing::NarrowEnv::new();
        for (path, view) in active {
            let survives = if immediately_invoked {
                self.narrowing_reaches_invoked_body(&path)
            } else {
                self.narrowing_survives_closure(&path, span)
            };
            if !survives {
                continue;
            }
            let Some(source_kind) = self.synthesize_unnarrowed_source(&path, span)? else {
                continue;
            };
            let Some(from_ty) = self.declared_root_ty(&path) else {
                continue;
            };
            let source = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: source_kind,
                    span,
                    ty: from_ty,
                })
                .map_err(crate::typechecker::arena_failure)?;
            // Fresh binding: reusing the outer `#narrow_N` is exactly the
            // dangling cross-frame reference the reset exists to prevent.
            let binding = self.mint_narrow_binding(span)?;
            seed.insert(
                path,
                narrowing::NarrowedView {
                    binding,
                    source,
                    ..view
                },
            );
        }
        self.push_narrow_frame(seed.clone());
        Ok(seed)
    }

    /// A boundary no narrowing crosses: a nested function declaration is
    /// hoisted, so TypeScript types its body with the declared types of the
    /// variables it reads, however they are narrowed where it is declared.
    /// Leave it with [`Self::exit_closure_narrow_boundary`].
    pub(super) fn enter_function_declaration_narrow_boundary(&mut self) {
        self.suspend_narrow_scopes();
        self.push_narrow_frame(narrowing::NarrowEnv::new());
    }

    /// Function-body exit facts cannot materialize in a later declaration.
    /// Keep each top-level function or class member's flow state isolated just
    /// as nested function declarations are isolated from their enclosing body.
    pub(super) fn infer_body_with_narrowing_boundary(
        &mut self,
        body: crate::StmtId,
    ) -> Result<Option<crate::StmtId>, CompilerFailure> {
        self.enter_function_declaration_narrow_boundary();
        let typed_body = self.infer_stmt(body)?;
        self.exit_closure_narrow_boundary()?;
        Ok(typed_body)
    }

    fn suspend_narrow_scopes(&mut self) {
        self.suspended_narrow_scopes.push(SuspendedNarrowing {
            narrow_scopes: std::mem::take(&mut self.narrow_scopes),
            assigned_scopes: std::mem::take(&mut self.assigned_scopes),
            clause_write_scopes: std::mem::take(&mut self.clause_write_scopes),
            tombstone_scopes: std::mem::take(&mut self.tombstone_scopes),
            pending_materializations: std::mem::take(&mut self.pending_post_if_materializations),
        });
    }

    /// Drops the seed frame and restores the enclosing narrowing state. The
    /// seed frame's `assigned` set must not merge outward — an assignment
    /// inside a closure body says nothing about the enclosing frame's flow.
    pub(super) fn exit_closure_narrow_boundary(&mut self) -> Result<(), CompilerFailure> {
        self.pop_narrow_frame()?;
        let saved = self
            .suspended_narrow_scopes
            .pop()
            .ok_or_else(|| super::inference_failure("missing suspended narrowing scope"))?;
        self.narrow_scopes = saved.narrow_scopes;
        self.assigned_scopes = saved.assigned_scopes;
        self.clause_write_scopes = saved.clause_write_scopes;
        self.tombstone_scopes = saved.tombstone_scopes;
        self.pending_post_if_materializations = saved.pending_materializations;
        Ok(())
    }

    /// The sound subset that crosses a closure boundary, per
    /// `docs/narrowing.md` § "Closure boundaries".
    ///
    /// Depth 0 only: a field or index path asserts something about a *heap*
    /// value, which a write can falsify before the closure runs, and
    /// `invalidate_for_write` cannot see across the boundary.
    ///
    /// Mutable roots must have no later assignments in the enclosing function
    /// and no writes in nested functions. Declaration identities keep shadowed
    /// bindings separate; statement ends account for branches and loops.
    fn narrowing_survives_closure(&self, path: &narrowing::ReferencePath, closure: Span) -> bool {
        if !path.chain.is_empty() {
            return false;
        }
        match &path.root {
            narrowing::BindingId::Local { name, decl_scope } => {
                if self.path_root_is_captured_mutator(path) {
                    return false;
                }
                // `decl_scope` equality rejects a root shadowed by one of the
                // closure's own params: same name, different binding.
                self.scopes.get(name).is_some_and(|entry| {
                    entry.decl_scope == *decl_scope
                        && (entry.is_const
                            || self
                                .last_assignments
                                .get(&entry.decl_span)
                                .is_none_or(|last| *last <= closure.start))
                })
            }
            narrowing::BindingId::Global(mangled) => self
                .top_symbols
                .values()
                .any(|e| &e.mangled_name == mangled && matches!(e.kind, ValueKind::Const { .. })),
            // Only a bare `this` reaches here (the depth-0 test above), and a
            // receiver is never null — there is no narrowing worth carrying into
            // the closure. `this.f` paths are depth 1 and already refused: the
            // field is heap state a write can falsify before the body runs.
            narrowing::BindingId::This => false,
        }
    }

    /// Whether a narrowing at an immediately-invoked call holds in its body:
    /// any whose root the body still reads, which a parameter may shadow.
    /// `this` is refused as in [`Self::narrowing_survives_closure`], since a
    /// function expression has its own.
    fn narrowing_reaches_invoked_body(&self, path: &narrowing::ReferencePath) -> bool {
        match &path.root {
            narrowing::BindingId::Local { name, decl_scope } => self
                .scopes
                .get(name)
                .is_some_and(|entry| entry.decl_scope == *decl_scope),
            narrowing::BindingId::Global(_) => true,
            narrowing::BindingId::This => false,
        }
    }

    /// The narrow binding a source expression ultimately reads through, if any.
    /// Walks the same receiver steps a [`narrowing::ReferencePath`] can hold —
    /// field *and* index — down to the root, which is a `LocalRef`, a
    /// `GlobalRef`, or the hazard this exists to catch: a `LocalNarrowRef`
    /// whose shadow may belong to a scope that has closed. Index steps matter
    /// because a view that can't be synthesized (`synthesize_*_source` bails on
    /// `PathElem::Index`) keeps the already-typed expression as its source, and
    /// that expression may read through an inner region's shadow.
    fn source_root_narrow_binding(
        &self,
        source: ExprId,
    ) -> Result<Option<String>, crate::compiler_error::CompilerFailure> {
        let mut id = source;
        loop {
            match &self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?
                .kind
            {
                crate::TypedExprKind::FieldAccess { receiver, .. }
                | crate::TypedExprKind::IndexAccess { receiver, .. } => id = *receiver,
                crate::TypedExprKind::LocalNarrowRef { binding, .. } => {
                    return Ok(Some(binding.name.clone()));
                }
                _ => return Ok(None),
            }
        }
    }

    /// Drops narrowings whose root binding has just gone out of scope.
    ///
    /// A loop folds its exits into the enclosing frame *before* popping the
    /// scope that holds the loop's own bindings (`for` pops its init scope
    /// after `fold_exits_into_outer`), so the join can hand the enclosing block
    /// a path rooted at a binding that no longer exists — and the region is
    /// emitted later, where reading it has no local slot.
    pub(super) fn drop_out_of_scope_narrowings(&mut self) {
        let doomed: Vec<narrowing::ReferencePath> = match self.narrow_scopes.last() {
            Some(frame) => frame
                .keys()
                .filter(|p| !self.path_root_in_scope(p))
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        if let Some(frame) = self.narrow_scopes.last_mut() {
            for path in doomed {
                frame.remove(&path);
            }
        }
        let pending = std::mem::take(&mut self.pending_post_if_materializations);
        self.pending_post_if_materializations = pending
            .into_iter()
            .filter(|m| self.path_root_in_scope(&m.path))
            .collect();
    }

    /// Whether a path's root binding, and each binding it indexes by, is still
    /// in lexical scope, by identity — `decl_scope` equality, so a same-named
    /// binding in a sibling scope does not count as the same root.
    pub(super) fn path_root_in_scope(&self, path: &narrowing::ReferencePath) -> bool {
        let keys = path.chain.iter().filter_map(|element| match element {
            narrowing::PathElem::Key(binding, _) => Some(binding),
            narrowing::PathElem::Field(_) | narrowing::PathElem::Index(_) => None,
        });
        std::iter::once(&path.root)
            .chain(keys)
            .all(|binding| self.binding_in_scope(binding))
    }

    fn binding_in_scope(&self, binding: &narrowing::BindingId) -> bool {
        match binding {
            narrowing::BindingId::Local { name, decl_scope } => self
                .scopes
                .get(name)
                .is_some_and(|e| e.decl_scope == *decl_scope),
            narrowing::BindingId::Global(_) => true,
            narrowing::BindingId::This => self.current_class.is_some(),
        }
    }

    /// Whether a narrow shadow named `name` is still live: defined by an open
    /// region, or by a real local (a post-`if` join rebinds a view to its
    /// declaring ident).
    fn shadow_binding_is_live(&self, name: &str) -> bool {
        self.narrow_scopes
            .iter()
            .flat_map(|f| f.values())
            .any(|v| v.binding.name == name)
            || self.scopes.get(name).is_some()
    }

    /// Whether a view's source can still be emitted: it either names no narrow
    /// shadow at all, or names one that is still live.
    ///
    /// "Still live" is the discriminator that makes this test useful. A shadow
    /// minted while inferring an `&&` RHS is already popped by the time the
    /// enclosing `if` re-runs its predicate, so a source naming it is dangling;
    /// a shadow from an *enclosing* region that is still open is fine, and such
    /// sources are common (`typeof x === "object" && "k" in x`).
    pub(super) fn source_shadow_is_live(
        &self,
        source: ExprId,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        Ok(self
            .source_root_narrow_binding(source)?
            .is_none_or(|name| self.shadow_binding_is_live(&name)))
    }

    /// As [`Self::source_shadow_is_live`], but a wrap site also has the env it
    /// is about to install: each of its views gets a region here, so naming one
    /// of their bindings is legal even though no frame holds them yet.
    ///
    /// Checked with a typed failure so a future gap fails in the compiler at
    /// the site that caused it, rather than as an opaque codegen panic. Reads
    /// the live `narrow_scopes`, never a suspended closure frame: inside a
    /// closure body only the fresh stack is in scope.
    fn narrow_source_is_in_scope(
        &self,
        env: &narrowing::NarrowEnv,
        source: ExprId,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        Ok(self.source_root_narrow_binding(source)?.is_none_or(|name| {
            env.values().any(|v| v.binding.name == name) || self.shadow_binding_is_live(&name)
        }))
    }

    /// Whether an enclosing (suspended) closure boundary narrows this path —
    /// the signal that the declared type a caller is complaining about is the
    /// closure reset, not a missing guard.
    pub(super) fn narrowed_in_suspended_frame(&self, path: &narrowing::ReferencePath) -> bool {
        self.suspended_narrow_scopes
            .iter()
            .flat_map(|s| s.narrow_scopes.iter())
            .any(|env| env.contains_key(path))
    }

    /// Declared type of a depth-0 path's root — the `from_ty` a seeded region
    /// casts away from. Keeps `null`: the cast is declared → narrowed.
    pub(super) fn declared_root_ty(&self, path: &narrowing::ReferencePath) -> Option<Type> {
        match &path.root {
            narrowing::BindingId::Local { name, .. } => Some(self.scopes.get(name)?.ty.clone()),
            narrowing::BindingId::This => self.current_class.clone(),
            narrowing::BindingId::Global(mangled) => {
                let entry = self
                    .top_symbols
                    .values()
                    .find(|e| &e.mangled_name == mangled)?;
                match &entry.kind {
                    ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. } => Some(ty.clone()),
                    _ => None,
                }
            }
        }
    }

    /// Recreate body-exit facts in a region at a loop's condition or update.
    /// Body-local bindings have left scope; surviving paths get fresh shadows
    /// so the tail never refers to a region defined inside the body.
    pub(super) fn loop_tail_env(
        &mut self,
        env: &narrowing::NarrowEnv,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        let mut tail = narrowing::NarrowEnv::new();
        tail.dropped = env
            .dropped
            .iter()
            .filter(|(path, _)| self.path_root_in_scope(path))
            .map(|(path, ty)| (path.clone(), ty.clone()))
            .collect();
        for (path, view) in env {
            if !self.path_root_in_scope(path) || matches!(view.narrowed_ty, Type::Error) {
                continue;
            }
            let span = self
                .typed_ast
                .try_expr(view.source)
                .map_err(crate::typechecker::arena_failure)?
                .span;
            tail.insert(
                path.clone(),
                narrowing::NarrowedView {
                    binding: self.mint_narrow_binding(span)?,
                    ..view.clone()
                },
            );
        }
        Ok(tail)
    }

    pub(super) fn push_narrow_frame(&mut self, mut env: narrowing::NarrowEnv) {
        let tombstones = std::mem::take(&mut env.dropped)
            .into_iter()
            .map(|(path, narrowed_ty)| {
                (
                    path,
                    narrowing::InvalidationReason::ShapeUnrebuildable { narrowed_ty },
                )
            })
            .collect();
        self.narrow_scopes.push(env);
        self.assigned_scopes.push(std::collections::BTreeSet::new());
        self.tombstone_scopes.push(tombstones);
    }

    pub(super) fn pop_narrow_frame(&mut self) -> Result<(), CompilerFailure> {
        self.pop_narrow_frame_capture()?;
        Ok(())
    }

    /// Infers an operand that may not run — the right side of `&&`, `||`, or
    /// `??`, or a ternary branch — under `env`. A write in it may have happened,
    /// so it still invalidates outer narrowings, but the narrowing the write
    /// installs does not outlive the operand: `c && (x = null)` leaves `x` at its
    /// declared type, not `null`, and not the view it had before.
    pub(super) fn infer_conditional_operand(
        &mut self,
        operand: ExprId,
        env: &narrowing::NarrowEnv,
        expected: Option<&Type>,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        self.push_narrow_frame(env.clone());
        let inferred = self.infer_expr(operand, expected)?;
        let (_, assigned) = self.pop_narrow_frame_capture()?;
        let span = self
            .ast
            .try_expr(operand)
            .map_err(super::arena_failure)?
            .span;
        self.merge_assigned_into_outer(assigned, span);
        Ok(inferred)
    }

    pub(super) fn pop_narrow_frame_capture(
        &mut self,
    ) -> Result<
        (
            narrowing::NarrowEnv,
            std::collections::BTreeSet<narrowing::ReferencePath>,
        ),
        CompilerFailure,
    > {
        if self.narrow_scopes.len() != self.assigned_scopes.len()
            || self.narrow_scopes.len() != self.tombstone_scopes.len()
        {
            return Err(super::inference_failure(
                "narrowing scope stack lengths differ",
            ));
        }
        let mut narrowings = self
            .narrow_scopes
            .pop()
            .ok_or_else(|| super::inference_failure("missing narrowing frame"))?;
        let assigned = self
            .assigned_scopes
            .pop()
            .ok_or_else(|| super::inference_failure("missing assignment frame"))?;
        for (path, reason) in self
            .tombstone_scopes
            .pop()
            .ok_or_else(|| super::inference_failure("missing narrowing tombstone frame"))?
        {
            if let narrowing::InvalidationReason::ShapeUnrebuildable { narrowed_ty } = reason
                && !narrowings.contains_key(&path)
            {
                narrowings.dropped.insert(path, narrowed_ty);
            }
        }
        Ok((narrowings, assigned))
    }

    pub(super) fn push_pending_join_frame(&mut self, kind: narrowing::PendingJoinKind) {
        self.pending_joins.push(narrowing::PendingJoinFrame {
            kind,
            narrow_depth: self.narrow_scopes.len(),
            breaks: Vec::new(),
            continues: Vec::new(),
        });
    }

    pub(super) fn pop_pending_join_frame(
        &mut self,
    ) -> Result<narrowing::PendingJoinFrame, CompilerFailure> {
        self.pending_joins
            .pop()
            .ok_or_else(|| super::inference_failure("pending join push/pop mismatch"))
    }

    /// Iterates outer-to-inner so inner entries overwrite outer ones,
    /// matching the inner-out lookup order.
    pub(super) fn snapshot_active_narrowings(
        &self,
        base_depth: usize,
    ) -> (
        narrowing::NarrowEnv,
        std::collections::BTreeSet<narrowing::ReferencePath>,
    ) {
        let mut env = narrowing::NarrowEnv::new();
        let mut assigned = std::collections::BTreeSet::new();
        for frame_idx in base_depth..self.narrow_scopes.len() {
            if let Some(tombs) = self.tombstone_scopes.get(frame_idx) {
                env.retain(|path, _| {
                    !tombs
                        .iter()
                        .any(|(written, reason)| reason.invalidates() && written.is_prefix_of(path))
                });
            }
            if let Some(tombs) = self.tombstone_scopes.get(frame_idx) {
                env.dropped.retain(|path, _| {
                    !tombs
                        .iter()
                        .any(|(written, reason)| reason.invalidates() && written.is_prefix_of(path))
                });
                for (path, reason) in tombs {
                    if let narrowing::InvalidationReason::ShapeUnrebuildable { narrowed_ty } =
                        reason
                    {
                        env.dropped.insert(path.clone(), narrowed_ty.clone());
                    }
                }
            }
            for (path, view) in &self.narrow_scopes[frame_idx] {
                env.insert(path.clone(), view.clone());
            }
            if let Some(frame_assigned) = self.assigned_scopes.get(frame_idx) {
                for path in frame_assigned {
                    assigned.insert(path.clone());
                }
            }
        }
        (env, assigned)
    }

    /// Join complete reachable exit snapshots and invalidate their writes.
    /// Returns whether any exit exists, independently of the final body's
    /// reachability (a loop can execute zero iterations).
    pub(super) fn fold_exits_into_outer(
        &mut self,
        natural_exit_env: Option<narrowing::NarrowEnv>,
        breaks: Vec<(
            narrowing::NarrowEnv,
            std::collections::BTreeSet<narrowing::ReferencePath>,
        )>,
        anchor_span: Span,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        let mut all_assigned: std::collections::BTreeSet<narrowing::ReferencePath> =
            std::collections::BTreeSet::new();
        for (_, b_assigned) in &breaks {
            all_assigned.extend(b_assigned.iter().cloned());
        }
        self.merge_assigned_into_outer(all_assigned, anchor_span);

        let mut exits: Vec<narrowing::NarrowEnv> = breaks.into_iter().map(|(env, _)| env).collect();
        if let Some(natural) = natural_exit_env {
            exits.push(natural);
        }
        let Some(post_env) = join_exit_envs(exits) else {
            return Ok(false);
        };
        if !post_env.is_empty() {
            self.install_joined_narrowings(post_env, anchor_span)?;
        }
        Ok(true)
    }

    /// Match the declaration, not its spelling: unrelated same-named locals stay stable.
    pub(super) fn path_root_is_captured_mutator(&self, path: &narrowing::ReferencePath) -> bool {
        match &path.root {
            narrowing::BindingId::Local { name, decl_scope } => self
                .scopes
                .get_binding(name, *decl_scope)
                .is_some_and(|entry| {
                    self.captured_mutators
                        .contains(&(name.clone(), entry.decl_span))
                }),
            narrowing::BindingId::Global(_) | narrowing::BindingId::This => false,
        }
    }

    /// The type a binding declared `declared` narrows to after a write of a
    /// `written` value. It is the value's own type, as for any write, unless the
    /// declaration has something `readonly` in it, or the value is a function.
    /// A `readonly` array, field, or property the declaration names must survive
    /// `o = { xs: [1] }`, and a function may declare fewer parameters than the
    /// declared type passes: `f = (x) => …` must still take `f(1, 2)`. Then the
    /// binding narrows to declared members, as TypeScript's assignment narrowing
    /// does (see [`Self::narrowed_part`]).
    pub(super) fn assignment_narrowed_ty(&self, declared: &Type, written: Type) -> Type {
        if !self.declares_readonly(declared) && !has_function_part(&written) {
            return written;
        }
        self.initializer_narrowed_ty(declared, written)
    }

    pub(super) fn initializer_narrowed_ty(&self, declared: &Type, written: Type) -> Type {
        let members: Vec<&Type> = match declared.peel_preserving_readonly() {
            Type::Union(members) => members.iter().collect(),
            other => vec![other],
        };
        let parts: Vec<Type> = match written.peel() {
            Type::Union(parts) => parts.clone(),
            _ => vec![written.clone()],
        };
        let narrowed: Option<Vec<Type>> = parts
            .into_iter()
            .map(|part| self.narrowed_part(&members, part))
            .collect();
        narrowed.map_or_else(|| declared.clone(), Type::union)
    }

    /// What a written `part` narrows its binding to, in order:
    /// - a primitive or literal: itself;
    /// - members differing only in `readonly` (`readonly T[] | T[]`): the readonly one;
    /// - the most specific accepting member, when it keeps every `readonly` of
    ///   the value's own;
    /// - among function types, the one with the most parameters, which is how
    ///   TypeScript calls their union;
    /// - a subclass instance's own class, when an ancestor class is that member;
    /// - otherwise every accepting member, which is TypeScript's answer.
    ///
    /// `None` when no member accepts it, which only a write already reported as
    /// an error reaches.
    fn narrowed_part(&self, members: &[&Type], part: Type) -> Option<Type> {
        if narrows_to_itself(&part) {
            return Some(part);
        }
        let accepting: Vec<&Type> = members
            .iter()
            .copied()
            .filter(|member| super::assignable(&part, member, self.resolver()))
            .collect();
        if accepting.is_empty() {
            return None;
        }
        // Members that differ only in `readonly` (`readonly T[] | T[]`) narrow to
        // the readonly one: it permits every read either does, and no write.
        if let Some(readonly) = accepting.iter().find(|m| m.is_readonly_array())
            && accepting.iter().all(|m| m.peel() == readonly.peel())
        {
            return Some((*readonly).clone());
        }
        if let Some(longest) = longest_function_member(&accepting) {
            return Some(longest.clone());
        }
        match self.most_specific_member(&accepting) {
            Some(member) if !self.may_lose_readonly(&part, member) => Some(member.clone()),
            // A subclass instance keeps what it makes `readonly` of its
            // ancestor's fields only as its own class, which has every field
            // and method of the ancestor.
            Some(member)
                if is_class_seen_as_class(&part, member)
                    && !self.may_lose_readonly(member, &part) =>
            {
                Some(part)
            }
            _ => Some(Type::union(accepting.into_iter().cloned().collect())),
        }
    }

    /// The accepting member assignable to all the others. Members assignable to
    /// each other are equally specific; one stands for the rest when it lists
    /// only fields they all list and keeps every `readonly` of theirs, so it
    /// permits no write and no read their union would not.
    fn most_specific_member<'t>(&self, accepting: &[&'t Type]) -> Option<&'t Type> {
        let most_specific: Vec<&Type> = accepting
            .iter()
            .copied()
            .filter(|candidate| {
                accepting
                    .iter()
                    .all(|other| super::assignable(candidate, other, self.resolver()))
            })
            .collect();
        most_specific.iter().copied().find(|candidate| {
            most_specific.iter().all(|other| {
                self.fields_within(candidate, other) && !self.may_lose_readonly(other, candidate)
            })
        })
    }

    /// Whether every field `candidate` lists, `other` lists too, at any depth:
    /// inside each shared field, array element, tuple element, function return,
    /// and union alternative.
    fn fields_within(&self, candidate: &Type, other: &Type) -> bool {
        self.fields_within_compared(candidate, other, &mut FieldComparison::default())
    }

    fn fields_within_compared(
        &self,
        candidate: &Type,
        other: &Type,
        comparison: &mut FieldComparison,
    ) -> bool {
        if candidate.peel() == other.peel() {
            return true;
        }
        let pair = (candidate.clone(), other.clone());
        if comparison.refuted.contains(&pair) {
            return false;
        }
        if comparison.in_progress.contains(&pair) {
            return true;
        }
        if comparison.in_progress.len() >= MAX_FIELD_COMPARISON_DEPTH {
            return false;
        }
        comparison.in_progress.insert(pair.clone());
        let within = self.fields_within_step(candidate, other, comparison);
        comparison.in_progress.remove(&pair);
        if !within {
            comparison.refuted.insert(pair);
        }
        within
    }

    fn fields_within_step(
        &self,
        candidate: &Type,
        other: &Type,
        comparison: &mut FieldComparison,
    ) -> bool {
        let candidates = alternatives(candidate);
        let others = alternatives(other);
        if candidates.len() > 1 || others.len() > 1 {
            // Every `candidate` alternative fits within some `other` one, and
            // every `other` alternative has some `candidate` one within it. A
            // lone type is its own single alternative, so `string` meets
            // `"a" | string`.
            return candidates.iter().all(|c| {
                others
                    .iter()
                    .any(|o| self.fields_within_compared(c, o, comparison))
            }) && others.iter().all(|o| {
                candidates
                    .iter()
                    .any(|c| self.fields_within_compared(c, o, comparison))
            });
        }
        match (candidate.peel(), other.peel()) {
            (Type::Array(c), Type::Array(o))
            | (Type::Function { ret: c, .. }, Type::Function { ret: o, .. }) => {
                self.fields_within_compared(c, o, comparison)
            }
            (Type::Tuple(cs), Type::Tuple(os)) => {
                cs.len() == os.len()
                    && cs
                        .iter()
                        .zip(os)
                        .all(|(c, o)| self.fields_within_compared(c, o, comparison))
            }
            _ => match (self.member_shape(candidate), self.member_shape(other)) {
                (Some(candidate_fields), Some(other_fields)) => {
                    candidate_fields.iter().all(|(name, field)| {
                        other_fields.get(name).is_some_and(|other_field| {
                            self.fields_within_compared(&field.ty, &other_field.ty, comparison)
                        })
                    })
                }
                (None, None) => true,
                _ => false,
            },
        }
    }

    /// Whether seeing a `part` value as `member` could make writable something
    /// the value's own type forbids writing: a `readonly` of the value's, at a
    /// place the member reaches, that the member does not repeat there.
    fn may_lose_readonly(&self, part: &Type, member: &Type) -> bool {
        self.may_lose_readonly_within(part, member, &mut Vec::new())
    }

    /// `compared` holds the pairs already met. Meeting one again adds nothing,
    /// which keeps a recursive type finite.
    fn may_lose_readonly_within(
        &self,
        part: &Type,
        member: &Type,
        compared: &mut Vec<(Type, Type)>,
    ) -> bool {
        if part.peel() == member.peel() || !self.declares_readonly(part) {
            return false;
        }
        let pair = (part.clone(), member.clone());
        if compared.contains(&pair) {
            return false;
        }
        compared.push(pair);
        // Two instances of one generic type differ only in their arguments.
        if let (Some((part_name, part_args)), Some((member_name, member_args))) =
            (named_instance(part), named_instance(member))
            && part_name == member_name
        {
            return part_args
                .iter()
                .zip(member_args)
                .any(|(p, m)| self.may_lose_readonly_within(p, m, compared));
        }
        let part = super::assignable::expand_alias_ref(part, self.resolver());
        let member = super::assignable::expand_alias_ref(member, self.resolver());
        match (
            part.peel_preserving_readonly(),
            member.peel_preserving_readonly(),
        ) {
            (Type::Union(parts), _) => parts
                .iter()
                .any(|p| self.may_lose_readonly_within(p, &member, compared)),
            // A write through a union is refused where any member refuses it,
            // so one member that keeps the `readonly` is enough.
            (_, Type::Union(members)) => members
                .iter()
                .all(|m| self.may_lose_readonly_within(&part, m, compared)),
            (Type::Readonly(p), Type::Readonly(m)) | (Type::Array(p), Type::Array(m)) => {
                self.may_lose_readonly_within(p, m, compared)
            }
            (Type::Readonly(_), _) => true,
            (_, Type::Readonly(m)) => self.may_lose_readonly_within(&part, m, compared),
            (Type::Tuple(ps), Type::Tuple(ms)) => {
                ps.len() != ms.len()
                    || ps
                        .iter()
                        .zip(ms)
                        .any(|(p, m)| self.may_lose_readonly_within(p, m, compared))
            }
            (Type::Function { ret: p, .. }, Type::Function { ret: m, .. }) => {
                self.may_lose_readonly_within(p, m, compared)
            }
            _ => self.fields_lose_readonly(&part, &member, compared),
        }
    }

    /// [`Self::may_lose_readonly_within`] over the fields `member` reaches,
    /// assuming the worst for a shape it cannot list.
    fn fields_lose_readonly(
        &self,
        part: &Type,
        member: &Type,
        compared: &mut Vec<(Type, Type)>,
    ) -> bool {
        let (Some(part_fields), Some(member_fields)) =
            (self.member_shape(part), self.member_shape(member))
        else {
            return true;
        };
        let part_forbids_write = self.write_forbidder(part.peel());
        member_fields.iter().any(|(name, m)| {
            part_fields.get(name).is_some_and(|p| {
                (part_forbids_write(name, p) && !m.readonly)
                    || self.may_lose_readonly_within(&p.ty, &m.ty, compared)
            })
        })
    }

    /// Whether `ty` forbids a write somewhere a value of it can reach: a
    /// `readonly` array or tuple, or a `readonly` field or property, at any depth.
    fn declares_readonly(&self, ty: &Type) -> bool {
        self.declares_readonly_within(ty, &mut Vec::new())
    }

    /// `opened` holds the named types already examined: each is looked into
    /// once, which keeps a recursive or widely shared type graph linear.
    fn declares_readonly_within(&self, ty: &Type, opened: &mut Vec<MangledName>) -> bool {
        match ty.peel_preserving_readonly() {
            Type::Readonly(_) => true,
            Type::Array(element) => self.declares_readonly_within(element, opened),
            Type::Tuple(elements) | Type::Union(elements) => elements
                .iter()
                .any(|element| self.declares_readonly_within(element, opened)),
            Type::Object { fields, index } => {
                index
                    .as_ref()
                    .is_some_and(|i| i.readonly || self.declares_readonly_within(&i.value, opened))
                    || fields
                        .values()
                        .any(|f| f.readonly || self.declares_readonly_within(&f.ty, opened))
            }
            Type::Function { params, ret, .. } => params
                .iter()
                .chain(std::iter::once(ret.as_ref()))
                .any(|part| self.declares_readonly_within(part, opened)),
            named @ (Type::AliasRef { mangled, args, .. }
            | Type::InterfaceRef { mangled, args, .. }
            | Type::ClassRef { mangled, args, .. }) => {
                self.args_declare_readonly(args, opened)
                    || (self.open_once(mangled, opened)
                        && self.body_declares_readonly(named, opened))
            }
            _ => false,
        }
    }

    fn args_declare_readonly(&self, args: &[Type], opened: &mut Vec<MangledName>) -> bool {
        args.iter()
            .any(|arg| self.declares_readonly_within(arg, opened))
    }

    /// Records `mangled` as opened; false when it already was, since its body
    /// is being examined further up.
    fn open_once(&self, mangled: &MangledName, opened: &mut Vec<MangledName>) -> bool {
        if opened.contains(mangled) {
            return false;
        }
        opened.push(mangled.clone());
        true
    }

    /// Whether a named type's body forbids a write: a recursive alias's
    /// expansion, or a class's or interface's members.
    fn body_declares_readonly(&self, named: &Type, opened: &mut Vec<MangledName>) -> bool {
        if let Type::AliasRef { .. } = named {
            let body = super::assignable::expand_alias_ref(named, self.resolver());
            return self.declares_readonly_within(&body, opened);
        }
        let Some(fields) = self.member_shape(named) else {
            return false;
        };
        let forbids_write = self.write_forbidder(named);
        fields.iter().any(|(field_name, field)| {
            forbids_write(field_name, field) || self.declares_readonly_within(&field.ty, opened)
        })
    }

    /// A predicate: whether a field of `owner` forbids writing data through it,
    /// as a `readonly` field or property does and a method does not. A class and
    /// an interface list their methods apart, so a `readonly` function-valued
    /// property forbids writes as any other does.
    fn write_forbidder(&self, owner: &Type) -> impl Fn(&str, &ObjectField) -> bool {
        let methods = self.method_names(owner);
        move |name, field| field.readonly && !methods.iter().any(|method| method == name)
    }

    /// The names of a class's or interface's methods; none for anything else.
    fn method_names(&self, owner: &Type) -> Vec<String> {
        match owner {
            Type::ClassRef { mangled, args, .. } => {
                self.resolver().class_method_names(mangled, args)
            }
            Type::InterfaceRef { mangled, name, .. } => {
                self.resolver().interface_method_names(mangled, name)
            }
            _ => Vec::new(),
        }
    }

    /// Uses `ident` as `NarrowedView.binding` so `LocalNarrowRef` reads the existing Wasm slot
    /// with a cast-at-use, rather than allocating a shadow. Records `path` in
    /// `assigned_scopes` so branch joins can invalidate it.
    pub(super) fn install_assignment_narrowing(
        &mut self,
        path: narrowing::ReferencePath,
        ident: Ident,
        narrowed_ty: Type,
        span: Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.invalidate_for_reassignment(path.clone(), span);
        // The assignment still commits; only the narrowing is dropped.
        if self.path_root_is_captured_mutator(&path) {
            return Ok(());
        }
        // Assignment narrowings don't use `wrap_narrow_regions` (no shadow),
        // but `NarrowedView` requires a `source` field. Use a fresh `LocalRef`
        // as a placeholder.
        let source = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: crate::TypedExprKind::LocalRef {
                    ident: ident.clone(),
                    boxed: false,
                },
                span,
                ty: narrowed_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let view = narrowing::NarrowedView {
            narrowed_ty,
            facts: narrowing::TypeFacts::EMPTY,
            excluded_literals: std::collections::BTreeSet::new(),
            binding: ident,
            source,
        };
        if let Some(top_narrowings) = self.narrow_scopes.last_mut() {
            top_narrowings.insert(path.clone(), view);
        }
        self.last_write_spans.insert(path.clone(), span);
        let _: () = if let Some(top_assigned) = self.assigned_scopes.last_mut() {
            top_assigned.insert(path);
        };
        Ok(())
    }

    /// A field or index write falsified the guard on `path`.
    pub(super) fn invalidate_for_write(
        &mut self,
        path: narrowing::ReferencePath,
        write_span: Span,
    ) {
        self.invalidate(
            path,
            narrowing::InvalidationReason::Write { span: write_span },
        );
    }

    pub(super) fn index_read_ty(
        &self,
        receiver: ExprId,
        index: ExprId,
        declared: &Type,
    ) -> Result<Type, crate::compiler_error::CompilerFailure> {
        let kind = crate::TypedExprKind::IndexAccess { receiver, index };
        Ok(self
            .kind_to_reference_path(&kind)?
            .and_then(|path| self.lookup_narrowed_view(&path))
            .map_or_else(|| declared.clone(), |view| view.narrowed_ty.clone()))
    }

    /// Literal writes kill one element path; computed writes can affect every
    /// guarded element below the receiver, but do not replace the receiver itself.
    pub(super) fn invalidate_index_write(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        span: Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let kind = crate::TypedExprKind::IndexAccess { receiver, index };
        let _: () = if let Some(path) = self.kind_to_reference_path(&kind)? {
            self.invalidate_for_write(path, span);
        };
        Ok(())
        // TypeScript retains literal-index facts across computed writes. The
        // live read lowering preserves the actual element if that fact is stale.
    }

    /// An identifier was reassigned. Named apart from [`Self::invalidate_for_write`]
    /// only so the diagnostic can call the statement what the reader sees.
    pub(super) fn invalidate_for_reassignment(
        &mut self,
        path: narrowing::ReferencePath,
        write_span: Span,
    ) {
        self.invalidate(
            path,
            narrowing::InvalidationReason::Reassignment { span: write_span },
        );
    }

    /// Drops narrowings on `path` and its extensions, and records `path` as
    /// assigned so the kill outlives the frame.
    ///
    /// Both halves are load-bearing: the drop is what stops the next read
    /// resolving to a shadow the write invalidated, and the `assigned_scopes`
    /// record is what carries that past a branch join, where the narrow state is
    /// snapshotted and restored and the drop alone would be rolled back.
    fn invalidate(
        &mut self,
        path: narrowing::ReferencePath,
        reason: narrowing::InvalidationReason,
    ) {
        for writes in &mut self.clause_write_scopes {
            writes.insert(path.clone());
        }
        if let Some(span) = reason.span() {
            self.last_write_spans.insert(path.clone(), span);
        }
        self.drop_narrowings_under(&path, reason);
        if let Some(top_assigned) = self.assigned_scopes.last_mut() {
            top_assigned.insert(path);
        }
    }

    /// The drop without the record, for callers that must not claim the path was
    /// assigned *here*: a loop back edge (the write was on the previous iteration)
    /// rather than on the current path.
    ///
    /// Drops from the current frame and tombstones there. It does not reach into
    /// enclosing frames: a narrowing an outer frame installed stays installed, and
    /// the tombstone shadows it for as long as this frame lives
    /// ([`Self::lookup_narrowed_view`] stops at it). That is what keeps a write in
    /// one `if` arm out of the other arm, which never ran it.
    pub(super) fn drop_narrowings_under(
        &mut self,
        path: &narrowing::ReferencePath,
        reason: narrowing::InvalidationReason,
    ) {
        let Some(frame) = self.narrow_scopes.last_mut() else {
            return;
        };
        let mut dropped: Vec<narrowing::ReferencePath> = Vec::new();
        frame.retain(|key, _| {
            if path.is_prefix_of(key) {
                dropped.push(key.clone());
                false
            } else {
                true
            }
        });
        // Tombstone the written path itself even when this frame narrowed nothing:
        // the narrowing it kills may live any number of frames out.
        dropped.push(path.clone());
        if let Some(tombs) = self.tombstone_scopes.last_mut() {
            for dropped_path in dropped {
                tombs.insert(dropped_path, reason.clone());
            }
        }
    }

    /// Lifts an inner region's writes into the frame it exited into: drops the
    /// narrowings they falsify, tombstones the paths, and carries the set outward
    /// so enclosing joins see it too.
    ///
    /// The tombstone is what reaches *past* the nearest frame. The retain below
    /// clears one frame; a guard any number of regions further out is still live
    /// in its own frame, and [`Self::lookup_narrowed_view`] stops at the tombstone
    /// before it gets there. A narrowing installed into this frame *after* the
    /// merge — the join's own, or a block's surviving assignment narrowings —
    /// wins over the tombstone, since a frame's narrowing is checked first.
    pub(super) fn merge_assigned_into_outer(
        &mut self,
        inner_assigned: std::collections::BTreeSet<narrowing::ReferencePath>,
        write_span: Span,
    ) {
        if inner_assigned.is_empty() {
            return;
        }
        if let Some(outer_narrowings) = self.narrow_scopes.last_mut() {
            // A field-write to `obj.foo` kills narrowings on `obj.foo` and all
            // extending paths. Identifier paths have distinct roots, so `a` is
            // a prefix only of itself.
            outer_narrowings.retain(|key, _| !inner_assigned.iter().any(|p| p.is_prefix_of(key)));
        }
        for path in &inner_assigned {
            // The write itself, when it is known: `write_span` is the enclosing
            // statement, which underlines a whole `if` or loop body and puts the
            // caret on the wrong thing.
            let span = self
                .last_write_spans
                .get(path)
                .copied()
                .unwrap_or(write_span);
            if let Some(tombs) = self.tombstone_scopes.last_mut() {
                tombs.insert(path.clone(), narrowing::InvalidationReason::Write { span });
            }
        }
        if let Some(outer_assigned) = self.assigned_scopes.last_mut() {
            outer_assigned.extend(inner_assigned);
        }
    }

    /// Skips `Type::Null` — null has the same `(ref null $Object)` repr as a
    /// source nullable union, so wrapping it produces a no-op cast.
    pub(super) fn wrap_narrow_regions(
        &mut self,
        body: StmtId,
        env: &narrowing::NarrowEnv,
        span: Span,
    ) -> Result<StmtId, crate::compiler_error::CompilerFailure> {
        let mut wrapped = body;
        // The wrap-around-body loop makes the last entry OUTERMOST, and codegen
        // resolves LocalNarrowRef receivers by walking scope inside-out — hence
        // `wrap_order`, which puts root narrowings last.
        //
        // Synthesize each source via `synthesize_wrap_time_source` rather than
        // trusting `view.source`: `view.source` may hold ExprIds from an inner
        // predicate_envs call whose binding names differ from the current wrap's.
        let mut entries: Vec<(&narrowing::ReferencePath, &narrowing::NarrowedView)> =
            env.iter().collect();
        entries.sort_by(|(a, _), (b, _)| narrowing::wrap_order(a, b));
        for (path, view) in &entries {
            // Type::Error means the predicate eliminated all members (never).
            // Skip the codegen wrapper — no Wasm type for the shadow.
            // `lookup_narrowed_view` has a matching skip so `resolve_ident`
            // doesn't rewrite references to a binding that was never emitted.
            if matches!(view.narrowed_ty, Type::Error) {
                continue;
            }
            // A global gets no shadow. A shadow is a snapshot, and anything can
            // write a global between the guard and the read — a later statement,
            // a call, another function — so the snapshot goes stale and the read
            // returns a value the binding no longer holds. `LocalNarrowRef` falls
            // back to `global.get` plus the cast the narrowing proves, which
            // reads what is actually there. It is also what lets an assignment
            // narrowing on a global work: the two kinds would otherwise disagree
            // about where the current value lives.
            if path.chain.is_empty() && matches!(path.root, narrowing::BindingId::Global(_)) {
                continue;
            }
            let (source_id, cast_info) = self.wrap_time_source(env, path, view, span)?;
            wrapped = self
                .typed_ast
                .try_push_stmt(TypedStmt {
                    kind: TypedStmtKind::NarrowRegion {
                        path: (*path).clone(),
                        source: source_id,
                        binding: view.binding.clone(),
                        cast_info,
                        body: wrapped,
                    },
                    span,
                })
                .map_err(crate::typechecker::arena_failure)?;
        }
        Ok(wrapped)
    }

    /// Field read type on a narrowing receiver, shared by narrow-time
    /// ([`Self::synthesize_unnarrowed_source`]) and wrap-time
    /// ([`Self::synthesize_wrap_time_source`]) source synthesis. Unlike a bare
    /// structural lookup, this resolves named receivers and strips `null` from a
    /// union — the narrowing guard guarantees the receiver is non-null wherever
    /// a synthesized source is evaluated, so an `Iface | null` receiver reads
    /// its field fine here.
    ///
    /// Classes resolve through `class_field_visible`, not `structural_form`:
    /// it is the same authority the access site used to accept the read, so a
    /// same-module `private` field stays narrowable and a foreign-module one
    /// returns `None` (no narrowing) rather than a source reading a field the
    /// access site would have rejected.
    pub(super) fn narrow_source_field_ty(
        &self,
        receiver_ty: &Type,
        field_name: &str,
    ) -> Option<Type> {
        match receiver_ty.peel() {
            Type::Object { fields, index } => field_member_ty(fields, field_name)
                .or_else(|| index.as_ref().map(crate::IndexSignature::read_ty)),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let fields = self.structural_form(mangled, name, args)?;
                field_member_ty(&fields, field_name)
            }
            Type::ClassRef { mangled, args, .. } => {
                let (field, _decl) = self.class_field_visible(mangled, args, field_name)?;
                Some(ObjectField::widen_optional(
                    field.optional,
                    field.ty.clone(),
                ))
            }
            Type::Union(members) => {
                let mut tys = Vec::new();
                for m in members {
                    if matches!(m.peel(), Type::Null) {
                        continue;
                    }
                    tys.push(self.narrow_source_field_ty(m, field_name)?);
                }
                (!tys.is_empty()).then(|| Type::union(tys))
            }
            _ => None,
        }
    }

    /// The source expression a `NarrowRegion`/`Narrowed` node uses, plus its
    /// cast. A root view reads it at region entry; a field/index view retains it
    /// as its per-use live-read recipe. Rebuilt from declared types where
    /// possible; the stored `view.source` is the fallback, and the
    /// `debug_assert` is what keeps that fallback honest — a source naming a
    /// shadow from a closed scope would otherwise surface as an opaque codegen
    /// panic far from the cause.
    fn wrap_time_source(
        &mut self,
        env: &narrowing::NarrowEnv,
        path: &narrowing::ReferencePath,
        view: &narrowing::NarrowedView,
        span: Span,
    ) -> Result<(ExprId, narrowing::CastInfo), crate::compiler_error::CompilerFailure> {
        let (source, from_ty) = self
            .synthesize_wrap_time_source(env, path, span)?
            .map_or_else(
                || {
                    let from_ty = self
                        .typed_ast
                        .try_expr(view.source)
                        .map_err(crate::typechecker::arena_failure)?
                        .ty
                        .clone();
                    Ok((view.source, from_ty))
                },
                Ok::<_, crate::compiler_error::CompilerFailure>,
            )?;
        if !self.narrow_source_is_in_scope(env, source)? {
            return Err(super::inference_failure(
                "narrow source names a shadow outside its scope",
            ));
        }
        self.record_runtime_type_test(&view.narrowed_ty)?;
        Ok((
            source,
            narrowing::cast_info_for(from_ty, view.narrowed_ty.clone()),
        ))
    }

    /// Rebuilds the un-narrowed read a root region evaluates at entry or a
    /// field/index region evaluates per use. `None` means the path can't be
    /// reconstructed — e.g. a root the scope stack no longer
    /// holds, an index step, or an unresolved receiver field. The caller then
    /// falls back to `view.source` if its shadow is still live.
    pub(super) fn synthesize_wrap_time_source(
        &mut self,
        env: &narrowing::NarrowEnv,
        path: &narrowing::ReferencePath,
        span: Span,
    ) -> Result<Option<(ExprId, Type)>, crate::compiler_error::CompilerFailure> {
        let mut current_path = narrowing::ReferencePath::root(path.root.clone());
        // Only consult narrowings for PROPER prefixes — `path` itself is the
        // one we're narrowing right now; its binding hasn't been
        // allocated yet (codegen will allocate it as the result of
        // THIS NarrowRegion's `define_local`), so referencing it
        // from its own source would self-loop. A prefix missing from `env`
        // can still be narrowed by an *enclosing* region (`if (p !== null) {
        // if (p.values !== null) … }` — the inner env only holds `p.values`),
        // so fall back to the active narrow-scope stack: without it the root
        // read would carry its declared `Iface | null` type, and codegen can't
        // lower a field access through a nullable interface union.
        let root_view = if &current_path == path {
            None
        } else {
            env.get(&current_path)
                .or_else(|| self.lookup_narrowed_view(&current_path))
        };
        let (mut current_kind, mut current_ty) = match root_view {
            Some(view) => (
                crate::TypedExprKind::LocalNarrowRef {
                    binding: view.binding.clone(),
                    path: current_path.clone(),
                },
                view.narrowed_ty.clone(),
            ),
            None => match &path.root {
                narrowing::BindingId::Local { name, .. } => {
                    let Some(entry) = self.scopes.get(name) else {
                        return Ok(None);
                    };
                    (
                        crate::TypedExprKind::LocalRef {
                            ident: crate::Ident {
                                name: name.clone(),
                                span,
                            },
                            boxed: false,
                        },
                        entry.ty.clone(),
                    )
                }
                narrowing::BindingId::This => (
                    crate::TypedExprKind::This,
                    match self.current_class.clone() {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                ),
                narrowing::BindingId::Global(_) => (
                    match self.synthesize_unnarrowed_source(&current_path, span)? {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                    match self.declared_root_ty(&current_path) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                ),
            },
        };
        let mut current_id = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: current_kind.clone(),
                span,
                ty: current_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let _ = &mut current_kind;
        let chain_len = path.chain.len();
        for (idx, elem) in path.chain.iter().enumerate() {
            current_path.chain.push(elem.clone());
            let Some((step_kind, step_ty)) =
                self.synthesize_path_step(current_id, &current_ty, elem, span)?
            else {
                return Ok(None);
            };
            // Intermediate steps use the env-narrowed type so the next
            // step dispatches against the right shape. The final element keeps
            // the raw read type — the surrounding NarrowRegion's cast widens it
            // to `narrowed_ty`.
            let is_final = idx + 1 == chain_len;
            let (kind, ty) = if is_final {
                (step_kind, step_ty)
            } else {
                match env
                    .get(&current_path)
                    .or_else(|| self.lookup_narrowed_view(&current_path))
                {
                    Some(view) => (
                        crate::TypedExprKind::LocalNarrowRef {
                            binding: view.binding.clone(),
                            path: current_path.clone(),
                        },
                        view.narrowed_ty.clone(),
                    ),
                    // No enclosing region pinned this prefix, but the guard
                    // still proves it non-null here — codegen can't read a
                    // field through a nullable union.
                    None => match non_null_form(step_ty) {
                        Some(value) => (step_kind, value),
                        None => return Ok(None),
                    },
                }
            };
            current_id = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind,
                    span,
                    ty: ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            current_ty = ty;
        }
        Ok(Some((current_id, current_ty)))
    }

    /// One unnarrowed read of a path element on `receiver`, typed `receiver_ty`:
    /// the read and its declared type. None when the declared types can't
    /// rebuild it.
    fn synthesize_path_step(
        &mut self,
        receiver: ExprId,
        receiver_ty: &Type,
        elem: &narrowing::PathElem,
        span: Span,
    ) -> Result<Option<(crate::TypedExprKind, Type)>, crate::compiler_error::CompilerFailure> {
        let (index_kind, key_ty) = match elem {
            narrowing::PathElem::Field(field_name) => {
                let Some(field_ty) = self.narrow_source_field_ty(receiver_ty, field_name) else {
                    return Ok(None);
                };
                let read = crate::TypedExprKind::FieldAccess {
                    receiver,
                    name: crate::Ident {
                        name: field_name.clone(),
                        span,
                    },
                };
                return Ok(Some((read, field_ty)));
            }
            narrowing::PathElem::Index(narrowing::LiteralValue::Number(n)) => {
                (crate::TypedExprKind::Number(n.0), Type::NumberLiteral(*n))
            }
            narrowing::PathElem::Index(narrowing::LiteralValue::String(key)) => (
                crate::TypedExprKind::String(key.clone()),
                Type::StringLiteral(key.clone()),
            ),
            narrowing::PathElem::Index(narrowing::LiteralValue::Boolean(_)) => return Ok(None),
            narrowing::PathElem::Key(binding, _) => {
                let key_path = narrowing::ReferencePath::root(binding.clone());
                let (Some(kind), Some(key_ty)) = (
                    self.synthesize_unnarrowed_source(&key_path, span)?,
                    self.declared_root_ty(&key_path),
                ) else {
                    return Ok(None);
                };
                (kind, key_ty)
            }
        };
        let Some(read_ty) = self.declared_index_read_ty(receiver_ty, &key_ty, elem, span) else {
            return Ok(None);
        };
        let index = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: index_kind,
                span,
                ty: key_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(Some((
            crate::TypedExprKind::IndexAccess { receiver, index },
            read_ty,
        )))
    }

    /// The declared type of an index read by a key of type `key_ty`, as
    /// [`Self::infer_index_access`] types it, for a read it already accepted.
    fn declared_index_read_ty(
        &mut self,
        receiver_ty: &Type,
        key_ty: &Type,
        elem: &narrowing::PathElem,
        span: Span,
    ) -> Option<Type> {
        if receiver_ty.is_structural_object() {
            let diagnostics_before = self.diagnostics.len();
            let read_ty = self.object_index_read_type(receiver_ty, key_ty, span);
            if self.diagnostics.len() != diagnostics_before || matches!(read_ty, Type::Error) {
                self.diagnostics.truncate(diagnostics_before);
                return None;
            }
            return Some(read_ty);
        }
        let position = match elem {
            narrowing::PathElem::Index(narrowing::LiteralValue::Number(n))
                if n.0.is_finite() && n.0.fract() == 0.0 && n.0 >= 0.0 =>
            {
                Some(n.0 as usize)
            }
            _ => None,
        };
        let element_at = |member: &Type| match (member.peel(), position) {
            (Type::Array(element), _) => Some((**element).clone()),
            (Type::Tuple(elements), Some(position)) => elements.get(position).cloned(),
            _ => None,
        };
        match receiver_ty.peel() {
            Type::Union(members) => members
                .iter()
                .map(element_at)
                .collect::<Option<Vec<_>>>()
                .map(Type::union),
            member => element_at(member),
        }
    }

    /// Three cases:
    /// - Identifier paths (empty chain, local root): rebind `NarrowedView.binding`
    ///   to the root ident so `LocalNarrowRef` reads the existing Wasm slot.
    /// - Field paths: mint a fresh shadow and push a
    ///   `PendingPostIfMaterialization` for the enclosing `Block` to drain.
    /// - Globals: retain the view; runtime lowering reads their live storage.
    pub(super) fn install_joined_narrowings(
        &mut self,
        joined_narrowings: narrowing::NarrowEnv,
        if_span: Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let mut joined_rebound: Vec<(narrowing::ReferencePath, narrowing::NarrowedView)> =
            Vec::new();
        let mut field_path_mats: Vec<(narrowing::ReferencePath, narrowing::NarrowedView)> =
            Vec::new();
        if let Some(tombstones) = self.tombstone_scopes.last_mut() {
            for (path, narrowed_ty) in &joined_narrowings.dropped {
                tombstones.insert(
                    path.clone(),
                    narrowing::InvalidationReason::ShapeUnrebuildable {
                        narrowed_ty: narrowed_ty.clone(),
                    },
                );
            }
        }
        for (path, view) in joined_narrowings {
            if self.lookup_narrowed_view(&path).is_some_and(|active| {
                active.binding == view.binding && active.narrowed_ty == view.narrowed_ty
            }) {
                continue;
            }
            // A branch/loop exit can carry a path rooted at a binding declared
            // *inside* the body it left (`while (true) { const p = …; if (p ===
            // null) continue; break; }`). Installing it would rebind the view
            // to an ident that no longer exists at the join point, and the
            // region would read a local the frame never defines.
            if !self.path_root_in_scope(&path) {
                continue;
            }
            if !path.chain.is_empty() {
                field_path_mats.push((path, view));
                continue;
            }
            let binding_name = match &path.root {
                narrowing::BindingId::Local { name, .. } => name.clone(),
                narrowing::BindingId::Global(_) => {
                    joined_rebound.push((path, view));
                    continue;
                }
                // Neither a global nor `this` has a local slot to rebind onto.
                narrowing::BindingId::This => {
                    if let Some(tombstones) = self.tombstone_scopes.last_mut() {
                        tombstones.insert(
                            path,
                            narrowing::InvalidationReason::ShapeUnrebuildable {
                                narrowed_ty: view.narrowed_ty.clone(),
                            },
                        );
                    }
                    continue;
                }
            };
            let source_span = self
                .typed_ast
                .try_expr(view.source)
                .map_err(crate::typechecker::arena_failure)?
                .span;
            let rebound = narrowing::NarrowedView {
                binding: crate::Ident {
                    name: binding_name,
                    span: source_span,
                },
                ..view
            };
            joined_rebound.push((path, rebound));
        }
        // The loop above rebound or dropped every root path, so these are all non-root;
        // `wrap_pending_materializations` nests the first entry innermost, and
        // `wrap_order` puts the shallowest of them outermost.
        field_path_mats.sort_by(|(a, _), (b, _)| narrowing::wrap_order(a, b));
        // Identifier paths were just rebound to their real slots; a field path
        // rooted at one of them must resolve against those, not the
        // branch-local shadows the stored views still name.
        let rebound_env: narrowing::NarrowEnv = joined_rebound.iter().cloned().collect();
        for (path, view) in field_path_mats {
            if matches!(view.narrowed_ty, Type::Error) {
                continue;
            }
            let (source, cast_info) = self.wrap_time_source(&rebound_env, &path, &view, if_span)?;
            let binding = self.mint_narrow_binding(if_span)?;
            self.pending_post_if_materializations
                .push(narrowing::PendingPostIfMaterialization {
                    path: path.clone(),
                    source,
                    binding: binding.clone(),
                    cast_info,
                    span: if_span,
                });
            let rebound = narrowing::NarrowedView { binding, ..view };
            joined_rebound.push((path, rebound));
        }
        let _: () = if let Some(outer) = self.narrow_scopes.last_mut() {
            for (path, view) in joined_rebound {
                outer.insert(path, view);
            }
        };
        Ok(())
    }

    /// Innermost-out, matching `wrap_narrow_regions`'s nesting order.
    pub(super) fn wrap_pending_materializations(
        &mut self,
        body: StmtId,
        pending: Vec<narrowing::PendingPostIfMaterialization>,
    ) -> Result<StmtId, crate::compiler_error::CompilerFailure> {
        let mut wrapped = body;
        for mat in pending {
            wrapped = self
                .typed_ast
                .try_push_stmt(TypedStmt {
                    kind: TypedStmtKind::NarrowRegion {
                        path: mat.path,
                        source: mat.source,
                        binding: mat.binding,
                        cast_info: mat.cast_info,
                        body: wrapped,
                    },
                    span: mat.span,
                })
                .map_err(crate::typechecker::arena_failure)?;
        }
        Ok(wrapped)
    }

    /// Expression-level analogue of [`Self::wrap_narrow_regions`] for
    /// short-circuit `&&`/`||`. The `Narrowed` wrapper changes the shadow
    /// binding's type but preserves the surrounding expression's `ty`.
    pub(super) fn wrap_narrow_exprs(
        &mut self,
        inner: ExprId,
        env: &narrowing::NarrowEnv,
        span: Span,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        let mut wrapped = inner;
        // `wrap_order` puts root narrowings OUTERMOST so codegen defines each
        // shadow's slot before inner `LocalNarrowRef`s look it up. Unsorted, a
        // longer path (`o.name`) can wrap outside its root (`o`), and codegen
        // emits the outer source referencing an undefined `o` shadow.
        // Synthesize the source per wrap for the same reason as
        // `wrap_narrow_regions` — `view.source` may name an inner env's binding.
        let mut entries: Vec<(&narrowing::ReferencePath, &narrowing::NarrowedView)> =
            env.iter().collect();
        entries.sort_by(|(a, _), (b, _)| narrowing::wrap_order(a, b));
        for (path, view) in &entries {
            if matches!(view.narrowed_ty, Type::Error) {
                continue;
            }
            // Globals use live reads in expressions as well as statements.
            if path.chain.is_empty() && matches!(path.root, narrowing::BindingId::Global(_)) {
                continue;
            }
            let (source_id, cast_info) = self.wrap_time_source(env, path, view, span)?;
            let inner_ty = self
                .typed_ast
                .try_expr(wrapped)
                .map_err(crate::typechecker::arena_failure)?
                .ty
                .clone();
            wrapped = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: crate::TypedExprKind::Narrowed {
                        path: (*path).clone(),
                        source: source_id,
                        binding: view.binding.clone(),
                        cast_info,
                        inner: wrapped,
                    },
                    span,
                    ty: inner_ty,
                })
                .map_err(crate::typechecker::arena_failure)?;
        }
        Ok(wrapped)
    }
}

/// Whether a written value is, may be, or holds a function, whose own type may
/// declare fewer parameters than the declared one it stands for.
fn has_function_part(ty: &Type) -> bool {
    match ty.peel() {
        Type::Function { .. } => true,
        Type::Union(members) | Type::Tuple(members) => members.iter().any(has_function_part),
        Type::Array(elem) => has_function_part(elem),
        Type::Object { fields, index } => {
            fields.values().any(|field| has_function_part(&field.ty))
                || index.as_ref().is_some_and(|i| has_function_part(&i.value))
        }
        _ => false,
    }
}

/// A value whose own type is the narrowing a write gives its binding: a
/// primitive carries nothing a declaration could restrict.
fn narrows_to_itself(ty: &Type) -> bool {
    matches!(
        ty.peel(),
        Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::String
            | Type::StringLiteral(_)
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
            | Type::Error
            | Type::Never
    )
}

/// How deep [`Inferer::fields_within`] follows nested types before it gives
/// up and answers no, which narrows to the members' union instead. A generic
/// type that refers to itself with a larger argument (`Box<Box<T>>`) never
/// meets the same pair twice.
const MAX_FIELD_COMPARISON_DEPTH: usize = 32;

/// The state of one [`Inferer::fields_within`] question. A pair in progress is
/// assumed to hold, which keeps a recursive type finite; a pair refuted stays
/// refuted, since an assumption can only make an answer more permissive.
#[derive(Default)]
struct FieldComparison {
    in_progress: std::collections::BTreeSet<(Type, Type)>,
    refuted: std::collections::BTreeSet<(Type, Type)>,
}

/// A union's alternatives, or the type itself as the only one.
fn alternatives(ty: &Type) -> Vec<&Type> {
    match ty.peel() {
        Type::Union(members) => members.iter().collect(),
        _ => vec![ty],
    }
}

/// A named alias, class, or interface instance's name and type arguments.
fn named_instance(ty: &Type) -> Option<(&MangledName, &[Type])> {
    super::assignable::alias_identity(ty).or_else(|| match ty.peel() {
        Type::InterfaceRef { mangled, args, .. } | Type::ClassRef { mangled, args, .. } => {
            Some((mangled, args.as_slice()))
        }
        _ => None,
    })
}

/// A class instance written where another class is expected: a subclass
/// instance seen as its ancestor.
fn is_class_seen_as_class(part: &Type, member: &Type) -> bool {
    matches!(
        (part.peel(), member.peel()),
        (Type::ClassRef { .. }, Type::ClassRef { .. })
    )
}

/// Joins exit environments pairwise in a balanced tree rather than left to
/// right: a switch with thousands of exits would otherwise rebuild a growing
/// union at every step. Joined narrowed types are unions, so they do not depend
/// on the grouping, and each join keeps the leftmost environment's bindings.
/// Only `dropped` entries, which feed a help hint, compare narrowed types for
/// equality and can differ for an authored union such as `"a" | string`.
fn join_exit_envs(mut level: Vec<narrowing::NarrowEnv>) -> Option<narrowing::NarrowEnv> {
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut envs = level.into_iter();
        while let Some(left) = envs.next() {
            next.push(match envs.next() {
                Some(right) => {
                    narrowing::union_envs(left, Default::default(), right, Default::default()).0
                }
                None => left,
            });
        }
        level = next;
    }
    level.pop()
}

/// The function type with the most parameters, when every member is a
/// function type. A call to a union of function types passes the longest
/// parameter list, and a function that fits a member with fewer parameters
/// ignores the extra arguments.
fn longest_function_member<'t>(members: &[&'t Type]) -> Option<&'t Type> {
    let mut longest: Option<(&Type, usize)> = None;
    for &member in members {
        let Type::Function { params, .. } = member.peel() else {
            return None;
        };
        if longest.is_none_or(|(_, count)| params.len() > count) {
            longest = Some((member, params.len()));
        }
    }
    longest.map(|(member, _)| member)
}

#[cfg(test)]
mod invariant_tests {
    use super::*;

    #[test]
    fn exhausted_narrowing_ids_return_a_limit_without_wrapping() {
        super::super::test_support::with_inferer(|tc| {
            tc.next_narrow_counter = u32::MAX;
            assert!(matches!(
                tc.mint_narrow_binding(Span::at(crate::FileId(0))),
                Err(CompilerFailure::Limit { .. })
            ));
            assert_eq!(tc.next_narrow_counter, u32::MAX);
        });
    }

    #[test]
    fn missing_join_and_mismatched_narrowing_frames_are_fatal() {
        super::super::test_support::with_inferer(|tc| {
            assert!(matches!(
                tc.pop_pending_join_frame(),
                Err(CompilerFailure::Internal { .. })
            ));
            assert!(matches!(
                tc.pop_narrow_frame_capture(),
                Err(CompilerFailure::Internal { .. })
            ));
            tc.push_narrow_frame(narrowing::NarrowEnv::new());
            tc.assigned_scopes.clear();
            assert!(matches!(
                tc.pop_narrow_frame_capture(),
                Err(CompilerFailure::Internal { .. })
            ));
            assert_eq!(
                tc.narrow_scopes.len(),
                1,
                "reject before consuming mismatched frames"
            );
        });
    }
}
