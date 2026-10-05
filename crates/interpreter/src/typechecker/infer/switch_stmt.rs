use crate::compiler_error::CompilerFailure;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Ast, BinOp, Diagnostic, EnumVariantPayload, ExprId, Ident, Severity, Span, StmtId, StmtKind,
    SwitchCase, SwitchDefault, Type, TypedExpr, TypedExprKind, TypedStmt, TypedStmtKind,
    TypedSwitchCase, TypedSwitchValue,
};

use super::{Inferer, narrowing};

/// An enclosing `switch`: the `let`/`const` its clauses declare directly, and
/// the clause being inferred.
pub(in crate::typechecker) struct SwitchFrame {
    locals: BTreeMap<String, Span>,
    /// The whole `switch` until a clause is entered, so that no local counts
    /// as another clause's in the discriminant or the hoisted functions.
    current_clause: Span,
    switch_span: Span,
    /// Clause statements whose functions were declared with the whole body.
    declared_with_body: BTreeSet<StmtId>,
}

fn span_contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// One `case` or `default` clause: the values it matches and the statements
/// directly in its body.
pub(super) struct SwitchClause<'a> {
    pub(super) values: &'a [ExprId],
    pub(super) stmts: Vec<StmtId>,
}

/// The clauses of a `switch` in source order, which is the order JavaScript
/// runs their declarations in; `default` may sit between cases.
pub(super) fn clauses_in_source_order<'a>(
    ast: &Ast,
    cases: &'a [SwitchCase],
    default: Option<&SwitchDefault>,
) -> Result<Vec<SwitchClause<'a>>, CompilerFailure> {
    let mut clauses: Vec<(Span, &'a [ExprId], StmtId)> = cases
        .iter()
        .map(|case| (case.span, case.values.as_slice(), case.body))
        .chain(default.map(|d| (d.span, &[][..], d.body)))
        .collect();
    clauses.sort_by_key(|(span, _, _)| span.start);
    clauses
        .into_iter()
        .map(|(_, values, body)| {
            let stmts = match &ast.try_stmt(body).map_err(super::arena_failure)?.kind {
                StmtKind::Block(stmts) => stmts.clone(),
                _ => vec![body],
            };
            Ok(SwitchClause { values, stmts })
        })
        .collect()
}

/// A discriminant inferred ahead of the switch body.
struct Discriminant {
    /// The narrowings in force before the discriminant ran.
    entry_env: narrowing::NarrowEnv,
    typed: ExprId,
    ty: Type,
    source_span: Span,
}

/// Narrowing anchor for the switch's default body.
enum ResidualSite {
    DiscriminatedReceiver {
        path: narrowing::ReferencePath,
        disc_key: String,
    },
    Scrutinee {
        path: narrowing::ReferencePath,
    },
    /// No default-body narrowing; exhaustiveness still fires.
    Anonymous,
}

impl Inferer<'_> {
    /// JavaScript scopes a `switch` body as one block: a function declared in
    /// one clause is hoisted to the body's start, so every clause can call it.
    /// Its `let`/`const` stay usable only in their own clause, which is the only
    /// one sure to have run their declaration.
    pub(super) fn infer_switch(
        &mut self,
        switch_id: StmtId,
        discriminant: ExprId,
        cases: Vec<SwitchCase>,
        default: Option<SwitchDefault>,
        switch_span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let clause_stmts: Vec<StmtId> =
            clauses_in_source_order(self.ast, &cases, default.as_ref())?
                .into_iter()
                .flat_map(|clause| clause.stmts)
                .collect();
        let discriminant = self.infer_discriminant(discriminant)?;
        let locals = self.clause_locals(&clause_stmts)?;
        self.switch_frames.push(SwitchFrame {
            locals,
            current_clause: switch_span,
            switch_span,
            declared_with_body: BTreeSet::new(),
        });
        let hoists_functions = self.declares_function(&clause_stmts)?;
        let opening = if hoists_functions {
            self.scopes.push();
            let opening = self.declare_nested_functions(switch_id, &clause_stmts)?;
            if let Some(frame) = self.switch_frames.last_mut() {
                frame.declared_with_body = clause_stmts.iter().copied().collect();
            }
            opening
        } else {
            Vec::new()
        };
        let switch =
            self.infer_switch_clauses(switch_id, discriminant, cases, default, switch_span);
        if hoists_functions {
            self.scopes.pop();
        }
        self.switch_frames.pop();
        let switch = switch?;
        if opening.is_empty() {
            return Ok(switch);
        }
        self.wrap_with_hoisted_functions(switch_id, switch, opening, switch_span)
    }

    /// Put the hoisted functions' opening statements ahead of the switch, after
    /// the discriminant: `{ let temp = discriminant; opening…; switch (temp) }`.
    fn wrap_with_hoisted_functions(
        &mut self,
        switch_id: StmtId,
        mut switch: TypedStmtKind,
        opening: Vec<StmtId>,
        switch_span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        // Narrowing reads the discriminant's own expression, so the temporary
        // takes its place only once the clauses are inferred.
        let TypedStmtKind::Switch {
            discriminant,
            discriminant_ty,
            ..
        } = &mut switch
        else {
            return Err(super::inference_failure("inferred switch is not a Switch"));
        };
        let mut stmts =
            vec![self.bind_discriminant_to_temporary(switch_id, discriminant, discriminant_ty)?];
        stmts.extend(opening);
        let switch_stmt = self
            .typed_ast
            .try_push_stmt(TypedStmt {
                kind: switch,
                span: switch_span,
            })
            .map_err(crate::typechecker::arena_failure)?;
        stmts.push(switch_stmt);
        Ok(TypedStmtKind::Block(stmts))
    }

    /// Bind the discriminant's value to a temporary ahead of the hoisted
    /// functions, which codegen would otherwise resolve its names against, and
    /// read the temporary in its place.
    fn bind_discriminant_to_temporary(
        &mut self,
        switch_id: StmtId,
        discriminant: &mut ExprId,
        ty: &Type,
    ) -> Result<StmtId, CompilerFailure> {
        let span = self
            .typed_ast
            .try_expr(*discriminant)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        let name = Ident {
            // `#` cannot occur in a source identifier.
            name: format!("#switch_discriminant_{}", switch_id.0),
            span,
        };
        let value = std::mem::replace(
            discriminant,
            self.typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::LocalRef {
                        ident: name.clone(),
                        boxed: false,
                    },
                    span,
                    ty: ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?,
        );
        self.typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Let {
                    name,
                    ty: ty.clone(),
                    value,
                    boxed: false,
                    doc: None,
                },
                span,
            })
            .map_err(crate::typechecker::arena_failure)
    }

    /// Whether `stmt` is a clause statement of the innermost `switch` whose
    /// function was already declared at the start of the switch body.
    pub(super) fn is_hoisted_to_switch_body(&self, stmt: StmtId) -> bool {
        self.switch_frames
            .last()
            .is_some_and(|frame| frame.declared_with_body.contains(&stmt))
    }

    /// Whether any of `stmts` is a function declaration.
    fn declares_function(&self, stmts: &[StmtId]) -> Result<bool, CompilerFailure> {
        for &stmt in stmts {
            if matches!(
                self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind,
                StmtKind::Function { .. }
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn clause_locals(&self, stmts: &[StmtId]) -> Result<BTreeMap<String, Span>, CompilerFailure> {
        let mut locals = BTreeMap::new();
        for &stmt in stmts {
            if let StmtKind::Let { name, .. }
            | StmtKind::Const { name, .. }
            | StmtKind::ConstRest { name, .. } =
                &self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind
            {
                locals.entry(name.name.clone()).or_insert(name.span);
            }
        }
        Ok(locals)
    }

    /// The declaration of `name` in a clause of an enclosing `switch` other than
    /// the one being inferred.
    ///
    /// The whole body is the name's scope, so a binding of it from outside the
    /// `switch` is shadowed there; only one declared inside the `switch`, in a
    /// block of the current clause, hides the other clause's declaration.
    pub(super) fn declaration_in_another_case_clause(&self, name: &str) -> Option<Span> {
        let frame = self
            .switch_frames
            .iter()
            .rev()
            .find(|frame| frame.locals.contains_key(name))?;
        let declaration = *frame.locals.get(name)?;
        if span_contains(frame.current_clause, declaration) {
            return None;
        }
        let visible_inside_switch = self
            .scopes
            .get(name)
            .is_some_and(|entry| span_contains(frame.switch_span, entry.decl_span));
        (!visible_inside_switch).then_some(declaration)
    }

    /// Infer one clause body. A hoisted function that captures a local of the
    /// clause is created there, so it exists only once that clause has run:
    /// other clauses, which may be entered directly, can't use it.
    fn infer_case_clause(
        &mut self,
        switch_id: StmtId,
        body: StmtId,
        body_span: Span,
    ) -> Result<StmtId, CompilerFailure> {
        if let Some(frame) = self.switch_frames.last_mut() {
            frame.current_clause = body_span;
        }
        let pending_before_clause = self.nested_functions_not_yet_defined(switch_id);
        let typed_body = self.infer_stmt(body)?.ok_or_else(|| {
            super::inference_failure("switch clause body is a Block, never a type-only decl")
        })?;
        self.mark_nested_functions_undefined(&pending_before_clause)?;
        Ok(typed_body)
    }

    /// Infer the discriminant, which runs outside the switch body's scope.
    fn infer_discriminant(
        &mut self,
        discriminant: ExprId,
    ) -> Result<Discriminant, CompilerFailure> {
        let (entry_env, _) = self.snapshot_active_narrowings(0);
        let source_span = self
            .ast
            .try_expr(discriminant)
            .map_err(super::arena_failure)?
            .span;
        let (typed, ty) = self.infer_expr(discriminant, None)?;
        // A `void` discriminant has nothing to compare against, and an empty
        // switch reaches codegen with no case to report a type error first.
        if ty.carries_void() {
            self.error_non_comparable_type(
                source_span,
                &ty,
                super::diagnostics::ComparisonPosition::SwitchDiscriminant,
            );
        }
        Ok(Discriminant {
            entry_env,
            typed,
            ty,
            source_span,
        })
    }

    fn infer_switch_clauses(
        &mut self,
        switch_id: StmtId,
        discriminant: Discriminant,
        cases: Vec<SwitchCase>,
        default: Option<SwitchDefault>,
        switch_span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        let Discriminant {
            entry_env,
            typed: typed_disc,
            ty: disc_ty,
            source_span: disc_source_span,
        } = discriminant;
        let entry_reachable = self.reachable;
        self.push_pending_join_frame(narrowing::PendingJoinKind::Switch);
        let mut typed_cases: Vec<TypedSwitchCase> = Vec::new();
        let mut seen: BTreeMap<narrowing::LiteralValue, Span> = BTreeMap::new();
        let mut saw_null: Option<Span> = None;
        let mut all_assigned: BTreeSet<narrowing::ReferencePath> = BTreeSet::new();
        let mut any_arm_reachable_exit = false;

        for case in cases {
            let case_span = case.span;
            let mut typed_values: Vec<TypedSwitchValue> = Vec::new();
            for value_expr in &case.values {
                let value_span = self
                    .ast
                    .try_expr(*value_expr)
                    .map_err(super::arena_failure)?
                    .span;
                let (typed_val, _) = self.infer_expr(*value_expr, None)?;
                let typed_val_expr = self
                    .typed_ast
                    .try_expr(typed_val)
                    .map_err(crate::typechecker::arena_failure)?;
                let val_kind = typed_val_expr.kind.clone();
                let val_ty = super::expr::literal_comparison_type(&self.typed_ast, typed_val_expr)?;
                if !super::comparable::comparable(&val_ty, &disc_ty, self.resolver()) {
                    self.error(
                        value_span,
                        format!(
                            "case label of type `{val_ty}` is not compatible with switch discriminant of type `{disc_ty}`",
                        ),
                    );
                    continue;
                }
                let Some(lit) = classify_switch_case_value(&val_kind, value_span) else {
                    self.error(
                        value_span,
                        "`case` label must be a literal (string, number, boolean, `null`, or enum member)".to_string(),
                    );
                    continue;
                };
                if let Some(prev) = duplicate_key(&lit, &mut seen, &mut saw_null) {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: value_span,
                        message: "duplicate `case` label".to_string(),
                        help: vec![],
                        notes: vec![(prev, "previously matched here".to_string())],
                    });
                    continue;
                }
                typed_values.push(lit);
            }

            let mut iter = typed_values.iter();
            let true_env = match iter.next() {
                None => narrowing::NarrowEnv::new(),
                Some(first) => {
                    let mut acc =
                        self.predicate_env_for_case_value(typed_disc, disc_source_span, first)?;
                    for value in iter {
                        let next =
                            self.predicate_env_for_case_value(typed_disc, disc_source_span, value)?;
                        let (joined, _) =
                            narrowing::union_envs(acc, BTreeSet::new(), next, BTreeSet::new());
                        acc = joined;
                    }
                    acc
                }
            };

            let body_span = self
                .ast
                .try_stmt(case.body)
                .map_err(super::arena_failure)?
                .span;
            self.push_narrow_frame(true_env.clone());
            self.switch_depth += 1;
            self.reachable = entry_reachable;
            let typed_body = self.infer_case_clause(switch_id, case.body, body_span)?;
            let body_reachable = self.reachable;
            self.switch_depth -= 1;
            let (_n, body_assigned) = self.pop_narrow_frame_capture()?;
            let typed_body = self.wrap_narrow_regions(typed_body, &true_env, body_span)?;
            all_assigned.extend(body_assigned);
            any_arm_reachable_exit |= body_reachable;

            typed_cases.push(TypedSwitchCase {
                values: typed_values,
                body: typed_body,
                span: case_span,
            });
        }

        let covered: BTreeSet<narrowing::LiteralValue> = seen.keys().cloned().collect();
        let (residual, site) = self.compute_switch_residual(typed_disc, &disc_ty, &covered)?;

        let typed_default = if let Some(d) = default {
            let body_span = self
                .ast
                .try_stmt(d.body)
                .map_err(super::arena_failure)?
                .span;
            let default_residual =
                if saw_null.is_some() && matches!(site, ResidualSite::Scrutinee { .. }) {
                    narrowing::strip_null(&residual)
                } else {
                    residual.clone()
                };
            let env = self.build_default_narrow_env(&default_residual, &site, body_span)?;
            self.push_narrow_frame(env.clone());
            self.switch_depth += 1;
            self.reachable = entry_reachable;
            let typed_body = self.infer_case_clause(switch_id, d.body, body_span)?;
            let body_reachable = self.reachable;
            self.switch_depth -= 1;
            let (_n, body_assigned) = self.pop_narrow_frame_capture()?;
            let typed_body = self.wrap_narrow_regions(typed_body, &env, body_span)?;
            all_assigned.extend(body_assigned);
            any_arm_reachable_exit |= body_reachable;
            Some(typed_body)
        } else {
            if matches!(residual, Type::Never) {
            } else if residual != disc_ty {
                self.emit_non_exhaustive(&residual, &site, switch_span);
                any_arm_reachable_exit |= entry_reachable;
            } else {
                any_arm_reachable_exit |= entry_reachable;
            }
            None
        };

        // Breaks are normal case exits; count them toward post-switch reachability.
        let frame = self.pop_pending_join_frame()?;
        if !frame.breaks.is_empty() {
            any_arm_reachable_exit = true;
        }
        self.merge_assigned_into_outer(all_assigned, switch_span);
        let natural =
            (typed_default.is_none() && !matches!(residual, Type::Never)).then_some(entry_env);
        self.fold_exits_into_outer(natural, frame.breaks, switch_span)?;

        self.reachable = any_arm_reachable_exit;

        Ok(TypedStmtKind::Switch {
            discriminant: typed_disc,
            discriminant_ty: disc_ty,
            cases: typed_cases,
            default: typed_default,
        })
    }

    fn push_switch_value_expr(
        &mut self,
        value: &TypedSwitchValue,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        let (kind, ty, span) = match value {
            TypedSwitchValue::String { value, span } => (
                TypedExprKind::String(value.clone()),
                Type::StringLiteral(value.clone()),
                *span,
            ),
            TypedSwitchValue::Number { value, span } => (
                TypedExprKind::Number(*value),
                Type::NumberLiteral(crate::types::LiteralF64(*value)),
                *span,
            ),
            TypedSwitchValue::Boolean { value, span } => {
                (TypedExprKind::Boolean(*value), Type::Boolean, *span)
            }
            TypedSwitchValue::Null { span } => (TypedExprKind::Null, Type::Null, *span),
            TypedSwitchValue::Enum {
                enum_name,
                member,
                value,
                span,
            } => match value {
                EnumVariantPayload::Number(n) => (
                    TypedExprKind::NumberEnumMember {
                        enum_mangled: enum_name.clone(),
                        variant: member.clone(),
                        value: *n,
                    },
                    Type::NumberLiteral(crate::types::LiteralF64(*n)),
                    *span,
                ),
                EnumVariantPayload::String(s) => (
                    TypedExprKind::StringEnumMember {
                        enum_mangled: enum_name.clone(),
                        variant: member.clone(),
                        value: s.clone(),
                    },
                    Type::StringLiteral(s.clone()),
                    *span,
                ),
            },
        };
        self.typed_ast
            .try_push_expr(TypedExpr { kind, span, ty })
            .map_err(crate::typechecker::arena_failure)
    }

    fn predicate_env_for_case_value(
        &mut self,
        typed_disc: ExprId,
        disc_source_span: Span,
        value: &TypedSwitchValue,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        let lit_expr_id = self.push_switch_value_expr(value)?;
        let synth = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: BinOp::Eq,
                    lhs: typed_disc,
                    rhs: lit_expr_id,
                },
                span: disc_source_span,
                ty: Type::Boolean,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(self.predicate_envs(synth)?.0)
    }

    fn compute_switch_residual(
        &self,
        typed_disc: ExprId,
        disc_ty: &Type,
        covered: &BTreeSet<narrowing::LiteralValue>,
    ) -> Result<(Type, ResidualSite), crate::compiler_error::CompilerFailure> {
        let disc_expr = self
            .typed_ast
            .try_expr(typed_disc)
            .map_err(crate::typechecker::arena_failure)?;
        if let TypedExprKind::FieldAccess { receiver, name } = &disc_expr.kind {
            let receiver_expr = self
                .typed_ast
                .try_expr(*receiver)
                .map_err(crate::typechecker::arena_failure)?;
            if let Type::Union(members) = receiver_expr.ty.peel()
                && let Some((disc_key, table)) = self.union_discriminant_with_nominals(members)
                && disc_key == name.name
            {
                let kept: Vec<Type> = members
                    .iter()
                    .enumerate()
                    .filter(|(idx, _)| {
                        !table.iter().any(|(lit, variant)| {
                            variant.0 as usize == *idx && covered.contains(lit)
                        })
                    })
                    .map(|(_, m)| m.clone())
                    .collect();
                let residual =
                    narrowing::with_source_refinement(&receiver_expr.ty, Type::union(kept));
                if let Some(receiver_path) = self.expr_to_reference_path(receiver_expr)? {
                    return Ok((
                        residual,
                        ResidualSite::DiscriminatedReceiver {
                            path: receiver_path,
                            disc_key,
                        },
                    ));
                }
                return Ok((residual, ResidualSite::Anonymous));
            }
        }
        if let TypedExprKind::IndexAccess { receiver, index } = &disc_expr.kind {
            let receiver_expr = self
                .typed_ast
                .try_expr(*receiver)
                .map_err(crate::typechecker::arena_failure)?;
            let index_expr = self
                .typed_ast
                .try_expr(*index)
                .map_err(crate::typechecker::arena_failure)?;
            if let Type::Union(members) = receiver_expr.ty.peel()
                && let Some(position) = index_position(&index_expr.kind)
                && let Some((disc_pos, table)) = narrowing::tuple_union_discriminant(members)
                && disc_pos == position
            {
                let kept: Vec<Type> = members
                    .iter()
                    .enumerate()
                    .filter(|(idx, _)| {
                        !table.iter().any(|(lit, variant)| {
                            variant.0 as usize == *idx && covered.contains(lit)
                        })
                    })
                    .map(|(_, m)| m.clone())
                    .collect();
                let residual =
                    narrowing::with_source_refinement(&receiver_expr.ty, Type::union(kept));
                if let Some(receiver_path) = self.expr_to_reference_path(receiver_expr)? {
                    return Ok((
                        residual,
                        ResidualSite::DiscriminatedReceiver {
                            path: receiver_path,
                            disc_key: format!("[{position}]"),
                        },
                    ));
                }
                return Ok((residual, ResidualSite::Anonymous));
            }
        }
        let residual = narrowing::subtract_literals(disc_ty, covered);
        Ok(
            if let Some(path) = self.expr_to_reference_path(disc_expr)? {
                (residual, ResidualSite::Scrutinee { path })
            } else {
                (residual, ResidualSite::Anonymous)
            },
        )
    }

    fn build_default_narrow_env(
        &mut self,
        residual: &Type,
        site: &ResidualSite,
        body_span: Span,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        let mut env = narrowing::NarrowEnv::new();
        let path = match site {
            ResidualSite::DiscriminatedReceiver { path, .. } => path.clone(),
            ResidualSite::Scrutinee { path } => path.clone(),
            ResidualSite::Anonymous => return Ok(env),
        };
        let source = match self.synthesize_unnarrowed_source(&path, body_span)? {
            Some(kind) => self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind,
                    span: body_span,
                    ty: residual.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?,
            None => return Ok(env),
        };
        let binding = self.mint_narrow_binding(body_span)?;
        env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty: residual.clone(),
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: BTreeSet::new(),
                binding,
                source,
            },
        );
        Ok(env)
    }

    fn emit_non_exhaustive(&mut self, residual: &Type, site: &ResidualSite, switch_span: Span) {
        let missing = format_residual_missing(residual);
        let suffix = match site {
            ResidualSite::DiscriminatedReceiver { disc_key, .. } => {
                format!(" for discriminant `.{disc_key}`")
            }
            ResidualSite::Scrutinee { .. } | ResidualSite::Anonymous => String::new(),
        };
        let help = if missing.is_empty() {
            "add a `default:` clause".to_string()
        } else {
            format!("add a `default:` clause or cases for: {missing}")
        };
        let message = if missing.is_empty() {
            format!("non-exhaustive `switch`{suffix}")
        } else {
            format!("non-exhaustive `switch`: missing case(s) {missing}{suffix}")
        };
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span: switch_span,
            message,
            help: vec![help],
            notes: vec![],
        });
    }
}

fn format_residual_missing(residual: &Type) -> String {
    let parts: Vec<String> = match residual.peel() {
        Type::Union(members) => members.iter().filter_map(format_one_literal).collect(),
        single => format_one_literal(single).into_iter().collect(),
    };
    parts.join(", ")
}

fn format_one_literal(ty: &Type) -> Option<String> {
    match ty.peel() {
        Type::StringLiteral(s) => Some(format!("\"{s}\"")),
        Type::NumberLiteral(n) => Some(format!("{}", n.0)),
        Type::BooleanLiteral(b) => Some(b.to_string()),
        Type::Object { fields, .. } => {
            // Discriminated-union residuals are object variants; find the discriminant field's literal.
            for field in fields.values() {
                if let Some(lit) = match field.ty.peel() {
                    Type::StringLiteral(s) => Some(format!("\"{s}\"")),
                    Type::NumberLiteral(n) => Some(format!("{}", n.0)),
                    Type::BooleanLiteral(b) => Some(b.to_string()),
                    _ => None,
                } {
                    return Some(lit);
                }
            }
            None
        }
        _ => None,
    }
}

fn classify_switch_case_value(kind: &TypedExprKind, span: Span) -> Option<TypedSwitchValue> {
    match kind {
        TypedExprKind::String(s) => Some(TypedSwitchValue::String {
            value: s.clone(),
            span,
        }),
        TypedExprKind::Number(n) => Some(TypedSwitchValue::Number { value: *n, span }),
        TypedExprKind::Boolean(b) => Some(TypedSwitchValue::Boolean { value: *b, span }),
        TypedExprKind::Null => Some(TypedSwitchValue::Null { span }),
        TypedExprKind::NumberEnumMember {
            enum_mangled,
            variant,
            value,
        } => Some(TypedSwitchValue::Enum {
            enum_name: enum_mangled.clone(),
            member: variant.clone(),
            value: EnumVariantPayload::Number(*value),
            span,
        }),
        TypedExprKind::StringEnumMember {
            enum_mangled,
            variant,
            value,
        } => Some(TypedSwitchValue::Enum {
            enum_name: enum_mangled.clone(),
            member: variant.clone(),
            value: EnumVariantPayload::String(value.clone()),
            span,
        }),
        _ => None,
    }
}

/// Null uses `saw_null` separately because there's no `LiteralValue::Null` variant.
fn duplicate_key(
    value: &TypedSwitchValue,
    seen: &mut BTreeMap<narrowing::LiteralValue, Span>,
    saw_null: &mut Option<Span>,
) -> Option<Span> {
    let key = switch_value_to_literal_value(value);
    let span = match value {
        TypedSwitchValue::String { span, .. }
        | TypedSwitchValue::Number { span, .. }
        | TypedSwitchValue::Boolean { span, .. }
        | TypedSwitchValue::Null { span }
        | TypedSwitchValue::Enum { span, .. } => *span,
    };
    if let Some(k) = key {
        if let Some(prev) = seen.insert(k, span) {
            return Some(prev);
        }
        None
    } else {
        if let Some(prev) = saw_null.replace(span) {
            return Some(prev);
        }
        None
    }
}

fn switch_value_to_literal_value(value: &TypedSwitchValue) -> Option<narrowing::LiteralValue> {
    match value {
        TypedSwitchValue::String { value, .. } => {
            Some(narrowing::LiteralValue::String(value.clone()))
        }
        TypedSwitchValue::Number { value, .. } => Some(narrowing::LiteralValue::Number(
            crate::types::LiteralF64(*value),
        )),
        TypedSwitchValue::Boolean { value, .. } => Some(narrowing::LiteralValue::Boolean(*value)),
        TypedSwitchValue::Null { .. } => None,
        TypedSwitchValue::Enum { value, .. } => match value {
            EnumVariantPayload::Number(n) => Some(narrowing::LiteralValue::Number(
                crate::types::LiteralF64(*n),
            )),
            EnumVariantPayload::String(s) => Some(narrowing::LiteralValue::String(s.clone())),
        },
    }
}

fn index_position(kind: &TypedExprKind) -> Option<usize> {
    match kind {
        TypedExprKind::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => {
            Some(*n as usize)
        }
        _ => None,
    }
}
