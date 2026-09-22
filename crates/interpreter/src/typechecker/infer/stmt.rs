use crate::{
    BinOp, BindingKind, Diagnostic, ExprId, ExprKind, Ident, Severity, Span, StmtId, StmtKind,
    Type, TypedExpr, TypedExprKind, TypedStmt, TypedStmtKind, ValueKind,
};

use super::classes::{FieldRw, StaticResolution};
use super::{Inferer, assignable, narrowing};

/// Outcome of peeking at `ClassName.member` on the left of a write.
pub(super) enum StaticWrite {
    /// The receiver is not a bare class name; fall through to instance-field inference.
    NotClassName,
    /// Diagnostic already reported; the caller poisons its own statement or expression.
    Rejected { receiver: Ident },
    Resolved {
        /// The *declaring* class's global — `B.x` writes `A`'s slot when `A` declares `x`.
        mangled: crate::MangledName,
        ty: Type,
    },
}

impl Inferer<'_> {
    /// Returns `None` for type-space-only declarations (`interface`, `type`, `import`);
    /// callers `filter_map` those away.
    pub(super) fn infer_stmt(&mut self, stmt_id: StmtId) -> Option<StmtId> {
        let stmt = self.ast.stmt(stmt_id).clone();
        let span = stmt.span;
        let typed_kind = match stmt.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                doc,
            } => {
                let hint = ty.as_ref().map(|a| self.resolve_type(a));
                let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref());
                let bound = hint.unwrap_or(value_ty);
                // Reject a void binding; poison the slot so codegen never
                // sees a void value-type.
                let bound = if self.reject_void_binding(&bound, span) {
                    Type::Error
                } else {
                    bound
                };
                self.scopes
                    .insert(name.name.clone(), bound.clone(), false, name.span);
                TypedStmtKind::Let {
                    name,
                    ty: bound,
                    value: typed_value,
                    boxed: false,
                    doc,
                }
            }
            StmtKind::Const {
                name,
                ty,
                value,
                doc,
            } => {
                let hint = ty.as_ref().map(|a| self.resolve_type(a));
                let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref());
                let bound = hint.unwrap_or(value_ty);
                // Reject a void binding; poison the slot so codegen never
                // sees a void value-type.
                let bound = if self.reject_void_binding(&bound, span) {
                    Type::Error
                } else {
                    bound
                };
                self.scopes
                    .insert(name.name.clone(), bound.clone(), true, name.span);
                TypedStmtKind::Const {
                    name,
                    ty: bound,
                    value: typed_value,
                    doc,
                }
            }
            StmtKind::Function { .. } => {
                self.error(
                    span,
                    "nested function declarations are not supported".to_string(),
                );
                return None;
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let (typed_cond, cond_ty) = self.infer_expr(condition, None);
                let cond_span = self.ast.expr(condition).span;
                self.check_condition_ty(&cond_ty, cond_span);
                let (true_env, false_env) = self.predicate_envs(typed_cond);
                let entry_reachable = self.reachable;
                let then_span = self.ast.stmt(then_block).span;
                self.push_narrow_frame(true_env.clone());
                self.reachable = entry_reachable;
                let typed_then = self
                    .infer_stmt(then_block)
                    .expect("if-branch is a Block, never a type-only decl");
                let then_reachable = self.reachable;
                let (then_narrowings, then_assigned) = self.pop_narrow_frame_capture();
                let typed_then = self.wrap_narrow_regions(typed_then, &true_env, then_span);
                let (typed_else, else_narrowings, else_assigned, else_reachable) = match else_block
                {
                    Some(b) => {
                        let else_span = self.ast.stmt(b).span;
                        self.push_narrow_frame(false_env.clone());
                        self.reachable = entry_reachable;
                        let typed_else = self
                            .infer_stmt(b)
                            .expect("else-branch is a Block, never a type-only decl");
                        let er = self.reachable;
                        let (en, ea) = self.pop_narrow_frame_capture();
                        let typed_else =
                            self.wrap_narrow_regions(typed_else, &false_env, else_span);
                        (Some(typed_else), en, ea, er)
                    }
                    None => {
                        // Implicit-else carries the false-side narrowings so
                        // `if (x === null) return;` propagates the non-null
                        // narrowing past the `if` when the then-branch is unreachable.
                        (
                            None,
                            false_env.clone(),
                            std::collections::BTreeSet::new(),
                            entry_reachable,
                        )
                    }
                };
                let (joined_narrowings, joined_assigned) = match (then_reachable, else_reachable) {
                    (true, true) => crate::typechecker::infer::narrowing::union_envs(
                        then_narrowings,
                        then_assigned,
                        else_narrowings,
                        else_assigned,
                    ),
                    (true, false) => (then_narrowings, then_assigned),
                    (false, true) => (else_narrowings, else_assigned),
                    (false, false) => (
                        crate::typechecker::infer::narrowing::NarrowEnv::new(),
                        std::collections::BTreeSet::new(),
                    ),
                };
                self.reachable = then_reachable || else_reachable;
                self.merge_assigned_into_outer(joined_assigned, span);
                self.install_joined_narrowings(joined_narrowings, span);
                TypedStmtKind::If {
                    condition: typed_cond,
                    then_block: typed_then,
                    else_block: typed_else,
                }
            }
            StmtKind::While { condition, body } => {
                let (typed_cond, cond_ty) = self.infer_expr(condition, None);
                let cond_span = self.ast.expr(condition).span;
                self.check_condition_ty(&cond_ty, cond_span);
                let (true_env, _) = self.predicate_envs(typed_cond);
                let body_span = self.ast.stmt(body).span;
                let body_scope_floor = self.scopes.next_scope_id();
                let (loop_entry, _) = self.snapshot_active_narrowings(0);
                let entry_reachable = self.reachable;
                self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
                let outcome = self.run_loop_body_with_fixed_point(
                    body,
                    true_env,
                    body_span,
                    body_scope_floor,
                    None,
                );
                let frame = self.pop_pending_join_frame();
                let (typed_cond, _) = self.infer_expr(condition, None);
                let (_, false_env) = self.predicate_envs(typed_cond);
                self.merge_assigned_into_outer(outcome.assigned.clone(), span);
                let natural = if cond_is_static_true(self, typed_cond) {
                    None
                } else {
                    Some(loop_exit_env(&loop_entry, &outcome, false_env))
                };
                let has_exit = self.fold_exits_into_outer(natural, frame.breaks, body_span);
                self.reachable = entry_reachable && has_exit;
                TypedStmtKind::While {
                    condition: typed_cond,
                    body: outcome.body,
                }
            }
            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                self.scopes.push();
                let typed_init = init.and_then(|id| self.infer_stmt(id));
                let typed_cond = condition.map(|c| {
                    let (id, ty) = self.infer_expr(c, None);
                    let cond_span = self.ast.expr(c).span;
                    self.check_condition_ty(&ty, cond_span);
                    id
                });
                let (true_env, _) = match typed_cond {
                    Some(c) => self.predicate_envs(c),
                    None => (narrowing::NarrowEnv::new(), narrowing::NarrowEnv::new()),
                };
                let body_span = self.ast.stmt(body).span;
                // Init-scope bindings persist across iterations, so the floor
                // sits above them — only body-internal scopes are per-iteration.
                let body_scope_floor = self.scopes.next_scope_id();
                let (loop_entry, _) = self.snapshot_active_narrowings(0);
                let entry_reachable = self.reachable;
                self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
                let outcome = self.run_loop_body_with_fixed_point(
                    body,
                    true_env,
                    body_span,
                    body_scope_floor,
                    update,
                );
                let frame = self.pop_pending_join_frame();
                let typed_update = outcome.update;
                let typed_cond = condition.map(|c| self.infer_expr(c, None).0);
                let false_env = typed_cond
                    .map(|c| self.predicate_envs(c).1)
                    .unwrap_or_default();
                self.merge_assigned_into_outer(outcome.assigned.clone(), span);
                // `for (;;)` or literal-true condition has no natural exit.
                let natural = match typed_cond {
                    Some(c) if !cond_is_static_true(self, c) => {
                        Some(loop_exit_env(&loop_entry, &outcome, false_env))
                    }
                    _ => None,
                };
                let has_exit = self.fold_exits_into_outer(natural, frame.breaks, body_span);
                self.reachable = entry_reachable && has_exit;
                self.scopes.pop();
                // The init scope is popped after the fold, so anything the fold
                // installed that is rooted in it is now unreadable.
                self.drop_out_of_scope_narrowings();
                TypedStmtKind::For {
                    init: typed_init,
                    condition: typed_cond,
                    update: typed_update,
                    body: outcome.body,
                }
            }
            StmtKind::ForOf {
                binding_kind,
                name,
                ty: ann,
                iter,
                body,
            } => {
                let (typed_iter, iter_ty) = self.infer_expr(iter, None);
                let classified = self.classify_for_of_source(&iter_ty);
                let (element_ty, for_of_kind) = if let Some(pair) = classified {
                    pair
                } else {
                    if !matches!(iter_ty.peel(), Type::Error) {
                        // `classify_for_of_source` needs `&mut self`, so the
                        // non-null form is classified up front and the probe
                        // reads the answer. Same question `nullable_culprit`
                        // would ask, since it applies `accepts` to exactly this
                        // form.
                        let non_null_iterates =
                            super::narrow_scopes::non_null_form(iter_ty.clone())
                                .is_some_and(|t| self.classify_for_of_source(&t).is_some());
                        let culprit =
                            self.nullable_culprit(&[(typed_iter, &iter_ty)], |_| non_null_iterates);
                        self.error_with_narrowing_hint(
                            self.ast.expr(iter).span,
                            format!(
                                "`for-of` requires an array, tuple, string, `Iterator<T>`, or `Iterable<T>`, got `{}`",
                                iter_ty.peel(),
                            ),
                            Vec::new(),
                            culprit,
                        );
                    }
                    (Type::Error, crate::ForOfKind::Array)
                };
                let bound_ty = if let Some(a) = ann.as_ref() {
                    let declared = self.resolve_type(a);
                    if !matches!(element_ty, Type::Error)
                        && !assignable(&element_ty, &declared, self.resolver())
                    {
                        self.error(
                            a.span,
                            format!(
                                "loop variable type `{declared}` is not compatible with array element type `{element_ty}`",
                            ),
                        );
                    }
                    declared
                } else {
                    element_ty.clone()
                };
                // Floor taken before the loop-var scope: the loop variable is
                // rebound each iteration, so back-edge narrowings on it don't
                // carry to the next iteration's entry.
                let body_scope_floor = self.scopes.next_scope_id();
                self.scopes.push();
                self.scopes.insert(
                    name.name.clone(),
                    bound_ty.clone(),
                    matches!(binding_kind, BindingKind::Const),
                    name.span,
                );
                let body_span = self.ast.stmt(body).span;
                let (loop_entry, _) = self.snapshot_active_narrowings(0);
                let entry_reachable = self.reachable;
                self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
                let outcome = self.run_loop_body_with_fixed_point(
                    body,
                    narrowing::NarrowEnv::new(),
                    body_span,
                    body_scope_floor,
                    None,
                );
                let frame = self.pop_pending_join_frame();
                self.merge_assigned_into_outer(outcome.assigned.clone(), span);
                // Natural exit always reachable — for-of terminates immediately on empty iterable.
                let has_exit = self.fold_exits_into_outer(
                    Some(loop_exit_env(
                        &loop_entry,
                        &outcome,
                        narrowing::NarrowEnv::new(),
                    )),
                    frame.breaks,
                    body_span,
                );
                self.reachable = entry_reachable && has_exit;
                self.scopes.pop();
                TypedStmtKind::ForOf {
                    binding_kind,
                    name,
                    element_ty: bound_ty,
                    iter: typed_iter,
                    body: outcome.body,
                    kind: for_of_kind,
                }
            }
            StmtKind::DoWhile { body, condition } => {
                // Condition runs after the body, so no entry narrowing for the body.
                let body_span = self.ast.stmt(body).span;
                let body_scope_floor = self.scopes.next_scope_id();
                let entry_reachable = self.reachable;
                self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
                let outcome = self.run_loop_body_with_fixed_point(
                    body,
                    narrowing::NarrowEnv::new(),
                    body_span,
                    body_scope_floor,
                    None,
                );
                let frame = self.pop_pending_join_frame();
                self.merge_assigned_into_outer(outcome.assigned.clone(), span);
                let tail_env = self.loop_tail_env(&outcome.back_edge);
                self.push_narrow_frame(tail_env.clone());
                let (typed_cond, cond_ty) = self.infer_expr(condition, None);
                let cond_span = self.ast.expr(condition).span;
                self.check_condition_ty(&cond_ty, cond_span);
                let (_, cond_false_env) = self.predicate_envs(typed_cond);
                let mut post_condition = self.snapshot_active_narrowings(0).0;
                let (_, condition_assigned) = self.pop_narrow_frame_capture();
                self.merge_assigned_into_outer(condition_assigned, cond_span);
                let condition_always_true = cond_is_static_true(self, typed_cond);
                let typed_cond = self.wrap_narrow_exprs(typed_cond, &tail_env, cond_span);
                let natural = if !outcome.reaches_back_edge || condition_always_true {
                    None
                } else {
                    post_condition.extend(cond_false_env);
                    Some(post_condition)
                };
                let has_exit = self.fold_exits_into_outer(natural, frame.breaks, body_span);
                self.reachable = entry_reachable && has_exit;
                TypedStmtKind::DoWhile {
                    body: outcome.body,
                    condition: typed_cond,
                }
            }
            StmtKind::Switch {
                discriminant,
                cases,
                default,
            } => self.infer_switch(discriminant, cases, default, span),
            StmtKind::Break => {
                if self.loop_depth == 0 && self.switch_depth == 0 {
                    self.error(span, "`break` outside of a loop or `switch`".to_string());
                } else if self.reachable
                    && let Some(target_idx) = self.pending_joins.iter().rposition(|_| true)
                {
                    let base = self.pending_joins[target_idx].narrow_depth;
                    let (env, _) = self.snapshot_active_narrowings(0);
                    let (_, assigned) = self.snapshot_active_narrowings(base);
                    let snap = (env, assigned);
                    self.pending_joins[target_idx].breaks.push(snap);
                }
                self.reachable = false;
                TypedStmtKind::Break
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.error(span, "`continue` outside of a loop".to_string());
                } else {
                    // `continue` is transparent to switch — skip to the innermost Loop frame.
                    if self.reachable
                        && let Some(target_idx) = self
                            .pending_joins
                            .iter()
                            .rposition(|f| matches!(f.kind, narrowing::PendingJoinKind::Loop))
                    {
                        let base = self.pending_joins[target_idx].narrow_depth;
                        let (env, _) = self.snapshot_active_narrowings(0);
                        let (_, assigned) = self.snapshot_active_narrowings(base);
                        let snap = (env, assigned);
                        self.pending_joins[target_idx].continues.push(snap);
                    }
                }
                self.reachable = false;
                TypedStmtKind::Continue
            }
            StmtKind::Return(value) => {
                let typed_value = if let Some(v) = value {
                    let hint = self.current_return.clone();
                    let (id, value_ty) = self.infer_expr(v, hint.as_ref());
                    if let Some(collected) = self.inferred_returns.as_mut() {
                        collected.push((value_ty, span));
                    }
                    if self.reachable {
                        self.validate_type_predicate_return(id, span);
                    }
                    Some(id)
                } else {
                    if let Some(ret) = &self.current_return
                        && !matches!(ret.peel(), Type::Void | Type::Error)
                    {
                        self.error(span, format!("expected `return` value of type `{ret}`"));
                    }
                    None
                };
                self.reachable = false;
                TypedStmtKind::Return(typed_value)
            }
            StmtKind::Expr(expr_id) => {
                let (typed_id, _) = self.infer_expr(expr_id, None);
                TypedStmtKind::Expr(typed_id)
            }
            StmtKind::Block(stmts) => {
                self.scopes.push();
                // Propagate `assigned` so enclosing blocks invalidate narrowings on reassigned paths.
                self.push_narrow_frame(crate::typechecker::infer::narrowing::NarrowEnv::new());
                let typed_stmts = self.block_stmts_with_drain(&stmts, span);
                let (inner_narrowings, inner_assigned) = self.pop_narrow_frame_capture();
                let exits_normally = self.reachable;
                let surviving = if exits_normally {
                    inner_narrowings
                } else {
                    narrowing::NarrowEnv::new()
                };
                self.scopes.pop();
                self.merge_assigned_into_outer(inner_assigned, span);
                // Both assignments and guards describe the block's normal exit.
                // Installation filters out bindings whose lexical scope just ended.
                if !surviving.is_empty() {
                    self.install_joined_narrowings(surviving, span);
                }
                TypedStmtKind::Block(typed_stmts)
            }
            StmtKind::Assign { target, value } => self.infer_assign(target, value, span),
            StmtKind::AssignField {
                receiver,
                field_name,
                value,
            } => self.infer_assign_field(receiver, field_name, value),
            StmtKind::AssignIndex {
                receiver,
                index,
                value,
            } => self.infer_assign_index(receiver, index, value, span),
            StmtKind::CompoundAssign {
                target,
                op,
                op_span,
                value,
            } => self.infer_compound_assign(target, op, op_span, value, span),
            StmtKind::CompoundAssignField {
                receiver,
                field_name,
                op,
                op_span,
                value,
            } => self.infer_compound_assign_field(receiver, field_name, op, op_span, value),
            StmtKind::CompoundAssignIndex {
                receiver,
                index,
                op,
                op_span,
                value,
            } => self.infer_compound_assign_index(receiver, index, op, op_span, value, span),
            StmtKind::InterfaceDecl { .. }
            | StmtKind::ClassDecl { .. }
            | StmtKind::EnumDecl { .. }
            | StmtKind::TypeAliasDecl { .. } => {
                // Classes are bound in `signatures()` and checked in `infer_classes`.
                return None;
            }
            StmtKind::Import { .. } => {
                // Parser rejects nested imports; this arm is dead code.
                return None;
            }
            StmtKind::ExportFrom { .. } => {
                // Re-exports never appear in a statement body (parser rejects
                // nested `export`); `collect_exports` handles top-level ones.
                return None;
            }
            StmtKind::Throw { value } => {
                let error_ty = Type::prelude_error_class();
                let value_span = self.ast.expr(value).span;
                let (typed_value, value_ty) = self.infer_expr(value, Some(&error_ty));
                if !matches!(value_ty, Type::Error)
                    && !assignable(&value_ty, &error_ty, self.resolver())
                {
                    self.error_with_help(
                        value_span,
                        format!("expected `Error`, got `{value_ty}`"),
                        vec![
                            "throw `new Error(\"...\")` or an instance of a class that `extends Error`"
                                .to_string(),
                        ],
                    );
                }
                self.reachable = false;
                TypedStmtKind::Throw { value: typed_value }
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => self.infer_try(body, catches, finally, span),
            StmtKind::LetPattern { .. }
            | StmtKind::ConstPattern { .. }
            | StmtKind::ForOfPattern { .. } => {
                unreachable!(
                    "destructuring patterns must be lowered before infer (see lower_patterns)"
                );
            }
            StmtKind::ConstRest {
                name,
                source,
                exclude,
                ty,
                doc,
            } => {
                let hint = ty.as_ref().map(|a| self.resolve_type(a));
                let (typed_value, source_ty) = self.infer_expr(source, hint.as_ref());
                let narrowed = match source_ty.clone() {
                    Type::Object { mut fields } => {
                        for excl in &exclude {
                            fields.remove(&excl.name);
                        }
                        Type::Object { fields }
                    }
                    other => {
                        self.error(
                            span,
                            format!(
                                "object rest can only destructure an object with a known shape; \
                                 source has type `{other}`",
                            ),
                        );
                        other
                    }
                };
                self.scopes
                    .insert(name.name.clone(), narrowed.clone(), true, name.span);
                TypedStmtKind::Const {
                    name,
                    ty: narrowed,
                    value: typed_value,
                    doc,
                }
            }
        };
        Some(self.typed_ast.push_stmt(TypedStmt {
            kind: typed_kind,
            span,
        }))
    }

    fn infer_try(
        &mut self,
        body: StmtId,
        catches: Vec<crate::CatchClause>,
        finally: Option<StmtId>,
        span: Span,
    ) -> TypedStmtKind {
        let entry_reachable = self.reachable;
        let pending_start = self.pending_exit_counts();
        let body_outcome = self.infer_isolated_clause(body, entry_reachable, &Default::default());
        let typed_body = body_outcome.body.expect("try body is a block");
        let mut exits = body_outcome.exit.into_iter().collect::<Vec<_>>();
        let body_assigned = body_outcome.all_writes;
        let mut all_assigned = body_assigned.clone();

        let mut typed_catches = Vec::with_capacity(catches.len());
        // Valid prior arms for the shadow check: (class, display name, anchor span).
        // Arms with an erroneous annotation stay out on both sides to
        // avoid cascading unreachable-arm diagnostics.
        let mut prior: Vec<(crate::MangledName, String, Span)> = Vec::new();
        for clause in catches {
            let clause_ty = self.infer_catch_type(&clause, &mut prior);
            self.scopes.push();
            self.scopes.insert(
                clause.binding.name.clone(),
                clause_ty.clone(),
                true,
                clause.binding.span,
            );
            let outcome = self.infer_isolated_clause(clause.body, entry_reachable, &body_assigned);
            exits.extend(outcome.exit);
            all_assigned.extend(outcome.all_writes);
            self.scopes.pop();
            if let Some(body) = outcome.body {
                typed_catches.push(crate::TypedCatchClause {
                    binding: clause.binding.clone(),
                    ty: clause_ty,
                    body,
                    boxed: false,
                    span: clause.span,
                });
            }
        }

        let mut post = join_reachable_envs(None, exits);
        let typed_finally = finally.and_then(|f| {
            let pending_end = self.pending_exit_counts();
            let outcome = self.infer_isolated_clause(f, entry_reachable, &all_assigned);
            self.apply_finally_to_pending(&pending_start, &pending_end, &outcome);
            apply_finally_to_exit(&mut post, &outcome);
            all_assigned.extend(outcome.all_writes);
            outcome.body
        });
        self.reachable = entry_reachable && post.is_some();
        self.merge_assigned_into_outer(all_assigned, span);
        if let Some(post) = post {
            self.install_joined_narrowings(post, span);
        }

        TypedStmtKind::Try {
            body: typed_body,
            catches: typed_catches,
            finally: typed_finally,
        }
    }

    fn infer_catch_type(
        &mut self,
        clause: &crate::CatchClause,
        prior: &mut Vec<(crate::MangledName, String, Span)>,
    ) -> Type {
        let error_class = Type::prelude_error_class();
        // `Error` or any class on its `extends` chain. A subclass
        // annotation makes the clause a filter: codegen tests the
        // caught error's nominal identity (vtable brand chain) and
        // tries the next arm — or re-raises — on mismatch.
        let mut annotation_valid = true;
        let clause_ty = if let Some(annotation) = &clause.ty {
            // The filter is the same nominal brand walk `instanceof`
            // uses, so a generic error class is spelled bare and
            // binds at erased args; explicit ones would claim a
            // check the runtime never makes.
            let ty = self.resolve_runtime_class_test(annotation);
            let is_error_class = matches!(ty.peel(), Type::ClassRef { .. })
                && assignable(&ty, &error_class, self.resolver());
            if is_error_class {
                ty
            } else if matches!(ty, Type::Error) {
                annotation_valid = false;
                ty
            } else {
                self.error_with_help(
                    annotation.span,
                    format!(
                        "a `catch` binding must be `Error` or a class extending `Error`; got `{ty}`"
                    ),
                    vec![
                        "remove the annotation (or use `: Error`) to catch every thrown error; a subclass annotation catches only that error type and re-raises the rest"
                            .to_string(),
                    ],
                );
                annotation_valid = false;
                error_class.clone()
            }
        } else {
            error_class.clone()
        };

        let anchor = clause.ty.as_ref().map_or(clause.binding.span, |a| a.span);
        if annotation_valid && let Type::ClassRef { mangled, name, .. } = clause_ty.peel() {
            let shadowing = prior
                .iter()
                .find(|(p, _, _)| self.resolver().is_subclass(mangled, p));
            if let Some((prev_mangled, prev_name, prev_span)) = shadowing {
                if prev_mangled == mangled {
                    self.error_with_help_and_notes(
                        anchor,
                        format!("duplicate `catch` clause for `{name}`"),
                        vec![],
                        vec![(*prev_span, "previously caught here".to_string())],
                    );
                } else {
                    self.error_with_help_and_notes(
                        anchor,
                        format!(
                            "unreachable `catch` clause: `{name}` extends `{prev_name}`, which an earlier clause already catches"
                        ),
                        vec![
                            "reorder the clauses so the more specific error class comes first"
                                .to_string(),
                        ],
                        vec![(*prev_span, format!("`{prev_name}` caught here"))],
                    );
                }
            } else {
                prior.push((mangled.clone(), name.clone(), anchor));
            }
        }

        clause_ty
    }

    fn pending_exit_counts(&self) -> Vec<(usize, usize)> {
        self.pending_joins
            .iter()
            .map(|frame| (frame.breaks.len(), frame.continues.len()))
            .collect()
    }

    fn apply_finally_to_pending(
        &mut self,
        starts: &[(usize, usize)],
        ends: &[(usize, usize)],
        outcome: &ClauseOutcome,
    ) {
        for ((frame, start), end) in self.pending_joins.iter_mut().zip(starts).zip(ends) {
            apply_finally_to_transfers(&mut frame.breaks, start.0..end.0, outcome);
            apply_finally_to_transfers(&mut frame.continues, start.1..end.1, outcome);
        }
    }

    /// Drain `pending_post_if_materializations` after each stmt, wrapping the tail in
    /// `NarrowRegion`s when non-empty. Does NOT push/pop scopes — the `Block` arm owns those.
    fn block_stmts_with_drain(&mut self, stmts: &[StmtId], outer_span: Span) -> Vec<StmtId> {
        let mut typed_stmts: Vec<StmtId> = Vec::new();
        let mut i = 0;
        while i < stmts.len() {
            if let Some(t) = self.infer_stmt(stmts[i]) {
                typed_stmts.push(t);
            }
            let pending = std::mem::take(&mut self.pending_post_if_materializations);
            if !pending.is_empty() {
                let tail = self.block_stmts_with_drain(&stmts[i + 1..], outer_span);
                let tail_block = self.typed_ast.push_stmt(TypedStmt {
                    kind: TypedStmtKind::Block(tail),
                    span: outer_span,
                });
                let wrapped = self.wrap_pending_materializations(tail_block, pending);
                typed_stmts.push(wrapped);
                break;
            }
            i += 1;
        }
        typed_stmts
    }

    /// `ClassName.member = …` / `+= …` / `++` — resolved before the receiver is
    /// typed, because a bare class name is not a value and `infer_expr` would
    /// report that instead of the real problem. Every write form shares this one
    /// resolver so their diagnostics stay identical.
    pub(super) fn resolve_static_field_write(
        &mut self,
        receiver: ExprId,
        name: &Ident,
    ) -> StaticWrite {
        let ExprKind::Identifier(recv_ident) = &self.ast.expr(receiver).kind.clone() else {
            return StaticWrite::NotClassName;
        };
        if self.scopes.get(&recv_ident.name).is_some()
            || self.top_symbols.contains_key(&recv_ident.name)
        {
            return StaticWrite::NotClassName;
        }
        let Some((class_name, class_mangled)) =
            self.lookup_named_type(&recv_ident.name).and_then(|sym| {
                matches!(sym.kind, crate::TypeKind::Class { .. })
                    .then(|| (recv_ident.name.clone(), sym.mangled_name.clone()))
            })
        else {
            return StaticWrite::NotClassName;
        };
        let receiver = recv_ident.clone();
        match self.class_static_in_chain(&class_mangled, &name.name) {
            Some((StaticResolution::Field(field), owner)) => {
                // Privacy first: a caller that cannot see the member should hear
                // that, not a readonly complaint about a member it can't name.
                self.check_static_privacy(field.visibility, &owner, &class_name, name);
                if field.readonly {
                    self.error_with_help(
                        name.span,
                        format!(
                            "cannot assign to static readonly field `{class_name}.{}`",
                            name.name,
                        ),
                        vec![format!(
                            "drop `readonly` from the declaration to make `{}` writable",
                            name.name
                        )],
                    );
                    return StaticWrite::Rejected { receiver };
                }
                StaticWrite::Resolved {
                    mangled: crate::mangle::static_member(&owner, &name.name),
                    ty: field.ty,
                }
            }
            Some((StaticResolution::Method(_, vis), owner)) => {
                self.check_static_privacy(vis, &owner, &class_name, name);
                self.error_with_help(
                    name.span,
                    format!(
                        "cannot assign to static method `{class_name}.{}`",
                        name.name
                    ),
                    vec![format!(
                        "static methods cannot be reassigned — call it: `{class_name}.{}(…)`",
                        name.name
                    )],
                );
                StaticWrite::Rejected { receiver }
            }
            None => {
                self.report_missing_static(&class_name, &class_mangled, name);
                StaticWrite::Rejected { receiver }
            }
        }
    }

    /// Read-modify-write against a module global — shared by module `let` and by
    /// a writable static field, which is the same global under a mangled key.
    /// The read half consults the narrowed view, so inside a guard it reads the
    /// shadow; the write still lands on the global. The narrowing is therefore
    /// dropped afterwards — leaving it live would make every later read in the
    /// guard return the pre-write shadow value.
    #[allow(clippy::too_many_arguments)]
    fn compound_assign_global(
        &mut self,
        ident: Ident,
        mangled: crate::MangledName,
        ty: Type,
        op: BinOp,
        op_span: Span,
        value: ExprId,
        span: Span,
    ) -> TypedStmtKind {
        let lhs_path =
            narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled.clone()));
        let (synth_lhs, lhs_ty) = if let Some(view) = self.lookup_narrowed_view(&lhs_path) {
            let binding = view.binding.clone();
            let narrowed_ty = view.narrowed_ty.clone();
            let id = self.typed_ast.push_expr(TypedExpr {
                kind: TypedExprKind::LocalNarrowRef {
                    binding,
                    path: lhs_path,
                },
                span: ident.span,
                ty: narrowed_ty.clone(),
            });
            (id, narrowed_ty)
        } else {
            let id = self.typed_ast.push_expr(TypedExpr {
                kind: TypedExprKind::GlobalRef {
                    mangled: mangled.clone(),
                    name: ident.clone(),
                },
                span: ident.span,
                ty: ty.clone(),
            });
            (id, ty.clone())
        };
        let (typed_value, value_ty) = self.infer_expr(value, Some(&lhs_ty));
        let result_ty =
            self.check_compound_arith(op, (synth_lhs, &lhs_ty), (typed_value, &value_ty), op_span);
        let synth_binary = self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::Binary {
                op,
                lhs: synth_lhs,
                rhs: typed_value,
            },
            span,
            ty: result_ty.clone(),
        });
        self.renarrow_global_after_write(&ident, &mangled, &ty, result_ty);
        TypedStmtKind::AssignGlobal {
            ident,
            mangled,
            target_ty: ty,
            value: synth_binary,
        }
    }

    /// Re-narrows a module-level `let` after a write to it — the global twin of
    /// [`Self::renarrow_local_after_write`], and deliberately the same rule, so
    /// `if (g !== null) { g = 5; g.toString() }` behaves the way the local
    /// spelling does.
    pub(super) fn renarrow_global_after_write(
        &mut self,
        ident: &Ident,
        mangled: &crate::MangledName,
        declared_ty: &Type,
        written_ty: Type,
    ) {
        if matches!(written_ty, Type::Error) {
            return;
        }
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled.clone()));
        if written_ty == *declared_ty {
            self.invalidate_for_reassignment(path, ident.span);
            return;
        }
        self.install_assignment_narrowing(path, ident.clone(), written_ty, ident.span);
    }

    /// Poison for a rejected class-name write: the diagnostic is already
    /// reported, but the RHS still needs inferring so nested errors surface.
    fn poisoned_static_field_write(
        &mut self,
        receiver: Ident,
        recv_span: Span,
        name: &Ident,
        value: ExprId,
    ) -> TypedStmtKind {
        let typed_receiver = self.typed_ast.push_expr(crate::TypedExpr {
            kind: TypedExprKind::LocalRef {
                ident: receiver,
                boxed: false,
            },
            span: recv_span,
            ty: Type::Error,
        });
        let (typed_value, _) = self.infer_expr(value, None);
        TypedStmtKind::AssignField {
            receiver: typed_receiver,
            name: name.clone(),
            value: typed_value,
        }
    }

    fn infer_assign_field(
        &mut self,
        receiver: ExprId,
        name: Ident,
        value: ExprId,
    ) -> TypedStmtKind {
        let recv_span = self.ast.expr(receiver).span;
        let value_span = self.ast.expr(value).span;
        match self.resolve_static_field_write(receiver, &name) {
            StaticWrite::NotClassName => {}
            StaticWrite::Rejected { receiver } => {
                return self.poisoned_static_field_write(receiver, recv_span, &name, value);
            }
            StaticWrite::Resolved { mangled, ty } => {
                let (typed_value, value_ty) = self.infer_expr(value, Some(&ty));
                if !assignable(&value_ty, &ty, self.resolver()) {
                    self.error(value_span, format!("expected `{ty}`, got `{value_ty}`"));
                }
                return TypedStmtKind::AssignGlobal {
                    ident: name,
                    mangled,
                    target_ty: ty,
                    value: typed_value,
                };
            }
        }
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None);
        // Compute target path before RHS inference: write invalidates after the RHS
        // is evaluated so `obj.foo = obj.foo + 1` still reads the narrowed shadow.
        let target_path = self
            .expr_to_reference_path(self.typed_ast.expr(typed_receiver))
            .map(|mut p| {
                p.chain
                    .push(super::narrowing::PathElem::Field(name.name.clone()));
                p
            });
        // The value is still inferred, and its own diagnostics still surface: a
        // bad name or call in it is a second real problem the reader needs in the
        // same pass. So every rejecting arm that can name the field's write type
        // passes it — without a hint a tuple, object, or empty-array literal is
        // inferred at the wrong shape and blames itself for the receiver's fault.
        let placeholder = |inferer: &mut Self, ty_hint: Option<&Type>| {
            let (typed_value, _) = inferer.infer_expr(value, ty_hint);
            TypedStmtKind::AssignField {
                receiver: typed_receiver,
                name: name.clone(),
                value: typed_value,
            }
        };
        let result = if let Type::ClassRef { mangled, args, .. } = receiver_ty.peel() {
            let mangled = mangled.clone();
            let class_args = args.clone();
            if let Some((field, decl_mangled)) =
                self.class_field_visible(&mangled, &class_args, &name.name)
            {
                let getter = self.class_getter(&mangled, &class_args, &name.name);
                let setter = self.class_setter(&mangled, &class_args, &name.name);
                // The write type: a setter's parameter type (independent of the
                // getter's read type), else the data field's type.
                let field_ty = if getter.is_some() || setter.is_some() {
                    // An accessor is writable iff it has a setter — the `readonly`
                    // ctor-only rule applies to data fields, not accessors.
                    if let Some(write_ty) = setter {
                        write_ty
                    } else {
                        self.error_with_help(
                            name.span,
                            format!(
                                "cannot assign to read-only accessor `{}` on `{}`",
                                name.name, receiver_ty,
                            ),
                            vec![format!(
                                "add a `set {}(v: T)` accessor to write it",
                                name.name
                            )],
                        );
                        Type::Error
                    }
                } else {
                    if field.readonly && !self.readonly_write_allowed(receiver, &decl_mangled) {
                        self.error_with_help(
                            name.span,
                            format!(
                                "cannot assign to readonly field `{}` on `{}`",
                                name.name, receiver_ty,
                            ),
                            vec![
                                "a `readonly` field is writable only in the constructor of its declaring class"
                                    .to_string(),
                            ],
                        );
                    }
                    if field.optional {
                        Type::union(vec![field.ty.clone(), Type::Null])
                    } else {
                        field.ty.clone()
                    }
                };
                let (typed_value, value_ty) = self.infer_expr(value, Some(&field_ty));
                if !matches!(value_ty, Type::Error)
                    && !assignable(&value_ty, &field_ty, self.resolver())
                {
                    let help = super::type_diff::type_mismatch_help(&field_ty, &value_ty);
                    self.error_with_help(
                        value_span,
                        format!("expected `{field_ty}`, got `{value_ty}`"),
                        help,
                    );
                }
                TypedStmtKind::AssignField {
                    receiver: typed_receiver,
                    name: name.clone(),
                    value: typed_value,
                }
            } else {
                if !self.try_report_method_assignment(name.span, &receiver_ty, &name.name) {
                    self.report_missing_field(name.span, &receiver_ty, &name.name);
                }
                placeholder(self, None)
            }
        } else if self.try_report_method_assignment(name.span, &receiver_ty, &name.name) {
            // Every non-class receiver whose member is a method: an interface
            // surfaces it as a readonly property of function type, a structural
            // one as no field at all, and neither says the member is a method.
            // The RHS gets no hint — hinting the method's own signature adds a
            // second error blaming the value for not being that function.
            placeholder(self, None)
        } else if let Some((prop_sig, _, _, _)) = self.find_property(&receiver_ty, &name.name) {
            if prop_sig.readonly {
                self.error(
                    name.span,
                    format!(
                        "cannot assign to readonly property `{}` on `{}`",
                        name.name, receiver_ty,
                    ),
                );
                placeholder(self, Some(&prop_sig.ty))
            } else {
                // Optional property widens to `T | null` on the write side.
                let field_ty = if prop_sig.optional {
                    Type::union(vec![prop_sig.ty.clone(), Type::Null])
                } else {
                    prop_sig.ty.clone()
                };
                let (typed_value, value_ty) = self.infer_expr(value, Some(&field_ty));
                if !matches!(value_ty, Type::Error)
                    && !assignable(&value_ty, &field_ty, self.resolver())
                {
                    let help = super::type_diff::type_mismatch_help(&field_ty, &value_ty);
                    self.error_with_help(
                        value_span,
                        format!("expected `{field_ty}`, got `{value_ty}`"),
                        help,
                    );
                }
                TypedStmtKind::AssignField {
                    receiver: typed_receiver,
                    name: name.clone(),
                    value: typed_value,
                }
            }
        } else if matches!(receiver_ty, Type::Error) {
            placeholder(self, None)
        } else if let Some(fields) = self.assignment_target_fields(&receiver_ty) {
            let field_lookup = fields.get(&name.name).cloned();
            if let Some(field) = field_lookup {
                if field.readonly {
                    self.error(
                        name.span,
                        format!(
                            "cannot assign to readonly property `{}` on `{}`",
                            name.name, receiver_ty,
                        ),
                    );
                }
                // Optional field widens to `T | null` on the write side.
                let field_ty = if field.optional {
                    Type::union(vec![field.ty.clone(), Type::Null])
                } else {
                    field.ty.clone()
                };
                let (typed_value, value_ty) = self.infer_expr(value, Some(&field_ty));
                if !assignable(&value_ty, &field_ty, self.resolver()) {
                    let help = super::type_diff::type_mismatch_help(&field_ty, &value_ty);
                    self.error_with_help(
                        value_span,
                        format!("expected `{field_ty}`, got `{value_ty}`"),
                        help,
                    );
                }
                TypedStmtKind::AssignField {
                    receiver: typed_receiver,
                    name: name.clone(),
                    value: typed_value,
                }
            } else {
                let help = vec![self.format_definition(&receiver_ty)];
                self.error_with_help(
                    name.span,
                    format!("no field `{}` on type `{}`", name.name, receiver_ty,),
                    help,
                );
                placeholder(self, None)
            }
        } else {
            let recv_path = self.expr_to_reference_path(self.typed_ast.expr(typed_receiver));
            self.report_unassignable_field_target(
                recv_span,
                &name,
                &receiver_ty,
                recv_path.as_ref(),
                None,
            );
            let target_ty = self.write_target_ty(&receiver_ty, &name.name);
            placeholder(self, target_ty.as_ref())
        };
        if let Some(path) = target_path {
            let preserved = self.lookup_narrowed_view(&path).cloned().filter(|view| {
                let TypedStmtKind::AssignField { value, .. } = &result else {
                    return false;
                };
                assignable(
                    &self.typed_ast.expr(*value).ty,
                    &view.narrowed_ty,
                    self.resolver(),
                )
            });
            self.invalidate_for_write(path.clone(), name.span);
            if let Some(view) = preserved {
                self.install_joined_narrowings([(path, view)].into_iter().collect(), name.span);
            }
        }
        result
    }

    fn infer_assign_index(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        value: ExprId,
        _span: Span,
    ) -> TypedStmtKind {
        let recv_span = self.ast.expr(receiver).span;
        let value_span = self.ast.expr(value).span;
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None);
        // Peel to match the read path: an alias of an array or `Uint8Array` is
        // assignable on the same terms as the type it names.
        let elem_ty = match receiver_ty.peel() {
            Type::Array(elem) => (**elem).clone(),
            // codegen truncates RHS to the low 8 bits for storage in `$rawUint8Array`.
            Type::Uint8Array => Type::Number,
            Type::Tuple(_) => {
                self.error_with_help(
                    recv_span,
                    "indexed write on tuple not supported".to_string(),
                    vec![
                        "tuples are read-only; reconstruct the tuple with the updated element"
                            .to_string(),
                    ],
                );
                Type::Error
            }
            Type::Error => Type::Error,
            _ => {
                let help = vec![self.format_definition(&receiver_ty)];
                self.error_with_help(
                    recv_span,
                    format!("cannot assign to index of `{receiver_ty}`"),
                    help,
                );
                Type::Error
            }
        };
        let (typed_index, _) = self.infer_expr(index, Some(&Type::Number));
        let (typed_value, value_ty) = self.infer_expr(value, Some(&elem_ty));
        if !matches!(elem_ty, Type::Error)
            && !matches!(value_ty, Type::Error)
            && !assignable(&value_ty, &elem_ty, self.resolver())
        {
            let help = super::type_diff::type_mismatch_help(&elem_ty, &value_ty);
            self.error_with_help(
                value_span,
                format!("expected `{elem_ty}`, got `{value_ty}`"),
                help,
            );
        }
        TypedStmtKind::AssignIndex {
            receiver: typed_receiver,
            index: typed_index,
            value: typed_value,
            elem_ty,
        }
    }

    /// Re-narrows a local after a write to it. A value whose type *differs* from
    /// the declared one installs a fresh narrowing — callers have already checked
    /// assignability, so differing means narrower — and the returned type tells
    /// codegen to materialize a shadow Wasm local for it. Anything else, the
    /// declared type itself included, kills whatever a guard installed: the binding
    /// no longer holds the value that guard tested.
    fn renarrow_local_after_write(
        &mut self,
        target: &Ident,
        decl_scope: narrowing::ScopeId,
        declared_ty: &Type,
        written_ty: Type,
    ) -> Option<Type> {
        // A poisoned RHS leaves the narrowing exactly as it was: the diagnostic for
        // whatever went wrong upstream already stands, and adding "re-narrow after
        // the reassignment" on top of it points at an edit that fixes nothing.
        if matches!(written_ty, Type::Error) {
            return None;
        }
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
            name: target.name.clone(),
            decl_scope,
        });
        if written_ty == *declared_ty {
            self.invalidate_for_reassignment(path, target.span);
            return None;
        }
        self.install_assignment_narrowing(path, target.clone(), written_ty.clone(), target.span);
        Some(written_ty)
    }

    fn infer_assign(&mut self, target: Ident, value: ExprId, span: Span) -> TypedStmtKind {
        if let Some(entry) = self.scopes.get(&target.name).cloned() {
            if entry.is_const {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: target.span,
                    message: format!("cannot assign to const binding `{}`", target.name),
                    help: vec![format!(
                        "declare with `let` if reassignment is required: `let {} = …;`",
                        target.name
                    )],
                    notes: vec![(entry.decl_span, "declared as `const` here".to_string())],
                });
            }
            let value_span = self.ast.expr(value).span;
            let (typed_value, value_ty) = self.infer_expr(value, Some(&entry.ty));
            // Re-check assignability: `infer_expr` skips when the hint contains `Type::Var`,
            // but assignment is always a real semantic constraint.
            if !assignable(&value_ty, &entry.ty, self.resolver()) {
                self.error(
                    value_span,
                    format!("expected `{}`, got `{}`", entry.ty, value_ty),
                );
            }
            let narrowed_shadow_ty =
                self.renarrow_local_after_write(&target, entry.decl_scope, &entry.ty, value_ty);
            return TypedStmtKind::AssignLocal {
                ident: target,
                target_ty: entry.ty.clone(),
                value: typed_value,
                boxed: false,
                narrowed_shadow_ty,
            };
        }
        if let Some(entry) = self.top_symbols.get(&target.name) {
            let kind_clone = entry.kind.clone();
            let prev_span = entry.declaration_span;
            let mangled = entry.mangled_name.clone();
            match kind_clone {
                ValueKind::Let { ty, .. } => {
                    let value_span = self.ast.expr(value).span;
                    let (typed_value, value_ty) = self.infer_expr(value, Some(&ty));
                    if !assignable(&value_ty, &ty, self.resolver()) {
                        self.error(value_span, format!("expected `{ty}`, got `{value_ty}`"));
                    }
                    self.renarrow_global_after_write(&target, &mangled, &ty, value_ty);
                    TypedStmtKind::AssignGlobal {
                        ident: target,
                        mangled,
                        target_ty: ty,
                        value: typed_value,
                    }
                }
                ValueKind::Const { ty, .. } => {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: target.span,
                        message: format!("cannot assign to const binding `{}`", target.name),
                        help: vec![format!(
                            "declare with `let` if reassignment is required: `let {} = …;`",
                            target.name
                        )],
                        notes: vec![(prev_span, "declared as `const` here".to_string())],
                    });
                    // Infer RHS so nested errors surface; the diagnostic blocks compilation.
                    let (typed_value, _) = self.infer_expr(value, Some(&ty));
                    TypedStmtKind::AssignGlobal {
                        ident: target,
                        mangled,
                        target_ty: ty,
                        value: typed_value,
                    }
                }
                ValueKind::Function { .. } => {
                    self.error_with_help(
                        target.span,
                        format!("cannot assign to function `{}`", target.name),
                        vec![
                            "top-level functions are immutable; declare a `let f = …` binding to reassign"
                                .to_string(),
                        ],
                    );
                    let (typed_value, _) = self.infer_expr(value, None);
                    TypedStmtKind::AssignGlobal {
                        ident: target,
                        mangled,
                        target_ty: Type::Error,
                        value: typed_value,
                    }
                }
            }
        } else {
            let help: Vec<String> = self
                .closest_local_or_global(&target.name)
                .map(|s| vec![format!("did you mean `{}`?", s)])
                .unwrap_or_default();
            self.error_with_help(
                span,
                format!("unresolved identifier `{}`", target.name),
                help,
            );
            let (typed_value, _) = self.infer_expr(value, None);
            // Placeholder; downstream Type::Error suppression handles cascades.
            TypedStmtKind::AssignLocal {
                ident: target,
                target_ty: Type::Error,
                value: typed_value,
                boxed: false,
                narrowed_shadow_ty: None,
            }
        }
    }

    /// `x += y` lowers to `x = x + y`; the synthesized LHS reads through any active
    /// narrowing so the binary type rule sees the narrowed view.
    fn infer_compound_assign(
        &mut self,
        target: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
        span: Span,
    ) -> TypedStmtKind {
        if let Some(entry) = self.scopes.get(&target.name).cloned() {
            if entry.is_const {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: target.span,
                    message: format!("cannot assign to const binding `{}`", target.name),
                    help: vec![format!(
                        "declare with `let` if reassignment is required: `let {} = …;`",
                        target.name
                    )],
                    notes: vec![(entry.decl_span, "declared as `const` here".to_string())],
                });
            }
            let target_ty = entry.ty.clone();
            let lhs_path =
                super::narrowing::ReferencePath::root(super::narrowing::BindingId::Local {
                    name: target.name.clone(),
                    decl_scope: entry.decl_scope,
                });
            let (synth_lhs, lhs_ty) = if let Some(view) = self.lookup_narrowed_view(&lhs_path) {
                let binding = view.binding.clone();
                let narrowed_ty = view.narrowed_ty.clone();
                let id = self.typed_ast.push_expr(TypedExpr {
                    kind: TypedExprKind::LocalNarrowRef {
                        binding,
                        path: lhs_path,
                    },
                    span: target.span,
                    ty: narrowed_ty.clone(),
                });
                (id, narrowed_ty)
            } else {
                let id = self.typed_ast.push_expr(TypedExpr {
                    kind: TypedExprKind::LocalRef {
                        ident: target.clone(),
                        boxed: false,
                    },
                    span: target.span,
                    ty: target_ty.clone(),
                });
                (id, target_ty.clone())
            };
            let (typed_value, value_ty) = self.infer_expr(value, Some(&lhs_ty));
            let result_ty = self.check_compound_arith(
                op,
                (synth_lhs, &lhs_ty),
                (typed_value, &value_ty),
                op_span,
            );
            // Re-check assignability: catches literal-refined slots (e.g. `1|2|3`)
            // where arithmetic widens the result to `number`.
            let value_span = self.ast.expr(value).span;
            if !matches!(result_ty, Type::Error)
                && !matches!(target_ty, Type::Error)
                && !assignable(&result_ty, &target_ty, self.resolver())
            {
                self.error(
                    value_span,
                    format!("expected `{target_ty}`, got `{result_ty}`"),
                );
            }
            let synth_binary = self.typed_ast.push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op,
                    lhs: synth_lhs,
                    rhs: typed_value,
                },
                span,
                ty: result_ty.clone(),
            });
            let narrowed_shadow_ty =
                self.renarrow_local_after_write(&target, entry.decl_scope, &target_ty, result_ty);
            return TypedStmtKind::AssignLocal {
                ident: target,
                target_ty,
                value: synth_binary,
                boxed: false,
                narrowed_shadow_ty,
            };
        }
        if let Some(entry) = self.top_symbols.get(&target.name) {
            let kind_clone = entry.kind.clone();
            let prev_span = entry.declaration_span;
            let mangled = entry.mangled_name.clone();
            match kind_clone {
                ValueKind::Let { ty, .. } => {
                    self.compound_assign_global(target, mangled, ty, op, op_span, value, span)
                }
                ValueKind::Const { ty, .. } => {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: target.span,
                        message: format!("cannot assign to const binding `{}`", target.name),
                        help: vec![format!(
                            "declare with `let` if reassignment is required: `let {} = …;`",
                            target.name
                        )],
                        notes: vec![(prev_span, "declared as `const` here".to_string())],
                    });
                    let (typed_value, _) = self.infer_expr(value, Some(&ty));
                    TypedStmtKind::AssignGlobal {
                        ident: target,
                        mangled,
                        target_ty: ty,
                        value: typed_value,
                    }
                }
                ValueKind::Function { .. } => {
                    self.error_with_help(
                        target.span,
                        format!("cannot assign to function `{}`", target.name),
                        vec![
                            "top-level functions are immutable; declare a `let f = …` binding to reassign"
                                .to_string(),
                        ],
                    );
                    let (typed_value, _) = self.infer_expr(value, None);
                    TypedStmtKind::AssignGlobal {
                        ident: target,
                        mangled,
                        target_ty: Type::Error,
                        value: typed_value,
                    }
                }
            }
        } else {
            let help: Vec<String> = self
                .closest_local_or_global(&target.name)
                .map(|s| vec![format!("did you mean `{}`?", s)])
                .unwrap_or_default();
            self.error_with_help(
                span,
                format!("unresolved identifier `{}`", target.name),
                help,
            );
            let (typed_value, _) = self.infer_expr(value, None);
            TypedStmtKind::AssignLocal {
                ident: target,
                target_ty: Type::Error,
                value: typed_value,
                boxed: false,
                narrowed_shadow_ty: None,
            }
        }
    }

    fn infer_compound_assign_field(
        &mut self,
        receiver: ExprId,
        name: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
    ) -> TypedStmtKind {
        let recv_span = self.ast.expr(receiver).span;
        let stmt_span = recv_span.merge(self.ast.expr(value).span);
        match self.resolve_static_field_write(receiver, &name) {
            StaticWrite::NotClassName => {}
            StaticWrite::Rejected { receiver } => {
                return self.poisoned_static_field_write(receiver, recv_span, &name, value);
            }
            StaticWrite::Resolved { mangled, ty } => {
                return self
                    .compound_assign_global(name, mangled, ty, op, op_span, value, stmt_span);
            }
        }
        let recv_span = self.ast.expr(receiver).span;
        let rw_op = super::diagnostics::RwOp::Compound(op);
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None);
        let target_path = self
            .expr_to_reference_path(self.typed_ast.expr(typed_receiver))
            .map(|mut p| {
                p.chain
                    .push(super::narrowing::PathElem::Field(name.name.clone()));
                p
            });
        let placeholder = |inferer: &mut Self| {
            let (typed_value, _) = inferer.infer_expr(value, None);
            TypedStmtKind::AssignField {
                receiver: typed_receiver,
                name: name.clone(),
                value: typed_value,
            }
        };
        let result = if let Type::ClassRef { mangled, args, .. } = receiver_ty.peel() {
            let mangled = mangled.clone();
            let class_args = args.clone();
            match self.class_read_write_target(
                &mangled,
                &class_args,
                receiver,
                &receiver_ty,
                &name,
                rw_op,
            ) {
                Some(rw) => self.build_compound_assign_field(
                    typed_receiver,
                    name.clone(),
                    op,
                    op_span,
                    value,
                    &rw,
                    stmt_span,
                ),
                None => placeholder(self),
            }
        } else if self.try_report_method_assignment(name.span, &receiver_ty, &name.name) {
            placeholder(self)
        } else if let Some((prop_sig, _, _, _)) = self.find_property(&receiver_ty, &name.name) {
            if prop_sig.readonly {
                self.error(
                    name.span,
                    format!(
                        "cannot assign to readonly property `{}` on `{}`",
                        name.name, receiver_ty,
                    ),
                );
            }
            if self.try_report_nullable_rw_target(
                recv_span,
                &receiver_ty,
                &name,
                &prop_sig.ty,
                prop_sig.optional,
                rw_op,
            ) {
                placeholder(self)
            } else {
                self.build_compound_assign_field(
                    typed_receiver,
                    name.clone(),
                    op,
                    op_span,
                    value,
                    &FieldRw::uniform(prop_sig.ty.clone()),
                    stmt_span,
                )
            }
        } else if matches!(receiver_ty, Type::Error) {
            placeholder(self)
        } else if let Some(fields) = self.assignment_target_fields(&receiver_ty) {
            if let Some(field) = fields.get(&name.name).cloned() {
                if field.readonly {
                    self.error(
                        name.span,
                        format!(
                            "cannot assign to readonly property `{}` on `{}`",
                            name.name, receiver_ty,
                        ),
                    );
                }
                if self.try_report_nullable_rw_target(
                    recv_span,
                    &receiver_ty,
                    &name,
                    &field.ty,
                    field.optional,
                    rw_op,
                ) {
                    placeholder(self)
                } else {
                    let rw = FieldRw::uniform(field.ty.clone());
                    self.build_compound_assign_field(
                        typed_receiver,
                        name.clone(),
                        op,
                        op_span,
                        value,
                        &rw,
                        stmt_span,
                    )
                }
            } else {
                let help = self.definition_help(&receiver_ty);
                self.error_with_help(
                    name.span,
                    format!("no field `{}` on type `{}`", name.name, receiver_ty),
                    help,
                );
                placeholder(self)
            }
        } else {
            let recv_path = self.expr_to_reference_path(self.typed_ast.expr(typed_receiver));
            self.report_unassignable_field_target(
                recv_span,
                &name,
                &receiver_ty,
                recv_path.as_ref(),
                Some(rw_op),
            );
            placeholder(self)
        };
        if let Some(path) = target_path {
            self.invalidate_for_write(path, name.span);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn build_compound_assign_field(
        &mut self,
        typed_receiver: ExprId,
        name: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
        rw: &FieldRw,
        stmt_span: Span,
    ) -> TypedStmtKind {
        let value_span = self.ast.expr(value).span;
        let (typed_value, value_ty) = self.infer_expr(value, Some(&rw.read));
        // Built before the operator check so the check can name it as the
        // narrowing culprit.
        let synth_lhs = self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::FieldAccess {
                receiver: typed_receiver,
                name: name.clone(),
            },
            span: name.span,
            ty: rw.read.clone(),
        });
        let result_ty =
            self.check_compound_arith(op, (synth_lhs, &rw.read), (typed_value, &value_ty), op_span);
        if !matches!(result_ty, Type::Error)
            && !matches!(rw.write, Type::Error)
            && !assignable(&result_ty, &rw.write, self.resolver())
        {
            self.error(
                value_span,
                format!("expected `{}`, got `{result_ty}`", rw.write),
            );
        }
        let synth_binary = self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::Binary {
                op,
                lhs: synth_lhs,
                rhs: typed_value,
            },
            span: stmt_span,
            ty: result_ty,
        });
        TypedStmtKind::AssignField {
            receiver: typed_receiver,
            name,
            value: synth_binary,
        }
    }

    fn infer_compound_assign_index(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        op: BinOp,
        op_span: Span,
        value: ExprId,
        _span: Span,
    ) -> TypedStmtKind {
        let recv_span = self.ast.expr(receiver).span;
        let value_span = self.ast.expr(value).span;
        let stmt_span = recv_span.merge(value_span);
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None);
        let elem_ty = match receiver_ty.peel() {
            Type::Array(elem) => (**elem).clone(),
            Type::Uint8Array => Type::Number,
            Type::Tuple(_) => {
                self.error_with_help(
                    recv_span,
                    "indexed write on tuple not supported".to_string(),
                    vec![
                        "tuples are read-only; reconstruct the tuple with the updated element"
                            .to_string(),
                    ],
                );
                Type::Error
            }
            Type::Error => Type::Error,
            _ => {
                let help = vec![self.format_definition(&receiver_ty)];
                self.error_with_help(
                    recv_span,
                    format!("cannot assign to index of `{receiver_ty}`"),
                    help,
                );
                Type::Error
            }
        };
        let (typed_index, _) = self.infer_expr(index, Some(&Type::Number));
        let (typed_value, value_ty) = self.infer_expr(value, Some(&elem_ty));
        // Built before the operator check so the check can name it as the
        // narrowing culprit.
        let synth_lhs = self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::IndexAccess {
                receiver: typed_receiver,
                index: typed_index,
            },
            span: recv_span,
            ty: elem_ty.clone(),
        });
        let result_ty =
            self.check_compound_arith(op, (synth_lhs, &elem_ty), (typed_value, &value_ty), op_span);
        if !matches!(result_ty, Type::Error)
            && !matches!(elem_ty, Type::Error)
            && !assignable(&result_ty, &elem_ty, self.resolver())
        {
            self.error(
                value_span,
                format!("expected `{elem_ty}`, got `{result_ty}`"),
            );
        }
        let synth_binary = self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::Binary {
                op,
                lhs: synth_lhs,
                rhs: typed_value,
            },
            span: stmt_span,
            ty: result_ty,
        });
        TypedStmtKind::AssignIndex {
            receiver: typed_receiver,
            index: typed_index,
            value: synth_binary,
            elem_ty,
        }
    }

    /// The result of `lhs op= rhs`, reported against `op_span` when the operator
    /// has no rule for the pair. The operand expressions come along so a dead
    /// narrowing on either side can be named as the real cause — the guard is
    /// often written and correct, just not where the compound assignment sits.
    fn check_compound_arith(
        &mut self,
        op: BinOp,
        lhs: (ExprId, &Type),
        rhs: (ExprId, &Type),
        op_span: Span,
    ) -> Type {
        let (lt, rt) = (lhs.1, rhs.1);
        if let Some(ty) = compound_arith_result(op, lt, rt) {
            return ty;
        }
        let sym = binary_op_text(op);
        let culprit = self
            .nullable_binary_culprit(lhs, rhs, |l, r| compound_arith_result(op, l, r).is_some());
        self.error_with_narrowing_hint(
            op_span,
            format!("`{sym}=` not defined for `{lt}` and `{rt}`"),
            Vec::new(),
            culprit,
        );
        Type::Error
    }
}

/// The arithmetic operator a compound assignment applies, as source text.
pub(super) fn binary_op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Pow => "**",
        _ => unreachable!("compound assignment only uses arithmetic ops"),
    }
}

/// The operator's own accept rule, shared by the result computation, the
/// narrowing-culprit probe, and the nullable read-modify-write target check: each
/// must ask exactly what the failure asked, or it recommends a fix that doesn't
/// apply to the site.
pub(super) fn compound_arith_result(op: BinOp, lt: &Type, rt: &Type) -> Option<Type> {
    match (op, lt.primitive_behavior(), rt.primitive_behavior()) {
        (_, Type::Error, _) | (_, _, Type::Error) => Some(Type::Error),
        (BinOp::Add, Type::Number, Type::Number) => Some(Type::Number),
        (BinOp::Add, Type::String, Type::String) => Some(Type::String),
        (BinOp::Add, Type::BigInt, Type::BigInt) => Some(Type::BigInt),
        (
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Pow,
            Type::Number,
            Type::Number,
        ) => Some(Type::Number),
        (
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Pow,
            Type::BigInt,
            Type::BigInt,
        ) => Some(Type::BigInt),
        _ => None,
    }
}

/// Views established by writes on a normal exit. Root views rebind to storage;
/// field views are rematerialized in the receiving scope.
fn assignment_narrowings(
    env: &narrowing::NarrowEnv,
    assigned: &std::collections::BTreeSet<narrowing::ReferencePath>,
) -> narrowing::NarrowEnv {
    env.iter()
        .filter(|(path, _)| assigned.contains(*path))
        .map(|(path, view)| (path.clone(), view.clone()))
        .collect()
}

struct ClauseOutcome {
    all_writes: std::collections::BTreeSet<narrowing::ReferencePath>,
    body: Option<StmtId>,
    exit: Option<narrowing::NarrowEnv>,
    assigned: std::collections::BTreeSet<narrowing::ReferencePath>,
}

fn apply_finally_to_exit(post: &mut Option<narrowing::NarrowEnv>, outcome: &ClauseOutcome) {
    match (&outcome.exit, post.as_mut()) {
        (Some(final_post), Some(post)) => {
            post.retain(|path, _| !outcome.all_writes.iter().any(|p| p.is_prefix_of(path)));
            post.extend(assignment_narrowings(final_post, &outcome.assigned));
        }
        (None, _) => *post = None,
        _ => {}
    }
}

fn apply_finally_to_transfers(
    transfers: &mut Vec<(
        narrowing::NarrowEnv,
        std::collections::BTreeSet<narrowing::ReferencePath>,
    )>,
    range: std::ops::Range<usize>,
    outcome: &ClauseOutcome,
) {
    if outcome.exit.is_none() {
        transfers.drain(range);
        return;
    }
    for (env, assigned) in &mut transfers[range] {
        let mut post = Some(std::mem::take(env));
        apply_finally_to_exit(&mut post, outcome);
        *env = post.unwrap_or_default();
        assigned.extend(outcome.all_writes.iter().cloned());
    }
}

/// The typed body/update and the state returning to the condition. For a
/// classic `for`, `back_edge` includes the update's effects.
pub(super) struct LoopBodyOutcome {
    pub body: StmtId,
    pub update: Option<StmtId>,
    pub assigned: std::collections::BTreeSet<narrowing::ReferencePath>,
    pub back_edge: narrowing::NarrowEnv,
    pub reaches_back_edge: bool,
}

/// Join complete snapshots from reachable paths. Their tombstones have already
/// removed invalidated views, so the union needs no additional write filtering.
fn join_reachable_envs(
    natural: Option<narrowing::NarrowEnv>,
    exits: Vec<narrowing::NarrowEnv>,
) -> Option<narrowing::NarrowEnv> {
    natural.into_iter().chain(exits).reduce(|a, b| {
        narrowing::union_envs(
            a,
            std::collections::BTreeSet::new(),
            b,
            std::collections::BTreeSet::new(),
        )
        .0
    })
}

fn loop_exit_env(
    entry: &narrowing::NarrowEnv,
    outcome: &LoopBodyOutcome,
    condition_false: narrowing::NarrowEnv,
) -> narrowing::NarrowEnv {
    let mut post = if outcome.reaches_back_edge {
        narrowing::union_envs(
            entry.clone(),
            Default::default(),
            outcome.back_edge.clone(),
            outcome.assigned.clone(),
        )
        .0
    } else {
        entry.clone()
    };
    post.extend(condition_false);
    post
}

fn cond_is_static_true(_inferer: &Inferer<'_>, expr_id: ExprId) -> bool {
    matches!(
        _inferer.typed_ast.expr(expr_id).kind,
        crate::TypedExprKind::Boolean(true)
    )
}

impl Inferer<'_> {
    /// Retype while a back edge falsifies an enclosing view. The set of outer
    /// views strictly shrinks, so retries terminate; condition guards are
    /// reinstalled for each body pass because they are tested every iteration.
    pub(super) fn run_loop_body_with_fixed_point(
        &mut self,
        body: StmtId,
        entry_env: narrowing::NarrowEnv,
        body_span: Span,
        body_scope_floor: narrowing::ScopeId,
        update: Option<StmtId>,
    ) -> LoopBodyOutcome {
        let diag_len = self.diagnostics.len();
        let mats_len = self.pending_post_if_materializations.len();
        let (breaks_len, continues_len) = self
            .pending_joins
            .last()
            .map_or((0, 0), |f| (f.breaks.len(), f.continues.len()));
        let entry_reachable = self.reachable;

        loop {
            let outcome = self.run_body_pass(body, &entry_env, body_span, continues_len, update);
            if !self.drop_narrowings_the_body_falsifies(&outcome, body_scope_floor, body_span) {
                return outcome;
            }
            // Discard speculative diagnostics and exits before retyping under
            // the widened state. Each retry removes at least one outer view.
            self.diagnostics.truncate(diag_len);
            self.pending_post_if_materializations.truncate(mats_len);
            if let Some(frame) = self.pending_joins.last_mut() {
                frame.breaks.truncate(breaks_len);
                frame.continues.truncate(continues_len);
            }
            self.reachable = entry_reachable;
        }
    }

    /// Infer a clause from its possible entry state. An exception may occur
    /// before or after any write in an earlier clause, so those writes kill
    /// incoming guards; only normal completion contributes an exit.
    fn infer_isolated_clause(
        &mut self,
        clause: StmtId,
        entry_reachable: bool,
        uncertain_writes: &std::collections::BTreeSet<narrowing::ReferencePath>,
    ) -> ClauseOutcome {
        self.push_narrow_frame(narrowing::NarrowEnv::new());
        let span = self.ast.stmt(clause).span;
        for path in uncertain_writes {
            self.drop_narrowings_under(path, narrowing::InvalidationReason::Write { span });
        }
        self.reachable = entry_reachable;
        self.clause_write_scopes.push(Default::default());
        let body = self.infer_stmt(clause);
        let all_writes = self
            .clause_write_scopes
            .pop()
            .expect("clause write collector");
        let exit = self.reachable.then(|| self.snapshot_active_narrowings(0).0);
        let (_, assigned) = self.pop_narrow_frame_capture();
        self.reachable = entry_reachable;
        ClauseOutcome {
            body,
            exit,
            assigned,
            all_writes,
        }
    }

    /// Invalidate outer views widened or killed on a reachable back edge.
    /// Bindings declared within the body are recreated on each iteration.
    fn drop_narrowings_the_body_falsifies(
        &mut self,
        outcome: &LoopBodyOutcome,
        body_scope_floor: narrowing::ScopeId,
        body_span: Span,
    ) -> bool {
        if !outcome.reaches_back_edge {
            return false;
        }
        let (active, _) = self.snapshot_active_narrowings(0);
        let falsified: Vec<_> = active
            .iter()
            .filter(|(path, view)| {
                let outlives_loop = match &path.root {
                    narrowing::BindingId::Local { decl_scope, .. } => {
                        *decl_scope < body_scope_floor
                    }
                    narrowing::BindingId::Global(_) | narrowing::BindingId::This => true,
                };
                outlives_loop
                    && outcome.assigned.iter().any(|p| p.is_prefix_of(path))
                    && outcome.back_edge.get(*path).is_none_or(|post| {
                        Type::union(vec![view.narrowed_ty.clone(), post.narrowed_ty.clone()])
                            != view.narrowed_ty
                    })
            })
            .map(|(path, _)| path.clone())
            .collect();
        for path in &falsified {
            self.drop_narrowings_under(
                path,
                narrowing::InvalidationReason::Write { span: body_span },
            );
        }
        !falsified.is_empty()
    }

    /// One walk of a loop body under `entry_env`: infer it, wrap the narrow
    /// regions the env calls for, and report what leaves on the back edge.
    fn run_body_pass(
        &mut self,
        body: StmtId,
        entry_env: &narrowing::NarrowEnv,
        body_span: Span,
        continues_base: usize,
        update: Option<StmtId>,
    ) -> LoopBodyOutcome {
        self.push_narrow_frame(entry_env.clone());
        self.loop_depth += 1;
        let (typed_body, body_post) = self.infer_loop_body(body);
        self.loop_depth -= 1;
        let body_end_reachable = self.reachable;
        let (_, mut assigned) = self.pop_narrow_frame_capture();
        let (continues, continued_assignments) = self.continue_exits_since(continues_base);
        assigned.extend(continued_assignments);
        let mut back_edge = join_reachable_envs(body_end_reachable.then_some(body_post), continues);
        let typed_update = update.and_then(|update| {
            self.push_narrow_frame(narrowing::NarrowEnv::new());
            for path in &assigned {
                self.drop_narrowings_under(
                    path,
                    narrowing::InvalidationReason::Write { span: body_span },
                );
            }
            let tail_env =
                self.loop_tail_env(back_edge.as_ref().unwrap_or(&narrowing::NarrowEnv::new()));
            self.push_narrow_frame(tail_env.clone());
            let typed = self
                .infer_stmt(update)
                .map(|body| self.wrap_narrow_regions(body, &tail_env, self.ast.stmt(update).span));
            let post = self.snapshot_active_narrowings(0).0;
            let (_, update_assigned) = self.pop_narrow_frame_capture();
            self.pop_narrow_frame();
            if back_edge.is_some() {
                back_edge = Some(post);
            }
            assigned.extend(update_assigned);
            typed
        });
        LoopBodyOutcome {
            update: typed_update,
            reaches_back_edge: back_edge.is_some(),
            body: self.wrap_narrow_regions(typed_body, entry_env, body_span),
            assigned,
            back_edge: back_edge.unwrap_or_default(),
        }
    }

    /// Capture the normal exit before the body's lexical frame is removed.
    /// The loop tail needs guard facts as well as assignment facts; its own
    /// materialization filters out names declared inside this block.
    fn infer_loop_body(&mut self, body: StmtId) -> (StmtId, narrowing::NarrowEnv) {
        let stmt = self.ast.stmt(body).clone();
        let StmtKind::Block(stmts) = stmt.kind else {
            unreachable!("loop bodies are blocks");
        };
        self.scopes.push();
        let typed_stmts = self.block_stmts_with_drain(&stmts, stmt.span);
        let post = self.snapshot_active_narrowings(0).0;
        self.scopes.pop();
        let typed = self.typed_ast.push_stmt(TypedStmt {
            kind: TypedStmtKind::Block(typed_stmts),
            span: stmt.span,
        });
        (typed, post)
    }

    fn continue_exits_since(
        &self,
        base_len: usize,
    ) -> (
        Vec<narrowing::NarrowEnv>,
        std::collections::BTreeSet<narrowing::ReferencePath>,
    ) {
        let Some(frame) = self.pending_joins.last() else {
            return Default::default();
        };
        let exits = &frame.continues[base_len..];
        let envs = exits.iter().map(|(env, _)| env.clone()).collect();
        let assigned = exits
            .iter()
            .flat_map(|(_, assigned)| assigned.iter().cloned())
            .collect();
        (envs, assigned)
    }

    /// Shared for-of source classification: what element type a `for-of` (or a
    /// synthesized iteration like `Array.from`) sees, and which desugar strategy
    /// applies. `None` means the type is not iterable.
    pub(super) fn classify_for_of_source(
        &mut self,
        iter_ty: &Type,
    ) -> Option<(Type, crate::ForOfKind)> {
        match iter_ty.peel() {
            Type::Array(elem) => Some(((**elem).clone(), crate::ForOfKind::Array)),
            // Tuples are `$Array` at runtime, so the array desugar reads them
            // directly; the positions' union is what each element can be.
            Type::Tuple(elements) => Some((Type::union(elements.clone()), crate::ForOfKind::Array)),
            // Strings iterate by code point through `String#iterator`.
            Type::String => Some((Type::String, crate::ForOfKind::Iterable)),
            // Exact-name match keeps Iterator<U> on its own desugar path
            // (it declares `next()`, not `iterator()`, so it fails the structural check below).
            Type::InterfaceRef { name, args, .. } if name == "Iterator" && args.len() == 1 => {
                Some((args[0].clone(), crate::ForOfKind::Iterator))
            }
            Type::InterfaceRef {
                mangled,
                package: _,
                name,
                args,
            }
            | Type::ClassRef {
                mangled,
                package: _,
                name,
                args,
            } => self
                .structural_form(mangled, name, args)
                .and_then(|fields| fields.get("iterator").cloned())
                .and_then(|field| match field.ty {
                    Type::Function { params, ret, .. } if params.is_empty() => match *ret {
                        Type::InterfaceRef {
                            name: rn, args: ra, ..
                        } if rn == "Iterator" && ra.len() == 1 => {
                            Some(ra.into_iter().next().unwrap())
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .map(|u| (u, crate::ForOfKind::Iterable)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::run;

    /// A rejected write checks its value against the field's *write* type, so an
    /// accessor pair that takes wider than it returns does not earn a second
    /// error. The count is the property — one rejection, not a rejection plus a
    /// mismatch the program has not committed — and no fixture directive can
    /// express a count.
    #[test]
    fn a_rejected_write_does_not_blame_a_value_its_setter_accepts() {
        for source in [
            // Nullable receiver: the hint comes from the null-stripped type.
            "class C { private n: number = 0; get v(): number { return this.n; } \
             set v(s: number | null) { this.n = 1; } }\n\
             function main(): string { let c: C | null = new C(); c.v = null; return \"x\"; }",
            // Union receiver: the hint comes from the members' agreed write type.
            "class A { private n: number = 0; get v(): number { return this.n; } \
             set v(s: number | null) { this.n = 1; } }\n\
             class B { private n: number = 0; get v(): number { return this.n; } \
             set v(s: number | null) { this.n = 2; } }\n\
             function main(): string { let u: A | B = new A(); u.v = null; return \"x\"; }",
        ] {
            let (_, diags) = run(source);
            let errors: Vec<&crate::Diagnostic> = diags
                .iter()
                .filter(|d| d.severity == crate::Severity::Error)
                .collect();
            assert_eq!(
                errors.len(),
                1,
                "expected one error for:\n{source}\ngot {errors:?}"
            );
        }
    }

    /// The other half of that contract: a value the setter really does reject is
    /// still reported, so the hint narrows the blame rather than removing it.
    #[test]
    fn a_rejected_write_still_reports_a_value_its_setter_rejects() {
        let (_, diags) = run(
            "class C { private n: number = 0; get v(): number { return this.n; } \
             set v(s: number | null) { this.n = 1; } }\n\
             function main(): string { let c: C | null = new C(); c.v = \"s\"; return \"x\"; }",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `number | null`, got `string`")),
            "value error lost: {diags:?}"
        );
    }

    #[test]
    fn if_with_boolean_condition() {
        let (_, diags) = run("function f(): void { if (true) { } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn if_with_truthiness_condition() {
        let (_, diags) = run("function f(): void { if (1) { } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn if_with_void_condition_diagnoses() {
        let (_, d) = run("function g(): void { } function f(): void { if (g()) { } }");
        assert_eq!(d.len(), 1);
        assert_eq!(
            d[0].message,
            "expected a value in this condition, got `void`"
        );
    }

    #[test]
    fn while_with_boolean_condition() {
        let (_, diags) = run("function f(): void { while (true) { } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn while_with_truthiness_condition() {
        let (_, diags) = run("function f(): void { let n: number = 0; while (n) { } }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn throw_error_value_ok() {
        let (_, diags) = run(r#"function f(): void { throw new Error("boom"); }"#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn throw_non_error_diagnoses() {
        let (_, d) = run(r#"function f(): void { throw "oops"; }"#);
        assert!(
            d.iter()
                .any(|x| x.message == "expected `Error`, got `string`"),
            "expected throw-type diagnostic, got: {d:?}",
        );
    }

    #[test]
    fn try_catch_finally_ok() {
        let (_, diags) = run(r#"function f(): void { try { } catch (e: Error) { } finally { } }"#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn try_catch_binding_reads_as_error() {
        let (_, diags) =
            run(r#"function f(): void { try { } catch (e: Error) { console.log(e.message); } }"#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn untyped_catch_binding_reads_as_error() {
        let (_, diags) =
            run(r#"function f(): void { try { } catch (e) { console.log(e.message); } }"#);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn catch_non_error_type_diagnoses() {
        let (_, d) = run(r#"function f(): void { try { } catch (e: string) { } }"#);
        assert!(
            d.iter().any(|x| x.message
                == "a `catch` binding must be `Error` or a class extending `Error`; got `string`"),
            "expected catch-type diagnostic, got: {d:?}",
        );
    }
}
