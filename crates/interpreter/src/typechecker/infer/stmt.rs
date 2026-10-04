use crate::compiler_error::CompilerFailure;

use crate::{
    BinOp, BindingKind, Diagnostic, ExprId, ExprKind, Ident, Severity, Span, StmtId, StmtKind,
    Type, TypedExpr, TypedExprKind, TypedStmt, TypedStmtKind, ValueKind,
};

use super::classes::{FieldRw, StaticResolution};
use super::{Inferer, assignable, narrowing};

/// The literal type of a bare literal initializer, for an unannotated `const`.
///
/// Only a literal token qualifies, through any number of parentheses. A computed
/// initializer widens even under `const` (`const a = 1 + 1` is `number`), matching
/// TypeScript, and so do arrays, object literals, and call results.
pub(super) fn literal_type_of(
    ast: &crate::Ast,
    value: crate::ExprId,
) -> Result<Option<Type>, CompilerFailure> {
    Ok(
        match &ast.try_expr(value).map_err(super::arena_failure)?.kind {
            ExprKind::Number(v) => Some(Type::NumberLiteral(crate::types::LiteralF64(*v))),
            ExprKind::String(s) => Some(Type::StringLiteral(s.clone())),
            ExprKind::Boolean(b) => Some(Type::BooleanLiteral(*b)),
            // `const a = (1)` is `1`, as in TypeScript: parentheses group, they do not
            // compute.
            ExprKind::Paren(inner) => literal_type_of(ast, *inner)?,
            _ => None,
        },
    )
}

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
    pub(super) fn infer_stmt(
        &mut self,
        stmt_id: StmtId,
    ) -> Result<Option<StmtId>, CompilerFailure> {
        let stmt = self
            .ast
            .try_stmt(stmt_id)
            .map_err(super::arena_failure)?
            .clone();
        let span = stmt.span;
        // As in `infer_expr`: a limit pending before this statement belongs to
        // whatever enclosing check met it.
        let limit_was_pending = self.type_limits.limit_reached();
        // Propagate once after dispatch: per-arm `?` creates large temporary
        // results that inflate every recursive frame in debug builds.
        let typed_kind = (match stmt.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                doc,
            } => self.infer_let_statement(name, ty, value, doc, span),
            StmtKind::Const {
                name,
                ty,
                value,
                doc,
            } => self.infer_const_statement(name, ty, value, doc, span),
            // Declared and assigned by its block (`declare_nested_functions`). A
            // brace-less body is reported by the parser but still wrapped in a
            // block, so every one has one.
            StmtKind::Function { .. } => return Ok(None),
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => self.infer_if_statement(condition, then_block, else_block, span),
            StmtKind::While { condition, body } => {
                let (condition, _, body) =
                    self.infer_condition_first_loop(Some(condition), None, body, span)?;
                Ok(TypedStmtKind::While {
                    condition: condition.ok_or_else(|| {
                        super::inference_failure("missing inferred while condition")
                    })?,
                    body,
                })
            }
            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                self.scopes.push();
                let typed_init = init.map(|id| self.infer_stmt(id)).transpose()?.flatten();
                // Init-scope bindings persist across iterations, so the body's
                // scope floor, taken in `infer_condition_first_loop`, sits
                // above them.
                let (typed_cond, typed_update, typed_body) =
                    self.infer_condition_first_loop(condition, update, body, span)?;
                self.scopes.pop();
                // The init scope is popped after the fold, so anything the fold
                // installed that is rooted in it is now unreadable.
                self.drop_out_of_scope_narrowings();
                Ok(TypedStmtKind::For {
                    init: typed_init,
                    condition: typed_cond,
                    update: typed_update,
                    body: typed_body,
                })
            }
            StmtKind::ForOf {
                binding_kind,
                name,
                ty: ann,
                iter,
                body,
            } => self.infer_for_of_statement(binding_kind, name, ann, iter, body, span),
            StmtKind::DoWhile { body, condition } => {
                self.infer_do_while_statement(body, condition, span)
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
                Ok(TypedStmtKind::Break)
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
                Ok(TypedStmtKind::Continue)
            }
            StmtKind::Return(value) => self.infer_return(value, span),
            StmtKind::Expr(expr_id) => {
                // `infer_super_call` takes the flag; cleared here too for a
                // statement whose inference never reaches it.
                self.super_call_is_statement = self.is_super_call(expr_id)?;
                let (typed_id, _) = self.infer_expr(expr_id, None)?;
                self.super_call_is_statement = false;
                Ok(TypedStmtKind::Expr(typed_id))
            }
            StmtKind::Block(stmts) => {
                self.scopes.push();
                // Propagate `assigned` so enclosing blocks invalidate narrowings on reassigned paths.
                self.push_narrow_frame(crate::typechecker::infer::narrowing::NarrowEnv::new());
                let mut typed_stmts = self.declare_nested_functions(stmt_id, &stmts)?;
                typed_stmts.extend(self.block_stmts_with_drain(&stmts, span)?);
                let (inner_narrowings, inner_assigned) = self.pop_narrow_frame_capture()?;
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
                    self.install_joined_narrowings(surviving, span)?;
                }
                Ok(TypedStmtKind::Block(typed_stmts))
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
                // Classes are bound in `signatures()?` and checked in `infer_classes`.
                return Ok(None);
            }
            StmtKind::Import { .. } => {
                // Parser rejects nested imports; this arm is dead code.
                return Ok(None);
            }
            StmtKind::ExportFrom { .. } => {
                // Re-exports never appear in a statement body (parser rejects
                // nested `export`); `collect_exports` handles top-level ones.
                return Ok(None);
            }
            StmtKind::Throw { value } => {
                let error_ty = Type::prelude_error_class();
                let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
                let (typed_value, value_ty) = self.infer_expr(value, Some(&error_ty))?;
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
                Ok(TypedStmtKind::Throw { value: typed_value })
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => self.infer_try(body, catches, finally, span),
            StmtKind::LetPattern { .. }
            | StmtKind::ConstPattern { .. }
            | StmtKind::ForOfPattern { .. } => {
                return Err(super::inference_failure(
                    "destructuring patterns must be lowered before inference",
                ));
            }
            StmtKind::ConstRest {
                name,
                source,
                exclude,
                ty,
                doc,
            } => {
                let hint = ty.as_ref().map(|a| self.resolve_type(a)).transpose()?;
                let (typed_value, source_ty) = self.infer_expr(source, hint.as_ref())?;
                let narrowed = match source_ty.clone() {
                    Type::Object { mut fields, .. } => {
                        for excl in &exclude {
                            fields.remove(&excl.name);
                        }
                        Type::Object {
                            index: None,
                            fields,
                        }
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
                Ok(TypedStmtKind::Const {
                    name,
                    ty: narrowed,
                    value: typed_value,
                    doc,
                })
            }
        })?;
        // Checks a statement makes itself, such as a `return` value against
        // the declared result, meet limits outside any expression.
        if !limit_was_pending {
            self.type_size_checkpoint(Some(span))?;
        }
        Ok(Some(
            self.typed_ast
                .try_push_stmt(TypedStmt {
                    kind: typed_kind,
                    span,
                })
                .map_err(crate::typechecker::arena_failure)?,
        ))
    }

    fn infer_return(
        &mut self,
        value: Option<ExprId>,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let typed_value = match value {
            Some(v) => {
                let hint = self.current_return.clone();
                let (id, value_ty) = self.infer_expr(v, hint.as_ref())?;
                if let Some(collected) = self.inferred_returns.as_mut() {
                    collected.push((value_ty, span));
                }
                if self.reachable {
                    self.validate_type_predicate_return(id, span)?;
                }
                Some(id)
            }
            None if self
                .current_return
                .as_ref()
                .is_some_and(|ret| matches!(ret.peel(), Type::Unknown)) =>
            {
                Some(self.null_return_value(span)?)
            }
            None => {
                if let Some(ret) = &self.current_return
                    && !matches!(ret.peel(), Type::Void | Type::Error)
                {
                    self.error(span, format!("expected `return` value of type `{ret}`"));
                } else if let Some(collected) = self.inferred_returns.as_mut() {
                    // A bare `return` is a `void` return: recording it lets
                    // a value `return` beside it be reported as a conflict.
                    // Under a declared value type it was reported just above.
                    collected.push((Type::Void, span));
                }
                None
            }
        };
        self.reachable = false;
        Ok(TypedStmtKind::Return(typed_value))
    }

    /// The value of a bare `return` under `unknown`, which admits the
    /// `undefined` it yields: `null` stands in for it.
    fn null_return_value(&mut self, span: Span) -> Result<ExprId, CompilerFailure> {
        let null = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Null,
                span,
                ty: Type::Null,
            })
            .map_err(crate::typechecker::arena_failure)?;
        if let Some(collected) = self.inferred_returns.as_mut() {
            collected.push((Type::Null, span));
        }
        Ok(null)
    }

    fn infer_let_statement(
        &mut self,
        name: Ident,
        ty: Option<crate::TypeAnnotation>,
        value: ExprId,
        doc: Option<crate::DocComment>,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let hint = ty.as_ref().map(|a| self.resolve_type(a)).transpose()?;
        let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref())?;
        // A `let` is reassignable, so an inferred literal type would be wrong
        // the moment it is written to: `const a = 1; let b = a;` binds `number`,
        // not `1`. An explicit annotation is honoured as written.
        let bound = hint.unwrap_or_else(|| value_ty.widen_literal());
        let bound = self.pattern_binding_storage_type(value, bound)?;
        // Reject a void binding; poison the slot so codegen never
        // sees a void value-type.
        let bound = if self.reject_void_binding(&bound, span) {
            Type::Error
        } else {
            bound
        };
        self.scopes
            .insert(name.name.clone(), bound.clone(), false, name.span);
        let flow_ty = self.pattern_binding_flow_type(value)?.unwrap_or(value_ty);
        self.narrow_local_initializer(&name, &bound, flow_ty)?;
        Ok(TypedStmtKind::Let {
            name,
            ty: bound,
            value: typed_value,
            boxed: false,
            doc,
        })
    }

    fn infer_const_statement(
        &mut self,
        name: Ident,
        ty: Option<crate::TypeAnnotation>,
        value: ExprId,
        doc: Option<crate::DocComment>,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        // An unannotated `const` bound to a bare literal keeps the literal type,
        // as in TypeScript: the binding cannot be reassigned, so nothing can
        // invalidate it. `let` widens (it is reassignable), and so does any
        // initializer that is not itself a literal.
        let hint = ty
            .as_ref()
            .map(|a| self.resolve_type(a))
            .transpose()?
            .map_or_else(|| literal_type_of(self.ast, value), |ty| Ok(Some(ty)))?;
        let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref())?;
        let bound = hint.unwrap_or_else(|| value_ty.clone());
        let bound = self.pattern_binding_storage_type(value, bound)?;
        // Reject a void binding; poison the slot so codegen never
        // sees a void value-type.
        let bound = if self.reject_void_binding(&bound, span) {
            Type::Error
        } else {
            bound
        };
        self.scopes
            .insert(name.name.clone(), bound.clone(), true, name.span);
        if name.name.starts_with("#pattern_dst_") {
            self.pattern_sources.insert(name.name.clone(), typed_value);
        }
        let flow_ty = self.pattern_binding_flow_type(value)?.unwrap_or(value_ty);
        self.narrow_local_initializer(&name, &bound, flow_ty)?;
        Ok(TypedStmtKind::Const {
            name,
            ty: bound,
            value: typed_value,
            doc,
        })
    }

    fn infer_if_statement(
        &mut self,
        condition: ExprId,
        then_block: StmtId,
        else_block: Option<StmtId>,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let (typed_cond, cond_ty) = self.infer_expr(condition, None)?;
        let cond_span = self
            .ast
            .try_expr(condition)
            .map_err(super::arena_failure)?
            .span;
        self.check_condition_ty(&cond_ty, cond_span);
        let (true_env, false_env) = self.predicate_envs(typed_cond)?;
        let entry_reachable = self.reachable;
        let then_possible = self.condition_can_be(typed_cond, true)?;
        let else_possible = self.condition_can_be(typed_cond, false)?;
        let then_span = self
            .ast
            .try_stmt(then_block)
            .map_err(super::arena_failure)?
            .span;
        self.push_narrow_frame(true_env.clone());
        self.reachable = entry_reachable;
        let typed_then = self.infer_if_branch(then_block)?;
        let then_reachable = self.reachable && then_possible;
        let then_narrowings = self.snapshot_active_narrowings(0).0;
        let (_, then_assigned) = self.pop_narrow_frame_capture()?;
        let typed_then = self.wrap_narrow_regions(typed_then, &true_env, then_span)?;
        let (typed_else, else_narrowings, else_assigned, else_reachable) =
            if let Some(b) = else_block {
                let else_span = self.ast.try_stmt(b).map_err(super::arena_failure)?.span;
                self.push_narrow_frame(false_env.clone());
                self.reachable = entry_reachable;
                let typed_else = self.infer_if_branch(b)?;
                let er = self.reachable && else_possible;
                let en = self.snapshot_active_narrowings(0).0;
                let (_, ea) = self.pop_narrow_frame_capture()?;
                let typed_else = self.wrap_narrow_regions(typed_else, &false_env, else_span)?;
                (Some(typed_else), en, ea, er)
            } else {
                // Implicit-else carries the false-side narrowings so
                // `if (x === null) return;` propagates the non-null
                // narrowing past the `if` when the then-branch is unreachable.
                self.push_narrow_frame(false_env.clone());
                let unchanged = self.snapshot_active_narrowings(0).0;
                self.pop_narrow_frame()?;
                (
                    None,
                    unchanged,
                    std::collections::BTreeSet::new(),
                    entry_reachable && else_possible,
                )
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
        self.install_joined_narrowings(joined_narrowings, span)?;
        Ok(TypedStmtKind::If {
            condition: typed_cond,
            then_block: typed_then,
            else_block: typed_else,
        })
    }

    fn infer_for_of_statement(
        &mut self,
        binding_kind: crate::BindingKind,
        name: Ident,
        ann: Option<crate::TypeAnnotation>,
        iter: ExprId,
        body: StmtId,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let (typed_iter, iter_ty) = self.infer_expr(iter, None)?;
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
                let non_null_iterates = super::narrow_scopes::non_null_form(iter_ty.clone())
                    .is_some_and(|t| self.classify_for_of_source(&t).is_some());
                let culprit =
                    self.nullable_culprit(&[(typed_iter, &iter_ty)], |_| non_null_iterates);
                self.error_with_narrowing_hint(
                            self.ast.try_expr(iter).map_err(super::arena_failure)?.span,
                            format!(
                                "`for-of` requires an array, tuple, string, `Iterator<T>`, or `Iterable<T>`, got `{}`",
                                iter_ty.peel(),
                            ),
                            Vec::new(),
                            culprit,
                        )?;
            }
            (Type::Error, crate::ForOfKind::Array)
        };
        let bound_ty = if let Some(a) = ann.as_ref() {
            let declared = self.resolve_type(a)?;
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
        let body_span = self.ast.try_stmt(body).map_err(super::arena_failure)?.span;
        let (loop_entry, _) = self.snapshot_active_narrowings(0);
        let entry_reachable = self.reachable;
        self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
        let outcome = self.run_loop_body_with_fixed_point(
            body,
            narrowing::NarrowEnv::new(),
            body_span,
            body_scope_floor,
            LoopTail::default(),
        )?;
        let frame = self.pop_pending_join_frame()?;
        self.merge_assigned_into_outer(outcome.assigned.clone(), span);
        // Natural exit always reachable — for-of terminates immediately on empty iterable.
        let has_exit = self.fold_exits_into_outer(
            Some(loop_head_env(&loop_entry, &outcome)),
            frame.breaks,
            body_span,
        )?;
        self.reachable = entry_reachable && has_exit;
        self.scopes.pop();
        Ok(TypedStmtKind::ForOf {
            binding_kind,
            name,
            element_ty: bound_ty,
            iter: typed_iter,
            body: outcome.body,
            kind: for_of_kind,
        })
    }

    fn infer_do_while_statement(
        &mut self,
        body: StmtId,
        condition: ExprId,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        // Condition runs after the body, so no entry narrowing for the body.
        let body_span = self.ast.try_stmt(body).map_err(super::arena_failure)?.span;
        let body_scope_floor = self.scopes.next_scope_id();
        let entry_reachable = self.reachable;
        self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
        let outcome = self.run_loop_body_with_fixed_point(
            body,
            narrowing::NarrowEnv::new(),
            body_span,
            body_scope_floor,
            LoopTail {
                update: None,
                condition: Some(condition),
            },
        )?;
        let frame = self.pop_pending_join_frame()?;
        self.merge_assigned_into_outer(outcome.assigned.clone(), span);
        let (typed_cond, exit) = self.check_condition_at_loop_head(
            condition,
            &LoopHead::after_every_pass(&outcome),
            body_span,
        )?;
        let natural = exit.filter(|_| outcome.reaches_back_edge);
        let has_exit = self.fold_exits_into_outer(natural, frame.breaks, body_span)?;
        self.reachable = entry_reachable && has_exit;
        Ok(TypedStmtKind::DoWhile {
            body: outcome.body,
            condition: typed_cond,
        })
    }

    fn infer_try(
        &mut self,
        body: StmtId,
        catches: Vec<crate::CatchClause>,
        finally: Option<StmtId>,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let entry_reachable = self.reachable;
        let pending_start = self.pending_exit_counts();
        let super_seen_before = self.super_seen;
        let body_outcome =
            self.infer_isolated_clause(body, entry_reachable, &Default::default())?;
        // A `catch` or `finally` also runs when a `super(...)` before it in the
        // `try` throws, so it counts as before the call. (Only the
        // constructor's own call sets `super_seen`.)
        let prev_super_handler = self.in_super_handler;
        self.in_super_handler |= !super_seen_before && self.super_seen;
        let typed_body = body_outcome
            .body
            .ok_or_else(|| super::inference_failure("try body is a block"))?;
        let mut exits = body_outcome.exit.into_iter().collect::<Vec<_>>();
        let body_assigned = body_outcome.all_writes;
        let mut all_assigned = body_assigned.clone();

        let mut typed_catches = Vec::with_capacity(catches.len());
        // Valid prior arms for the shadow check: (class, display name, anchor span).
        // Arms with an erroneous annotation stay out on both sides to
        // avoid cascading unreachable-arm diagnostics.
        let mut prior: Vec<(crate::MangledName, String, Span)> = Vec::new();
        for clause in catches {
            let clause_ty = self.infer_catch_type(&clause, &mut prior)?;
            self.scopes.push();
            self.scopes.insert(
                clause.binding.name.clone(),
                clause_ty.clone(),
                true,
                clause.binding.span,
            );
            let outcome =
                self.infer_isolated_clause(clause.body, entry_reachable, &body_assigned)?;
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
        // A `super(...)` in a `catch` can throw too, so the `finally` after it
        // counts as before the call as well.
        self.in_super_handler |= !super_seen_before && self.super_seen;
        let typed_finally = finally
            .map(|f| {
                let pending_end = self.pending_exit_counts();
                let outcome = self.infer_isolated_clause(f, entry_reachable, &all_assigned)?;
                self.apply_finally_to_pending(&pending_start, &pending_end, &outcome)?;
                apply_finally_to_exit(&mut post, &outcome);
                all_assigned.extend(outcome.all_writes);
                Ok::<_, CompilerFailure>(outcome.body)
            })
            .transpose()?
            .flatten();
        self.in_super_handler = prev_super_handler;
        self.reachable = entry_reachable && post.is_some();
        self.merge_assigned_into_outer(all_assigned, span);
        if let Some(post) = post {
            self.install_joined_narrowings(post, span)?;
        }

        Ok(TypedStmtKind::Try {
            body: typed_body,
            catches: typed_catches,
            finally: typed_finally,
        })
    }

    fn infer_catch_type(
        &mut self,
        clause: &crate::CatchClause,
        prior: &mut Vec<(crate::MangledName, String, Span)>,
    ) -> Result<Type, CompilerFailure> {
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
            let ty = self.resolve_runtime_class_test(annotation)?;
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

        Ok(clause_ty)
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
    ) -> Result<(), CompilerFailure> {
        if self.pending_joins.len() != starts.len() || starts.len() != ends.len() {
            return Err(super::inference_failure(
                "finally pending-frame snapshot lengths differ",
            ));
        }
        for ((frame, start), end) in self.pending_joins.iter_mut().zip(starts).zip(ends) {
            apply_finally_to_transfers(&mut frame.breaks, start.0..end.0, outcome)?;
            apply_finally_to_transfers(&mut frame.continues, start.1..end.1, outcome)?;
        }
        Ok(())
    }

    fn infer_if_branch(&mut self, body: StmtId) -> Result<StmtId, CompilerFailure> {
        let pending = std::mem::take(&mut self.pending_post_if_materializations);
        let typed = self.infer_stmt(body)?.ok_or_else(|| {
            super::inference_failure("if-branch is a Block, never a type-only decl")
        })?;
        // Block exits export views for their following statements. A branch has
        // no such continuation: the if join rematerializes its surviving views.
        // Keep these exports out of the sibling branch and its return paths.
        self.pending_post_if_materializations = pending;
        Ok(typed)
    }

    /// Field views are lazy reads, so register them around the statement that
    /// creates them as well as its continuation. An assignment expression can
    /// use its new view later in the same statement, including inside a branch.
    /// Does NOT push/pop scopes — the `Block` arm owns those.
    fn block_stmts_with_drain(
        &mut self,
        stmts: &[StmtId],
        outer_span: Span,
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let mut typed_stmts: Vec<StmtId> = Vec::new();
        for (i, &stmt) in stmts.iter().enumerate() {
            let mut typed: Vec<StmtId> = self.infer_stmt(stmt)?.into_iter().collect();
            typed.extend(self.define_nested_functions_after(stmt)?);
            let pending = std::mem::take(&mut self.pending_post_if_materializations);
            if !pending.is_empty() {
                let mut tail = typed;
                tail.extend(self.block_stmts_with_drain(&stmts[i + 1..], outer_span)?);
                let tail_block = self
                    .typed_ast
                    .try_push_stmt(TypedStmt {
                        kind: TypedStmtKind::Block(tail),
                        span: outer_span,
                    })
                    .map_err(crate::typechecker::arena_failure)?;
                let wrapped = self.wrap_pending_materializations(tail_block, pending)?;
                typed_stmts.push(wrapped);
                break;
            }
            typed_stmts.extend(typed);
        }
        Ok(typed_stmts)
    }

    /// `ClassName.member = …` / `+= …` / `++` — resolved before the receiver is
    /// typed, because a bare class name is not a value and `infer_expr` would
    /// report that instead of the real problem. Every write form shares this one
    /// resolver so their diagnostics stay identical.
    pub(super) fn resolve_static_field_write(
        &mut self,
        receiver: ExprId,
        name: &Ident,
    ) -> Result<StaticWrite, CompilerFailure> {
        let ExprKind::Identifier(recv_ident) = &self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .kind
            .clone()
        else {
            return Ok(StaticWrite::NotClassName);
        };
        if self.scopes.get(&recv_ident.name).is_some()
            || self.top_symbols.contains_key(&recv_ident.name)
        {
            return Ok(StaticWrite::NotClassName);
        }
        let Some((class_name, class_mangled)) =
            self.lookup_named_type(&recv_ident.name).and_then(|sym| {
                matches!(sym.kind, crate::TypeKind::Class { .. })
                    .then(|| (recv_ident.name.clone(), sym.mangled_name.clone()))
            })
        else {
            return Ok(StaticWrite::NotClassName);
        };
        let receiver = recv_ident.clone();
        Ok(
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
                        return Ok(StaticWrite::Rejected { receiver });
                    }
                    self.note_rebindable_static(&field, &owner, &name.name);
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
            },
        )
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
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let lhs_path =
            narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled.clone()));
        let (synth_lhs, lhs_ty) = if let Some(view) = self.lookup_narrowed_view(&lhs_path) {
            let binding = view.binding.clone();
            let narrowed_ty = view.narrowed_ty.clone();
            let id = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::LocalNarrowRef {
                        binding,
                        path: lhs_path,
                    },
                    span: ident.span,
                    ty: narrowed_ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            (id, narrowed_ty)
        } else {
            let id = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::GlobalRef {
                        mangled: mangled.clone(),
                        name: ident.clone(),
                    },
                    span: ident.span,
                    ty: ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            (id, ty.clone())
        };
        let (typed_value, value_ty) = self.infer_expr(value, Some(&lhs_ty))?;
        let result_ty =
            self.check_compound_arith(op, (synth_lhs, &lhs_ty), (typed_value, &value_ty), op_span)?;
        let synth_binary = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op,
                    lhs: synth_lhs,
                    rhs: typed_value,
                },
                span,
                ty: result_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        self.renarrow_global_after_write(&ident, &mangled, &ty, result_ty)?;
        Ok(TypedStmtKind::AssignGlobal {
            ident,
            mangled,
            target_ty: ty,
            value: synth_binary,
        })
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
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if matches!(written_ty, Type::Error) {
            return Ok(());
        }
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled.clone()));
        let written_ty = self.assignment_narrowed_ty(declared_ty, written_ty);
        if written_ty == *declared_ty {
            self.invalidate_for_reassignment(path, ident.span);
            return Ok(());
        }
        self.install_assignment_narrowing(path, ident.clone(), written_ty, ident.span)?;
        Ok(())
    }

    /// Poison for a rejected class-name write: the diagnostic is already
    /// reported, but the RHS still needs inferring so nested errors surface.
    fn poisoned_static_field_write(
        &mut self,
        receiver: Ident,
        recv_span: Span,
        name: &Ident,
        value: ExprId,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let typed_receiver = self
            .typed_ast
            .try_push_expr(crate::TypedExpr {
                kind: TypedExprKind::LocalRef {
                    ident: receiver,
                    boxed: false,
                },
                span: recv_span,
                ty: Type::Error,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let (typed_value, _) = self.infer_expr(value, None)?;
        Ok(TypedStmtKind::AssignField {
            receiver: typed_receiver,
            name: name.clone(),
            value: typed_value,
        })
    }

    pub(super) fn infer_assign_field(
        &mut self,
        receiver: ExprId,
        name: Ident,
        value: ExprId,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
        match self.resolve_static_field_write(receiver, &name)? {
            StaticWrite::NotClassName => {}
            StaticWrite::Rejected { receiver } => {
                return self.poisoned_static_field_write(receiver, recv_span, &name, value);
            }
            StaticWrite::Resolved { mangled, ty } => {
                let (typed_value, value_ty) = self.infer_expr(value, Some(&ty))?;
                if !assignable(&value_ty, &ty, self.resolver()) {
                    self.error(value_span, format!("expected `{ty}`, got `{value_ty}`"));
                }
                return Ok(TypedStmtKind::AssignGlobal {
                    ident: name,
                    mangled,
                    target_ty: ty,
                    value: typed_value,
                });
            }
        }
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        // Compute target path before RHS inference: write invalidates after the RHS
        // is evaluated so `obj.foo = obj.foo + 1` still reads the narrowed shadow.
        let target_path = self
            .expr_to_reference_path(
                self.typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?,
            )?
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
        let placeholder =
            |inferer: &mut Self, ty_hint: Option<&Type>| -> Result<_, CompilerFailure> {
                let (typed_value, _) = inferer.infer_expr(value, ty_hint)?;
                Ok(TypedStmtKind::AssignField {
                    receiver: typed_receiver,
                    name: name.clone(),
                    value: typed_value,
                })
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
                    if field.readonly && !self.readonly_write_allowed(receiver, &decl_mangled)? {
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
                let (typed_value, value_ty, reported) =
                    self.infer_assigned_value(value, Some(&field_ty))?;
                if !reported && !assignable(&value_ty, &field_ty, self.resolver()) {
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
                placeholder(self, None)?
            }
        } else if self.try_report_method_assignment(name.span, &receiver_ty, &name.name) {
            // Every non-class receiver whose member is a method: an interface
            // surfaces it as a readonly property of function type, a structural
            // one as no field at all, and neither says the member is a method.
            // The RHS gets no hint — hinting the method's own signature adds a
            // second error blaming the value for not being that function.
            placeholder(self, None)?
        } else if let Some((prop_sig, _, _, _)) = self.find_property(&receiver_ty, &name.name) {
            if prop_sig.readonly {
                self.error(
                    name.span,
                    format!(
                        "cannot assign to readonly property `{}` on `{}`",
                        name.name, receiver_ty,
                    ),
                );
                placeholder(self, Some(&prop_sig.ty))?
            } else {
                // Optional property widens to `T | null` on the write side.
                let field_ty = if prop_sig.optional {
                    Type::union(vec![prop_sig.ty.clone(), Type::Null])
                } else {
                    prop_sig.ty.clone()
                };
                let (typed_value, value_ty, reported) =
                    self.infer_assigned_value(value, Some(&field_ty))?;
                if !reported && !assignable(&value_ty, &field_ty, self.resolver()) {
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
            placeholder(self, None)?
        } else if let Some(fields) = self.assignment_target_fields(&receiver_ty) {
            let field_lookup = fields.get(&name.name).cloned().or_else(|| {
                self.resolver()
                    .index_signature(&receiver_ty)
                    .map(|i| crate::ObjectField {
                        ty: *i.value,
                        optional: false,
                        readonly: i.readonly,
                    })
            });
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
                let (typed_value, value_ty, reported) =
                    self.infer_assigned_value(value, Some(&field_ty))?;
                if !reported && !assignable(&value_ty, &field_ty, self.resolver()) {
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
                placeholder(self, None)?
            }
        } else {
            let recv_path = self.expr_to_reference_path(
                self.typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?,
            )?;
            self.report_unassignable_field_target(
                recv_span,
                &name,
                &receiver_ty,
                recv_path.as_ref(),
                None,
            );
            let target_ty = self.write_target_ty(&receiver_ty, &name.name);
            placeholder(self, target_ty.as_ref())?
        };
        if let Some(path) = target_path {
            self.invalidate_for_write(path.clone(), name.span);
            if let TypedStmtKind::AssignField { value, .. } = &result {
                self.narrow_field_after_write(path, typed_receiver, &receiver_ty, &name, *value)?;
            }
        }
        Ok(result)
    }

    fn narrow_field_after_write(
        &mut self,
        path: narrowing::ReferencePath,
        receiver: ExprId,
        receiver_ty: &Type,
        name: &Ident,
        value: ExprId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let kind = TypedExprKind::FieldAccess {
            receiver,
            name: name.clone(),
        };
        if self.kind_to_reference_path(&kind)?.is_none()
            || self.path_root_is_captured_mutator(&path)
        {
            return Ok(());
        }
        let Some(declared) = self.write_target_ty(receiver_ty, &name.name) else {
            return Ok(());
        };
        let written = self
            .typed_ast
            .try_expr(value)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .clone();
        if matches!(written, Type::Error) || !assignable(&written, &declared, self.resolver()) {
            return Ok(());
        }
        let narrowed_ty = self.assignment_narrowed_ty(&declared, written);
        if narrowed_ty == declared {
            return Ok(());
        }
        let source = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind,
                span: name.span,
                ty: declared,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let view = narrowing::NarrowedView {
            narrowed_ty,
            facts: narrowing::TypeFacts::EMPTY,
            excluded_literals: Default::default(),
            binding: self.mint_narrow_binding(name.span)?,
            source,
        };
        self.install_joined_narrowings([(path, view)].into_iter().collect(), name.span)?;
        Ok(())
    }

    pub(super) fn infer_assign_index(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        value: ExprId,
        _span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        let object = receiver_ty.is_structural_object();
        let (typed_index, key_ty) = if object {
            self.infer_object_key(index)?
        } else {
            self.infer_expr(index, Some(&Type::Number))?
        };
        let targets = if object {
            self.object_index_write_targets(
                &receiver_ty,
                &key_ty,
                self.ast.try_expr(index).map_err(super::arena_failure)?.span,
            )
        } else {
            vec![self.indexed_write_elem_ty(&receiver_ty, recv_span, recv_span)]
        };
        let elem_ty = self.common_index_write_type(&targets);
        let hint = (!matches!(elem_ty, Type::Never)).then_some(&elem_ty);
        let (typed_value, value_ty, reported) = self.infer_assigned_value(value, hint)?;
        if !reported
            && !matches!(value_ty, Type::Error)
            && let Some(target) = targets.iter().find(|target| {
                !matches!(target, Type::Error) && !assignable(&value_ty, target, self.resolver())
            })
        {
            let help = super::type_diff::type_mismatch_help(target, &value_ty);
            self.error_with_help(
                value_span,
                format!("expected `{target}`, got `{value_ty}`"),
                help,
            );
        }
        self.invalidate_index_write(
            typed_receiver,
            typed_index,
            self.ast.try_expr(index).map_err(super::arena_failure)?.span,
        )?;
        Ok(TypedStmtKind::AssignIndex {
            receiver: typed_receiver,
            index: typed_index,
            value: typed_value,
            elem_ty,
        })
    }

    /// The element type a write through `receiver_ty[i]` stores, or `Type::Error`
    /// after reporting why the receiver cannot be written through. Shared by plain,
    /// compound, and postfix index writes so all three reject the same receivers.
    /// `fallback_span` is where a receiver of the wrong kind altogether is reported.
    pub(super) fn indexed_write_elem_ty(
        &mut self,
        receiver_ty: &Type,
        recv_span: Span,
        fallback_span: Span,
    ) -> Type {
        if receiver_ty.is_readonly_array() {
            let copy_help = if matches!(receiver_ty.peel(), Type::Tuple(_)) {
                "reconstruct the tuple with the updated element"
            } else {
                "copy it first (`[...xs]` or `xs.slice()`) and write to the copy, \
                 or drop `readonly` from the declared type"
            };
            self.error_with_help(
                recv_span,
                format!("cannot assign to an element of `{receiver_ty}`"),
                vec![
                    "a `readonly` array or tuple only permits reading".to_string(),
                    copy_help.to_string(),
                ],
            );
            return Type::Error;
        }
        // Peel to match the read path: an alias of an array or `Uint8Array` is
        // assignable on the same terms as the type it names.
        match receiver_ty.peel() {
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
                let help = vec![self.format_definition(receiver_ty)];
                self.error_with_help(
                    fallback_span,
                    format!("cannot assign to index of `{receiver_ty}`"),
                    help,
                );
                Type::Error
            }
        }
    }

    fn pattern_binding_storage_type(
        &self,
        value: ExprId,
        declared: Type,
    ) -> Result<Type, CompilerFailure> {
        let Some((source, element)) = self.pattern_binding_source(value)? else {
            return Ok(declared);
        };
        Ok(self.pattern_source_view(source, &element)?.map_or_else(
            || declared.clone(),
            |view| self.initializer_narrowed_ty(&declared, view),
        ))
    }

    fn pattern_binding_flow_type(&self, value: ExprId) -> Result<Option<Type>, CompilerFailure> {
        use narrowing::{LiteralValue, PathElem};
        let Some((source, element)) = self.pattern_binding_source(value)? else {
            return Ok(None);
        };
        if let Some(narrowed) = self.pattern_source_view(source, &element)? {
            return Ok(Some(narrowed));
        }
        Ok(match element {
            // Destructuring snapshots a getter's result; its declared read type
            // is usable even though repeated getter reads cannot be narrowed.
            PathElem::Field(field) => self.narrow_source_field_ty(&source.ty, &field),
            PathElem::Index(LiteralValue::Number(index)) => {
                Self::pattern_index_flow_type(&source.ty, index.0 as usize)
            }
            _ => None,
        })
    }

    fn pattern_binding_source(
        &self,
        value: ExprId,
    ) -> Result<Option<(&TypedExpr, narrowing::PathElem)>, CompilerFailure> {
        use narrowing::{LiteralValue, PathElem};
        let (receiver, element) =
            match &self.ast.try_expr(value).map_err(super::arena_failure)?.kind {
                ExprKind::FieldAccess { receiver, name } => {
                    (*receiver, PathElem::Field(name.name.clone()))
                }
                ExprKind::IndexAccess { receiver, index } => {
                    let ExprKind::Number(index) = self
                        .ast
                        .try_expr(*index)
                        .map_err(super::arena_failure)?
                        .kind
                    else {
                        return Ok(None);
                    };
                    (
                        *receiver,
                        PathElem::Index(LiteralValue::Number(crate::types::LiteralF64(index))),
                    )
                }
                _ => return Ok(None),
            };
        let ExprKind::Identifier(source) = &self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .kind
        else {
            return Ok(None);
        };
        let source = self
            .typed_ast
            .try_expr(*match self.pattern_sources.get(&source.name) {
                Some(value) => value,
                None => return Ok(None),
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(Some((source, element)))
    }

    fn pattern_source_view(
        &self,
        source: &TypedExpr,
        element: &narrowing::PathElem,
    ) -> Result<Option<Type>, crate::compiler_error::CompilerFailure> {
        if let narrowing::PathElem::Field(field) = element
            && self.type_has_getter(&source.ty, field)
        {
            return Ok(None);
        }
        let Some(mut path) = self.expr_to_reference_path(source)? else {
            return Ok(None);
        };
        path.chain.push(element.clone());
        Ok(self
            .lookup_narrowed_view(&path)
            .map(|view| view.narrowed_ty.clone()))
    }

    fn pattern_index_flow_type(source: &Type, index: usize) -> Option<Type> {
        match source.peel() {
            Type::Tuple(elems) => elems.get(index).cloned(),
            Type::Array(elem) => Some((**elem).clone()),
            Type::Union(members) => members
                .iter()
                .map(|member| Self::pattern_index_flow_type(member, index))
                .collect::<Option<Vec<_>>>()
                .map(Type::union),
            _ => None,
        }
    }

    /// Keep the declared storage type while a union initializer establishes its
    /// current member. Non-union annotations still define the object's surface.
    fn narrow_local_initializer(
        &mut self,
        name: &Ident,
        declared: &Type,
        value: Type,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if !matches!(declared.peel(), Type::Union(_))
            || matches!(value, Type::Error)
            || value == *declared
        {
            return Ok(());
        }
        let scope = self
            .scopes
            .get(&name.name)
            .ok_or_else(|| super::inference_failure("binding just inserted"))?
            .decl_scope;
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
            name: name.name.clone(),
            decl_scope: scope,
        });
        if self.path_root_is_captured_mutator(&path) {
            return Ok(());
        }
        let narrowed = self.initializer_narrowed_ty(declared, value);
        self.renarrow_local_after_write(name, scope, declared, narrowed)?;
        Ok(())
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
    ) -> Result<Option<Type>, crate::compiler_error::CompilerFailure> {
        // A poisoned RHS leaves the narrowing exactly as it was: the diagnostic for
        // whatever went wrong upstream already stands, and adding "re-narrow after
        // the reassignment" on top of it points at an edit that fixes nothing.
        if matches!(written_ty, Type::Error) {
            return Ok(None);
        }
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
            name: target.name.clone(),
            decl_scope,
        });
        let written_ty = self.assignment_narrowed_ty(declared_ty, written_ty);
        if written_ty == *declared_ty {
            self.invalidate_for_reassignment(path, target.span);
            return Ok(None);
        }
        self.install_assignment_narrowing(path, target.clone(), written_ty.clone(), target.span)?;
        Ok(Some(written_ty))
    }

    /// Infer an assigned value against its target's type, and whether that
    /// reported an error. The caller still checks assignability, since
    /// `infer_expr` skips hints holding type variables, but an error already
    /// reported covers the value.
    fn infer_assigned_value(
        &mut self,
        value: ExprId,
        target_ty: Option<&Type>,
    ) -> Result<(ExprId, Type, bool), CompilerFailure> {
        let errors_before = self.error_count();
        let (typed_value, value_ty) = self.infer_expr(value, target_ty)?;
        Ok((typed_value, value_ty, self.error_count() > errors_before))
    }

    pub(super) fn infer_assign(
        &mut self,
        target: Ident,
        value: ExprId,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        if let Some(entry) = self.scopes.get(&target.name).cloned() {
            if entry.is_const {
                self.report_const_local_write(&target, &entry);
            }
            let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
            // Reassigning a const is already an error. Hinting the declared type would
            // stack a second `expected X, got Y` on top of it — and now always would,
            // since an unannotated const's type is its own initializer's literal, which
            // no new value can match. Infer unhinted; nested errors still surface.
            // Paired with the `is_const` guard on the re-check below: both must stay.
            let hint = (!entry.is_const).then(|| entry.ty.clone());
            let (typed_value, value_ty, reported) =
                self.infer_assigned_value(value, hint.as_ref())?;
            if !entry.is_const && !reported && !assignable(&value_ty, &entry.ty, self.resolver()) {
                self.error(
                    value_span,
                    format!("expected `{}`, got `{}`", entry.ty, value_ty),
                );
            }
            let narrowed_shadow_ty =
                self.renarrow_local_after_write(&target, entry.decl_scope, &entry.ty, value_ty)?;
            return Ok(TypedStmtKind::AssignLocal {
                ident: target,
                target_ty: entry.ty.clone(),
                value: typed_value,
                boxed: false,
                narrowed_shadow_ty,
            });
        }
        Ok(if let Some(entry) = self.top_symbols.get(&target.name) {
            let kind_clone = entry.kind.clone();
            let prev_span = entry.declaration_span;
            let mangled = entry.mangled_name.clone();
            match kind_clone {
                ValueKind::Let { ty, .. } => {
                    let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
                    let (typed_value, value_ty, reported) =
                        self.infer_assigned_value(value, Some(&ty))?;
                    if !reported && !assignable(&value_ty, &ty, self.resolver()) {
                        self.error(value_span, format!("expected `{ty}`, got `{value_ty}`"));
                    }
                    self.renarrow_global_after_write(&target, &mangled, &ty, value_ty)?;
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
                    // Unhinted, as in the block-scoped case above: the declared type is
                    // the initializer's literal, so hinting it would add a redundant
                    // `expected X, got Y` beneath the reassignment error.
                    let (typed_value, _) = self.infer_expr(value, None)?;
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
                    let (typed_value, _) = self.infer_expr(value, None)?;
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
            let (typed_value, _) = self.infer_expr(value, None)?;
            // Placeholder; downstream Type::Error suppression handles cascades.
            TypedStmtKind::AssignLocal {
                ident: target,
                target_ty: Type::Error,
                value: typed_value,
                boxed: false,
                narrowed_shadow_ty: None,
            }
        })
    }

    /// `x += y` lowers to `x = x + y`; the synthesized LHS reads through any active
    /// narrowing so the binary type rule sees the narrowed view.
    pub(super) fn infer_compound_assign(
        &mut self,
        target: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        if let Some(entry) = self.scopes.get(&target.name).cloned() {
            if entry.is_const {
                self.report_const_local_write(&target, &entry);
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
                let id = self
                    .typed_ast
                    .try_push_expr(TypedExpr {
                        kind: TypedExprKind::LocalNarrowRef {
                            binding,
                            path: lhs_path,
                        },
                        span: target.span,
                        ty: narrowed_ty.clone(),
                    })
                    .map_err(crate::typechecker::arena_failure)?;
                (id, narrowed_ty)
            } else {
                let id = self
                    .typed_ast
                    .try_push_expr(TypedExpr {
                        kind: TypedExprKind::LocalRef {
                            ident: target.clone(),
                            boxed: false,
                        },
                        span: target.span,
                        ty: target_ty.clone(),
                    })
                    .map_err(crate::typechecker::arena_failure)?;
                (id, target_ty.clone())
            };
            let (typed_value, value_ty) = self.infer_expr(value, Some(&lhs_ty))?;
            let result_ty = self.check_compound_arith(
                op,
                (synth_lhs, &lhs_ty),
                (typed_value, &value_ty),
                op_span,
            )?;
            // Re-check assignability: catches literal-refined slots (e.g. `1|2|3`)
            // where arithmetic widens the result to `number`.
            let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
            if !matches!(result_ty, Type::Error)
                && !matches!(target_ty, Type::Error)
                && !assignable(&result_ty, &target_ty, self.resolver())
            {
                self.error(
                    value_span,
                    format!("expected `{target_ty}`, got `{result_ty}`"),
                );
            }
            let synth_binary = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Binary {
                        op,
                        lhs: synth_lhs,
                        rhs: typed_value,
                    },
                    span,
                    ty: result_ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
            let narrowed_shadow_ty =
                self.renarrow_local_after_write(&target, entry.decl_scope, &target_ty, result_ty)?;
            return Ok(TypedStmtKind::AssignLocal {
                ident: target,
                target_ty,
                value: synth_binary,
                boxed: false,
                narrowed_shadow_ty,
            });
        }
        Ok(if let Some(entry) = self.top_symbols.get(&target.name) {
            let kind_clone = entry.kind.clone();
            let prev_span = entry.declaration_span;
            let mangled = entry.mangled_name.clone();
            match kind_clone {
                ValueKind::Let { ty, .. } => {
                    self.compound_assign_global(target, mangled, ty, op, op_span, value, span)?
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
                    let (typed_value, _) = self.infer_expr(value, Some(&ty))?;
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
                    let (typed_value, _) = self.infer_expr(value, None)?;
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
            let (typed_value, _) = self.infer_expr(value, None)?;
            TypedStmtKind::AssignLocal {
                ident: target,
                target_ty: Type::Error,
                value: typed_value,
                boxed: false,
                narrowed_shadow_ty: None,
            }
        })
    }

    pub(super) fn infer_compound_assign_field(
        &mut self,
        receiver: ExprId,
        name: Ident,
        op: BinOp,
        op_span: Span,
        value: ExprId,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
        let stmt_span = Span {
            file: recv_span.file,
            start: recv_span.start.min(value_span.start),
            end: recv_span.end.max(value_span.end),
        };
        match self.resolve_static_field_write(receiver, &name)? {
            StaticWrite::NotClassName => {}
            StaticWrite::Rejected { receiver } => {
                return self.poisoned_static_field_write(receiver, recv_span, &name, value);
            }
            StaticWrite::Resolved { mangled, ty } => {
                return self
                    .compound_assign_global(name, mangled, ty, op, op_span, value, stmt_span);
            }
        }
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let rw_op = super::diagnostics::RwOp::Compound(op);
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        let target_path = self
            .expr_to_reference_path(
                self.typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?,
            )?
            .map(|mut p| {
                p.chain
                    .push(super::narrowing::PathElem::Field(name.name.clone()));
                p
            });
        let placeholder = |inferer: &mut Self| -> Result<_, CompilerFailure> {
            let (typed_value, _) = inferer.infer_expr(value, None)?;
            Ok(TypedStmtKind::AssignField {
                receiver: typed_receiver,
                name: name.clone(),
                value: typed_value,
            })
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
            )? {
                Some(rw) => self.build_compound_assign_field(
                    typed_receiver,
                    name.clone(),
                    op,
                    op_span,
                    value,
                    &rw,
                    stmt_span,
                )?,
                None => placeholder(self)?,
            }
        } else if self.try_report_method_assignment(name.span, &receiver_ty, &name.name) {
            placeholder(self)?
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
                placeholder(self)?
            } else {
                self.build_compound_assign_field(
                    typed_receiver,
                    name.clone(),
                    op,
                    op_span,
                    value,
                    &FieldRw::uniform(prop_sig.ty.clone()),
                    stmt_span,
                )?
            }
        } else if matches!(receiver_ty, Type::Error) {
            placeholder(self)?
        } else if let Some(fields) = self.assignment_target_fields(&receiver_ty) {
            if let Some(field) = fields.get(&name.name).cloned().or_else(|| {
                self.resolver()
                    .index_signature(&receiver_ty)
                    .map(|index| crate::ObjectField {
                        ty: *index.value,
                        optional: true,
                        readonly: index.readonly,
                    })
            }) {
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
                    placeholder(self)?
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
                    )?
                }
            } else {
                let help = self.definition_help(&receiver_ty);
                self.error_with_help(
                    name.span,
                    format!("no field `{}` on type `{}`", name.name, receiver_ty),
                    help,
                );
                placeholder(self)?
            }
        } else {
            let recv_path = self.expr_to_reference_path(
                self.typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?,
            )?;
            self.report_unassignable_field_target(
                recv_span,
                &name,
                &receiver_ty,
                recv_path.as_ref(),
                Some(rw_op),
            );
            placeholder(self)?
        };
        if let Some(path) = target_path {
            self.invalidate_for_write(path, name.span);
        }
        Ok(result)
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
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
        let (typed_value, value_ty) = self.infer_expr(value, Some(&rw.read))?;
        // Built before the operator check so the check can name it as the
        // narrowing culprit.
        let synth_lhs = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::FieldAccess {
                    receiver: typed_receiver,
                    name: name.clone(),
                },
                span: name.span,
                ty: rw.read.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let result_ty = self.check_compound_arith(
            op,
            (synth_lhs, &rw.read),
            (typed_value, &value_ty),
            op_span,
        )?;
        if !matches!(result_ty, Type::Error)
            && !matches!(rw.write, Type::Error)
            && !assignable(&result_ty, &rw.write, self.resolver())
        {
            self.error(
                value_span,
                format!("expected `{}`, got `{result_ty}`", rw.write),
            );
        }
        let synth_binary = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op,
                    lhs: synth_lhs,
                    rhs: typed_value,
                },
                span: stmt_span,
                ty: result_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(TypedStmtKind::AssignField {
            receiver: typed_receiver,
            name,
            value: synth_binary,
        })
    }

    pub(super) fn infer_compound_assign_index(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        op: BinOp,
        op_span: Span,
        value: ExprId,
        _span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let value_span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
        let stmt_span = Span {
            file: recv_span.file,
            start: recv_span.start.min(value_span.start),
            end: recv_span.end.max(value_span.end),
        };
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        let object = receiver_ty.is_structural_object();
        let (typed_index, key_ty) = if object {
            self.infer_object_key(index)?
        } else {
            self.infer_expr(index, Some(&Type::Number))?
        };
        let elem_ty = if object {
            self.object_index_write_type(
                &receiver_ty,
                &key_ty,
                self.ast.try_expr(index).map_err(super::arena_failure)?.span,
            )
        } else {
            self.indexed_write_elem_ty(&receiver_ty, recv_span, recv_span)
        };
        let declared_read = if object {
            self.object_index_read_type(
                &receiver_ty,
                &key_ty,
                self.ast.try_expr(index).map_err(super::arena_failure)?.span,
            )
        } else {
            elem_ty.clone()
        };
        let read_ty = self.index_read_ty(typed_receiver, typed_index, &declared_read)?;
        let (typed_value, value_ty) = self.infer_expr(value, Some(&elem_ty))?;
        // Built before the operator check so the check can name it as the
        // narrowing culprit.
        let synth_lhs = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::IndexAccess {
                    receiver: typed_receiver,
                    index: typed_index,
                },
                span: recv_span,
                ty: read_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let result_ty = self.check_compound_arith(
            op,
            (synth_lhs, &read_ty),
            (typed_value, &value_ty),
            op_span,
        )?;
        if !matches!(result_ty, Type::Error)
            && !matches!(elem_ty, Type::Error)
            && !assignable(&result_ty, &elem_ty, self.resolver())
        {
            self.error(
                value_span,
                format!("expected `{elem_ty}`, got `{result_ty}`"),
            );
        }
        let synth_binary = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op,
                    lhs: synth_lhs,
                    rhs: typed_value,
                },
                span: stmt_span,
                ty: result_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        self.invalidate_index_write(
            typed_receiver,
            typed_index,
            self.ast.try_expr(index).map_err(super::arena_failure)?.span,
        )?;
        Ok(TypedStmtKind::AssignIndex {
            receiver: typed_receiver,
            index: typed_index,
            value: synth_binary,
            elem_ty,
        })
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
    ) -> Result<Type, crate::compiler_error::CompilerFailure> {
        let (lt, rt) = (lhs.1, rhs.1);
        if let Some(ty) = compound_arith_result(op, lt, rt) {
            return Ok(ty);
        }
        let sym = binary_op_text(op);
        let culprit = self
            .nullable_binary_culprit(lhs, rhs, |l, r| compound_arith_result(op, l, r).is_some());
        self.error_with_narrowing_hint(
            op_span,
            format!("`{sym}=` not defined for `{lt}` and `{rt}`"),
            Vec::new(),
            culprit,
        )?;
        Ok(Type::Error)
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
        BinOp::Eq => "===",
        BinOp::NotEq => "!==",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::In => "in",
        BinOp::NullishCoalesce => "??",
    }
}

/// The operator's own accept rule, shared by the result computation, the
/// narrowing-culprit probe, and the nullable read-modify-write target check: each
/// must ask exactly what the failure asked, or it recommends a fix that doesn't
/// apply to the site.
pub(super) fn compound_arith_result(op: BinOp, lt: &Type, rt: &Type) -> Option<Type> {
    if matches!(lt.peel(), Type::Error) || matches!(rt.peel(), Type::Error) {
        return Some(Type::Error);
    }
    match op {
        BinOp::Add => super::expr::plus_result(lt, rt),
        BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Pow => {
            super::expr::arithmetic_result(lt, rt)
        }
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
) -> Result<(), CompilerFailure> {
    if transfers.get(range.clone()).is_none() {
        return Err(super::inference_failure(
            "finally transfer snapshot range is invalid",
        ));
    }
    if outcome.exit.is_none() {
        transfers.drain(range);
        return Ok(());
    }
    for (env, assigned) in transfers
        .get_mut(range)
        .ok_or_else(|| super::inference_failure("finally transfer range changed"))?
    {
        let mut post = Some(std::mem::take(env));
        apply_finally_to_exit(&mut post, outcome);
        *env =
            post.ok_or_else(|| super::inference_failure("reachable finally lost its transfer"))?;
        assigned.extend(outcome.all_writes.iter().cloned());
    }
    Ok(())
}

/// What runs on a loop's back edge, after the body and before its next pass: a
/// `for` update, then a `while`, `for`, or `do … while` condition.
#[derive(Clone, Copy, Default)]
pub(super) struct LoopTail {
    pub update: Option<StmtId>,
    pub condition: Option<ExprId>,
}

/// A body pass, and the states on its back edge.
pub(super) struct LoopBodyOutcome {
    pub body: StmtId,
    pub update: Option<StmtId>,
    pub assigned: std::collections::BTreeSet<narrowing::ReferencePath>,
    /// The state reaching the loop's condition: after the body and any update.
    pub head_edge: narrowing::NarrowEnv,
    /// The state entering the body's next pass: `head_edge` once the condition
    /// holds, or `head_edge` itself for a loop without one. None when the body
    /// cannot run again, because no pass reaches the back edge or the condition
    /// cannot hold there.
    pub next_pass: Option<narrowing::NarrowEnv>,
    pub reaches_back_edge: bool,
}

/// A `while` or `for` condition's check as it first runs.
struct FirstConditionCheck {
    condition: ExprId,
    true_env: narrowing::NarrowEnv,
    writes: std::collections::BTreeSet<narrowing::ReferencePath>,
    /// The diagnostics it reported, which the loop-head check replaces. They
    /// stay valid indices because the body is retyped, and its diagnostics
    /// truncated, only after them.
    diagnostic_range: std::ops::Range<usize>,
}

/// What a loop condition does on the back edge.
struct ConditionEffects {
    writes: std::collections::BTreeSet<narrowing::ReferencePath>,
    /// The whole state on its true branch, or none when it cannot hold there.
    after: Option<narrowing::NarrowEnv>,
}

/// The state a loop's update or condition runs in: `env`, over the enclosing
/// state with the narrowings under `dropped` removed.
struct LoopHead {
    env: narrowing::NarrowEnv,
    dropped: std::collections::BTreeSet<narrowing::ReferencePath>,
}

impl LoopHead {
    /// Before a `while` or `for` condition, which the state before the loop
    /// and every back edge reach. A path whose narrowing the join loses, or
    /// that the condition writes, has none there.
    fn before_every_pass(
        before_loop: narrowing::NarrowEnv,
        outcome: &LoopBodyOutcome,
        condition_writes: &std::collections::BTreeSet<narrowing::ReferencePath>,
    ) -> Self {
        let env = loop_head_env(&before_loop, outcome);
        let mut dropped = outcome.assigned.clone();
        dropped.extend(condition_writes.iter().cloned());
        dropped.extend(
            before_loop
                .keys()
                .chain(outcome.head_edge.keys())
                .filter(|path| !env.contains_key(*path))
                .cloned(),
        );
        LoopHead { env, dropped }
    }

    /// After a `do … while` body, which only the back edge reaches.
    fn after_every_pass(outcome: &LoopBodyOutcome) -> Self {
        LoopHead {
            env: outcome.head_edge.clone(),
            dropped: outcome.assigned.clone(),
        }
    }
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

/// The state before a loop joined with every back edge that reaches its head.
fn loop_head_env(
    before_loop: &narrowing::NarrowEnv,
    outcome: &LoopBodyOutcome,
) -> narrowing::NarrowEnv {
    if !outcome.reaches_back_edge {
        return before_loop.clone();
    }
    narrowing::union_envs(
        before_loop.clone(),
        Default::default(),
        outcome.head_edge.clone(),
        outcome.assigned.clone(),
    )
    .0
}

/// Widen each narrowing a body pass starts with that the next pass falsifies
/// to cover that pass too, or drop it when the next pass has none there or it
/// was already widened once. Returns whether any changed.
fn widen_entry_to_cover_next_pass(
    entry_env: &mut narrowing::NarrowEnv,
    outcome: &LoopBodyOutcome,
    widened: &mut std::collections::BTreeSet<narrowing::ReferencePath>,
) -> bool {
    let Some(next_pass) = &outcome.next_pass else {
        return false;
    };
    let falsified: Vec<_> = entry_env
        .iter()
        .filter(|(path, view)| next_pass_falsifies(view, next_pass.get(*path)))
        .map(|(path, view)| (path.clone(), view.clone()))
        .collect();
    for (path, view) in &falsified {
        entry_env.remove(path);
        let Some(post) = next_pass.get(path) else {
            continue;
        };
        if !widened.insert(path.clone()) {
            continue;
        }
        let single = |view: &narrowing::NarrowedView| {
            let mut env = narrowing::NarrowEnv::new();
            env.insert(path.clone(), view.clone());
            env
        };
        let joined = narrowing::union_envs(
            single(view),
            Default::default(),
            single(post),
            Default::default(),
        )
        .0;
        entry_env.extend_env(joined);
    }
    !falsified.is_empty()
}

/// Whether a narrowing `view` fails to cover the same path's `post` state on
/// the next pass, which has no narrowing there when `post` is `None`.
fn next_pass_falsifies(
    view: &narrowing::NarrowedView,
    post: Option<&narrowing::NarrowedView>,
) -> bool {
    post.is_none_or(|post| {
        Type::union(vec![view.narrowed_ty.clone(), post.narrowed_ty.clone()]) != view.narrowed_ty
    })
}

fn cond_is_static_true(
    _inferer: &Inferer<'_>,
    expr_id: ExprId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    Ok({
        matches!(
            _inferer
                .typed_ast
                .try_expr(expr_id)
                .map_err(crate::typechecker::arena_failure)?
                .kind,
            crate::TypedExprKind::Boolean(true)
        )
    })
}

/// A disproved loop-entry view, keyed by its declaration rather than the
/// transient scope number assigned when an enclosing loop is retyped.
#[derive(Clone, PartialEq)]
pub(super) struct LoopInvalidation {
    path: narrowing::ReferencePath,
    declaration: Option<Span>,
    narrowed_ty: Type,
}

impl Inferer<'_> {
    /// Retype while the next pass falsifies a narrowing the body starts with:
    /// an enclosing view, which is dropped, or one from the condition's true
    /// branch, which holds again on each pass but may narrow differently after
    /// what the body did, and is widened to cover it. Each retry drops a view
    /// or widens one for the only time, so retries terminate.
    pub(super) fn run_loop_body_with_fixed_point(
        &mut self,
        body: StmtId,
        mut entry_env: narrowing::NarrowEnv,
        body_span: Span,
        body_scope_floor: narrowing::ScopeId,
        tail: LoopTail,
    ) -> Result<LoopBodyOutcome, CompilerFailure> {
        self.apply_loop_invalidations(body, body_span);
        let diag_len = self.diagnostics.len();
        let mats_len = self.pending_post_if_materializations.len();
        let (breaks_len, continues_len) = self
            .pending_joins
            .last()
            .map_or((0, 0), |f| (f.breaks.len(), f.continues.len()));
        let entry_reachable = self.reachable;
        let mut widened = std::collections::BTreeSet::new();

        loop {
            let outcome = self.run_body_pass(body, &entry_env, body_span, continues_len, tail)?;
            let outer_falsified = self.drop_narrowings_the_body_falsifies(
                body,
                &outcome,
                body_scope_floor,
                body_span,
            );
            let entry_falsified =
                widen_entry_to_cover_next_pass(&mut entry_env, &outcome, &mut widened);
            if !outer_falsified && !entry_falsified {
                return Ok(outcome);
            }
            // Discard speculative diagnostics and exits before retyping under
            // the widened state.
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
    ) -> Result<ClauseOutcome, CompilerFailure> {
        self.push_narrow_frame(narrowing::NarrowEnv::new());
        let span = self
            .ast
            .try_stmt(clause)
            .map_err(super::arena_failure)?
            .span;
        for path in uncertain_writes {
            self.drop_narrowings_under(path, narrowing::InvalidationReason::Write { span });
        }
        self.reachable = entry_reachable;
        self.clause_write_scopes.push(Default::default());
        let body = self.infer_stmt(clause)?;
        let all_writes = self
            .clause_write_scopes
            .pop()
            .ok_or_else(|| super::inference_failure("clause write collector"))?;
        let exit = self.reachable.then(|| self.snapshot_active_narrowings(0).0);
        let (_, assigned) = self.pop_narrow_frame_capture()?;
        self.reachable = entry_reachable;
        Ok(ClauseOutcome {
            body,
            exit,
            assigned,
            all_writes,
        })
    }

    /// Invalidate outer views that the next pass widens or kills.
    /// Bindings declared within the body are recreated on each iteration.
    fn drop_narrowings_the_body_falsifies(
        &mut self,
        body: StmtId,
        outcome: &LoopBodyOutcome,
        body_scope_floor: narrowing::ScopeId,
        body_span: Span,
    ) -> bool {
        let Some(next_pass) = &outcome.next_pass else {
            return false;
        };
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
                    && next_pass_falsifies(view, next_pass.get(*path))
            })
            .map(|(path, view)| (path.clone(), view.clone()))
            .collect();
        for (path, view) in &falsified {
            let invalidation = self.loop_invalidation(path, &view.narrowed_ty);
            let cached = self.loop_invalidations.entry(body).or_default();
            if !cached.contains(&invalidation) {
                cached.push(invalidation);
            }
            self.drop_narrowings_under(
                path,
                narrowing::InvalidationReason::Write { span: body_span },
            );
        }
        !falsified.is_empty()
    }

    /// Reuse widening discovered by an earlier pass through this syntax node.
    /// Inner loops therefore do not repeat their fixed point for every retry
    /// of every enclosing loop. Guards are still installed afresh by the body.
    fn apply_loop_invalidations(&mut self, body: StmtId, span: Span) {
        let Some(cached) = self.loop_invalidations.get(&body) else {
            return;
        };
        let (active, _) = self.snapshot_active_narrowings(0);
        let falsified: Vec<_> = active
            .iter()
            .filter(|(path, view)| {
                cached.contains(&self.loop_invalidation(path, &view.narrowed_ty))
            })
            .map(|(path, _)| path.clone())
            .collect();
        for path in falsified {
            self.drop_narrowings_under(&path, narrowing::InvalidationReason::Write { span });
        }
    }

    fn loop_invalidation(&self, path: &narrowing::ReferencePath, ty: &Type) -> LoopInvalidation {
        let mut path = path.clone();
        let declaration = if let narrowing::BindingId::Local { name, decl_scope } = &mut path.root {
            let span = self
                .scopes
                .get_binding(name, *decl_scope)
                .map(|entry| entry.decl_span);
            *decl_scope = narrowing::ScopeId(0);
            span
        } else {
            None
        };
        LoopInvalidation {
            path,
            declaration,
            narrowed_ty: ty.clone(),
        }
    }

    /// One walk of a loop body under `entry_env`: infer it, wrap the narrow
    /// regions the env calls for, and report what leaves on the back edge.
    fn run_body_pass(
        &mut self,
        body: StmtId,
        entry_env: &narrowing::NarrowEnv,
        body_span: Span,
        continues_base: usize,
        tail: LoopTail,
    ) -> Result<LoopBodyOutcome, CompilerFailure> {
        self.push_narrow_frame(entry_env.clone());
        self.loop_depth += 1;
        let (typed_body, body_post) = self.infer_loop_body(body)?;
        self.loop_depth -= 1;
        let body_end_reachable = self.reachable;
        let (_, mut assigned) = self.pop_narrow_frame_capture()?;
        let (continues, continued_assignments) = self.continue_exits_since(continues_base)?;
        assigned.extend(continued_assignments);
        let mut back_edge = join_reachable_envs(body_end_reachable.then_some(body_post), continues);
        let mut typed_update = None;
        if let Some(update) = tail.update {
            let state = LoopHead {
                env: back_edge.clone().unwrap_or_default(),
                dropped: assigned.clone(),
            };
            let (typed, post, writes) = self.infer_loop_update(update, &state, body_span)?;
            typed_update = typed;
            back_edge = back_edge.map(|_| post);
            assigned.extend(writes);
        }
        let head_edge = back_edge.clone().unwrap_or_default();
        let reaches_back_edge = back_edge.is_some();
        let next_pass = match (tail.condition, back_edge) {
            (Some(condition), Some(edge)) => {
                let state = LoopHead {
                    env: edge,
                    dropped: assigned.clone(),
                };
                let effects = self.condition_effects(condition, &state, body_span)?;
                assigned.extend(effects.writes);
                effects.after
            }
            (_, edge) => edge,
        };
        Ok(LoopBodyOutcome {
            update: typed_update,
            head_edge,
            reaches_back_edge,
            body: self.wrap_narrow_regions(typed_body, entry_env, body_span)?,
            assigned,
            next_pass,
        })
    }

    /// Infer a loop condition on the back edge, after the body's writes. Its
    /// diagnostics are dropped: the caller checks the condition once in a state
    /// that covers every pass.
    fn condition_effects(
        &mut self,
        condition: ExprId,
        state: &LoopHead,
        body_span: Span,
    ) -> Result<ConditionEffects, CompilerFailure> {
        let diag_len = self.diagnostics.len();
        let mats_len = self.pending_post_if_materializations.len();
        self.enter_loop_head(state, body_span)?;
        let (typed_condition, _) = self.infer_expr(condition, None)?;
        let (true_env, _) = self.predicate_envs(typed_condition)?;
        let after = self.condition_can_hold(typed_condition)?.then(|| {
            let mut after = self.snapshot_active_narrowings(0).0;
            after.extend_env(true_env);
            after
        });
        let writes = self.leave_loop_head()?;
        self.diagnostics.truncate(diag_len);
        self.pending_post_if_materializations.truncate(mats_len);
        Ok(ConditionEffects { writes, after })
    }

    /// Infer a `for` update in `state`, the back edge after the body's writes.
    /// Returns the typed update, the state after it, and the paths it writes.
    fn infer_loop_update(
        &mut self,
        update: StmtId,
        state: &LoopHead,
        body_span: Span,
    ) -> Result<
        (
            Option<StmtId>,
            narrowing::NarrowEnv,
            std::collections::BTreeSet<narrowing::ReferencePath>,
        ),
        CompilerFailure,
    > {
        let tail_env = self.enter_loop_head(state, body_span)?;
        let typed = self
            .infer_stmt(update)?
            .map(|body| {
                self.wrap_narrow_regions(
                    body,
                    &tail_env,
                    self.ast
                        .try_stmt(update)
                        .map_err(super::arena_failure)?
                        .span,
                )
            })
            .transpose()?;
        let post = self.snapshot_active_narrowings(0).0;
        let writes = self.leave_loop_head()?;
        Ok((typed, post, writes))
    }

    /// Enter `state`: the enclosing state without the narrowings under
    /// `state.dropped`, with `state.env` on top under fresh bindings. Returns
    /// that env, for wrapping what is inferred in it. Pair with
    /// `leave_loop_head`.
    fn enter_loop_head(
        &mut self,
        state: &LoopHead,
        span: Span,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        self.push_narrow_frame(narrowing::NarrowEnv::new());
        for path in &state.dropped {
            self.drop_narrowings_under(path, narrowing::InvalidationReason::Write { span });
        }
        let env = self.loop_tail_env(&state.env)?;
        self.push_narrow_frame(env.clone());
        Ok(env)
    }

    /// Leave the state `enter_loop_head` entered, and give the paths written
    /// in it.
    fn leave_loop_head(
        &mut self,
    ) -> Result<std::collections::BTreeSet<narrowing::ReferencePath>, CompilerFailure> {
        let (_, writes) = self.pop_narrow_frame_capture()?;
        self.pop_narrow_frame()?;
        Ok(writes)
    }

    /// Infer a `while` or `for` loop. Its condition runs before every pass, so
    /// each pass starts from the condition's true branch, and the condition is
    /// checked where the state before the loop meets every back edge. Returns
    /// the typed condition, update, and body.
    fn infer_condition_first_loop(
        &mut self,
        condition: Option<ExprId>,
        update: Option<StmtId>,
        body: StmtId,
        span: Span,
    ) -> Result<(Option<ExprId>, Option<StmtId>, StmtId), CompilerFailure> {
        let (before_loop, _) = self.snapshot_active_narrowings(0);
        let first_check = condition
            .map(|c| self.check_first_condition(c))
            .transpose()?;
        let true_env = first_check
            .as_ref()
            .map(|check| check.true_env.clone())
            .unwrap_or_default();
        let body_span = self.ast.try_stmt(body).map_err(super::arena_failure)?.span;
        let body_scope_floor = self.scopes.next_scope_id();
        let entry_reachable = self.reachable;
        self.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
        let outcome = self.run_loop_body_with_fixed_point(
            body,
            true_env,
            body_span,
            body_scope_floor,
            LoopTail { update, condition },
        )?;
        let frame = self.pop_pending_join_frame()?;
        self.merge_assigned_into_outer(outcome.assigned.clone(), span);
        let (typed_cond, natural) = match first_check {
            Some(check) => {
                let head = LoopHead::before_every_pass(before_loop, &outcome, &check.writes);
                // The head covers the first run, so its check replaces that one.
                self.diagnostics.drain(check.diagnostic_range);
                let (typed, exit) =
                    self.check_condition_at_loop_head(check.condition, &head, body_span)?;
                (Some(typed), exit)
            }
            // `for (;;)` has no natural exit.
            None => (None, None),
        };
        let has_exit = self.fold_exits_into_outer(natural, frame.breaks, body_span)?;
        self.reachable = entry_reachable && has_exit;
        Ok((typed_cond, outcome.update, outcome.body))
    }

    /// Check a `while` or `for` condition as it first runs, for what its true
    /// branch gives the body's first pass.
    fn check_first_condition(
        &mut self,
        condition: ExprId,
    ) -> Result<FirstConditionCheck, CompilerFailure> {
        let diagnostics_from = self.diagnostics.len();
        self.clause_write_scopes.push(Default::default());
        let (typed, ty) = self.infer_expr(condition, None)?;
        self.check_condition_ty(
            &ty,
            self.ast
                .try_expr(condition)
                .map_err(super::arena_failure)?
                .span,
        );
        let writes = self
            .clause_write_scopes
            .pop()
            .ok_or_else(|| super::inference_failure("condition write collector"))?;
        let (true_env, _) = self.predicate_envs(typed)?;
        Ok(FirstConditionCheck {
            condition,
            true_env,
            writes,
            diagnostic_range: diagnostics_from..self.diagnostics.len(),
        })
    }

    /// Check a loop condition in `head`, the state that reaches it on any pass,
    /// and give the state on its false exit, or none when it is literally `true`.
    fn check_condition_at_loop_head(
        &mut self,
        condition: ExprId,
        head: &LoopHead,
        body_span: Span,
    ) -> Result<(ExprId, Option<narrowing::NarrowEnv>), CompilerFailure> {
        let head_env = self.enter_loop_head(head, body_span)?;
        let cond_span = self
            .ast
            .try_expr(condition)
            .map_err(super::arena_failure)?
            .span;
        let (typed, ty) = self.infer_expr(condition, None)?;
        self.check_condition_ty(&ty, cond_span);
        let (_, false_env) = self.predicate_envs(typed)?;
        let mut exit = self.snapshot_active_narrowings(0).0;
        exit.extend_env(false_env);
        let condition_assigned = self.leave_loop_head()?;
        self.merge_assigned_into_outer(condition_assigned, cond_span);
        let natural = (!cond_is_static_true(self, typed)?).then_some(exit);
        Ok((
            self.wrap_narrow_exprs(typed, &head_env, cond_span)?,
            natural,
        ))
    }

    /// Capture the normal exit before the body's lexical frame is removed.
    /// The loop tail needs guard facts as well as assignment facts; its own
    /// materialization filters out names declared inside this block.
    fn infer_loop_body(
        &mut self,
        body: StmtId,
    ) -> Result<(StmtId, narrowing::NarrowEnv), CompilerFailure> {
        let stmt = self
            .ast
            .try_stmt(body)
            .map_err(super::arena_failure)?
            .clone();
        let StmtKind::Block(stmts) = stmt.kind else {
            return Err(super::inference_failure("loop bodies are blocks"));
        };
        self.scopes.push();
        let mut typed_stmts = self.declare_nested_functions(body, &stmts)?;
        typed_stmts.extend(self.block_stmts_with_drain(&stmts, stmt.span)?);
        let post = self.snapshot_active_narrowings(0).0;
        self.scopes.pop();
        let typed = self
            .typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Block(typed_stmts),
                span: stmt.span,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok((typed, post))
    }

    fn continue_exits_since(
        &self,
        base_len: usize,
    ) -> Result<
        (
            Vec<narrowing::NarrowEnv>,
            std::collections::BTreeSet<narrowing::ReferencePath>,
        ),
        CompilerFailure,
    > {
        let frame = self
            .pending_joins
            .last()
            .ok_or_else(|| super::inference_failure("missing loop pending-join frame"))?;
        let exits = frame.continues.get(base_len..).ok_or_else(|| {
            super::inference_failure("continue snapshot offset exceeds pending transfers")
        })?;
        let envs = exits.iter().map(|(env, _)| env.clone()).collect();
        let assigned = exits
            .iter()
            .flat_map(|(_, assigned)| assigned.iter().cloned())
            .collect();
        Ok((envs, assigned))
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
                        } if rn == "Iterator" && ra.len() == 1 => ra.into_iter().next(),
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

    #[test]
    fn nested_loop_retries_do_not_multiply() {
        let shallow = nested_loop_expression_count(8);
        let deep = nested_loop_expression_count(16);
        assert!(
            deep < shallow * 12,
            "typed expression growth: {shallow} -> {deep}"
        );
        assert!(deep < 150_000, "excessive speculative expressions: {deep}");
    }

    fn nested_loop_expression_count(depth: usize) -> usize {
        use std::fmt::Write;
        let mut source = String::from("function main(): void {\n");
        for i in 0..depth {
            writeln!(
                source,
                "let x{i}: string | number | boolean | null = null; x{i} = 'a';"
            )
            .unwrap();
            writeln!(
                source,
                "let y{i}: string | number | boolean | null = null; y{i} = 'a';"
            )
            .unwrap();
            writeln!(source, "let i{i} = 0;").unwrap();
        }
        for i in 0..depth {
            writeln!(source, "while (x{i} !== null && y{i} !== null && typeof x{i} !== 'boolean' && i{i} < 2) {{ i{i}++;").unwrap();
        }
        for i in (0..depth).rev() {
            for j in 0..=i {
                writeln!(
                    source,
                    "if (i{j} > 7) {{ x{j} = {j}; }} else if (i{j} > 9) {{ y{j} = true; }}"
                )
                .unwrap();
            }
            source.push_str("}\n");
        }
        source.push('}');
        let (ast, diagnostics) = run(&source);
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        ast.exprs_len()
    }

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
             function main(): string { let c: C | null = new C() as C | null; c.v = null; return \"x\"; }",
            // Union receiver: the hint comes from the members' agreed write type.
            "class A { private n: number = 0; get v(): number { return this.n; } \
             set v(s: number | null) { this.n = 1; } }\n\
             class B { private n: number = 0; get v(): number { return this.n; } \
             set v(s: number | null) { this.n = 2; } }\n\
             function main(): string { let u: A | B = new A() as A | B; u.v = null; return \"x\"; }",
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

#[cfg(test)]
mod invariant_tests {
    use super::super::test_support::with_inferer;
    use super::*;

    #[test]
    fn invalid_transfer_snapshots_return_internal_errors() {
        with_inferer(|tc| {
            assert!(matches!(
                tc.continue_exits_since(0),
                Err(CompilerFailure::Internal { .. })
            ));
            tc.push_pending_join_frame(narrowing::PendingJoinKind::Loop);
            assert!(matches!(
                tc.continue_exits_since(1),
                Err(CompilerFailure::Internal { .. })
            ));
            let outcome = ClauseOutcome {
                all_writes: Default::default(),
                body: None,
                exit: Some(Default::default()),
                assigned: Default::default(),
            };
            assert!(matches!(
                tc.apply_finally_to_pending(&[], &[], &outcome),
                Err(CompilerFailure::Internal { .. })
            ));
            for exit in [None, Some(narrowing::NarrowEnv::new())] {
                let outcome = ClauseOutcome {
                    exit,
                    all_writes: Default::default(),
                    body: None,
                    assigned: Default::default(),
                };
                let mut transfers = Vec::new();
                assert!(matches!(
                    apply_finally_to_transfers(&mut transfers, 0..1, &outcome),
                    Err(CompilerFailure::Internal { .. })
                ));
            }
        });
    }
}
