use std::collections::BTreeMap;

use crate::{
    ExprId, Ident, ObjectField, Span, StmtId, Type, TypedExpr, TypedStmt, TypedStmtKind, ValueKind,
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
    /// Call after the closure's params are in scope.
    pub(super) fn enter_closure_narrow_boundary(&mut self, span: Span) -> narrowing::NarrowEnv {
        let (active, _assigned) = self.snapshot_active_narrowings(0);
        self.suspended_narrow_scopes.push(SuspendedNarrowing {
            narrow_scopes: std::mem::take(&mut self.narrow_scopes),
            assigned_scopes: std::mem::take(&mut self.assigned_scopes),
            clause_write_scopes: std::mem::take(&mut self.clause_write_scopes),
            tombstone_scopes: std::mem::take(&mut self.tombstone_scopes),
            pending_materializations: std::mem::take(&mut self.pending_post_if_materializations),
        });
        let mut seed = narrowing::NarrowEnv::new();
        for (path, view) in active {
            if !self.narrowing_survives_closure(&path, span) {
                continue;
            }
            let Some(source_kind) = self.synthesize_unnarrowed_source(&path, span) else {
                continue;
            };
            let Some(from_ty) = self.declared_root_ty(&path) else {
                continue;
            };
            let source = self.typed_ast.push_expr(TypedExpr {
                kind: source_kind,
                span,
                ty: from_ty,
            });
            // Fresh binding: reusing the outer `#narrow_N` is exactly the
            // dangling cross-frame reference the reset exists to prevent.
            let binding = self.mint_narrow_binding(span);
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
        seed
    }

    /// Drops the seed frame and restores the enclosing narrowing state. The
    /// seed frame's `assigned` set must not merge outward — an assignment
    /// inside a closure body says nothing about the enclosing frame's flow.
    pub(super) fn exit_closure_narrow_boundary(&mut self) {
        self.pop_narrow_frame();
        let Some(saved) = self.suspended_narrow_scopes.pop() else {
            return;
        };
        self.narrow_scopes = saved.narrow_scopes;
        self.assigned_scopes = saved.assigned_scopes;
        self.clause_write_scopes = saved.clause_write_scopes;
        self.tombstone_scopes = saved.tombstone_scopes;
        self.pending_post_if_materializations = saved.pending_materializations;
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

    /// The narrow binding a source expression ultimately reads through, if any.
    /// Walks the same receiver steps a [`narrowing::ReferencePath`] can hold —
    /// field *and* index — down to the root, which is a `LocalRef`, a
    /// `GlobalRef`, or the hazard this exists to catch: a `LocalNarrowRef`
    /// whose shadow may belong to a scope that has closed. Index steps matter
    /// because a view that can't be synthesized (`synthesize_*_source` bails on
    /// `PathElem::Index`) keeps the already-typed expression as its source, and
    /// that expression may read through an inner region's shadow.
    fn source_root_narrow_binding(&self, source: ExprId) -> Option<String> {
        let mut id = source;
        loop {
            match &self.typed_ast.expr(id).kind {
                crate::TypedExprKind::FieldAccess { receiver, .. }
                | crate::TypedExprKind::IndexAccess { receiver, .. } => id = *receiver,
                crate::TypedExprKind::LocalNarrowRef { binding, .. } => {
                    return Some(binding.name.clone());
                }
                _ => return None,
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

    /// Whether a path's root binding is still in lexical scope, by identity —
    /// `decl_scope` equality, so a same-named binding in a sibling scope does
    /// not count as the same root.
    fn path_root_in_scope(&self, path: &narrowing::ReferencePath) -> bool {
        match &path.root {
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
    pub(super) fn source_shadow_is_live(&self, source: ExprId) -> bool {
        self.source_root_narrow_binding(source)
            .is_none_or(|name| self.shadow_binding_is_live(&name))
    }

    /// As [`Self::source_shadow_is_live`], but a wrap site also has the env it
    /// is about to install: each of its views gets a region here, so naming one
    /// of their bindings is legal even though no frame holds them yet.
    ///
    /// Checked under `debug_assert!` so a future gap fails in the compiler at
    /// the site that caused it, rather than as an opaque codegen panic. Reads
    /// the live `narrow_scopes`, never a suspended closure frame: inside a
    /// closure body only the fresh stack is in scope.
    fn narrow_source_is_in_scope(&self, env: &narrowing::NarrowEnv, source: ExprId) -> bool {
        self.source_root_narrow_binding(source).is_none_or(|name| {
            env.values().any(|v| v.binding.name == name) || self.shadow_binding_is_live(&name)
        })
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
    fn declared_root_ty(&self, path: &narrowing::ReferencePath) -> Option<Type> {
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
    pub(super) fn loop_tail_env(&mut self, env: &narrowing::NarrowEnv) -> narrowing::NarrowEnv {
        let mut tail = narrowing::NarrowEnv::new();
        for (path, view) in env {
            if !self.path_root_in_scope(path) || matches!(view.narrowed_ty, Type::Error) {
                continue;
            }
            let span = self.typed_ast.expr(view.source).span;
            tail.insert(
                path.clone(),
                narrowing::NarrowedView {
                    binding: self.mint_narrow_binding(span),
                    ..view.clone()
                },
            );
        }
        tail
    }

    pub(super) fn push_narrow_frame(&mut self, env: narrowing::NarrowEnv) {
        self.narrow_scopes.push(env);
        self.assigned_scopes.push(std::collections::BTreeSet::new());
        self.tombstone_scopes
            .push(std::collections::BTreeMap::new());
    }

    pub(super) fn pop_narrow_frame(&mut self) {
        self.narrow_scopes.pop();
        self.assigned_scopes.pop();
        self.tombstone_scopes.pop();
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
    ) -> (ExprId, Type) {
        self.push_narrow_frame(env.clone());
        let inferred = self.infer_expr(operand, expected);
        let (_, assigned) = self.pop_narrow_frame_capture();
        let span = self.ast.expr(operand).span;
        self.merge_assigned_into_outer(assigned, span);
        inferred
    }

    pub(super) fn pop_narrow_frame_capture(
        &mut self,
    ) -> (
        narrowing::NarrowEnv,
        std::collections::BTreeSet<narrowing::ReferencePath>,
    ) {
        let narrowings = self.narrow_scopes.pop().unwrap_or_default();
        let assigned = self.assigned_scopes.pop().unwrap_or_default();
        self.tombstone_scopes.pop();
        (narrowings, assigned)
    }

    pub(super) fn push_pending_join_frame(&mut self, kind: narrowing::PendingJoinKind) {
        self.pending_joins.push(narrowing::PendingJoinFrame {
            kind,
            narrow_depth: self.narrow_scopes.len(),
            breaks: Vec::new(),
            continues: Vec::new(),
        });
    }

    pub(super) fn pop_pending_join_frame(&mut self) -> narrowing::PendingJoinFrame {
        self.pending_joins
            .pop()
            .expect("pending_joins push/pop mismatch")
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
                env.retain(|path, _| !tombs.keys().any(|written| written.is_prefix_of(path)));
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
    ) -> bool {
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
        let post_env = match exits.len() {
            0 => return false,
            1 => exits.pop().expect("len == 1"),
            _ => exits
                .into_iter()
                .reduce(|a_env, b_env| {
                    let (joined, _) = narrowing::union_envs(
                        a_env,
                        std::collections::BTreeSet::new(),
                        b_env,
                        std::collections::BTreeSet::new(),
                    );
                    joined
                })
                .expect("len >= 2"),
        };
        if !post_env.is_empty() {
            self.install_joined_narrowings(post_env, anchor_span);
        }
        true
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
    /// declaration has something `readonly` in it: a `readonly` array, field, or
    /// property the declaration names must survive `o = { xs: [1] }`. Then the
    /// binding narrows to declared members, as TypeScript's assignment narrowing
    /// does (see [`Self::declared_member_for`]).
    pub(super) fn assignment_narrowed_ty(&self, declared: &Type, written: Type) -> Type {
        if !self.declares_readonly(declared) {
            return written;
        }
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
            .map(|part| self.declared_member_for(&members, part))
            .collect();
        narrowed.map_or_else(|| declared.clone(), Type::union)
    }

    /// The declared members a written `part` narrows its binding to: the most
    /// specific member that accepts it, when that member keeps every `readonly`
    /// of the value's own, and otherwise all the members that accept it, which is
    /// TypeScript's answer. `None` when no member accepts it, which only a write
    /// already reported as an error reaches.
    fn declared_member_for(&self, members: &[&Type], part: Type) -> Option<Type> {
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
        let most_specific = accepting.iter().find(|candidate| {
            accepting
                .iter()
                .all(|other| super::assignable(candidate, other, self.resolver()))
        });
        match most_specific {
            Some(member) if !self.may_lose_readonly(&part, member) => Some((*member).clone()),
            _ => Some(Type::union(accepting.into_iter().cloned().collect())),
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
            // A class instance seen as an ancestor class keeps what it
            // inherits, and what it adds is out of the ancestor's reach.
            (Type::ClassRef { .. }, Type::ClassRef { .. }) => false,
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
        member_fields.iter().any(|(name, m)| {
            part_fields.get(name).is_some_and(|p| {
                (forbids_data_write(p) && !m.readonly)
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
    fn declares_readonly_within(&self, ty: &Type, opened: &mut Vec<crate::MangledName>) -> bool {
        match ty.peel_preserving_readonly() {
            Type::Readonly(_) => true,
            Type::Array(element) => self.declares_readonly_within(element, opened),
            Type::Tuple(elements) | Type::Union(elements) => elements
                .iter()
                .any(|element| self.declares_readonly_within(element, opened)),
            Type::Object { fields } => fields
                .values()
                .any(|f| f.readonly || self.declares_readonly_within(&f.ty, opened)),
            Type::Function { params, ret, .. } => params
                .iter()
                .chain(std::iter::once(ret.as_ref()))
                .any(|part| self.declares_readonly_within(part, opened)),
            // A recursion back-edge's body is examined where the alias is
            // expanded; only what it is instantiated with is new here.
            Type::AliasRef { args, .. } => args
                .iter()
                .any(|arg| self.declares_readonly_within(arg, opened)),
            Type::InterfaceRef { .. } | Type::ClassRef { .. } => {
                self.named_type_declares_readonly(ty.peel(), opened)
            }
            _ => false,
        }
    }

    fn named_type_declares_readonly(
        &self,
        named: &Type,
        opened: &mut Vec<crate::MangledName>,
    ) -> bool {
        let (Type::InterfaceRef { mangled, args, .. } | Type::ClassRef { mangled, args, .. }) =
            named
        else {
            return false;
        };
        if args
            .iter()
            .any(|arg| self.declares_readonly_within(arg, opened))
        {
            return true;
        }
        if opened.contains(mangled) {
            return false;
        }
        opened.push(mangled.clone());
        self.member_shape(named).is_some_and(|fields| {
            fields
                .values()
                .any(|f| forbids_data_write(f) || self.declares_readonly_within(&f.ty, opened))
        })
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
    ) {
        self.invalidate_for_reassignment(path.clone(), span);
        // The assignment still commits; only the narrowing is dropped.
        if self.path_root_is_captured_mutator(&path) {
            return;
        }
        // Assignment narrowings don't use `wrap_narrow_regions` (no shadow),
        // but `NarrowedView` requires a `source` field. Use a fresh `LocalRef`
        // as a placeholder.
        let source = self.typed_ast.push_expr(TypedExpr {
            kind: crate::TypedExprKind::LocalRef {
                ident: ident.clone(),
                boxed: false,
            },
            span,
            ty: narrowed_ty.clone(),
        });
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
        if let Some(top_assigned) = self.assigned_scopes.last_mut() {
            top_assigned.insert(path);
        }
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
    ) -> StmtId {
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
            let (source_id, cast_info) = self.wrap_time_source(env, path, view, span);
            wrapped = self.typed_ast.push_stmt(TypedStmt {
                kind: TypedStmtKind::NarrowRegion {
                    path: (*path).clone(),
                    source: source_id,
                    binding: view.binding.clone(),
                    cast_info,
                    body: wrapped,
                },
                span,
            });
        }
        wrapped
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
            Type::Object { fields } => field_member_ty(fields, field_name),
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
    ) -> (ExprId, narrowing::CastInfo) {
        let (source, from_ty) = self
            .synthesize_wrap_time_source(env, path, span)
            .unwrap_or_else(|| {
                let from_ty = self.typed_ast.expr(view.source).ty.clone();
                (view.source, from_ty)
            });
        debug_assert!(
            self.narrow_source_is_in_scope(env, source),
            "narrow source for `{}` names a shadow that is not in scope here",
            path.render(),
        );
        self.record_runtime_type_test(&view.narrowed_ty);
        (
            source,
            narrowing::cast_info_for(from_ty, view.narrowed_ty.clone()),
        )
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
    ) -> Option<(ExprId, Type)> {
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
                    let entry = self.scopes.get(name)?;
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
                narrowing::BindingId::This => {
                    (crate::TypedExprKind::This, self.current_class.clone()?)
                }
                narrowing::BindingId::Global(_) => (
                    self.synthesize_unnarrowed_source(&current_path, span)?,
                    self.declared_root_ty(&current_path)?,
                ),
            },
        };
        let mut current_id = self.typed_ast.push_expr(TypedExpr {
            kind: current_kind.clone(),
            span,
            ty: current_ty.clone(),
        });
        let _ = &mut current_kind;
        let chain_len = path.chain.len();
        for (idx, elem) in path.chain.iter().enumerate() {
            let narrowing::PathElem::Field(field_name) = elem else {
                return None;
            };
            current_path.chain.push(elem.clone());
            let field_ty = self.narrow_source_field_ty(&current_ty, field_name)?;
            // Intermediate steps use the env-narrowed type so the next
            // FieldAccess dispatches against the right shape. The final
            // element keeps the raw field type — the surrounding NarrowRegion's
            // cast widens it to `narrowed_ty`.
            let is_final = idx + 1 == chain_len;
            let (kind, ty) = if is_final {
                (
                    crate::TypedExprKind::FieldAccess {
                        receiver: current_id,
                        name: crate::Ident {
                            name: field_name.clone(),
                            span,
                        },
                    },
                    field_ty,
                )
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
                    None => (
                        crate::TypedExprKind::FieldAccess {
                            receiver: current_id,
                            name: crate::Ident {
                                name: field_name.clone(),
                                span,
                            },
                        },
                        non_null_form(field_ty)?,
                    ),
                }
            };
            current_id = self.typed_ast.push_expr(TypedExpr {
                kind,
                span,
                ty: ty.clone(),
            });
            current_ty = ty;
        }
        Some((current_id, current_ty))
    }

    /// Three cases:
    /// - Identifier paths (empty chain, local root): rebind `NarrowedView.binding`
    ///   to the root ident so `LocalNarrowRef` reads the existing Wasm slot.
    /// - Field paths: mint a fresh shadow and push a
    ///   `PendingPostIfMaterialization` for the enclosing `Block` to drain.
    /// - Globals: dropped — `GlobalRef` shadows can't be preserved past an `if`.
    pub(super) fn install_joined_narrowings(
        &mut self,
        joined_narrowings: narrowing::NarrowEnv,
        if_span: Span,
    ) {
        let mut joined_rebound: Vec<(narrowing::ReferencePath, narrowing::NarrowedView)> =
            Vec::new();
        let mut field_path_mats: Vec<(narrowing::ReferencePath, narrowing::NarrowedView)> =
            Vec::new();
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
                // Neither a global nor `this` has a local slot to rebind onto.
                narrowing::BindingId::Global(_) | narrowing::BindingId::This => continue,
            };
            let source_span = self.typed_ast.expr(view.source).span;
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
            let (source, cast_info) = self.wrap_time_source(&rebound_env, &path, &view, if_span);
            let binding = self.mint_narrow_binding(if_span);
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
        if let Some(outer) = self.narrow_scopes.last_mut() {
            for (path, view) in joined_rebound {
                outer.insert(path, view);
            }
        }
    }

    /// Innermost-out, matching `wrap_narrow_regions`'s nesting order.
    pub(super) fn wrap_pending_materializations(
        &mut self,
        body: StmtId,
        pending: Vec<narrowing::PendingPostIfMaterialization>,
    ) -> StmtId {
        let mut wrapped = body;
        for mat in pending {
            wrapped = self.typed_ast.push_stmt(TypedStmt {
                kind: TypedStmtKind::NarrowRegion {
                    path: mat.path,
                    source: mat.source,
                    binding: mat.binding,
                    cast_info: mat.cast_info,
                    body: wrapped,
                },
                span: mat.span,
            });
        }
        wrapped
    }

    /// Expression-level analogue of [`Self::wrap_narrow_regions`] for
    /// short-circuit `&&`/`||`. The `Narrowed` wrapper changes the shadow
    /// binding's type but preserves the surrounding expression's `ty`.
    pub(super) fn wrap_narrow_exprs(
        &mut self,
        inner: ExprId,
        env: &narrowing::NarrowEnv,
        span: Span,
    ) -> ExprId {
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
            let (source_id, cast_info) = self.wrap_time_source(env, path, view, span);
            let inner_ty = self.typed_ast.expr(wrapped).ty.clone();
            wrapped = self.typed_ast.push_expr(TypedExpr {
                kind: crate::TypedExprKind::Narrowed {
                    path: (*path).clone(),
                    source: source_id,
                    binding: view.binding.clone(),
                    cast_info,
                    inner: wrapped,
                },
                span,
                ty: inner_ty,
            });
        }
        wrapped
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

/// A named alias, class, or interface instance's name and type arguments.
fn named_instance(ty: &Type) -> Option<(&crate::MangledName, &[Type])> {
    super::assignable::alias_identity(ty).or_else(|| match ty.peel() {
        Type::InterfaceRef { mangled, args, .. } | Type::ClassRef { mangled, args, .. } => {
            Some((mangled, args.as_slice()))
        }
        _ => None,
    })
}

/// Whether a field forbids writing data through it. A method is a read-only
/// member of a structural form, but not data a write could reach.
fn forbids_data_write(field: &ObjectField) -> bool {
    field.readonly && !matches!(field.ty.peel(), Type::Function { .. })
}
