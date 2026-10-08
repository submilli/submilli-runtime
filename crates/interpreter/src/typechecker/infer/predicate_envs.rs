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
                let Some(element) = self.index_path_elem(*index)? else {
                    return Ok(None);
                };
                let element = self
                    .constant_key_field(&element, &receiver_expr.ty)
                    .unwrap_or(element);
                state.path.chain.push(element);
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

    /// The path element an index reads at: its literal value, or the binding
    /// holding it when that binding holds one value for its whole life.
    pub(super) fn index_path_elem(
        &self,
        index: ExprId,
    ) -> Result<Option<narrowing::PathElem>, crate::compiler_error::CompilerFailure> {
        let index_expr = self
            .typed_ast
            .try_expr(index)
            .map_err(crate::typechecker::arena_failure)?;
        if let Some(literal) = index_literal_value(&index_expr.kind) {
            return Ok(Some(narrowing::PathElem::Index(literal)));
        }
        let Some(state) = self.kind_to_reference_path_state(&index_expr.kind)? else {
            return Ok(None);
        };
        let root = state.path.root;
        if !state.path.chain.is_empty()
            || matches!(root, narrowing::BindingId::This)
            || self.constant_root_type(&root).is_none()
        {
            return Ok(None);
        }
        let kind = if is_property_key(&index_expr.ty) {
            narrowing::KeyKind::Property
        } else {
            narrowing::KeyKind::Element
        };
        Ok(Some(narrowing::PathElem::Key(root, kind)))
    }

    /// The field a string key names, when it is a literal or a constant holding
    /// one, and the receiver declares that field: TypeScript reads `o["a"]`, and
    /// `o[key]` after `const key = "a"`, as `o.a`, so a guard or write through
    /// either spelling is one through the other.
    fn constant_key_field(
        &self,
        element: &narrowing::PathElem,
        receiver_ty: &Type,
    ) -> Option<narrowing::PathElem> {
        let name = match element {
            narrowing::PathElem::Index(narrowing::LiteralValue::String(name)) => name.clone(),
            narrowing::PathElem::Key(root, narrowing::KeyKind::Property) => {
                let Type::StringLiteral(name) = self.constant_root_type(root)?.peel().clone()
                else {
                    return None;
                };
                name
            }
            _ => return None,
        };
        let Type::Object { fields, .. } = receiver_ty.peel() else {
            return None;
        };
        fields
            .contains_key(&name)
            .then_some(narrowing::PathElem::Field(name))
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
                    !is_unreachable_env(&env)
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

    pub(super) fn predicate_envs_unfiltered(
        &mut self,
        cond_expr_id: ExprId,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let (mut true_env, mut false_env) = self.direct_predicate_envs(cond_expr_id)?;
        if let Some((alias_true, alias_false)) = self.aliased_condition_envs(cond_expr_id)? {
            add_missing_views(&mut true_env, alias_true);
            add_missing_views(&mut false_env, alias_false);
        }
        Ok((true_env, false_env))
    }

    /// What `cond_expr_id` narrows by its own form, before any `const` it
    /// reads is seen through.
    fn direct_predicate_envs(
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
                // read after anything it writes. A constant on the right writes
                // nothing after the left is read; skipping the scan also keeps a
                // switch's synthesized `discriminant === case` comparisons, whose
                // case value is allocated after every earlier case body, from
                // rescanning those bodies for each case.
                if !self.is_constant_operand(lhs)? && !self.is_constant_operand(rhs)? {
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
        let false_env = join_reachable_envs(lhs_false, rhs_failure);
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
        let true_env = join_reachable_envs(lhs_true, rhs_success);
        let mut composed = lhs_false;
        for (path, view) in rhs_false.into_iter() {
            composed.insert(path, view);
        }
        Ok((true_env, composed))
    }

    /// The narrowing where `operand` is `null`: the right side of `operand ?? …`.
    pub(super) fn null_operand_env(
        &mut self,
        operand: ExprId,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        let span = self
            .typed_ast
            .try_expr(operand)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        let null = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: crate::TypedExprKind::Null,
                span,
                ty: Type::Null,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(self
            .predicate_envs_eq_null(crate::BinOp::Eq, operand, null)?
            .0)
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
            if let Some(envs) =
                self.narrow_optional_chain_discriminant(op, chain_id, &Type::Null)?
            {
                return Ok(envs);
            }
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
        let exclusions = self.known_exclusions(&path);
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
        let (mut eq_env, mut neq_env) =
            self.null_discriminant_envs(&path, &fallback_kind, path_span)?;

        // Prefer un-narrowed source so NarrowRegion materialization avoids dangling chain shadows.
        let source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)?
            .unwrap_or(fallback_kind);
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
        // A local whose type can't hold `null` is `never` where it equals
        // `null`. A type parameter can hold anything, so it narrows nothing.
        let never_null = !can_be_null
            && self.rules_out_to_never(&path)
            && !narrowing::has_erased_member(&path_ty);
        if can_be_null || never_null {
            eq_env.insert(
                path.clone(),
                narrowing::NarrowedView {
                    narrowed_ty: if can_be_null {
                        Type::Null
                    } else {
                        narrowing::RULED_OUT
                    },
                    facts: narrowing::TypeFacts::EQ_NULL,
                    excluded_literals: exclusions.clone(),
                    binding: self.mint_narrow_binding(path_span)?,
                    source: source_eq,
                },
            );
        }
        // A field that is `null` reads as `never` once proven otherwise, but
        // re-reads its live value: an alias may have written it.
        let non_null_ty = match narrowing::strip_null(&path_ty) {
            ty if narrowing::is_ruled_out(&ty) && !self.rules_out_to_never(&path) => Type::Never,
            ty => ty,
        };
        neq_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: non_null_ty,
                facts: narrowing::TypeFacts::NE_NULL,
                excluded_literals: exclusions.clone(),
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

    /// The views `s.kind === null` puts on `s` when `kind` is a discriminant
    /// some member types `null`: as for a literal, the members whose `kind` may
    /// be `null` where it is, and the rest where it isn't. Empty otherwise.
    fn null_discriminant_envs(
        &mut self,
        path: &narrowing::ReferencePath,
        path_kind: &crate::TypedExprKind,
        path_span: Span,
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        let none = (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new());
        if path.chain.is_empty() {
            return Ok(none);
        }
        let Some(root) = self.discriminant_root(path, path_kind, path_span)? else {
            return Ok(none);
        };
        let Type::Union(members) = root.ty.peel() else {
            return Ok(none);
        };
        let Some(key_tys) = (match &root.key {
            DiscriminantKey::Field(key) => self.discriminant_field_types(members, key),
            DiscriminantKey::Position(position) => discriminant_element_types(members, *position),
        }) else {
            return Ok(none);
        };
        let split = self.split_by_key_types(members, key_tys, &Type::Null);
        self.root_discriminant_envs(
            crate::BinOp::Eq,
            root.path,
            root.ty,
            root.span,
            root.kind,
            split,
        )
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
            // A primitive's property (`s?.length`) is no field path: a view on
            // it would read `s` as an object.
            if !Self::is_field_bearing(&narrowing::strip_null(&receiver_expr.ty)) {
                return Ok(env);
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
        // The left side was read before the right one ran: if the right one
        // writes it (`a === f(a = "y")`), the comparison says nothing about the
        // value it holds now.
        let lhs_rewritten = self.is_written_within(lhs_id, rhs_id)?;
        match (lhs_lit, rhs_lit) {
            (None, Some(_) | None) if lhs_rewritten => Ok(None),
            (None, Some(lit)) => self.narrow_to_other_literal(op, lhs_id, lit),
            (Some(lit), None) => self.narrow_to_other_literal(op, rhs_id, lit),
            (None, None) => self.narrow_equal_to_value(op, lhs_id, rhs_id),
            (Some(lhs_lit), Some(_)) if lhs_rewritten => {
                self.narrow_to_other_literal(op, rhs_id, lhs_lit)
            }
            (Some(lhs_lit), Some(rhs_lit)) => {
                let lhs_envs = self.narrow_to_other_literal(op, lhs_id, rhs_lit)?;
                let rhs_envs = self.narrow_to_other_literal(op, rhs_id, lhs_lit)?;
                Ok(match (lhs_envs, rhs_envs) {
                    (Some((mut equal, mut unequal)), Some((other_equal, other_unequal))) => {
                        add_missing_views(&mut equal, other_equal);
                        add_missing_views(&mut unequal, other_unequal);
                        Some((equal, unequal))
                    }
                    (envs, other) => envs.or(other),
                })
            }
        }
    }

    /// Whether the reference `path_id` reads is written while `other_id` runs.
    fn is_written_within(
        &self,
        path_id: ExprId,
        other_id: ExprId,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        let path_expr = self
            .typed_ast
            .try_expr(path_id)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(path_expr)? else {
            return Ok(false);
        };
        let other_span = self
            .typed_ast
            .try_expr(other_id)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        Ok(self.last_write_spans.iter().any(|(written, write_span)| {
            written.is_prefix_of(&path) && other_span.encloses(*write_span)
        }))
    }

    /// Narrows `path_id` compared with a value whose type is the literal
    /// `other_lit`, unless `path_id` is itself written out as a literal.
    fn narrow_to_other_literal(
        &mut self,
        op: crate::BinOp,
        path_id: ExprId,
        other_lit: narrowing::LiteralValue,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        if is_written_literal(&self.typed_ast, path_id)? {
            return Ok(None);
        }
        self.narrow_equal_to_literal(op, path_id, other_lit)
    }

    /// Narrows a path compared with a value whose type is not one literal, as
    /// TypeScript does: where they are equal, the path holds a value both
    /// types allow. Where they differ nothing is known, since the value may be
    /// any member.
    fn narrow_equal_to_value(
        &mut self,
        op: crate::BinOp,
        lhs_id: ExprId,
        rhs_id: ExprId,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        // Where they are equal, each side holds a value the other allows.
        let equal = match (
            self.equal_to(lhs_id, rhs_id)?,
            self.equal_to(rhs_id, lhs_id)?,
        ) {
            (None, None) => return Ok(None),
            (Some(equal), None) | (None, Some(equal)) => equal,
            (Some(mut equal), Some(other)) => {
                add_missing_views(&mut equal, other);
                equal
            }
        };
        Ok(Some(if op == crate::BinOp::Eq {
            (equal, narrowing::NarrowEnv::new())
        } else {
            (narrowing::NarrowEnv::new(), equal)
        }))
    }

    /// The narrowing where `path_id` equals `value_id`, whose type is not one
    /// literal.
    fn equal_to(
        &mut self,
        path_id: ExprId,
        value_id: ExprId,
    ) -> Result<Option<narrowing::NarrowEnv>, crate::compiler_error::CompilerFailure> {
        match comparison_literal_union(&self.typed_ast, value_id)? {
            Some(literals) => self.equal_to_one_of(path_id, literals),
            None => self.equal_to_value_of(path_id, value_id),
        }
    }

    /// The narrowing where `path_id` equals one of `literals`: the join of
    /// its narrowing equal to each. This also narrows a discriminated union
    /// through its discriminant (`m.kind === x`).
    fn equal_to_one_of(
        &mut self,
        path_id: ExprId,
        literals: Vec<narrowing::LiteralValue>,
    ) -> Result<Option<narrowing::NarrowEnv>, crate::compiler_error::CompilerFailure> {
        let mut equal: Option<narrowing::NarrowEnv> = None;
        for literal in literals {
            // A literal the path can't hold can't be the one it equals.
            let Some((literal_eq, _)) =
                self.narrow_equal_to_literal(crate::BinOp::Eq, path_id, literal)?
            else {
                continue;
            };
            equal = Some(match equal {
                None => literal_eq,
                Some(so_far) => {
                    narrowing::union_envs(
                        so_far,
                        std::collections::BTreeSet::new(),
                        literal_eq,
                        std::collections::BTreeSet::new(),
                    )
                    .0
                }
            });
        }
        Ok(equal)
    }

    /// The narrowing where `path_id` equals `value_id`, whose type is not a
    /// literal or a union of literals: the path keeps what it shares with the
    /// value's type, so `number | "a"` equal to `1 | string` is `1 | "a"`, and
    /// `E | null` equal to an `E` is `E`.
    fn equal_to_value_of(
        &mut self,
        path_id: ExprId,
        value_id: ExprId,
    ) -> Result<Option<narrowing::NarrowEnv>, crate::compiler_error::CompilerFailure> {
        let value_ty = self
            .typed_ast
            .try_expr(value_id)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .clone();
        // A comparison with `null` has a narrowing of its own.
        if matches!(value_ty.peel(), Type::Null) {
            return Ok(None);
        }
        // TypeScript compares `null` as comparable to a type parameter, so
        // `x === y` with `x: T | null` and `y: T` leaves `x` as it was.
        if !matches!(value_ty.peel(), Type::Union(_))
            && narrowing::has_type_parameter_member(&value_ty)
        {
            return Ok(None);
        }
        let path_expr = self
            .typed_ast
            .try_expr(path_id)
            .map_err(crate::typechecker::arena_failure)?;
        if matches!(path_expr.kind, crate::TypedExprKind::OptionalChain { .. }) {
            return Ok(None);
        }
        let Some(path) = self.expr_to_reference_path(path_expr)? else {
            return Ok(None);
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok(None);
        }
        let path_ty = self.narrowing_source_ty(path_expr)?;
        let path_span = path_expr.span;
        let fallback_kind = path_expr.kind.clone();
        let equal_ty = self.shared_values(&path_ty, &value_ty);
        if matches!(equal_ty, Type::Never) || equal_ty.peel() == path_ty.peel() {
            return Ok(None);
        }
        let source_kind = self
            .synthesize_unnarrowed_source(&path, path_span)?
            .unwrap_or(fallback_kind);
        let source = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind,
                span: path_span,
                ty: path_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let mut env = narrowing::NarrowEnv::new();
        env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: narrowing::with_source_refinement(&path_ty, equal_ty),
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: std::collections::BTreeSet::new(),
                binding: self.mint_narrow_binding(path_span)?,
                source,
            },
        );
        Ok(Some(env))
    }

    /// The values `path_ty` and `value_ty` both allow, member by member, as
    /// TypeScript keeps them: a path member related to a value member stays,
    /// except that it gives way to a narrower unit value member (a literal, an
    /// enum, or `null`), so `number` and `1` share `1`, and `"a"` and `string`
    /// share `"a"`. An `Animal` compared with a `Dog` stays an `Animal`.
    /// Unrelated members share nothing.
    fn shared_values(&self, path_ty: &Type, value_ty: &Type) -> Type {
        let value_members = narrowing::union_members(value_ty);
        let mut shared = Vec::new();
        for member in narrowing::union_members(path_ty) {
            for value_member in &value_members {
                if super::assignable(member, value_member, self.resolver())
                    || self.objects_may_be_equal(member, value_member)
                {
                    shared.push(member.clone());
                } else if super::assignable(value_member, member, self.resolver()) {
                    let value_is_unit = narrowing::has_unit_member(value_member);
                    shared.push(if value_is_unit { *value_member } else { member }.clone());
                }
            }
        }
        Type::union(shared)
    }

    /// [`may_share_an_object`], unless a property both shapes require can't
    /// hold the same value in each (`kind: "a"` against `kind: "b"`).
    fn objects_may_be_equal(&self, left: &Type, right: &Type) -> bool {
        if !may_share_an_object(left, right) {
            return false;
        }
        match (self.member_shape(left), self.member_shape(right)) {
            (Some(left), Some(right)) => !have_disjoint_unit_property(&left, &right),
            _ => true,
        }
    }

    /// Narrows the path `path_id` compared with `literal`.
    fn narrow_equal_to_literal(
        &mut self,
        op: crate::BinOp,
        path_id: ExprId,
        literal: narrowing::LiteralValue,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        let path_expr = self
            .typed_ast
            .try_expr(path_id)
            .map_err(crate::typechecker::arena_failure)?;
        if matches!(path_expr.kind, crate::TypedExprKind::OptionalChain { .. }) {
            let literal_ty = narrowing::literal_type(&literal);
            if let Some(envs) = self.narrow_optional_chain_discriminant(op, path_id, &literal_ty)? {
                return Ok(Some(envs));
            }
            // Equal to a literal, the chain reached its end: nothing on it is `null`.
            let reached = self.optional_chain_nonnull_env(path_id)?;
            return Ok(Some(if op == crate::BinOp::Eq {
                (reached, narrowing::NarrowEnv::new())
            } else {
                (narrowing::NarrowEnv::new(), reached)
            }));
        }
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

    /// The object a discriminant read `path` tests, with the key it reads:
    /// `s.kind` tests `s` by its `kind` field, and `t[0]` tests `t` by position.
    fn discriminant_root(
        &mut self,
        path: &narrowing::ReferencePath,
        path_kind: &crate::TypedExprKind,
        path_span: Span,
    ) -> Result<Option<DiscriminantRoot>, crate::compiler_error::CompilerFailure> {
        use crate::TypedExprKind;
        Ok(Some(match path_kind {
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
                DiscriminantRoot {
                    path: root_path,
                    ty: receiver_ty,
                    span: receiver_span,
                    kind: root_kind,
                    key: DiscriminantKey::Field(name.name.clone()),
                }
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
                DiscriminantRoot {
                    path: root_path,
                    ty: receiver_ty,
                    span: receiver_span,
                    kind: root_kind,
                    key: DiscriminantKey::Position(position),
                }
            }
            TypedExprKind::LocalNarrowRef { .. } => {
                let disc_key = match match path.chain.last().cloned() {
                    Some(value) => value,
                    None => return Ok(None),
                } {
                    narrowing::PathElem::Field(name) => DiscriminantKey::Field(name),
                    narrowing::PathElem::Index(narrowing::LiteralValue::Number(n))
                        if n.0.is_finite() && n.0.fract() == 0.0 && n.0 >= 0.0 =>
                    {
                        DiscriminantKey::Position(n.0 as usize)
                    }
                    _ => return Ok(None),
                };
                let mut root_path = path.clone();
                root_path.chain.pop();
                let Some((root_ty, root_kind)) = self.derive_root_source(&root_path, path_span)
                else {
                    return Ok(None);
                };
                DiscriminantRoot {
                    path: root_path,
                    ty: root_ty,
                    span: path_span,
                    kind: root_kind,
                    key: disc_key,
                }
            }
            _ => return Ok(None),
        }))
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
        let Some(DiscriminantRoot {
            path: root_path,
            ty: root_ty,
            span: root_span,
            kind: root_kind,
            key: disc_key_from_path,
        }) = self.discriminant_root(&path, &path_kind, path_span)?
        else {
            return Ok(None);
        };

        // Peel aliases so `type Shape = A | B` pattern-matches as union.
        let Type::Union(members) = root_ty.peel() else {
            return Ok(None);
        };
        let (matching, remaining) = match &disc_key_from_path {
            DiscriminantKey::Field(key_field) => {
                let literal_ty = narrowing::literal_type(&literal);
                let Some(split) = self.discriminant_split(members, key_field, &literal_ty) else {
                    return Ok(None);
                };
                split
            }
            DiscriminantKey::Position(pos) => {
                let Some((disc_pos, table)) = narrowing::tuple_union_discriminant(members) else {
                    return Ok(None);
                };
                if disc_pos != *pos {
                    return Ok(None);
                }
                let Some(matching_idx) = table.get(&literal).copied() else {
                    return Ok(None);
                };
                let (matching, remaining): (Vec<_>, Vec<_>) = members
                    .iter()
                    .enumerate()
                    .partition(|(i, _)| (*i as u32) == matching_idx.0);
                (
                    matching.into_iter().map(|(_, m)| m.clone()).collect(),
                    remaining.into_iter().map(|(_, m)| m.clone()).collect(),
                )
            }
        };
        let (mut true_env, mut false_env) = self.root_discriminant_envs(
            op,
            root_path,
            root_ty,
            root_span,
            root_kind,
            (matching, remaining),
        )?;
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

    /// The views a discriminant test puts on the tested object: the members
    /// `split` found equal to the literal on one side, the rest on the other.
    fn root_discriminant_envs(
        &mut self,
        op: crate::BinOp,
        root_path: narrowing::ReferencePath,
        root_ty: Type,
        root_span: Span,
        root_kind: crate::TypedExprKind,
        (matching, remaining): (Vec<Type>, Vec<Type>),
    ) -> Result<(narrowing::NarrowEnv, narrowing::NarrowEnv), crate::compiler_error::CompilerFailure>
    {
        use crate::BinOp;
        let matched_variant = Type::union(matching);
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
        Ok((true_env, false_env))
    }

    /// `x?.key === literal` (or `=== null`): a discriminant test that is never equal when `x`
    /// is `null`. Only `x` narrows; `x.key` has no reading where `x` is `null`.
    fn narrow_optional_chain_discriminant(
        &mut self,
        op: crate::BinOp,
        chain: ExprId,
        literal_ty: &Type,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        use crate::{TypedChainPart, TypedExprKind};
        let TypedExprKind::OptionalChain { base, parts } = self
            .typed_ast
            .try_expr(chain)
            .map_err(crate::typechecker::arena_failure)?
            .kind
            .clone()
        else {
            return Ok(None);
        };
        let [
            TypedChainPart::Field {
                name,
                optional: true,
                ..
            },
        ] = parts.as_slice()
        else {
            return Ok(None);
        };
        let base_expr = self
            .typed_ast
            .try_expr(base)
            .map_err(crate::typechecker::arena_failure)?
            .clone();
        let Some(root_path) = self.expr_to_reference_path(&base_expr)? else {
            return Ok(None);
        };
        if self.path_root_is_captured_mutator(&root_path) {
            return Ok(None);
        }
        let root_ty = self.narrowing_source_ty(&base_expr)?;
        let Type::Union(members) = root_ty.peel() else {
            return Ok(None);
        };
        let Some(split) = self.discriminant_split(members, &name.name, literal_ty) else {
            return Ok(None);
        };
        let root_kind = self
            .synthesize_unnarrowed_source(&root_path, base_expr.span)?
            .unwrap_or(base_expr.kind);
        self.root_discriminant_envs(op, root_path, root_ty, base_expr.span, root_kind, split)
            .map(Some)
    }

    /// The members of a union `x.key === literal` keeps, and those
    /// `x.key !== literal` keeps, as TypeScript narrows by a discriminant:
    /// a member stays on the equal side when its `key` can hold the literal,
    /// and leaves the unequal side only when its `key` is that literal alone.
    /// A `null` member is reached through `x?.key`, which is then `null`: equal
    /// to a `null` literal and to nothing else (where TypeScript's `undefined`
    /// is never `=== null`). None unless
    /// some member types `key` with a literal, which makes it a discriminant.
    fn discriminant_split(
        &self,
        members: &[Type],
        key: &str,
        literal_ty: &Type,
    ) -> Option<(Vec<Type>, Vec<Type>)> {
        let field_tys = self.discriminant_field_types(members, key)?;
        Some(self.split_by_key_types(members, field_tys, literal_ty))
    }

    /// The members whose discriminant field or element, typed `key_tys`
    /// member by member, may equal `literal_ty`'s value, and those whose
    /// discriminant may differ.
    fn split_by_key_types(
        &self,
        members: &[Type],
        key_tys: Vec<Option<Type>>,
        literal_ty: &Type,
    ) -> (Vec<Type>, Vec<Type>) {
        let mut equal = Vec::new();
        let mut unequal = Vec::new();
        for (member, field_ty) in members.iter().zip(key_tys) {
            let Some(field_ty) = field_ty else {
                if matches!(literal_ty, Type::Null) {
                    equal.push(member.clone());
                } else {
                    unequal.push(member.clone());
                }
                continue;
            };
            if self.field_may_hold(&field_ty, literal_ty) {
                equal.push(member.clone());
            }
            // An enum member type `E.A` is the literal of its value alone.
            let is_only_literal = field_ty.peel() == literal_ty
                || narrowing::unit_literal_value(&field_ty)
                    .is_some_and(|value| narrowing::unit_literal_value(literal_ty) == Some(value));
            if !is_only_literal {
                unequal.push(member.clone());
            }
        }
        (equal, unequal)
    }

    /// Whether a discriminant field typed `field_ty` can hold `literal_ty`'s
    /// value: an enum holds the literals of its members' values, which is the
    /// type a `case K.A` label has.
    fn field_may_hold(&self, field_ty: &Type, literal_ty: &Type) -> bool {
        narrowing::union_members(field_ty)
            .into_iter()
            .any(|member| {
                super::assignable(literal_ty, member, self.resolver())
                    || super::comparable::enum_admits_literal(
                        member.peel(),
                        literal_ty,
                        self.resolver(),
                    )
                    .unwrap_or(false)
            })
    }

    /// Each member's `key` type, or None for a `null` member, when `key` is a
    /// discriminant: every other member has it, the members type it
    /// differently, some with a unit type, and none with a type parameter
    /// (which TypeScript never treats as a discriminant).
    pub(super) fn discriminant_field_types(
        &self,
        members: &[Type],
        key: &str,
    ) -> Option<Vec<Option<Type>>> {
        let mut has_unit_member = false;
        let mut field_tys = Vec::with_capacity(members.len());
        for member in members {
            if matches!(member.peel(), Type::Null) {
                field_tys.push(None);
                continue;
            }
            let field_ty = self.member_shape(member)?.get(key)?.read_ty();
            if narrowing::has_type_parameter_member(&field_ty) {
                return None;
            }
            has_unit_member |= narrowing::has_unit_member(&field_ty);
            field_tys.push(Some(field_ty));
        }
        let mut present = field_tys.iter().flatten();
        let first = present.next()?;
        let is_uniform = present.all(|field_ty| field_ty == first);
        (has_unit_member && !is_uniform).then_some(field_tys)
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
        let mut excluded = self.known_exclusions(&path);
        excluded.insert(literal.clone());
        let mut matched: Vec<Type> = Vec::new();
        let mut remaining: Vec<Type> = Vec::new();
        for member in members {
            let (equal, unequal) = self.split_by_literal(member, &literal, &excluded);
            matched.extend(equal);
            remaining.extend(unequal);
        }
        if matched.is_empty() {
            // Predicate is statically false — typechecker accepted
            // the comparison anyway. Skip narrowing.
            return Ok(None);
        }
        // A side that keeps every member keeps the type as written, alias
        // included.
        let matched_ty = if matched == members {
            path_ty.clone()
        } else {
            Type::union(matched)
        };
        // Unequal, a path that can only be the literal holds no value.
        let remaining_ty = if remaining.is_empty() {
            narrowing::RULED_OUT
        } else if remaining == members {
            path_ty.clone()
        } else {
            Type::union(remaining)
        };
        let (true_ty, false_ty) = match op {
            BinOp::Eq => (matched_ty, remaining_ty),
            BinOp::NotEq => (remaining_ty, matched_ty),
            _ => {
                return Err(super::inference_failure(
                    "non-equality operator reached predicate narrowing",
                ));
            }
        };

        let refine = |ty: Type| {
            if narrowing::is_ruled_out(&ty) {
                ty
            } else {
                narrowing::with_source_refinement(&path_ty, ty)
            }
        };
        let true_ty = refine(true_ty);
        let false_ty = refine(false_ty);
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
        let (true_excluded, false_excluded) = if op == BinOp::NotEq {
            (excluded, std::collections::BTreeSet::new())
        } else {
            (std::collections::BTreeSet::new(), excluded)
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

    /// [`Self::split_by_literal`] for an enum, which is the union of its
    /// members: equal, the value is the member holding `literal`, and unequal,
    /// one of the members whose value hasn't been ruled out. None unless a
    /// member of `member` holds `literal`.
    fn split_enum_by_literal(
        &self,
        member: &Type,
        literal: &narrowing::LiteralValue,
        excluded: &std::collections::BTreeSet<narrowing::LiteralValue>,
    ) -> Option<(Option<Type>, Option<Type>)> {
        let members = super::comparable::enum_member_types(member.peel(), self.resolver())?;
        let holding = |keep: &dyn Fn(&narrowing::LiteralValue) -> bool| -> Vec<Type> {
            members
                .iter()
                .filter(|m| narrowing::LiteralValue::of_enum_member(m).is_some_and(|v| keep(&v)))
                .cloned()
                .collect()
        };
        let equal = holding(&|value| value == literal);
        if equal.is_empty() {
            return None;
        }
        let unequal = holding(&|value| !excluded.contains(value));
        let unequal = if unequal.is_empty() {
            None
        } else if unequal.len() == members.len() {
            Some(member.clone())
        } else {
            Some(Type::union(unequal))
        };
        Some((Some(Type::union(equal)), unequal))
    }

    /// What `member` leaves when the value equals `literal`, and when it
    /// doesn't, given the literals already ruled out with it (`excluded`).
    fn split_by_literal(
        &self,
        member: &Type,
        literal: &narrowing::LiteralValue,
        excluded: &std::collections::BTreeSet<narrowing::LiteralValue>,
    ) -> (Option<Type>, Option<Type>) {
        let literal_ty = literal_to_type(literal);
        if let Some(value) = narrowing::LiteralValue::of_enum_member(member) {
            return if value == *literal {
                (Some(member.clone()), None)
            } else {
                (None, Some(member.clone()))
            };
        }
        if let Some(split) = self.split_enum_by_literal(member, literal, excluded) {
            return split;
        }
        if let Some(values) = super::comparable::enum_literal_values(member.peel(), self.resolver())
            && values.contains(literal)
        {
            // An enum of one member is that member's type: equal, the value
            // keeps it, and unequal, it leaves once that value is ruled out.
            let all_excluded = values.iter().all(|value| excluded.contains(value));
            return (
                Some(member.clone()),
                (!all_excluded).then(|| member.clone()),
            );
        }
        if member.peel() == &literal_ty {
            return (Some(member.clone()), None);
        }
        if let (Type::Boolean, narrowing::LiteralValue::Boolean(value)) = (member.peel(), literal) {
            // `boolean` is `true | false`: `b === true` leaves `false`.
            return (Some(literal_ty), Some(Type::BooleanLiteral(!value)));
        }
        let equal = if matches!(member.peel(), Type::Unknown)
            || member.peel() == &literal_ty.widen_literal()
        {
            Some(literal_ty)
        } else if !narrowing::has_erased_member(member)
            && super::comparable::comparable(member, &literal_ty, self.resolver())
        {
            // As in TypeScript, a member that can be compared with the literal
            // (a weak object type a string matches) stays.
            Some(member.clone())
        } else {
            None
        };
        (equal, Some(member.clone()))
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
        let exclusions = self.known_exclusions(&path);
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
                excluded_literals: exclusions.clone(),
                binding: self.mint_narrow_binding(value_span)?,
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: false_ty,
                facts: false_facts,
                excluded_literals: exclusions.clone(),
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
        let (mut true_env, mut false_env) = self.predicate_envs_truthiness_of_path(path_expr_id)?;
        // `(z = x)` has the value of `x`, so testing it tests `x` too, unless
        // the write replaced what `x` reads from.
        let Some((target, value)) = self.assignment_target_and_value(path_expr_id)? else {
            return Ok((true_env, false_env));
        };
        let value_expr = self
            .typed_ast
            .try_expr(value)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(value_path) = self.expr_to_reference_path(value_expr)? else {
            return Ok((true_env, false_env));
        };
        if target.is_prefix_of(&value_path) {
            return Ok((true_env, false_env));
        }
        let (value_true, value_false) = self.predicate_envs_truthiness(value)?;
        for (env, value_env) in [(&mut true_env, value_true), (&mut false_env, value_false)] {
            for (path, view) in value_env {
                env.entry(path).or_insert(view);
            }
        }
        Ok((true_env, false_env))
    }

    /// The path an assignment expression writes and the value it writes.
    fn assignment_target_and_value(
        &self,
        expr_id: ExprId,
    ) -> Result<Option<(narrowing::ReferencePath, ExprId)>, crate::compiler_error::CompilerFailure>
    {
        let crate::TypedExprKind::Sequence { stmts, .. } = &self
            .typed_ast
            .try_expr(expr_id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        else {
            return Ok(None);
        };
        let Some(&last) = stmts.last() else {
            return Ok(None);
        };
        let value = match &self
            .typed_ast
            .try_stmt(last)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            crate::TypedStmtKind::AssignLocal { value, .. }
            | crate::TypedStmtKind::AssignGlobal { value, .. } => *value,
            _ => return Ok(None),
        };
        let stmts = stmts.clone();
        let Some(target) = self.sequence_binding_path(&stmts)? else {
            return Ok(None);
        };
        Ok(Some((target, self.held_value(&stmts, value)?)))
    }

    /// The expression a sequence's temporary `value` holds, when `value`
    /// reads one the sequence declares; otherwise `value` itself.
    fn held_value(
        &self,
        stmts: &[crate::StmtId],
        value: ExprId,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        let crate::TypedExprKind::LocalRef { ident, .. } = &self
            .typed_ast
            .try_expr(value)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        else {
            return Ok(value);
        };
        for &stmt in stmts {
            if let crate::TypedStmtKind::Const {
                name, value: held, ..
            } = &self
                .typed_ast
                .try_stmt(stmt)
                .map_err(crate::typechecker::arena_failure)?
                .kind
                && name.name == ident.name
            {
                return Ok(*held);
            }
        }
        Ok(value)
    }

    fn predicate_envs_truthiness_of_path(
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

        let root_envs = self.truthiness_discriminant_envs(&fallback_kind)?;

        let true_ty = narrowing::intersect_with(&from_ty, narrowing::TypeFacts::TRUTHY);
        let false_ty = narrowing::intersect_with(&from_ty, narrowing::TypeFacts::FALSY);
        // An outcome a local's type can't take makes it `never` there (a
        // ruled-out view), when every member of the type is always truthy or
        // always falsy: an object is never falsy, as `null` is never truthy.
        let assigns = matches!(fallback_kind, crate::TypedExprKind::Sequence { .. });
        let empty_is_never =
            self.rules_out_to_never(&path) && narrowing::has_known_truthiness(&from_ty) && !assigns;
        // An assignment tested for truthiness (`c && (x = 10)`) narrows its
        // target to the assigned value where the test holds, even when the
        // test itself rules nothing out: an operand that may not run doesn't
        // keep the narrowing its write installs.
        let refines = |ty: &Type| {
            let possible = empty_is_never || !narrowing::is_ruled_out(ty);
            let narrower = assigns || ty.peel() != from_ty.peel();
            possible && narrower
        };
        let (mut true_env, mut false_env) = root_envs.unwrap_or_default();

        if !refines(&true_ty) && !refines(&false_ty) {
            return Ok((true_env, false_env));
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
                    excluded_literals: self.known_exclusions(&path),
                    binding: self.mint_narrow_binding(span)?,
                    source,
                },
            );
        }
        Ok((true_env, false_env))
    }

    /// `if (x.error)` where `error` is a discriminant (`null` in one member,
    /// an object in another) narrows `x` to the members whose `error` can be
    /// truthy, and `!x.error` to those whose `error` can be falsy.
    fn truthiness_discriminant_envs(
        &mut self,
        path_kind: &crate::TypedExprKind,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        let crate::TypedExprKind::FieldAccess { receiver, name } = path_kind else {
            return Ok(None);
        };
        let receiver_expr = self
            .typed_ast
            .try_expr(*receiver)
            .map_err(crate::typechecker::arena_failure)?
            .clone();
        let Some(root_path) = self.expr_to_reference_path(&receiver_expr)? else {
            return Ok(None);
        };
        let Type::Union(members) = receiver_expr.ty.peel() else {
            return Ok(None);
        };
        let mut is_discriminant = false;
        let mut truthy = Vec::new();
        let mut falsy = Vec::new();
        for member in members {
            let Some(field_ty) = self
                .member_shape(member)
                .and_then(|shape| shape.get(&name.name).map(crate::ObjectField::read_ty))
            else {
                return Ok(None);
            };
            is_discriminant |= narrowing::has_unit_member(&field_ty);
            let can_be = |facts| {
                !matches!(
                    narrowing::intersect_with(&field_ty, facts).peel(),
                    Type::Error | Type::Never
                )
            };
            if can_be(narrowing::TypeFacts::TRUTHY) {
                truthy.push(member.clone());
            }
            if can_be(narrowing::TypeFacts::FALSY) {
                falsy.push(member.clone());
            }
        }
        if !is_discriminant || (truthy.len() == members.len() && falsy.len() == members.len()) {
            return Ok(None);
        }
        let root_kind = self
            .synthesize_unnarrowed_source(&root_path, receiver_expr.span)?
            .unwrap_or_else(|| receiver_expr.kind.clone());
        self.root_discriminant_envs(
            crate::BinOp::Eq,
            root_path,
            receiver_expr.ty.clone(),
            receiver_expr.span,
            root_kind,
            (truthy, falsy),
        )
        .map(Some)
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
                narrowing::PathElem::Index(_) | narrowing::PathElem::Key(..) => return None,
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
        // `never` narrowings get no shadow local in codegen; fall through to
        // the un-narrowed type rather than naming a binding that has none.
        self.innermost_narrowing(path)
            .filter(|view| !narrowing::is_ruled_out(&view.narrowed_ty))
    }

    /// The literals `path` is already known not to hold, which a further
    /// narrowing of it keeps: `s !== S.X` still holds inside `s !== null`.
    pub(super) fn known_exclusions(
        &self,
        path: &narrowing::ReferencePath,
    ) -> std::collections::BTreeSet<narrowing::LiteralValue> {
        self.lookup_narrowed_view(path)
            .map(|view| view.excluded_literals.clone())
            .unwrap_or_default()
    }

    /// A read of `path` under the narrowing that holds there. A guard that
    /// rules out every value (its view [`narrowing::RULED_OUT`]) reads as `never`, as in
    /// TypeScript, where [`Self::rules_out_to_never`] allows: no value
    /// reaches the read, and codegen emits a trap for it.
    ///
    /// Code that never runs by its syntax (after a `return`, a `throw` or an
    /// endless loop) reads declared types, as in TypeScript, whose binder
    /// gives it no flow. Code after an exhaustive `switch` still narrows: there
    /// TypeScript's flow reaches it.
    pub(super) fn narrowed_read(
        &self,
        path: narrowing::ReferencePath,
    ) -> Option<(crate::TypedExprKind, Type)> {
        if self.declared_read.as_ref() == Some(&path) {
            return None;
        }
        if !self.reachable && !self.unreachable_by_exhaustive_switch {
            return None;
        }
        let view = self.innermost_narrowing(&path)?;
        let narrowed_ty = if !narrowing::is_ruled_out(&view.narrowed_ty) {
            view.narrowed_ty.clone()
        } else if self.reads_as_never(&path) {
            Type::Never
        } else {
            return None;
        };
        let binding = view.binding.clone();
        Some((
            crate::TypedExprKind::LocalNarrowRef { binding, path },
            narrowed_ty,
        ))
    }

    /// Whether a guard that ruled out every value of `path` makes it read as
    /// `never`. A type parameter or `unknown` hides values a guard can't see
    /// ruled out, so `typeof x === "object"` on a `T` is not a contradiction.
    fn reads_as_never(&self, path: &narrowing::ReferencePath) -> bool {
        self.rules_out_to_never(path)
            && self.declared_root_ty(path).is_some_and(|declared| {
                !narrowing::has_erased_member(&declared)
                    && !matches!(declared.peel(), Type::Unknown)
            })
    }

    /// The view the innermost frame holding `path` gives it, unless a frame
    /// in between tombstones it.
    pub(super) fn innermost_narrowing(
        &self,
        path: &narrowing::ReferencePath,
    ) -> Option<&narrowing::NarrowedView> {
        for (frame_idx, frame) in self.narrow_scopes.iter().enumerate().rev() {
            if let Some(view) = frame.get(path) {
                return Some(view);
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

/// Whether a key of type `ty` reads a property, as a string does, rather
/// than an element.
fn is_property_key(ty: &Type) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().all(is_property_key),
        Type::String | Type::StringLiteral(_) | Type::StringEnum { .. } => true,
        _ => false,
    }
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
    if let Some(value) = enum_member_literal(&expr.kind) {
        return Ok(Some(value));
    }
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

/// The value an enum member reference holds, which is what comparing with
/// it tests, as a `case` label with it does.
fn enum_member_literal(kind: &crate::TypedExprKind) -> Option<narrowing::LiteralValue> {
    match kind {
        crate::TypedExprKind::NumberEnumMember { value, .. } => Some(
            narrowing::LiteralValue::Number(crate::types::LiteralF64(*value)),
        ),
        crate::TypedExprKind::StringEnumMember { value, .. } => {
            Some(narrowing::LiteralValue::String(value.clone()))
        }
        _ => None,
    }
}

/// Whether the operand is a literal written in the source, not a reference
/// whose type is a literal.
fn is_written_literal(
    ast: &crate::TypedAst,
    id: ExprId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let expr = ast
        .try_expr(id)
        .map_err(crate::typechecker::arena_failure)?;
    Ok(literal_value_of(&expr.kind).is_some())
}

/// The literals an operand's type allows, when it is a union of two or more
/// literals and nothing else.
fn comparison_literal_union(
    ast: &crate::TypedAst,
    id: ExprId,
) -> Result<Option<Vec<narrowing::LiteralValue>>, crate::compiler_error::CompilerFailure> {
    let expr = ast
        .try_expr(id)
        .map_err(crate::typechecker::arena_failure)?;
    let Type::Union(members) = expr.ty.peel() else {
        return Ok(None);
    };
    Ok(members.iter().map(narrowing::unit_literal_value).collect())
}

/// Joins the two ways a short-circuit condition can reach one outcome. A way
/// that narrows some path to nothing cannot happen, so the other way alone
/// decides: in `typeof x === "string" || typeof x === "string"` the right
/// side is never true, and the true branch keeps `x: string`.
fn join_reachable_envs(
    left: narrowing::NarrowEnv,
    right: narrowing::NarrowEnv,
) -> narrowing::NarrowEnv {
    if is_unreachable_env(&right) {
        return left;
    }
    if is_unreachable_env(&left) {
        return right;
    }
    narrowing::union_envs(left, Default::default(), right, Default::default()).0
}

fn is_unreachable_env(env: &narrowing::NarrowEnv) -> bool {
    env.values().any(|view| {
        let ty = view.narrowed_ty.peel();
        matches!(ty, Type::Never) || narrowing::is_ruled_out(ty)
    })
}

/// Whether a property both shapes require holds unit values in each and
/// none in common, as `kind: "a"` and `kind: "b"` do.
fn have_disjoint_unit_property(
    left: &std::collections::BTreeMap<String, crate::types::ObjectField>,
    right: &std::collections::BTreeMap<String, crate::types::ObjectField>,
) -> bool {
    left.iter().any(|(key, left_field)| {
        let Some(right_field) = right.get(key) else {
            return false;
        };
        if left_field.optional || right_field.optional {
            return false;
        }
        let (Some(left_values), Some(right_values)) = (
            unit_values(&left_field.read_ty()),
            unit_values(&right_field.read_ty()),
        ) else {
            return false;
        };
        left_values.is_disjoint(&right_values)
    })
}

/// The values a type made only of literals and `null` holds, or `None` if it
/// has another member. In the set, `None` stands for `null`.
fn unit_values(ty: &Type) -> Option<std::collections::BTreeSet<Option<narrowing::LiteralValue>>> {
    narrowing::union_members(ty)
        .into_iter()
        .map(|member| match member.peel() {
            Type::Null => Some(None),
            Type::Boolean => None,
            other => narrowing::unit_literal_value(other).map(Some),
        })
        .collect()
}

/// Whether one object can have both types though neither is assignable to the
/// other: structural types overlap (`{ a: number }` and `{ b: number }` both
/// hold `{ a: 1, b: 2 }`). Two classes can't, since an instance has one class
/// and assignability already covers a subclass.
fn may_share_an_object(left: &Type, right: &Type) -> bool {
    let is_non_primitive = |ty: &Type| {
        !matches!(
            ty.peel(),
            Type::Null
                | Type::String
                | Type::StringLiteral(_)
                | Type::Number
                | Type::NumberLiteral(_)
                | Type::Boolean
                | Type::BooleanLiteral(_)
                | Type::BigInt
                | Type::BigIntLiteral(_)
        )
    };
    let is_class = |ty: &Type| matches!(ty.peel(), Type::ClassRef { .. });
    is_non_primitive(left) && is_non_primitive(right) && !(is_class(left) && is_class(right))
}

/// Adds the views of `extra` on paths `env` doesn't narrow.
fn add_missing_views(env: &mut narrowing::NarrowEnv, extra: narrowing::NarrowEnv) {
    for (path, view) in extra.into_iter() {
        if !env.contains_key(&path) {
            env.insert(path, view);
        }
    }
}

/// Each tuple member's element type at `position`, when that position is a
/// discriminant: the members type it differently, some with a unit type such
/// as `null`, as [`Inferer::discriminant_field_types`] requires of a field.
fn discriminant_element_types(members: &[Type], position: usize) -> Option<Vec<Option<Type>>> {
    let element_tys: Vec<Type> = members
        .iter()
        .map(|member| match member.peel() {
            Type::Tuple(elements) => elements.get(position).cloned(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if element_tys.iter().any(narrowing::has_type_parameter_member) {
        return None;
    }
    let is_uniform = element_tys.iter().all(|ty| Some(ty) == element_tys.first());
    (element_tys.iter().any(narrowing::has_unit_member) && !is_uniform)
        .then(|| element_tys.into_iter().map(Some).collect())
}

/// What a discriminant read tests the object it reads from by.
enum DiscriminantKey {
    Field(String),
    Position(usize),
}

/// The object a discriminant read tests: its path, declared type, span, a
/// source that reads it, and the key the read takes.
struct DiscriminantRoot {
    path: narrowing::ReferencePath,
    ty: Type,
    span: Span,
    kind: crate::TypedExprKind,
    key: DiscriminantKey,
}
