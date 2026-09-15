//! Predicate-environment extraction for the narrowing engine.

use crate::{ExprId, Ident, Span, Type, TypedExpr};

use super::assignable::{literal_to_type, literal_value_of};
use super::{Inferer, narrowing};

impl<'a> Inferer<'a> {
    pub(super) fn expr_to_reference_path(
        &self,
        expr: &TypedExpr,
    ) -> Option<narrowing::ReferencePath> {
        self.kind_to_reference_path(&expr.kind)
    }

    pub(super) fn kind_to_reference_path(
        &self,
        kind: &crate::TypedExprKind,
    ) -> Option<narrowing::ReferencePath> {
        use crate::TypedExprKind;
        match kind {
            TypedExprKind::LocalRef { ident, .. } => {
                let entry = self.scopes.get(&ident.name)?;
                Some(narrowing::ReferencePath::root(
                    narrowing::BindingId::Local {
                        name: ident.name.clone(),
                        decl_scope: entry.decl_scope,
                    },
                ))
            }
            TypedExprKind::LocalNarrowRef { path, .. } => Some(path.clone()),
            // Guarding a nullable field before use — `if (this.inner !== null)`
            // — is the most common narrowing shape in class-based code, so
            // `this` roots a path like any other binding. `current_class` gates
            // it: a `this` outside an instance member is already a diagnostic,
            // and rooting a path there would have no type to rebuild from.
            TypedExprKind::This if self.current_class.is_some() => {
                Some(narrowing::ReferencePath::root(narrowing::BindingId::This))
            }
            TypedExprKind::GlobalRef { mangled, .. } => Some(narrowing::ReferencePath::root(
                narrowing::BindingId::Global(mangled.clone()),
            )),
            TypedExprKind::FieldAccess { receiver, name } => {
                let receiver_expr = self.typed_ast.expr(*receiver);
                let mut path = self.expr_to_reference_path(receiver_expr)?;
                path.chain
                    .push(narrowing::PathElem::Field(name.name.clone()));
                Some(path)
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                let receiver_expr = self.typed_ast.expr(*receiver);
                let mut path = self.expr_to_reference_path(receiver_expr)?;
                let lit = index_literal_value(&self.typed_ast.expr(*index).kind)?;
                path.chain.push(narrowing::PathElem::Index(lit));
                Some(path)
            }
            _ => None,
        }
    }

    /// True/false narrowing envs for a condition, with any view whose source
    /// cannot be safely re-emitted dropped.
    ///
    /// A view survives if its source is still emittable (names no narrow shadow,
    /// or one that is still live) or the wrap site could rebuild it from
    /// declared types. The `or` matters in both directions: index paths have no
    /// synthesizable form — `synthesize_*_source` bails on `PathElem::Index` —
    /// and survive only on the first test; a path whose shadow has been popped
    /// survives only on the second.
    ///
    /// Dropping here rather than at wrap time is deliberate: by wrap time the
    /// branch body has already been inferred and every in-region read rewritten
    /// to a `LocalNarrowRef`, so removing the region would leave those reads
    /// with nothing to bind to. Refusing up front costs a missed narrowing (an
    /// ordinary "possibly null" error, with a hint) instead of a compiler panic.
    pub(super) fn predicate_envs(
        &mut self,
        cond_expr_id: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        let (mut true_env, mut false_env) = self.predicate_envs_unfiltered(cond_expr_id);
        self.retain_emittable_views(&mut true_env);
        self.retain_emittable_views(&mut false_env);
        (true_env, false_env)
    }

    fn retain_emittable_views(&mut self, env: &mut narrowing::NarrowEnv) {
        let candidates: Vec<(narrowing::ReferencePath, ExprId)> =
            env.iter().map(|(p, v)| (p.clone(), v.source)).collect();
        let mut doomed: Vec<narrowing::ReferencePath> = Vec::new();
        for (path, source) in candidates {
            if self.source_shadow_is_live(source) {
                continue;
            }
            // Ask the real wrap-time synthesizer rather than a second copy of
            // its rules: it runs against this same env, so its answer here is
            // the answer the wrap site will give. The throwaway nodes it
            // allocates are unreachable and never emitted.
            let span = self.typed_ast.expr(source).span;
            if self
                .synthesize_wrap_time_source(&*env, &path, span)
                .is_some()
            {
                continue;
            }
            doomed.push(path);
        }
        // Dropped silently. Telling a later diagnostic *why* needs the drop to
        // reach the region this env installs, and nothing here knows which of
        // the two envs the caller will push — see `unpreserved_shape_hint`.
        for path in doomed {
            env.remove(&path);
        }
    }

    fn predicate_envs_unfiltered(
        &mut self,
        cond_expr_id: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        use crate::{BinOp, TypedExprKind, UnOp};
        // Take an owned copy of the condition's kind / span / ty so we
        // can drop the immutable borrow on `self.typed_ast` before
        // calling helpers that mutate `self`.
        let cond = self.typed_ast.expr(cond_expr_id);
        let cond_kind = cond.kind.clone();
        match cond_kind {
            TypedExprKind::Binary { op, lhs, rhs } if matches!(op, BinOp::Eq | BinOp::NotEq) => {
                if let Some(envs) = self.try_predicate_envs_literal_equality(op, lhs, rhs) {
                    envs
                } else {
                    self.predicate_envs_eq_null(op, lhs, rhs)
                }
            }
            TypedExprKind::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } => self.predicate_envs_and(lhs, rhs),
            TypedExprKind::Binary {
                op: BinOp::Or,
                lhs,
                rhs,
            } => self.predicate_envs_or(lhs, rhs),
            TypedExprKind::Binary {
                op: BinOp::In,
                lhs,
                rhs,
            } => self.predicate_envs_in_operator(lhs, rhs),
            TypedExprKind::InstanceOf { value, class } => {
                // An always-false / non-class test resolves `class` to `Type::Error` in
                // inference; don't narrow.
                if matches!(class.peel(), crate::types::Type::Error) {
                    (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())
                } else {
                    self.narrow_path_to_asserted(value, &class)
                        .unwrap_or_else(|| {
                            (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())
                        })
                }
            }
            TypedExprKind::TypeofTag { value, tag } => {
                let facts = match tag {
                    crate::TypeofTagKind::Number => narrowing::TypeFacts::IS_NUMBER,
                    crate::TypeofTagKind::String => narrowing::TypeFacts::IS_STRING,
                    crate::TypeofTagKind::Boolean => narrowing::TypeFacts::IS_BOOLEAN,
                    crate::TypeofTagKind::Object => narrowing::TypeFacts::IS_OBJECT,
                    crate::TypeofTagKind::Function => narrowing::TypeFacts::IS_FUNCTION,
                };
                self.predicate_envs_typeof_tag(value, facts)
            }
            TypedExprKind::Unary {
                op: UnOp::Not,
                operand,
            } => {
                let (t, f) = self.predicate_envs_unfiltered(operand);
                (f, t)
            }
            TypedExprKind::Narrowed { inner, .. } => {
                // See through `Narrowed` wrappers that `wrap_narrow_exprs` installs on `&&`/`||` RHS.
                self.predicate_envs_unfiltered(inner)
            }
            TypedExprKind::Call {
                args,
                type_predicate: Some(predicate),
                ..
            } => self
                .predicate_envs_user_guard(&predicate, &args)
                .unwrap_or_else(|| (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())),
            TypedExprKind::GenericCall {
                args,
                type_predicate: Some(predicate),
                ..
            } => {
                // `asserted_type` already substituted with type-arg bindings during inference.
                let exprs: Vec<ExprId> = args.iter().map(|a| a.expr).collect();
                self.predicate_envs_user_guard(&predicate, &exprs)
                    .unwrap_or_else(|| (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()))
            }
            TypedExprKind::MethodCall {
                args,
                type_predicate: Some(predicate),
                ..
            } => self
                .predicate_envs_user_guard(&predicate, &args)
                .unwrap_or_else(|| (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())),
            TypedExprKind::GenericMethodCall {
                args,
                type_predicate: Some(predicate),
                ..
            } => {
                let exprs: Vec<ExprId> = args.iter().map(|a| a.expr).collect();
                self.predicate_envs_user_guard(&predicate, &exprs)
                    .unwrap_or_else(|| (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()))
            }
            TypedExprKind::LocalRef { .. }
            | TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FieldAccess { .. }
            | TypedExprKind::IndexAccess { .. } => self.predicate_envs_truthiness(cond_expr_id),
            _ => (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()),
        }
    }

    /// False-env is empty: `a && b` fails two ways, neither safe to narrow.
    fn predicate_envs_and(
        &mut self,
        lhs: ExprId,
        rhs: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        let (lhs_true, _lhs_false) = self.predicate_envs_unfiltered(lhs);
        self.push_narrow_frame(lhs_true.clone());
        let (rhs_true, _rhs_false) = self.predicate_envs_unfiltered(rhs);
        self.pop_narrow_frame();
        // RHS supersedes LHS — ran under LHS narrowing, so its view is at least as specific.
        let mut composed = lhs_true;
        for (path, view) in rhs_true.into_iter() {
            composed.insert(path, view);
        }
        (composed, narrowing::NarrowEnv::new())
    }

    fn predicate_envs_or(
        &mut self,
        lhs: ExprId,
        rhs: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        let (_lhs_true, lhs_false) = self.predicate_envs_unfiltered(lhs);
        self.push_narrow_frame(lhs_false.clone());
        let (_rhs_true, rhs_false) = self.predicate_envs_unfiltered(rhs);
        self.pop_narrow_frame();
        let mut composed = lhs_false;
        for (path, view) in rhs_false.into_iter() {
            composed.insert(path, view);
        }
        (narrowing::NarrowEnv::new(), composed)
    }

    fn predicate_envs_eq_null(
        &mut self,
        op: crate::BinOp,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        use crate::{BinOp, TypedExprKind};
        let lhs = self.typed_ast.expr(lhs_id);
        let rhs = self.typed_ast.expr(rhs_id);
        let lhs_is_null = matches!(lhs.kind, TypedExprKind::Null);
        let rhs_is_null = matches!(rhs.kind, TypedExprKind::Null);
        let path_expr = match (lhs_is_null, rhs_is_null) {
            (false, true) => lhs,
            (true, false) => rhs,
            _ => return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()),
        };
        let Some(path) = self.expr_to_reference_path(path_expr) else {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        };
        if self.path_root_is_captured_mutator(&path) {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        }
        let path_ty = path_expr.ty.clone();
        // Never introduce a null alternative that the operand cannot hold.
        // Keep the non-null fact even when already proven: loop rechecking
        // needs it after invalidating the enclosing guard at a back edge.
        let can_be_null = super::assignable(&Type::Null, &path_ty, self.resolver());
        let path_span = path_expr.span;
        let fallback_kind = path_expr.kind.clone();

        // Prefer un-narrowed source so NarrowRegion materialization avoids dangling chain shadows.
        let source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)
            .unwrap_or(fallback_kind);
        let mut eq_env = narrowing::NarrowEnv::new();
        let mut neq_env = narrowing::NarrowEnv::new();
        let source_eq = self.typed_ast.push_expr(TypedExpr {
            kind: source_kind.clone(),
            span: path_span,
            ty: path_ty.clone(),
        });
        let source_neq = self.typed_ast.push_expr(TypedExpr {
            kind: source_kind,
            span: path_span,
            ty: path_ty.clone(),
        });
        if can_be_null {
            eq_env.insert(
                path.clone(),
                narrowing::NarrowedView {
                    narrowed_ty: Type::Null,
                    facts: narrowing::TypeFacts::EQ_NULL,
                    excluded_literals: std::collections::BTreeSet::new(),
                    binding: self.mint_narrow_binding(path_span),
                    source: source_eq,
                },
            );
        }
        neq_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: narrowing::strip_null(&path_ty),
                facts: narrowing::TypeFacts::NE_NULL,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span),
                source: source_neq,
            },
        );

        match op {
            BinOp::Eq => (eq_env, neq_env),
            BinOp::NotEq => (neq_env, eq_env),
            _ => unreachable!("matched Eq | NotEq above"),
        }
    }

    fn predicate_envs_in_operator(
        &mut self,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        use crate::TypedExprKind;
        let lhs = self.typed_ast.expr(lhs_id);
        let field_name = match &lhs.kind {
            TypedExprKind::String(s) => s.clone(),
            _ => return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()),
        };
        let rhs = self.typed_ast.expr(rhs_id);
        let Some(path) = self.expr_to_reference_path(rhs) else {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        };
        if self.path_root_is_captured_mutator(&path) {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        }
        let path_ty = rhs.ty.clone();
        let path_span = rhs.span;
        let fallback_kind = rhs.kind.clone();

        let (true_ty, false_ty) =
            narrowing::narrow_field_presence(&path_ty, &field_name, &|member, field| {
                self.member_has_field(member, field)
            });

        let source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)
            .unwrap_or(fallback_kind);
        let source_true = self.typed_ast.push_expr(TypedExpr {
            kind: source_kind.clone(),
            span: path_span,
            ty: path_ty.clone(),
        });
        let source_false = self.typed_ast.push_expr(TypedExpr {
            kind: source_kind,
            span: path_span,
            ty: path_ty,
        });
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span),
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span),
                source: source_false,
            },
        );
        (true_env, false_env)
    }

    fn try_predicate_envs_literal_equality(
        &mut self,
        op: crate::BinOp,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)> {
        use crate::{BinOp, TypedExprKind};
        enum DiscKey {
            Field(String),
            Position(usize),
        }
        let lhs_lit = literal_value_of(&self.typed_ast.expr(lhs_id).kind);
        let rhs_lit = literal_value_of(&self.typed_ast.expr(rhs_id).kind);
        let (path_id, literal) = match (lhs_lit, rhs_lit) {
            (None, Some(lit)) => (lhs_id, lit),
            (Some(lit), None) => (rhs_id, lit),
            _ => return None,
        };

        let path_expr = self.typed_ast.expr(path_id);
        let path = self.expr_to_reference_path(path_expr)?;
        if self.path_root_is_captured_mutator(&path) {
            return None;
        }
        let path_ty = path_expr.ty.clone();
        let path_span = path_expr.span;
        let path_kind = path_expr.kind.clone();

        // Prefer un-narrowed source so NarrowRegion doesn't depend on a chain shadow.
        let direct_source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)
            .unwrap_or_else(|| path_kind.clone());

        if path.chain.is_empty() {
            return self.narrow_direct_literal(
                op,
                path,
                path_ty,
                direct_source_kind,
                path_span,
                literal,
            );
        }

        let (root_path, root_ty, root_span, root_kind, disc_key_from_path) = match &path_kind {
            TypedExprKind::FieldAccess { receiver, name } => {
                let receiver_expr = self.typed_ast.expr(*receiver);
                let root_path = self.expr_to_reference_path(receiver_expr)?;
                let receiver_ty = receiver_expr.ty.clone();
                let receiver_span = receiver_expr.span;
                let fallback_kind = receiver_expr.kind.clone();
                let root_kind = self
                    .synthesize_unnarrowed_source(&root_path, receiver_span)
                    .unwrap_or(fallback_kind);
                (
                    root_path,
                    receiver_ty,
                    receiver_span,
                    root_kind,
                    DiscKey::Field(name.name.clone()),
                )
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                let receiver_expr = self.typed_ast.expr(*receiver);
                let root_path = self.expr_to_reference_path(receiver_expr)?;
                let receiver_ty = receiver_expr.ty.clone();
                let receiver_span = receiver_expr.span;
                let fallback_kind = receiver_expr.kind.clone();
                let root_kind = self
                    .synthesize_unnarrowed_source(&root_path, receiver_span)
                    .unwrap_or(fallback_kind);
                let position = index_position(&self.typed_ast.expr(*index).kind)?;
                (
                    root_path,
                    receiver_ty,
                    receiver_span,
                    root_kind,
                    DiscKey::Position(position),
                )
            }
            TypedExprKind::LocalNarrowRef { .. } => {
                let disc_key = match path.chain.last().cloned()? {
                    narrowing::PathElem::Field(name) => DiscKey::Field(name),
                    narrowing::PathElem::Index(narrowing::LiteralValue::Number(n))
                        if n.0.is_finite() && n.0.fract() == 0.0 && n.0 >= 0.0 =>
                    {
                        DiscKey::Position(n.0 as usize)
                    }
                    _ => return None,
                };
                let mut root_path = path.clone();
                root_path.chain.pop();
                let (root_ty, root_kind) = self.derive_root_source(&root_path, path_span)?;
                (root_path, root_ty, path_span, root_kind, disc_key)
            }
            _ => return None,
        };

        // Peel aliases so `type Shape = A | B` pattern-matches as union.
        let Type::Union(members) = root_ty.peel() else {
            return None;
        };
        let table_lookup: Option<narrowing::VariantIdx> = match &disc_key_from_path {
            DiscKey::Field(key_field) => {
                let (disc_key, table) = self.union_discriminant_with_nominals(members)?;
                if disc_key != *key_field {
                    return None;
                }
                table.get(&literal).copied()
            }
            DiscKey::Position(pos) => {
                let (disc_pos, table) = narrowing::tuple_union_discriminant(members)?;
                if disc_pos != *pos {
                    return None;
                }
                table.get(&literal).copied()
            }
        };
        let matching_idx = table_lookup?;
        let matched_variant = members[matching_idx.0 as usize].clone();
        let remaining: Vec<Type> = members
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as u32) != matching_idx.0)
            .map(|(_, m)| m.clone())
            .collect();
        let remaining_ty = Type::union(remaining);

        let (true_root_ty, false_root_ty) = match op {
            BinOp::Eq => (matched_variant, remaining_ty),
            BinOp::NotEq => (remaining_ty, matched_variant),
            _ => unreachable!("matched Eq | NotEq at the predicate_envs dispatch"),
        };

        let source_true = self.typed_ast.push_expr(TypedExpr {
            kind: root_kind.clone(),
            span: root_span,
            ty: root_ty.clone(),
        });
        let source_false = self.typed_ast.push_expr(TypedExpr {
            kind: root_kind,
            span: root_span,
            ty: root_ty,
        });
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            root_path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(root_span),
                source: source_true,
            },
        );
        false_env.insert(
            root_path,
            narrowing::NarrowedView {
                narrowed_ty: false_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(root_span),
                source: source_false,
            },
        );
        // Also narrow the field path: `s.kind` stays usable as a refined literal
        // and `excluded_literals` enables 3+-variant chain composition.
        let direct_source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)
            .unwrap_or(path_kind);
        if let Some((extra_true, extra_false)) =
            self.narrow_direct_literal(op, path, path_ty, direct_source_kind, path_span, literal)
        {
            for (p, v) in extra_true {
                true_env.insert(p, v);
            }
            for (p, v) in extra_false {
                false_env.insert(p, v);
            }
        }

        Some((true_env, false_env))
    }

    fn narrow_direct_literal(
        &mut self,
        op: crate::BinOp,
        path: narrowing::ReferencePath,
        path_ty: Type,
        path_kind: crate::TypedExprKind,
        path_span: Span,
        literal: narrowing::LiteralValue,
    ) -> Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)> {
        use crate::BinOp;
        let Type::Union(members) = &path_ty else {
            return None;
        };
        let literal_ty = literal_to_type(&literal);
        let mut matched: Vec<Type> = Vec::new();
        let mut remaining: Vec<Type> = Vec::new();
        for m in members {
            if m == &literal_ty {
                matched.push(m.clone());
            } else {
                remaining.push(m.clone());
            }
        }
        if matched.is_empty() {
            // Predicate is statically false — typechecker accepted
            // the comparison anyway. Skip narrowing.
            return None;
        }
        let matched_ty = Type::union(matched);
        let remaining_ty = Type::union(remaining);
        let (true_ty, false_ty) = match op {
            BinOp::Eq => (matched_ty, remaining_ty),
            BinOp::NotEq => (remaining_ty, matched_ty),
            _ => unreachable!(),
        };

        let source_true = self.typed_ast.push_expr(TypedExpr {
            kind: path_kind.clone(),
            span: path_span,
            ty: path_ty.clone(),
        });
        let source_false = self.typed_ast.push_expr(TypedExpr {
            kind: path_kind,
            span: path_span,
            ty: path_ty,
        });
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        let mut false_excluded = std::collections::BTreeSet::new();
        false_excluded.insert(literal);
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span),
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: false_excluded,
                binding: self.mint_narrow_binding(path_span),
                source: source_false,
            },
        );
        Some((true_env, false_env))
    }

    fn predicate_envs_typeof_tag(
        &mut self,
        value_id: ExprId,
        facts: narrowing::TypeFacts,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        if facts.is_empty() {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        }
        let (mut true_env, mut false_env) = self.predicate_envs_from_facts(value_id, facts);
        self.predicate_envs_tuple_typeof_rollup(value_id, facts, &mut true_env, &mut false_env);
        (true_env, false_env)
    }

    fn predicate_envs_tuple_typeof_rollup(
        &mut self,
        value_id: ExprId,
        facts: narrowing::TypeFacts,
        true_env: &mut narrowing::NarrowEnv,
        false_env: &mut narrowing::NarrowEnv,
    ) {
        let value_expr = self.typed_ast.expr(value_id);
        let crate::TypedExprKind::IndexAccess { receiver, index } = &value_expr.kind else {
            return;
        };
        let Some(position) = index_position(&self.typed_ast.expr(*index).kind) else {
            return;
        };
        let receiver_expr = self.typed_ast.expr(*receiver);
        let Some(root_path) = self.expr_to_reference_path(receiver_expr) else {
            return;
        };
        if self.path_root_is_captured_mutator(&root_path) {
            return;
        }
        let root_ty = receiver_expr.ty.clone();
        let Type::Union(members) = root_ty.peel() else {
            return;
        };
        if !members.iter().all(|m| matches!(m.peel(), Type::Tuple(_))) {
            return;
        }
        let mut matching: Vec<Type> = Vec::new();
        let mut non_matching: Vec<Type> = Vec::new();
        for m in members {
            let Type::Tuple(elems) = m.peel() else {
                continue;
            };
            let Some(elem) = elems.get(position) else {
                continue;
            };
            if narrowing::type_matches_facts(elem, facts) {
                matching.push(m.clone());
            } else {
                non_matching.push(m.clone());
            }
        }
        let receiver_span = receiver_expr.span;
        let fallback_kind = receiver_expr.kind.clone();
        let root_kind = self
            .synthesize_unnarrowed_source(&root_path, receiver_span)
            .unwrap_or(fallback_kind);
        let true_root_ty = Type::union(matching);
        let false_root_ty = Type::union(non_matching);
        let source_true = self.typed_ast.push_expr(TypedExpr {
            kind: root_kind.clone(),
            span: receiver_span,
            ty: root_ty.clone(),
        });
        let source_false = self.typed_ast.push_expr(TypedExpr {
            kind: root_kind,
            span: receiver_span,
            ty: root_ty,
        });
        true_env.insert(
            root_path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(receiver_span),
                source: source_true,
            },
        );
        false_env.insert(
            root_path,
            narrowing::NarrowedView {
                narrowed_ty: false_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(receiver_span),
                source: source_false,
            },
        );
    }

    fn predicate_envs_from_facts(
        &mut self,
        value_id: ExprId,
        facts: narrowing::TypeFacts,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        let value_expr = self.typed_ast.expr(value_id);
        let Some(path) = self.expr_to_reference_path(value_expr) else {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        };
        if self.path_root_is_captured_mutator(&path) {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        }
        let from_ty = value_expr.ty.clone();
        let value_span = value_expr.span;
        let fallback_kind = value_expr.kind.clone();

        let true_ty = narrowing::intersect_with(&from_ty, facts);
        let false_ty = narrowing::subtract(&from_ty, facts);

        let true_facts = facts;
        // False-branch facts left EMPTY — `narrowed_ty` is load-bearing; `facts` only
        // consulted for EQ_NULL/NE_NULL.
        let false_facts = narrowing::TypeFacts::EMPTY;

        let source_kind = self
            .synthesize_unnarrowed_source(&path, value_span)
            .unwrap_or(fallback_kind);
        let source_true = self.typed_ast.push_expr(TypedExpr {
            kind: source_kind.clone(),
            span: value_span,
            ty: from_ty.clone(),
        });
        let source_false = self.typed_ast.push_expr(TypedExpr {
            kind: source_kind,
            span: value_span,
            ty: from_ty,
        });
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty,
                facts: true_facts,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(value_span),
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: false_facts,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(value_span),
                source: source_false,
            },
        );
        (true_env, false_env)
    }

    /// Bare-path condition (`if (x)`): JS truthiness. Each branch inserts a
    /// view only when it actually refines the type — `string | null` narrows to
    /// `string` in the true branch, but the false branch stays `string | null`
    /// (`""` is falsy but not null), so no view is minted there. A no-op view
    /// would also reference an inner narrowing's binding that's out of scope
    /// at NarrowRegion entry.
    fn predicate_envs_truthiness(
        &mut self,
        path_expr_id: ExprId,
    ) -> (narrowing::NarrowEnv, narrowing::NarrowEnv) {
        let path_expr = self.typed_ast.expr(path_expr_id);
        let Some(path) = self.expr_to_reference_path(path_expr) else {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        };
        if self.path_root_is_captured_mutator(&path) {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        }
        let from_ty = path_expr.ty.clone();
        let span = path_expr.span;
        let fallback_kind = path_expr.kind.clone();

        let true_ty = narrowing::intersect_with(&from_ty, narrowing::TypeFacts::TRUTHY);
        let false_ty = narrowing::intersect_with(&from_ty, narrowing::TypeFacts::FALSY);
        let refines = |ty: &Type| !matches!(ty, Type::Error) && ty.peel() != from_ty.peel();

        if !refines(&true_ty) && !refines(&false_ty) {
            return (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        }

        // A falsy outcome proves `x === null` only when null is the sole falsy
        // value the type can hold.
        let false_facts = if narrowing::falsy_values_are_only_null(&from_ty) {
            narrowing::TypeFacts::FALSY | narrowing::TypeFacts::EQ_NULL
        } else {
            narrowing::TypeFacts::FALSY
        };

        let source_kind = self
            .synthesize_unnarrowed_source(&path, span)
            .unwrap_or(fallback_kind);
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        for (env, ty, facts) in [
            (
                &mut true_env,
                true_ty,
                narrowing::TypeFacts::TRUTHY | narrowing::TypeFacts::NE_NULL,
            ),
            (&mut false_env, false_ty, false_facts),
        ] {
            if !refines(&ty) {
                continue;
            }
            let source = self.typed_ast.push_expr(TypedExpr {
                kind: source_kind.clone(),
                span,
                ty: from_ty.clone(),
            });
            env.insert(
                path.clone(),
                narrowing::NarrowedView {
                    narrowed_ty: ty,
                    facts,
                    excluded_literals: std::collections::BTreeSet::new(),
                    binding: self.mint_narrow_binding(span),
                    source,
                },
            );
        }
        (true_env, false_env)
    }

    /// Leading `#` prevents collision with user identifiers (first-char rule: `[A-Za-z_$]`).
    pub(super) fn mint_narrow_binding(&mut self, span: Span) -> Ident {
        let n = self.next_narrow_counter;
        self.next_narrow_counter += 1;
        Ident {
            name: format!("#narrow_{n}"),
            span,
        }
    }

    /// Builds an expression for `path` that bypasses chain-shadow `Narrowed` wrappers.
    /// Codegen evaluates this at NarrowRegion entry to seed the shadow local.
    pub(super) fn synthesize_unnarrowed_source(
        &mut self,
        path: &narrowing::ReferencePath,
        path_span: Span,
    ) -> Option<crate::TypedExprKind> {
        if path.chain.is_empty() {
            return Some(match &path.root {
                narrowing::BindingId::Local { name, .. } => {
                    let _entry = self.scopes.get(name)?;
                    crate::TypedExprKind::LocalRef {
                        ident: Ident {
                            name: name.clone(),
                            span: path_span,
                        },
                        boxed: false,
                    }
                }
                narrowing::BindingId::Global(mangled) => crate::TypedExprKind::GlobalRef {
                    mangled: mangled.clone(),
                    name: Ident {
                        name: mangled
                            .as_str()
                            .rsplit('#')
                            .next()
                            .unwrap_or(mangled.as_str())
                            .to_string(),
                        span: path_span,
                    },
                },
                narrowing::BindingId::This => crate::TypedExprKind::This,
            });
        }
        let mut receiver_path = path.clone();
        let last_field = receiver_path.chain.pop()?;
        let narrowing::PathElem::Field(field_name) = last_field else {
            return None;
        };
        let receiver_kind = self.synthesize_unnarrowed_source(&receiver_path, path_span)?;
        let receiver_ty = self.derive_type_for_reference_path(&receiver_path)?;
        // Bail when the field can't be resolved on the receiver's shape: without
        // a source we can reconstruct from declared types, the view would fall
        // back to an already-narrowed expression whose shadow is scoped to an
        // inner region.
        self.narrow_source_field_ty(&receiver_ty, &field_name)?;
        let receiver_id = self.typed_ast.push_expr(TypedExpr {
            kind: receiver_kind,
            span: path_span,
            ty: receiver_ty,
        });
        Some(crate::TypedExprKind::FieldAccess {
            receiver: receiver_id,
            name: Ident {
                name: field_name,
                span: path_span,
            },
        })
    }

    /// Receiver type for a synthesized narrow source, rebuilt from declared
    /// types. Each hop drops `null`: the guard has already proved every prefix
    /// non-null wherever this source runs, and the type is stamped onto a
    /// `FieldAccess` that codegen lowers by receiver shape.
    fn derive_type_for_reference_path(&self, path: &narrowing::ReferencePath) -> Option<Type> {
        let root_ty = match &path.root {
            narrowing::BindingId::Local { name, .. } => self.scopes.get(name)?.ty.clone(),
            narrowing::BindingId::This => self.current_class.clone()?,
            narrowing::BindingId::Global(_) => return None,
        };
        let mut ty = super::narrow_scopes::non_null_form(root_ty)?;
        for elem in &path.chain {
            match elem {
                narrowing::PathElem::Field(name) => {
                    let field_ty = self.narrow_source_field_ty(&ty, name)?;
                    ty = super::narrow_scopes::non_null_form(field_ty)?;
                }
                narrowing::PathElem::Index(_) => return None,
            }
        }
        Some(ty)
    }

    pub(super) fn derive_root_source(
        &self,
        root_path: &narrowing::ReferencePath,
        fallback_span: Span,
    ) -> Option<(Type, crate::TypedExprKind)> {
        if let Some(view) = self.lookup_narrowed_view(root_path) {
            return Some((
                view.narrowed_ty.clone(),
                crate::TypedExprKind::LocalNarrowRef {
                    binding: view.binding.clone(),
                    path: root_path.clone(),
                },
            ));
        }
        if !root_path.chain.is_empty() {
            return None;
        }
        match &root_path.root {
            narrowing::BindingId::Local { name, .. } => {
                let entry = self.scopes.get(name)?;
                let ident = Ident {
                    name: name.clone(),
                    span: fallback_span,
                };
                Some((
                    entry.ty.clone(),
                    crate::TypedExprKind::LocalRef {
                        ident,
                        boxed: false,
                    },
                ))
            }
            narrowing::BindingId::This => {
                Some((self.current_class.clone()?, crate::TypedExprKind::This))
            }
            narrowing::BindingId::Global(_) => None,
        }
    }

    pub(super) fn lookup_tombstone(
        &self,
        path: &narrowing::ReferencePath,
    ) -> Option<narrowing::InvalidationReason> {
        self.tombstone_scopes
            .iter()
            .rev()
            .find_map(|frame| tombstone_covering(frame, path).cloned())
    }

    /// Inner-out, stopping at the first frame that either narrows `path` or
    /// tombstones it. A tombstone shadows an outer frame's narrowing rather than
    /// deleting it: the write that killed the guard belongs to *this* frame, so
    /// the kill has to disappear with the frame — an `if` arm's write must not
    /// reach the `else` arm, which never ran it. Carrying the kill past the frame
    /// is `assigned_scopes`' job, at the join.
    pub(super) fn lookup_narrowed_view(
        &self,
        path: &narrowing::ReferencePath,
    ) -> Option<&narrowing::NarrowedView> {
        for (frame_idx, frame) in self.narrow_scopes.iter().enumerate().rev() {
            if let Some(view) = frame.get(path) {
                // `never` narrowings get no shadow local in codegen; fall through
                // to the un-narrowed type rather than naming a binding that has none.
                return (!matches!(view.narrowed_ty, Type::Error)).then_some(view);
            }
            if self
                .tombstone_scopes
                .get(frame_idx)
                .is_some_and(|tombs| tombstone_covering(tombs, path).is_some())
            {
                return None;
            }
        }
        None
    }
}

/// The tombstone in `frame` that kills `path`, if any.
///
/// A write to `o.a` invalidates `o.a.n` as well, so the match is by *prefix*, not
/// by key. Dropping narrowings uses the same prefix rule; asking for an exact key
/// here would let a guard on `o.a.n` survive a write to `o.a` — and the read that
/// followed would return the value the guard saw, not the one that is there.
fn tombstone_covering<'a>(
    frame: &'a std::collections::BTreeMap<narrowing::ReferencePath, narrowing::InvalidationReason>,
    path: &narrowing::ReferencePath,
) -> Option<&'a narrowing::InvalidationReason> {
    frame
        .iter()
        .find(|(key, _)| key.is_prefix_of(path))
        .map(|(_, reason)| reason)
}

pub(super) fn index_literal_value(kind: &crate::TypedExprKind) -> Option<narrowing::LiteralValue> {
    match kind {
        crate::TypedExprKind::Number(n) => Some(narrowing::LiteralValue::Number(
            crate::types::LiteralF64(*n),
        )),
        crate::TypedExprKind::String(s) => Some(narrowing::LiteralValue::String(s.clone())),
        _ => None,
    }
}

fn index_position(kind: &crate::TypedExprKind) -> Option<usize> {
    match kind {
        crate::TypedExprKind::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => {
            Some(*n as usize)
        }
        _ => None,
    }
}
