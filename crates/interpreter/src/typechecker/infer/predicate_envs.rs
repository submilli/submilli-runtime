//! Predicate-environment extraction for the narrowing engine.

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Ident, Span, Type, TypedExpr};

use super::assignable::{literal_to_type, literal_value_of};
use super::{Inferer, narrowing};

pub(super) struct ReferencePathState {
    pub path: narrowing::ReferencePath,
    pub contains_getter: bool,
}

impl<'a> Inferer<'a> {
    pub(super) fn expr_to_reference_path(
        &self,
        expr: &TypedExpr,
    ) -> Result<Option<narrowing::ReferencePath>, crate::compiler_error::CompilerFailure> {
        self.kind_to_reference_path(&expr.kind)
    }

    pub(super) fn kind_to_reference_path(
        &self,
        kind: &crate::TypedExprKind,
    ) -> Result<Option<narrowing::ReferencePath>, crate::compiler_error::CompilerFailure> {
        let Some(state) = self.kind_to_reference_path_state(kind)? else {
            return Ok(None);
        };
        Ok((!state.contains_getter).then_some(state.path))
    }

    pub(super) fn kind_to_reference_path_state(
        &self,
        kind: &crate::TypedExprKind,
    ) -> Result<Option<ReferencePathState>, crate::compiler_error::CompilerFailure> {
        use crate::TypedExprKind;
        Ok(match kind {
            TypedExprKind::Sequence { stmts, .. } => Some(ReferencePathState {
                path: match self.sequence_binding_path(stmts)? {
                    Some(value) => value,
                    None => return Ok(None),
                },
                contains_getter: false,
            }),
            TypedExprKind::LocalRef { ident, .. } => {
                let Some(entry) = self.scopes.get(&ident.name) else {
                    return Ok(None);
                };
                Some(ReferencePathState {
                    path: narrowing::ReferencePath::root(narrowing::BindingId::Local {
                        name: ident.name.clone(),
                        decl_scope: entry.decl_scope,
                    }),
                    contains_getter: false,
                })
            }
            TypedExprKind::LocalNarrowRef { path, .. } => Some(ReferencePathState {
                path: path.clone(),
                contains_getter: false,
            }),
            // Guarding a nullable field before use — `if (this.inner !== null)`
            // — is the most common narrowing shape in class-based code, so
            // `this` roots a path like any other binding. `current_class` gates
            // it: a `this` outside an instance member is already a diagnostic,
            // and rooting a path there would have no type to rebuild from.
            TypedExprKind::This if self.current_class.is_some() => Some(ReferencePathState {
                path: narrowing::ReferencePath::root(narrowing::BindingId::This),
                contains_getter: false,
            }),
            TypedExprKind::GlobalRef { mangled, .. } => Some(ReferencePathState {
                path: narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled.clone())),
                contains_getter: false,
            }),
            TypedExprKind::FieldAccess { receiver, name } => {
                let receiver_expr = self
                    .typed_ast
                    .try_expr(*receiver)
                    .map_err(crate::typechecker::arena_failure)?;
                let Some(mut state) = self.kind_to_reference_path_state(&receiver_expr.kind)?
                else {
                    return Ok(None);
                };
                state.contains_getter |=
                    self.receiver_type_has_getter(receiver_expr, &name.name)?;
                state
                    .path
                    .chain
                    .push(narrowing::PathElem::Field(name.name.clone()));
                Some(state)
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                let receiver_expr = self
                    .typed_ast
                    .try_expr(*receiver)
                    .map_err(crate::typechecker::arena_failure)?;
                let Some(mut state) = self.kind_to_reference_path_state(&receiver_expr.kind)?
                else {
                    return Ok(None);
                };
                let Some(lit) = index_literal_value(
                    &self
                        .typed_ast
                        .try_expr(*index)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind,
                ) else {
                    return Ok(None);
                };
                state.path.chain.push(narrowing::PathElem::Index(lit));
                Some(state)
            }
            TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => self
                .kind_to_reference_path_state(
                    &self
                        .typed_ast
                        .try_expr(*value)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind,
                )?,
            _ => None,
        })
    }

    pub(super) fn type_has_getter(&self, ty: &Type, field: &str) -> bool {
        match ty.peel() {
            Type::ClassRef { mangled, args, .. } => {
                self.class_getter(mangled, args, field).is_some()
            }
            Type::Union(members) => members
                .iter()
                .any(|member| self.type_has_getter(member, field)),
            _ => false,
        }
    }

    fn receiver_type_has_getter(
        &self,
        receiver: &TypedExpr,
        field: &str,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        if self.type_has_getter(&receiver.ty, field) {
            return Ok(true);
        }
        Ok(match &receiver.kind {
            crate::TypedExprKind::NonNullAssert { value }
            | crate::TypedExprKind::Cast { value, .. } => self.receiver_type_has_getter(
                self.typed_ast
                    .try_expr(*value)
                    .map_err(crate::typechecker::arena_failure)?,
                field,
            )?,
            _ => false,
        })
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
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let (mut true_env, mut false_env) = self.predicate_envs_unfiltered(cond_expr_id)?;
        self.retain_emittable_views(&mut true_env)?;
        self.retain_emittable_views(&mut false_env)?;
        Ok((true_env, false_env))
    }

    /// Whether `cond` can be true. A comparison cannot when it narrows a path
    /// to nothing, as `s !== null` does where `s` can only be `null`; `&&`,
    /// `||` and `!` combine their operands' answers. Anything else can.
    pub(super) fn condition_can_hold(
        &mut self,
        cond_expr_id: ExprId,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        self.condition_can_be(cond_expr_id, true)
    }

    pub(super) fn condition_can_be(
        &mut self,
        cond_expr_id: ExprId,
        outcome: bool,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        use crate::{BinOp, TypedExprKind, UnOp};
        Ok(
            match self
                .typed_ast
                .try_expr(cond_expr_id)
                .map_err(crate::typechecker::arena_failure)?
                .kind
                .clone()
            {
                TypedExprKind::Binary {
                    op: op @ (BinOp::And | BinOp::Or),
                    lhs,
                    rhs,
                } => {
                    // `a && b` is true only if both are, and false if either is.
                    let needs_both = (op == BinOp::And) == outcome;
                    let lhs_can = self.condition_can_be(lhs, outcome)?;
                    let rhs_can = self.condition_can_be(rhs, outcome)?;
                    if needs_both {
                        lhs_can && rhs_can
                    } else {
                        lhs_can || rhs_can
                    }
                }
                TypedExprKind::Unary {
                    op: UnOp::Not,
                    operand,
                } => self.condition_can_be(operand, !outcome)?,
                TypedExprKind::Narrowed { inner, .. } => self.condition_can_be(inner, outcome)?,
                // A type guard proves its predicate only when it returns true.
                TypedExprKind::Call { .. }
                | TypedExprKind::CallClosure { .. }
                | TypedExprKind::GenericCall { .. }
                | TypedExprKind::MethodCall { .. }
                | TypedExprKind::GenericMethodCall { .. }
                    if !outcome =>
                {
                    true
                }
                _ => {
                    // Unfiltered: dropping unemittable views could drop the empty one.
                    let (true_env, false_env) = self.predicate_envs_unfiltered(cond_expr_id)?;
                    let env = if outcome { true_env } else { false_env };
                    !env.values()
                        .any(|view| matches!(view.narrowed_ty.peel(), Type::Never | Type::Error))
                }
            },
        )
    }

    fn retain_emittable_views(
        &mut self,
        env: &mut narrowing::NarrowEnv,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let candidates: Vec<(narrowing::ReferencePath, ExprId)> =
            env.iter().map(|(p, v)| (p.clone(), v.source)).collect();
        let mut doomed: Vec<narrowing::ReferencePath> = Vec::new();
        for (path, source) in candidates {
            if self.source_shadow_is_live(source)? {
                continue;
            }
            // Ask the real wrap-time synthesizer rather than a second copy of
            // its rules: it runs against this same env, so its answer here is
            // the answer the wrap site will give. The throwaway nodes it
            // allocates are unreachable and never emitted.
            let span = self
                .typed_ast
                .try_expr(source)
                .map_err(crate::typechecker::arena_failure)?
                .span;
            if self
                .synthesize_wrap_time_source(&*env, &path, span)?
                .is_some()
            {
                continue;
            }
            doomed.push(path);
        }
        for path in doomed {
            if let Some(view) = env.remove(&path) {
                env.dropped.insert(path, view.narrowed_ty);
            }
        }
        Ok(())
    }

    fn predicate_envs_unfiltered(
        &mut self,
        cond_expr_id: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        use crate::{BinOp, TypedExprKind, UnOp};
        // Take an owned copy of the condition's kind / span / ty so we
        // can drop the immutable borrow on `self.typed_ast` before
        // calling helpers that mutate `self`.
        let cond = self
            .typed_ast
            .try_expr(cond_expr_id)
            .map_err(crate::typechecker::arena_failure)?;
        let cond_kind = cond.kind.clone();
        Ok(match cond_kind {
            TypedExprKind::Binary { op, lhs, rhs } if matches!(op, BinOp::Eq | BinOp::NotEq) => {
                let (mut true_env, mut false_env) = self
                    .try_predicate_envs_literal_equality(op, lhs, rhs)?
                    .map(Ok)
                    .unwrap_or_else(|| self.predicate_envs_eq_null(op, lhs, rhs))?;
                // A constant on the left leaves the right as the tested path,
                // read after anything it writes.
                if !self.is_constant_operand(lhs)? {
                    self.forget_later_writes(&mut true_env, lhs, rhs)?;
                    self.forget_later_writes(&mut false_env, lhs, rhs)?;
                }
                (true_env, false_env)
            }
            TypedExprKind::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } => self.predicate_envs_and(lhs, rhs)?,
            TypedExprKind::Binary {
                op: BinOp::Or,
                lhs,
                rhs,
            } => self.predicate_envs_or(lhs, rhs)?,
            TypedExprKind::Binary {
                op: BinOp::In,
                lhs,
                rhs,
            } => self.predicate_envs_in_operator(lhs, rhs)?,
            TypedExprKind::InstanceOf { value, class } => {
                // An always-false / non-class test resolves `class` to `Type::Error` in
                // inference; don't narrow.
                if matches!(class.peel(), crate::types::Type::Error) {
                    (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())
                } else {
                    self.narrow_path_to_asserted(value, &class)?
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
                self.predicate_envs_typeof_tag(value, facts)?
            }
            TypedExprKind::Unary {
                op: UnOp::Not,
                operand,
            } => {
                let (t, f) = self.predicate_envs_unfiltered(operand)?;
                (f, t)
            }
            TypedExprKind::Narrowed { inner, .. } => {
                // See through `Narrowed` wrappers that `wrap_narrow_exprs` installs on `&&`/`||` RHS.
                self.predicate_envs_unfiltered(inner)?
            }
            TypedExprKind::Call {
                args,
                type_predicate: Some(predicate),
                ..
            } => self.guard_envs(&predicate, &args, cond_expr_id)?,
            TypedExprKind::CallClosure { callee, args, .. } => {
                let Type::Function {
                    predicate: Some(predicate),
                    ..
                } = self
                    .typed_ast
                    .try_expr(callee)
                    .map_err(crate::typechecker::arena_failure)?
                    .ty
                    .peel()
                    .clone()
                else {
                    return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
                };
                self.guard_envs(&predicate, &args, cond_expr_id)?
            }
            TypedExprKind::GenericCall {
                args,
                type_predicate: Some(predicate),
                ..
            } => {
                // `asserted_type` already substituted with type-arg bindings during inference.
                let exprs: Vec<ExprId> = args.iter().map(|a| a.expr).collect();
                self.guard_envs(&predicate, &exprs, cond_expr_id)?
            }
            TypedExprKind::MethodCall {
                args,
                type_predicate: Some(predicate),
                ..
            } => self.guard_envs(&predicate, &args, cond_expr_id)?,
            TypedExprKind::GenericMethodCall {
                args,
                type_predicate: Some(predicate),
                ..
            } => {
                let exprs: Vec<ExprId> = args.iter().map(|a| a.expr).collect();
                self.guard_envs(&predicate, &exprs, cond_expr_id)?
            }
            TypedExprKind::LocalRef { .. }
            | TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FieldAccess { .. }
            | TypedExprKind::IndexAccess { .. }
            | TypedExprKind::Sequence { .. } => self.predicate_envs_truthiness(cond_expr_id)?,
            TypedExprKind::OptionalChain { .. } => (
                self.optional_chain_nonnull_env(cond_expr_id)?,
                narrowing::NarrowEnv::new(),
            ),
            _ => (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()),
        })
    }

    /// A user-defined guard's narrowing of its argument, without what a later
    /// argument writes.
    fn guard_envs(
        &mut self,
        predicate: &crate::TypePredicate,
        args: &[ExprId],
        call: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let Some((mut true_env, mut false_env)) =
            self.predicate_envs_user_guard(predicate, args)?
        else {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        };
        if let Some(&arg) = args.get(predicate.parameter_index as usize) {
            self.forget_later_writes(&mut true_env, arg, call)?;
            self.forget_later_writes(&mut false_env, arg, call)?;
        }
        Ok((true_env, false_env))
    }

    /// A literal or `null`: an operand that reads no reference path.
    fn is_constant_operand(
        &self,
        id: ExprId,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        Ok(matches!(
            self.typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?
                .kind,
            crate::TypedExprKind::Null
        ) || comparison_literal(&self.typed_ast, id)?.is_some())
    }

    fn predicate_envs_and(
        &mut self,
        lhs: ExprId,
        rhs: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let (mut lhs_true, lhs_false) = self.predicate_envs_unfiltered(lhs)?;
        // The right side runs only when the left is true.
        self.forget_later_writes(&mut lhs_true, lhs, rhs)?;
        self.push_narrow_frame(lhs_true.clone());
        let (rhs_true, rhs_false) = self.predicate_envs_unfiltered(rhs)?;
        self.pop_narrow_frame()?;
        let mut rhs_failure = lhs_true.clone();
        rhs_failure.extend(rhs_false);
        let (false_env, _) = narrowing::union_envs(
            lhs_false,
            Default::default(),
            rhs_failure,
            Default::default(),
        );
        // RHS supersedes LHS — ran under LHS narrowing, so its view is at least as specific.
        let mut composed = lhs_true;
        for (path, view) in rhs_true.into_iter() {
            composed.insert(path, view);
        }
        Ok((composed, false_env))
    }

    fn predicate_envs_or(
        &mut self,
        lhs: ExprId,
        rhs: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let (lhs_true, mut lhs_false) = self.predicate_envs_unfiltered(lhs)?;
        // The right side runs only when the left is false.
        self.forget_later_writes(&mut lhs_false, lhs, rhs)?;
        self.push_narrow_frame(lhs_false.clone());
        let (rhs_true, rhs_false) = self.predicate_envs_unfiltered(rhs)?;
        self.pop_narrow_frame()?;
        let mut rhs_success = lhs_false.clone();
        rhs_success.extend(rhs_true);
        let (true_env, _) = narrowing::union_envs(
            lhs_true,
            Default::default(),
            rhs_success,
            Default::default(),
        );
        let mut composed = lhs_false;
        for (path, view) in rhs_false.into_iter() {
            composed.insert(path, view);
        }
        Ok((true_env, composed))
    }

    fn predicate_envs_eq_null(
        &mut self,
        op: crate::BinOp,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        use crate::{BinOp, TypedExprKind};
        let lhs = self
            .typed_ast
            .try_expr(lhs_id)
            .map_err(crate::typechecker::arena_failure)?;
        let rhs = self
            .typed_ast
            .try_expr(rhs_id)
            .map_err(crate::typechecker::arena_failure)?;
        let lhs_is_null = matches!(lhs.kind, TypedExprKind::Null);
        let rhs_is_null = matches!(rhs.kind, TypedExprKind::Null);
        let chain_id = match (lhs_is_null, rhs_is_null) {
            (false, true) => lhs_id,
            (true, false) => rhs_id,
            _ => return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())),
        };
        if matches!(
            self.typed_ast
                .try_expr(chain_id)
                .map_err(crate::typechecker::arena_failure)?
                .kind,
            TypedExprKind::OptionalChain { .. }
        ) {
            let nonnull = self.optional_chain_nonnull_env(chain_id)?;
            return Ok(if op == BinOp::Eq {
                (narrowing::NarrowEnv::new(), nonnull)
            } else {
                (nonnull, narrowing::NarrowEnv::new())
            });
        }
        let lhs = self
            .typed_ast
            .try_expr(lhs_id)
            .map_err(crate::typechecker::arena_failure)?;
        let rhs = self
            .typed_ast
            .try_expr(rhs_id)
            .map_err(crate::typechecker::arena_failure)?;
        let path_expr = match (lhs_is_null, rhs_is_null) {
            (false, true) => lhs,
            (true, false) => rhs,
            _ => return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())),
        };
        let Some(path) = self.expr_to_reference_path(path_expr)? else {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }
        let path_ty = self.narrowing_source_ty(path_expr)?;
        // Never introduce a null alternative that the operand cannot hold.
        // Keep the non-null fact even when already proven: loop rechecking
        // needs it after invalidating the enclosing guard at a back edge.
        let can_be_null = super::assignable(&Type::Null, &path_ty, self.resolver());
        let path_span = path_expr.span;
        let fallback_kind = path_expr.kind.clone();

        // Prefer un-narrowed source so NarrowRegion materialization avoids dangling chain shadows.
        let source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)?
            .unwrap_or(fallback_kind);
        let mut eq_env = narrowing::NarrowEnv::new();
        let mut neq_env = narrowing::NarrowEnv::new();
        let source_eq = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind.clone(),
                span: path_span,
                ty: path_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_neq = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind,
                span: path_span,
                ty: path_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        if can_be_null {
            eq_env.insert(
                path.clone(),
                narrowing::NarrowedView {
                    narrowed_ty: Type::Null,
                    facts: narrowing::TypeFacts::EQ_NULL,
                    excluded_literals: std::collections::BTreeSet::new(),
                    binding: self.mint_narrow_binding(path_span)?,
                    source: source_eq,
                },
            );
        }
        neq_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: if matches!(path_ty.peel(), Type::Null) {
                    Type::Never
                } else {
                    narrowing::strip_null(&path_ty)
                },
                facts: narrowing::TypeFacts::NE_NULL,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span)?,
                source: source_neq,
            },
        );

        Ok(match op {
            BinOp::Eq => (eq_env, neq_env),
            BinOp::NotEq => (neq_env, eq_env),
            _ => {
                return Err(super::inference_failure(
                    "non-equality operator reached predicate narrowing",
                ));
            }
        })
    }

    /// A non-null chain result proves every optional receiver was present.
    /// A null result proves no individual field was null: an earlier receiver
    /// may have short-circuited, so it deliberately contributes no false facts.
    fn optional_chain_nonnull_env(
        &mut self,
        chain_id: ExprId,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        use crate::{BinOp, TypedChainPart, TypedExprKind};
        let chain = self
            .typed_ast
            .try_expr(chain_id)
            .map_err(crate::typechecker::arena_failure)?
            .clone();
        let TypedExprKind::OptionalChain { base, parts } = chain.kind else {
            return Ok(narrowing::NarrowEnv::new());
        };
        let mut env = narrowing::NarrowEnv::new();
        let null = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Null,
                span: chain.span,
                ty: Type::Null,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let mut receiver = base;
        for part in parts {
            let (TypedChainPart::Field {
                name,
                result_ty,
                optional,
                span,
            }
            | TypedChainPart::InterfaceProperty {
                name,
                result_ty,
                optional,
                span,
                ..
            }) = part
            else {
                return Ok(narrowing::NarrowEnv::new());
            };
            if optional {
                env.extend(self.predicate_envs_eq_null(BinOp::NotEq, receiver, null)?.0);
            }
            let receiver_expr = self
                .typed_ast
                .try_expr(receiver)
                .map_err(crate::typechecker::arena_failure)?;
            if self.receiver_type_has_getter(receiver_expr, &name.name)? {
                return Ok(narrowing::NarrowEnv::new());
            }
            receiver = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::FieldAccess { receiver, name },
                    span,
                    ty: result_ty,
                })
                .map_err(crate::typechecker::arena_failure)?;
        }
        env.extend(self.predicate_envs_eq_null(BinOp::NotEq, receiver, null)?.0);
        Ok(env)
    }

    fn predicate_envs_in_operator(
        &mut self,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        use crate::TypedExprKind;
        let lhs = self
            .typed_ast
            .try_expr(lhs_id)
            .map_err(crate::typechecker::arena_failure)?;
        let field_name = match &lhs.kind {
            TypedExprKind::String(s) => s.clone(),
            _ => return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new())),
        };
        let rhs = self
            .typed_ast
            .try_expr(rhs_id)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(rhs)? else {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }
        let path_ty = self.narrowing_source_ty(rhs)?;
        let path_span = rhs.span;
        let fallback_kind = rhs.kind.clone();

        let (true_ty, false_ty) =
            narrowing::narrow_field_presence(&path_ty, &field_name, &|member, field| {
                self.member_shape(member)?.get(field).cloned()
            });

        let source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)?
            .unwrap_or(fallback_kind);
        let source_true = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind.clone(),
                span: path_span,
                ty: path_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_false = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind,
                span: path_span,
                ty: path_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty.clone(),
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span)?,
                source: source_true,
            },
        );
        self.narrow_present_field(&mut true_env, &path, &true_ty, &field_name, path_span)?;
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span)?,
                source: source_false,
            },
        );
        Ok((true_env, false_env))
    }

    fn narrow_present_field(
        &mut self,
        env: &mut narrowing::NarrowEnv,
        receiver_path: &narrowing::ReferencePath,
        receiver_ty: &Type,
        field_name: &str,
        span: Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let members = match receiver_ty.peel() {
            Type::Union(members) => members.clone(),
            _ => vec![receiver_ty.clone()],
        };
        let Some(fields): Option<Vec<_>> = members
            .iter()
            .map(|member| self.member_shape(member)?.get(field_name).cloned())
            .collect()
        else {
            return Ok(());
        };
        let narrowed_ty = Type::union(fields.iter().map(|field| field.ty.clone()).collect());
        let mut path = receiver_path.clone();
        path.chain
            .push(narrowing::PathElem::Field(field_name.to_string()));
        let Some(receiver_kind) = self.synthesize_unnarrowed_source(receiver_path, span)? else {
            return Ok(());
        };
        let receiver = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: receiver_kind,
                span,
                ty: receiver_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: crate::TypedExprKind::FieldAccess {
                    receiver,
                    name: Ident {
                        name: field_name.to_string(),
                        span,
                    },
                },
                span,
                ty: Type::union(fields.iter().map(crate::ObjectField::read_ty).collect()),
            })
            .map_err(crate::typechecker::arena_failure)?;
        env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(span)?,
                source,
            },
        );
        Ok(())
    }

    fn try_predicate_envs_literal_equality(
        &mut self,
        op: crate::BinOp,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        let lhs_lit = comparison_literal(&self.typed_ast, lhs_id)?;
        let rhs_lit = comparison_literal(&self.typed_ast, rhs_id)?;
        let (path_id, literal) = match (lhs_lit, rhs_lit) {
            (None, Some(lit)) => (lhs_id, lit),
            (Some(lit), None) => (rhs_id, lit),
            _ => return Ok(None),
        };

        let path_expr = self
            .typed_ast
            .try_expr(path_id)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(path_expr)? else {
            return Ok(None);
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok(None);
        }
        let path_ty = self.narrowing_source_ty(path_expr)?;
        let path_span = path_expr.span;
        let path_kind = path_expr.kind.clone();

        // Prefer un-narrowed source so NarrowRegion doesn't depend on a chain shadow.
        let direct_source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)?
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

        self.narrow_literal_discriminant(
            op,
            path.clone(),
            path_ty.clone(),
            path_kind,
            path_span,
            literal.clone(),
        )?
        .map(|value| Ok(Some(value)))
        .unwrap_or_else(|| {
            Ok::<_, crate::compiler_error::CompilerFailure>({
                self.narrow_direct_literal(
                    op,
                    path,
                    path_ty,
                    direct_source_kind,
                    path_span,
                    literal,
                )?
            })
        })
    }

    fn narrow_literal_discriminant(
        &mut self,
        op: crate::BinOp,
        path: narrowing::ReferencePath,
        path_ty: Type,
        path_kind: crate::TypedExprKind,
        path_span: Span,
        literal: narrowing::LiteralValue,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        use crate::{BinOp, TypedExprKind};
        enum DiscKey {
            Field(String),
            Position(usize),
        }
        let (root_path, root_ty, root_span, root_kind, disc_key_from_path) = match &path_kind {
            TypedExprKind::FieldAccess { receiver, name } => {
                let receiver_expr = self
                    .typed_ast
                    .try_expr(*receiver)
                    .map_err(crate::typechecker::arena_failure)?;
                let Some(root_path) = self.expr_to_reference_path(receiver_expr)? else {
                    return Ok(None);
                };
                let receiver_ty = receiver_expr.ty.clone();
                let receiver_span = receiver_expr.span;
                let fallback_kind = receiver_expr.kind.clone();
                let root_kind = self
                    .synthesize_unnarrowed_source(&root_path, receiver_span)?
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
                let receiver_expr = self
                    .typed_ast
                    .try_expr(*receiver)
                    .map_err(crate::typechecker::arena_failure)?;
                let Some(root_path) = self.expr_to_reference_path(receiver_expr)? else {
                    return Ok(None);
                };
                let receiver_ty = receiver_expr.ty.clone();
                let receiver_span = receiver_expr.span;
                let fallback_kind = receiver_expr.kind.clone();
                let root_kind = self
                    .synthesize_unnarrowed_source(&root_path, receiver_span)?
                    .unwrap_or(fallback_kind);
                let Some(position) = index_position(
                    &self
                        .typed_ast
                        .try_expr(*index)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind,
                ) else {
                    return Ok(None);
                };
                (
                    root_path,
                    receiver_ty,
                    receiver_span,
                    root_kind,
                    DiscKey::Position(position),
                )
            }
            TypedExprKind::LocalNarrowRef { .. } => {
                let disc_key = match match path.chain.last().cloned() {
                    Some(value) => value,
                    None => return Ok(None),
                } {
                    narrowing::PathElem::Field(name) => DiscKey::Field(name),
                    narrowing::PathElem::Index(narrowing::LiteralValue::Number(n))
                        if n.0.is_finite() && n.0.fract() == 0.0 && n.0 >= 0.0 =>
                    {
                        DiscKey::Position(n.0 as usize)
                    }
                    _ => return Ok(None),
                };
                let mut root_path = path.clone();
                root_path.chain.pop();
                let Some((root_ty, root_kind)) = self.derive_root_source(&root_path, path_span)
                else {
                    return Ok(None);
                };
                (root_path, root_ty, path_span, root_kind, disc_key)
            }
            _ => return Ok(None),
        };

        // Peel aliases so `type Shape = A | B` pattern-matches as union.
        let Type::Union(members) = root_ty.peel() else {
            return Ok(None);
        };
        let table_lookup: Option<narrowing::VariantIdx> = match &disc_key_from_path {
            DiscKey::Field(key_field) => {
                let Some((disc_key, table)) = self.union_discriminant_with_nominals(members) else {
                    return Ok(None);
                };
                if disc_key != *key_field {
                    return Ok(None);
                }
                table.get(&literal).copied()
            }
            DiscKey::Position(pos) => {
                let Some((disc_pos, table)) = narrowing::tuple_union_discriminant(members) else {
                    return Ok(None);
                };
                if disc_pos != *pos {
                    return Ok(None);
                }
                table.get(&literal).copied()
            }
        };
        let Some(matching_idx) = table_lookup else {
            return Ok(None);
        };
        let matched_variant = members
            .get(matching_idx.0 as usize)
            .ok_or_else(|| super::inference_failure("invalid discriminant variant index"))?
            .clone();
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
            _ => {
                return Err(super::inference_failure(
                    "non-equality operator reached predicate narrowing",
                ));
            }
        };

        let true_root_ty = narrowing::with_source_refinement(&root_ty, true_root_ty);
        let false_root_ty = narrowing::with_source_refinement(&root_ty, false_root_ty);
        let source_true = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: root_kind.clone(),
                span: root_span,
                ty: root_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_false = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: root_kind,
                span: root_span,
                ty: root_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            root_path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(root_span)?,
                source: source_true,
            },
        );
        false_env.insert(
            root_path,
            narrowing::NarrowedView {
                narrowed_ty: false_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(root_span)?,
                source: source_false,
            },
        );
        // Also narrow the field path: `s.kind` stays usable as a refined literal
        // and `excluded_literals` enables 3+-variant chain composition.
        let direct_source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)?
            .unwrap_or(path_kind);
        if let Some((extra_true, extra_false)) =
            self.narrow_direct_literal(op, path, path_ty, direct_source_kind, path_span, literal)?
        {
            for (p, v) in extra_true {
                true_env.insert(p, v);
            }
            for (p, v) in extra_false {
                false_env.insert(p, v);
            }
        }

        Ok(Some((true_env, false_env)))
    }

    fn narrow_direct_literal(
        &mut self,
        op: crate::BinOp,
        path: narrowing::ReferencePath,
        path_ty: Type,
        path_kind: crate::TypedExprKind,
        path_span: Span,
        literal: narrowing::LiteralValue,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        use crate::BinOp;
        let members = match path_ty.peel() {
            Type::Union(members) => members.as_slice(),
            ty => std::slice::from_ref(ty),
        };
        let literal_ty = literal_to_type(&literal);
        if path_ty.peel() == &literal_ty {
            return Ok(None);
        }
        let mut matched: Vec<Type> = Vec::new();
        let mut remaining: Vec<Type> = Vec::new();
        for m in members {
            if m.peel() == &literal_ty {
                matched.push(m.clone());
            } else if let (Type::Boolean, narrowing::LiteralValue::Boolean(value)) =
                (m.peel(), &literal)
            {
                // `boolean` is `true | false`: `b === true` leaves `false`.
                matched.push(literal_ty.clone());
                remaining.push(Type::BooleanLiteral(!value));
            } else {
                if matches!(m.peel(), Type::Unknown) || m.peel() == &literal_ty.widen_literal() {
                    matched.push(literal_ty.clone());
                }
                remaining.push(m.clone());
            }
        }
        if matched.is_empty() {
            // Predicate is statically false — typechecker accepted
            // the comparison anyway. Skip narrowing.
            return Ok(None);
        }
        let matched_ty = Type::union(matched);
        let remaining_ty = Type::union(remaining);
        let (true_ty, false_ty) = match op {
            BinOp::Eq => (matched_ty, remaining_ty),
            BinOp::NotEq => (remaining_ty, matched_ty),
            _ => {
                return Err(super::inference_failure(
                    "non-equality operator reached predicate narrowing",
                ));
            }
        };

        let true_ty = narrowing::with_source_refinement(&path_ty, true_ty);
        let false_ty = narrowing::with_source_refinement(&path_ty, false_ty);
        let source_true = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: path_kind.clone(),
                span: path_span,
                ty: path_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_false = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: path_kind,
                span: path_span,
                ty: path_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        let mut false_excluded = std::collections::BTreeSet::new();
        false_excluded.insert(literal);
        let (true_excluded, false_excluded) = if op == BinOp::NotEq {
            (false_excluded, std::collections::BTreeSet::new())
        } else {
            (std::collections::BTreeSet::new(), false_excluded)
        };
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: true_excluded,
                binding: self.mint_narrow_binding(path_span)?,
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: false_excluded,
                binding: self.mint_narrow_binding(path_span)?,
                source: source_false,
            },
        );
        Ok(Some((true_env, false_env)))
    }

    fn predicate_envs_typeof_tag(
        &mut self,
        value_id: ExprId,
        facts: narrowing::TypeFacts,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        if facts.is_empty() {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }
        let (mut true_env, mut false_env) = self.predicate_envs_from_facts(value_id, facts)?;
        self.predicate_envs_tuple_typeof_rollup(value_id, facts, &mut true_env, &mut false_env)?;
        Ok((true_env, false_env))
    }

    fn predicate_envs_tuple_typeof_rollup(
        &mut self,
        value_id: ExprId,
        facts: narrowing::TypeFacts,
        true_env: &mut narrowing::NarrowEnv,
        false_env: &mut narrowing::NarrowEnv,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let value_expr = self
            .typed_ast
            .try_expr(value_id)
            .map_err(crate::typechecker::arena_failure)?;
        let crate::TypedExprKind::IndexAccess { receiver, index } = &value_expr.kind else {
            return Ok(());
        };
        let Some(position) = index_position(
            &self
                .typed_ast
                .try_expr(*index)
                .map_err(crate::typechecker::arena_failure)?
                .kind,
        ) else {
            return Ok(());
        };
        let receiver_expr = self
            .typed_ast
            .try_expr(*receiver)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(root_path) = self.expr_to_reference_path(receiver_expr)? else {
            return Ok(());
        };
        if self.path_root_is_captured_mutator(&root_path) {
            return Ok(());
        }
        let root_ty = receiver_expr.ty.clone();
        let Type::Union(members) = root_ty.peel() else {
            return Ok(());
        };
        if !members.iter().all(|m| matches!(m.peel(), Type::Tuple(_))) {
            return Ok(());
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
            .synthesize_unnarrowed_source(&root_path, receiver_span)?
            .unwrap_or(fallback_kind);
        let true_root_ty = Type::union(matching);
        let false_root_ty = Type::union(non_matching);
        let true_root_ty = narrowing::with_source_refinement(&root_ty, true_root_ty);
        let false_root_ty = narrowing::with_source_refinement(&root_ty, false_root_ty);
        let source_true = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: root_kind.clone(),
                span: receiver_span,
                ty: root_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_false = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: root_kind,
                span: receiver_span,
                ty: root_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        true_env.insert(
            root_path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(receiver_span)?,
                source: source_true,
            },
        );
        false_env.insert(
            root_path,
            narrowing::NarrowedView {
                narrowed_ty: false_root_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(receiver_span)?,
                source: source_false,
            },
        );
        Ok(())
    }

    fn predicate_envs_from_facts(
        &mut self,
        value_id: ExprId,
        facts: narrowing::TypeFacts,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let value_expr = self
            .typed_ast
            .try_expr(value_id)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(value_expr)? else {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }
        let from_ty = self.narrowing_source_ty(value_expr)?;
        let value_span = value_expr.span;
        let fallback_kind = value_expr.kind.clone();

        let true_ty = narrowing::intersect_with(&from_ty, facts);
        let false_ty = narrowing::subtract(&from_ty, facts);

        let true_facts = facts;
        // False-branch facts left EMPTY — `narrowed_ty` is load-bearing; `facts` only
        // consulted for EQ_NULL/NE_NULL.
        let false_facts = narrowing::TypeFacts::EMPTY;

        let source_kind = self
            .synthesize_unnarrowed_source(&path, value_span)?
            .unwrap_or(fallback_kind);
        let source_true = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind.clone(),
                span: value_span,
                ty: from_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_false = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind,
                span: value_span,
                ty: from_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                narrowed_ty: true_ty,
                facts: true_facts,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(value_span)?,
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: false_facts,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(value_span)?,
                source: source_false,
            },
        );
        Ok((true_env, false_env))
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
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let path_expr = self
            .typed_ast
            .try_expr(path_expr_id)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(path_expr)? else {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }
        let from_ty = self.narrowing_source_ty(path_expr)?;
        let span = path_expr.span;
        let fallback_kind = path_expr.kind.clone();

        // `if (r.ok)` on a union tagged `ok: true` / `ok: false` tests the tag
        // exactly as `r.ok === true` does, and narrows `r` the same way.
        if !path.chain.is_empty()
            && matches!(from_ty.peel(), Type::Boolean)
            && let Some(envs) = self.narrow_literal_discriminant(
                crate::BinOp::Eq,
                path.clone(),
                from_ty.clone(),
                fallback_kind.clone(),
                span,
                narrowing::LiteralValue::Boolean(true),
            )?
        {
            return Ok(envs);
        }

        let true_ty = narrowing::intersect_with(&from_ty, narrowing::TypeFacts::TRUTHY);
        let false_ty = narrowing::intersect_with(&from_ty, narrowing::TypeFacts::FALSY);
        let refines = |ty: &Type| !matches!(ty, Type::Error) && ty.peel() != from_ty.peel();

        if !refines(&true_ty) && !refines(&false_ty) {
            return Ok((narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()));
        }

        // A falsy outcome proves `x === null` only when null is the sole falsy
        // value the type can hold.
        let false_facts = if narrowing::falsy_values_are_only_null(&from_ty) {
            narrowing::TypeFacts::FALSY | narrowing::TypeFacts::EQ_NULL
        } else {
            narrowing::TypeFacts::FALSY
        };

        let source_kind = self
            .synthesize_unnarrowed_source(&path, span)?
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
            let source = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: source_kind.clone(),
                    span,
                    ty: from_ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            env.insert(
                path.clone(),
                narrowing::NarrowedView {
                    narrowed_ty: ty,
                    facts,
                    excluded_literals: std::collections::BTreeSet::new(),
                    binding: self.mint_narrow_binding(span)?,
                    source,
                },
            );
        }
        Ok((true_env, false_env))
    }

    /// Leading `#` prevents collision with user identifiers (first-char rule: `[A-Za-z_$]`).
    pub(super) fn mint_narrow_binding(&mut self, span: Span) -> Result<Ident, CompilerFailure> {
        let n = self.next_narrow_counter;
        self.next_narrow_counter =
            self.next_narrow_counter
                .checked_add(1)
                .ok_or_else(|| CompilerFailure::Limit {
                    stage: crate::compiler_error::CompilerStage::Infer,
                    span: Some(span),
                    message: "narrowing binding capacity exceeded".into(),
                    help: Vec::new(),
                })?;
        Ok(Ident {
            name: format!("#narrow_{n}"),
            span,
        })
    }

    /// Builds an expression for `path` that bypasses chain-shadow `Narrowed` wrappers.
    /// Codegen evaluates this at NarrowRegion entry to seed the shadow local.
    pub(super) fn synthesize_unnarrowed_source(
        &mut self,
        path: &narrowing::ReferencePath,
        path_span: Span,
    ) -> Result<Option<crate::TypedExprKind>, crate::compiler_error::CompilerFailure> {
        if path.chain.is_empty() {
            return Ok(Some(match &path.root {
                narrowing::BindingId::Local { name, .. } => {
                    let Some(_entry) = self.scopes.get(name) else {
                        return Ok(None);
                    };
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
            }));
        }
        let mut receiver_path = path.clone();
        let Some(last_field) = receiver_path.chain.pop() else {
            return Ok(None);
        };
        let narrowing::PathElem::Field(field_name) = last_field else {
            return Ok(None);
        };
        let Some(receiver_kind) = self.synthesize_unnarrowed_source(&receiver_path, path_span)?
        else {
            return Ok(None);
        };
        let Some(receiver_ty) = self.derive_type_for_reference_path(&receiver_path) else {
            return Ok(None);
        };
        // Bail when the field can't be resolved on the receiver's shape: without
        // a source we can reconstruct from declared types, the view would fall
        // back to an already-narrowed expression whose shadow is scoped to an
        // inner region.
        match self.narrow_source_field_ty(&receiver_ty, &field_name) {
            Some(value) => value,
            None => return Ok(None),
        };
        let receiver_id = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: receiver_kind,
                span: path_span,
                ty: receiver_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(Some(crate::TypedExprKind::FieldAccess {
            receiver: receiver_id,
            name: Ident {
                name: field_name,
                span: path_span,
            },
        }))
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
            if self.tombstone_scopes.get(frame_idx).is_some_and(|tombs| {
                tombs
                    .iter()
                    .any(|(written, reason)| reason.invalidates() && written.is_prefix_of(path))
            }) {
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

fn comparison_literal(
    ast: &crate::TypedAst,
    id: ExprId,
) -> Result<Option<narrowing::LiteralValue>, crate::compiler_error::CompilerFailure> {
    let expr = ast
        .try_expr(id)
        .map_err(crate::typechecker::arena_failure)?;
    literal_value_of(&expr.kind).map_or_else(
        || {
            Ok::<_, crate::compiler_error::CompilerFailure>({
                match super::expr::literal_comparison_type(ast, expr)? {
                    Type::NumberLiteral(value) => Some(narrowing::LiteralValue::Number(value)),
                    Type::StringLiteral(value) => Some(narrowing::LiteralValue::String(value)),
                    Type::BooleanLiteral(value) => Some(narrowing::LiteralValue::Boolean(value)),
                    _ => None,
                }
            })
        },
        |value| Ok(Some(value)),
    )
}
