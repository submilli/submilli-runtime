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
    /// The discriminant expression as written, which a `case` label is
    /// compared against.
    source: ExprId,
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
        if opening.is_empty() && !has_expression_label(&switch) {
            return Ok(switch);
        }
        self.bind_discriminant_first(switch_id, switch, opening, switch_span)
    }

    /// Bind the discriminant to a temporary ahead of the switch, which a label
    /// that isn't a literal compares against, and put the hoisted functions'
    /// opening statements after it: `{ let temp = discriminant; opening…;
    /// switch (temp) }`.
    fn bind_discriminant_first(
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
        let name = discriminant_temporary(switch_id, span);
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
            source: discriminant,
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
            source: disc_source,
        } = discriminant;
        let disc_operand = self.comparison_operand(disc_source, typed_disc)?;
        let case_discriminant = CaseDiscriminant {
            switch_id,
            typed: typed_disc,
            ty: &disc_ty,
            operand: &disc_operand,
        };
        let disc_exclusions = self.discriminant_exclusions(typed_disc)?;
        let narrowed_before =
            !disc_exclusions.is_empty() || self.discriminant_narrowed(typed_disc)?;
        let entry_reachable = self.reachable;
        self.push_pending_join_frame(narrowing::PendingJoinKind::Switch);
        let mut typed_cases: Vec<TypedSwitchCase> = Vec::new();
        let mut seen: BTreeMap<CaseKey, Span> = BTreeMap::new();
        let mut saw_null: Option<Span> = None;
        let mut all_assigned: BTreeSet<narrowing::ReferencePath> = BTreeSet::new();
        let mut any_arm_reachable_exit = false;
        // Whether the `case`s, with no `default`, match every value.
        let mut covers_every_value = false;
        // The clause last in the source leaves the switch when it runs off
        // its end, as a `break` there would.
        let last_clause = cases
            .iter()
            .map(|case| (case.span.start, case.body))
            .chain(default.as_ref().map(|d| (d.span.start, d.body)))
            .max()
            .map(|(_, body)| body);

        for case in cases {
            let case_span = case.span;
            // Each label with its own type, which an enum member's test keeps.
            let mut labels: Vec<(TypedSwitchValue, Type)> = Vec::new();
            for value_expr in &case.values {
                let Some((label, label_ty)) =
                    self.infer_case_label(&case_discriminant, *value_expr, case_span)?
                else {
                    continue;
                };
                let value_span = case_value_span(&label);
                if matches!(label, TypedSwitchValue::Null { .. }) {
                    saw_null.get_or_insert(value_span);
                }
                // tsc reports no duplicate for an expression label; one of a
                // single literal type still covers that value.
                let duplicate_of = if matches!(label, TypedSwitchValue::Expr { .. }) {
                    None
                } else {
                    seen.insert(case_key(&label), value_span)
                };
                if let Some(prev) = duplicate_of {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: value_span,
                        message: "duplicate `case` label".to_string(),
                        help: vec![],
                        notes: vec![(prev, "previously matched here".to_string())],
                    });
                    continue;
                }
                labels.push((label, label_ty));
            }

            let true_env = self.case_true_env(typed_disc, disc_source_span, &labels)?;

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
            if body_reachable && last_clause == Some(case.body) {
                self.record_break_exit();
            }
            self.switch_depth -= 1;
            let (_n, body_assigned) = self.pop_narrow_frame_capture()?;
            let typed_body = self.wrap_narrow_regions(typed_body, &true_env, body_span)?;
            all_assigned.extend(body_assigned);
            any_arm_reachable_exit |= body_reachable;

            typed_cases.push(TypedSwitchCase {
                values: labels.into_iter().map(|(value, _)| value).collect(),
                body: typed_body,
                span: case_span,
            });
        }

        let covered = CaseCoverage::of(&typed_cases, saw_null.is_some(), disc_exclusions);
        let (residual, site) = self.compute_switch_residual(typed_disc, &disc_ty, &covered)?;

        let typed_default = if let Some(d) = default {
            let body_span = self
                .ast
                .try_stmt(d.body)
                .map_err(super::arena_failure)?
                .span;
            let default_residual = self.unmatched_residual(&residual, &site, saw_null.is_some());
            let env = self.build_default_narrow_env(
                &default_residual,
                &site,
                covered.ruled_out(&site),
                narrowed_before,
                body_span,
            )?;
            self.push_narrow_frame(env.clone());
            self.switch_depth += 1;
            self.reachable = entry_reachable;
            let typed_body = self.infer_case_clause(switch_id, d.body, body_span)?;
            let body_reachable = self.reachable;
            if body_reachable && last_clause == Some(d.body) {
                self.record_break_exit();
            }
            self.switch_depth -= 1;
            let (_n, body_assigned) = self.pop_narrow_frame_capture()?;
            let typed_body = self.wrap_narrow_regions(typed_body, &env, body_span)?;
            all_assigned.extend(body_assigned);
            any_arm_reachable_exit |= body_reachable;
            Some(typed_body)
        } else {
            let unmatched = self.unmatched_residual(&residual, &site, saw_null.is_some());
            let leaves_values_unmatched =
                !matches!(unmatched, Type::Never) && !narrowing::is_ruled_out(&unmatched);
            if leaves_values_unmatched {
                if requires_every_case(&disc_ty) {
                    self.emit_non_exhaustive(&unmatched, &site, switch_span);
                }
                any_arm_reachable_exit |= entry_reachable;
            } else {
                self.typed_ast.exhaustive_switches.insert(typed_disc);
                covers_every_value = true;
            }
            None
        };

        // Breaks are normal case exits; count them toward post-switch reachability.
        let frame = self.pop_pending_join_frame()?;
        if !frame.breaks.is_empty() {
            any_arm_reachable_exit = true;
        }
        self.merge_assigned_into_outer(all_assigned, switch_span);
        let natural = if typed_default.is_none() && !matches!(residual, Type::Never) {
            // The no-match path saw none of the case values, so it carries the
            // narrowing to the rest, as a `default` arm would.
            let mut natural = entry_env;
            natural.extend_env(self.build_default_narrow_env(
                &self.unmatched_residual(&residual, &site, saw_null.is_some()),
                &site,
                covered.ruled_out(&site),
                narrowed_before,
                switch_span,
            )?);
            Some(natural)
        } else {
            None
        };
        self.fold_exits_into_outer(natural, frame.breaks, switch_span)?;

        self.reachable = any_arm_reachable_exit;
        if covers_every_value && !any_arm_reachable_exit {
            self.unreachable_by_exhaustive_switch = true;
            self.rule_out_after_exhaustive_switch(&site, switch_span)?;
        }

        Ok(TypedStmtKind::Switch {
            discriminant: typed_disc,
            discriminant_ty: disc_ty,
            cases: typed_cases,
            default: typed_default,
        })
    }

    /// Type one `case` label and check it against the discriminant: a literal
    /// label, or an expression label compared at run time, with the label's type.
    /// `None` after an error.
    fn infer_case_label(
        &mut self,
        discriminant: &CaseDiscriminant<'_>,
        value_expr: ExprId,
        case_span: Span,
    ) -> Result<Option<(TypedSwitchValue, Type)>, CompilerFailure> {
        let value_span = self
            .ast
            .try_expr(value_expr)
            .map_err(super::arena_failure)?
            .span;
        let (typed_val, label_ty) = self.infer_expr(value_expr, None)?;
        let case_operand = self.comparison_operand(value_expr, typed_val)?;
        if !super::comparison_operand::operands_comparable(
            &case_operand,
            discriminant.operand,
            self.resolver(),
        ) {
            self.error(
                value_span,
                format!(
                    "case label of type `{}` is not compatible with switch discriminant of type `{}`",
                    case_operand.label, discriminant.operand.label
                ),
            );
            return Ok(None);
        }
        if let Some(literal) = self.literal_case_label(value_expr, typed_val, value_span)? {
            return Ok(Some((literal, label_ty)));
        }
        let label =
            self.expression_case_label(discriminant, typed_val, &case_operand.ty, case_span)?;
        Ok(Some((label, label_ty)))
    }

    /// A label spelled as a literal: `"a"`, `1`, `null`, `E.A`, a template of
    /// constants, or a signed number, which `tsc` also reads as a literal.
    fn literal_case_label(
        &self,
        value_expr: ExprId,
        typed_val: ExprId,
        value_span: Span,
    ) -> Result<Option<TypedSwitchValue>, CompilerFailure> {
        let typed = self
            .typed_ast
            .try_expr(typed_val)
            .map_err(crate::typechecker::arena_failure)?;
        if let Some(literal) = classify_switch_case_value(typed, value_span) {
            return Ok(Some(literal));
        }
        // A template of constants is the string it spells, as `tsc` has it.
        if let Some(value) = super::comparison_operand::constant_template(self.ast, value_expr)? {
            return Ok(Some(TypedSwitchValue::String {
                value,
                span: value_span,
            }));
        }
        // Adding `0.0` makes `-0` the same label as `0`, which `===` can't tell
        // apart either.
        Ok(
            super::comparison_operand::constant_number(self.ast, value_expr)?.map(|value| {
                TypedSwitchValue::Number {
                    value: value + 0.0,
                    span: value_span,
                }
            }),
        )
    }

    /// A `case` label that isn't a literal, compared with `===` at run time as
    /// `tsc` allows. `label_ty` is its type as compared, which keeps literal types.
    /// The comparison spans the whole clause, `case_span`: it is no expression
    /// in the source, so it must not take the label's own span.
    fn expression_case_label(
        &mut self,
        discriminant: &CaseDiscriminant<'_>,
        label: ExprId,
        label_ty: &Type,
        case_span: Span,
    ) -> Result<TypedSwitchValue, CompilerFailure> {
        let span = self
            .typed_ast
            .try_expr(label)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        let disc_span = self
            .typed_ast
            .try_expr(discriminant.typed)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        let discriminant_ref = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::LocalRef {
                    ident: discriminant_temporary(discriminant.switch_id, disc_span),
                    boxed: false,
                },
                span: disc_span,
                ty: discriminant.ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let comparison = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary {
                    op: BinOp::Eq,
                    lhs: discriminant_ref,
                    rhs: label,
                },
                span: case_span,
                ty: Type::Boolean,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(TypedSwitchValue::Expr {
            label,
            comparison,
            literal: narrowing::unit_literal_value(label_ty),
            span,
        })
    }

    /// The narrowing a clause's body runs under: the discriminant equals one of
    /// its labels.
    fn case_true_env(
        &mut self,
        typed_disc: ExprId,
        disc_source_span: Span,
        labels: &[(TypedSwitchValue, Type)],
    ) -> Result<narrowing::NarrowEnv, CompilerFailure> {
        let mut literal_values: Vec<(TypedSwitchValue, Type)> = Vec::new();
        for (value, value_ty) in labels {
            let TypedSwitchValue::Expr { label, span, .. } = value else {
                literal_values.push((value.clone(), value_ty.clone()));
                continue;
            };
            // A label of literal type narrows to its literals; any other leaves
            // the discriminant as it is.
            let label_ty = self
                .typed_ast
                .try_expr(*label)
                .map_err(crate::typechecker::arena_failure)?
                .ty
                .clone();
            let Some(literals) = literal_members(&label_ty).and_then(|literals| {
                literals
                    .into_iter()
                    .map(|literal| {
                        literal_switch_value(literal, *span).map(|value| (value, label_ty.clone()))
                    })
                    .collect::<Option<Vec<_>>>()
            }) else {
                return Ok(narrowing::NarrowEnv::new());
            };
            literal_values.extend(literals);
        }
        let mut iter = literal_values.iter();
        let Some((first, first_ty)) = iter.next() else {
            return Ok(narrowing::NarrowEnv::new());
        };
        let mut acc =
            self.predicate_env_for_case_value(typed_disc, disc_source_span, first, first_ty)?;
        for (value, value_ty) in iter {
            let next =
                self.predicate_env_for_case_value(typed_disc, disc_source_span, value, value_ty)?;
            let (joined, _) = narrowing::union_envs(acc, BTreeSet::new(), next, BTreeSet::new());
            acc = joined;
        }
        Ok(acc)
    }

    /// The label as an expression for the synthesized `disc === label` test. An
    /// enum member keeps the label's own type (`E`), as its `case` reads in source.
    fn push_switch_value_expr(
        &mut self,
        value: &TypedSwitchValue,
        label_ty: &Type,
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
            TypedSwitchValue::Expr { label, .. } => return Ok(*label),
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
                    label_ty.clone(),
                    *span,
                ),
                EnumVariantPayload::String(s) => (
                    TypedExprKind::StringEnumMember {
                        enum_mangled: enum_name.clone(),
                        variant: member.clone(),
                        value: s.clone(),
                    },
                    label_ty.clone(),
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
        label_ty: &Type,
    ) -> Result<narrowing::NarrowEnv, crate::compiler_error::CompilerFailure> {
        let lit_expr_id = self.push_switch_value_expr(value, label_ty)?;
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
        covered: &CaseCoverage,
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
                && let Some(field_tys) = self.discriminant_field_types(members, &name.name)
            {
                let disc_key = name.name.clone();
                let kept = self.members_left_unmatched(members, &field_tys, covered);
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
                            variant.0 as usize == *idx && covered.literals.contains(lit)
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
        let residual = self.unmatched_values(disc_ty, covered);
        // A `case null` leaves `null` out only of the default's view, so the
        // residual keeps it; a residual of only `null` it matched is empty.
        let residual = if covered.null && matches!(residual.peel(), Type::Null) {
            Type::Never
        } else {
            residual
        };
        Ok(
            if let Some(path) = self.expr_to_reference_path(disc_expr)? {
                (residual, ResidualSite::Scrutinee { path })
            } else {
                (residual, ResidualSite::Anonymous)
            },
        )
    }

    /// The values of `ty` no case matches, less `null`, which the caller
    /// handles.
    fn unmatched_values(&self, ty: &Type, covered: &CaseCoverage) -> Type {
        narrowing::subtract_literals(
            &self.without_named_members(ty, covered),
            &covered.literals_ruled_out(),
        )
    }

    /// The members of a union switched on its discriminant field that the
    /// cases leave, as in TypeScript: the field's values no case matches are
    /// gathered over every member, and a member stays when its field can hold
    /// one of them. `{ tag: S }` stays beside `{ tag: "p" }` though cases name
    /// every member of `S`, since `S.P` holds `"p"`.
    fn members_left_unmatched(
        &self,
        members: &[Type],
        field_tys: &[Option<Type>],
        covered: &CaseCoverage,
    ) -> Vec<Type> {
        let unmatched: Vec<Option<Type>> = field_tys
            .iter()
            .map(|field_ty| {
                field_ty
                    .as_ref()
                    .map(|ty| self.unmatched_values(ty, covered))
            })
            .collect();
        let all_unmatched = Type::union(unmatched.iter().flatten().cloned().collect());
        let unmatched_null = !covered.null
            && narrowing::union_members(&all_unmatched)
                .iter()
                .any(|m| matches!(m.peel(), Type::Null));
        let values_left = without_null(&all_unmatched);
        members
            .iter()
            .zip(field_tys.iter().zip(&unmatched))
            .filter(|(_, (field_ty, unmatched))| {
                let (Some(field_ty), Some(unmatched)) = (field_ty, unmatched) else {
                    return true;
                };
                let own = without_null(unmatched);
                own != Type::Never
                    || (unmatched_null
                        && narrowing::union_members(field_ty)
                            .iter()
                            .any(|m| matches!(m.peel(), Type::Null)))
                    || self.shares_a_value(field_ty, &values_left, covered)
            })
            .map(|(member, _)| member.clone())
            .collect()
    }

    /// Whether a value of `field_ty` can be one of the unmatched `values`, as
    /// TypeScript's comparability decides it for a discriminant. An enum
    /// shares a value with its own members' literals, never with another enum,
    /// and an unmatched enum holds only the members no case names.
    fn shares_a_value(&self, field_ty: &Type, values: &Type, covered: &CaseCoverage) -> bool {
        let types = self.resolver();
        narrowing::union_members(field_ty).into_iter().any(|field| {
            narrowing::union_members(values).into_iter().any(|value| {
                match (field.peel(), value.peel()) {
                    (Type::Null, _) | (_, Type::Null | Type::Never) => false,
                    (
                        Type::NumberEnum { mangled: a, .. } | Type::StringEnum { mangled: a, .. },
                        Type::NumberEnum { mangled: b, .. } | Type::StringEnum { mangled: b, .. },
                    ) => a == b,
                    (
                        literal,
                        unmatched @ (Type::NumberEnum { mangled, .. }
                        | Type::StringEnum { mangled, .. }),
                    ) => narrowing::unit_literal_value(literal).is_some_and(|literal| {
                        !covered
                            .named_members
                            .contains(&(mangled.clone(), literal.clone()))
                            && super::comparable::enum_literal_values(unmatched, types)
                                .is_some_and(|members| members.contains(&literal))
                    }),
                    (field, value) => super::comparable::enum_admits_literal(field, value, types)
                        .unwrap_or_else(|| super::comparable::comparable(field, value, types)),
                }
            })
        })
    }

    /// `ty` without the enum members the cases name or an earlier check ruled
    /// out: an enum leaves the other members, as their member types.
    fn without_named_members(&self, ty: &Type, covered: &CaseCoverage) -> Type {
        let is_named = |member: &Type| match member.peel() {
            Type::NumberEnum { mangled, .. } | Type::StringEnum { mangled, .. } => {
                narrowing::LiteralValue::of_enum_member(member).is_some_and(|value| {
                    covered.ruled_out_before.contains(&value)
                        || covered.named_members.contains(&(mangled.clone(), value))
                })
            }
            _ => false,
        };
        let mut left = Vec::new();
        for member in narrowing::union_members(ty) {
            let whole_enum = !member.peel().is_enum_member();
            if whole_enum
                && let Some(members) =
                    super::comparable::enum_member_types(member.peel(), self.resolver())
            {
                if !self.names_every_member(member, covered) {
                    left.extend(members.into_iter().filter(|m| !is_named(m)));
                }
                continue;
            }
            if !is_named(member) {
                left.push(member.clone());
            }
        }
        Type::union(left)
    }

    /// Whether the cases, with the values ruled out before the switch, leave
    /// no member of the enum `ty`.
    fn names_every_member(&self, ty: &Type, covered: &CaseCoverage) -> bool {
        let (Type::NumberEnum { mangled, .. } | Type::StringEnum { mangled, .. }) = ty.peel()
        else {
            return false;
        };
        super::comparable::enum_literal_values(ty.peel(), self.resolver()).is_some_and(|values| {
            values.into_iter().all(|value| {
                covered.ruled_out_before.contains(&value)
                    || covered.named_members.contains(&(mangled.clone(), value))
            })
        })
    }

    /// Whether a check before the switch narrowed the discriminant or the
    /// object it is read from.
    fn discriminant_narrowed(&self, typed_disc: ExprId) -> Result<bool, CompilerFailure> {
        let disc_expr = self
            .typed_ast
            .try_expr(typed_disc)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(disc_expr)? else {
            return Ok(false);
        };
        Ok((0..=path.chain.len()).any(|len| {
            let prefix = narrowing::ReferencePath {
                root: path.root.clone(),
                chain: path.chain[..len].to_vec(),
            };
            self.innermost_narrowing(&prefix).is_some()
        }))
    }

    /// The values the discriminant was already known not to hold.
    fn discriminant_exclusions(
        &self,
        typed_disc: ExprId,
    ) -> Result<BTreeSet<narrowing::LiteralValue>, CompilerFailure> {
        let disc_expr = self
            .typed_ast
            .try_expr(typed_disc)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(path) = self.expr_to_reference_path(disc_expr)? else {
            return Ok(BTreeSet::new());
        };
        Ok(self.known_exclusions(&path))
    }

    /// What the discriminant can be when no case matched: the residual, less
    /// `null` when a `case null` matched it.
    fn unmatched_residual(&self, residual: &Type, site: &ResidualSite, saw_null: bool) -> Type {
        if saw_null && matches!(site, ResidualSite::Scrutinee { .. }) {
            narrowing::strip_null(residual)
        } else {
            residual.clone()
        }
    }

    /// After a `switch` whose cases match every value and all leave, the tested
    /// local holds no value, as in TypeScript: `assertNever(x)` there checks.
    /// The code is unreachable, so the narrowing says nothing about a run.
    fn rule_out_after_exhaustive_switch(
        &mut self,
        site: &ResidualSite,
        switch_span: Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let (ResidualSite::DiscriminatedReceiver { path, .. } | ResidualSite::Scrutinee { path }) =
            site
        else {
            return Ok(());
        };
        if !self.rules_out_to_never(path) {
            return Ok(());
        }
        let env = self.build_default_narrow_env(
            &narrowing::RULED_OUT,
            site,
            BTreeSet::new(),
            true,
            switch_span,
        )?;
        if let Some(frame) = self.narrow_scopes.last_mut() {
            frame.extend_env(env);
        }
        Ok(())
    }

    fn build_default_narrow_env(
        &mut self,
        residual: &Type,
        site: &ResidualSite,
        excluded_literals: BTreeSet<narrowing::LiteralValue>,
        narrowed_before: bool,
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
        // No value is left. When the cases alone cover the declared type, the
        // path reads `never`, as in TypeScript. When an earlier check helped,
        // a call or alias may have changed the value since, so the view rules
        // the path out instead: it reads `never` only where
        // `rules_out_to_never` allows and elsewhere keeps its declared type,
        // rather than handing that value to code that trusts `never`.
        let narrowed_ty = if narrowed_before && matches!(residual, Type::Never) {
            narrowing::RULED_OUT
        } else {
            residual.clone()
        };
        let binding = self.mint_narrow_binding(body_span)?;
        env.insert(
            path,
            narrowing::NarrowedView {
                narrowed_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals,
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

/// Whether a `switch` without `default` must list every value of its
/// discriminant: when its type is made of literals alone, which cases can
/// cover. One with a member such as `string` or `null`, even in a single
/// member of a discriminated union, can't be listed out, so the switch may
/// simply fall through.
fn requires_every_case(disc_ty: &Type) -> bool {
    narrowing::union_members(disc_ty).into_iter().all(|member| {
        narrowing::unit_literal_value(member).is_some() || matches!(member.peel(), Type::Boolean)
    })
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

fn classify_switch_case_value(typed: &TypedExpr, span: Span) -> Option<TypedSwitchValue> {
    match &typed.kind {
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

fn has_expression_label(switch: &TypedStmtKind) -> bool {
    let TypedStmtKind::Switch { cases, .. } = switch else {
        return false;
    };
    cases.iter().any(|case| {
        case.values
            .iter()
            .any(|value| matches!(value, TypedSwitchValue::Expr { .. }))
    })
}

/// The name of the temporary a switch's discriminant is bound to, when a hoisted
/// function or a label that isn't a literal needs it read more than once.
fn discriminant_temporary(switch_id: StmtId, span: Span) -> Ident {
    Ident {
        // `#` cannot occur in a source identifier.
        name: format!("#switch_discriminant_{}", switch_id.0),
        span,
    }
}

/// The literals `ty` is made of, or `None` if it holds any other value.
fn literal_members(ty: &Type) -> Option<Vec<narrowing::LiteralValue>> {
    match ty.peel() {
        Type::Union(members) => members.iter().map(narrowing::unit_literal_value).collect(),
        other => narrowing::unit_literal_value(other).map(|literal| vec![literal]),
    }
}

/// The literal `case` value a label of literal type compares as; a bigint has
/// none, so its label narrows nothing.
fn literal_switch_value(literal: narrowing::LiteralValue, span: Span) -> Option<TypedSwitchValue> {
    Some(match literal {
        narrowing::LiteralValue::String(value) => TypedSwitchValue::String { value, span },
        narrowing::LiteralValue::Number(value) => TypedSwitchValue::Number {
            value: value.0,
            span,
        },
        narrowing::LiteralValue::Boolean(value) => TypedSwitchValue::Boolean { value, span },
        narrowing::LiteralValue::BigInt(_) => return None,
    })
}

/// The discriminant every `case` label of one `switch` is checked against.
struct CaseDiscriminant<'d> {
    switch_id: StmtId,
    typed: ExprId,
    ty: &'d Type,
    operand: &'d super::comparison_operand::ComparisonOperand,
}

fn case_value_span(value: &TypedSwitchValue) -> Span {
    match value {
        TypedSwitchValue::String { span, .. }
        | TypedSwitchValue::Number { span, .. }
        | TypedSwitchValue::Boolean { span, .. }
        | TypedSwitchValue::Null { span }
        | TypedSwitchValue::Enum { span, .. }
        | TypedSwitchValue::Expr { span, .. } => *span,
    }
}

fn without_null(ty: &Type) -> Type {
    Type::union(
        narrowing::union_members(ty)
            .into_iter()
            .filter(|member| !matches!(member.peel(), Type::Null))
            .cloned()
            .collect(),
    )
}

/// The values a `switch`'s cases match, with those an earlier check ruled out.
/// A case naming an enum member matches only that enum's member, and a bare
/// literal case no member, as in TypeScript: `case 0` leaves `E.A` unmatched,
/// and `case E.A` leaves `0`.
struct CaseCoverage {
    /// The values of the bare literal cases.
    literals: BTreeSet<narrowing::LiteralValue>,
    /// The enum members the cases name, by enum and value.
    named_members: BTreeSet<(crate::MangledName, narrowing::LiteralValue)>,
    null: bool,
    /// The values the discriminant was known not to hold before the switch,
    /// such as the enum members an earlier `if` returned on.
    ruled_out_before: BTreeSet<narrowing::LiteralValue>,
}

impl CaseCoverage {
    fn of(
        cases: &[TypedSwitchCase],
        null: bool,
        ruled_out_before: BTreeSet<narrowing::LiteralValue>,
    ) -> Self {
        let mut coverage = Self {
            literals: BTreeSet::new(),
            named_members: BTreeSet::new(),
            null,
            ruled_out_before,
        };
        for value in cases.iter().flat_map(|case| &case.values) {
            let Some(literal) = switch_value_to_literal_value(value) else {
                continue;
            };
            match value {
                TypedSwitchValue::Enum { enum_name, .. } => {
                    coverage.named_members.insert((enum_name.clone(), literal));
                }
                _ => {
                    coverage.literals.insert(literal);
                }
            }
        }
        coverage
    }

    /// The literal values no case leaves: the bare literal cases and the
    /// values ruled out before. The members a case names are left to
    /// `without_named_enums`, since `case E.A` matches no bare literal.
    fn literals_ruled_out(&self) -> BTreeSet<narrowing::LiteralValue> {
        self.literals
            .union(&self.ruled_out_before)
            .cloned()
            .collect()
    }

    /// The values the discriminant can't hold where no case matched, which a
    /// further check of it keeps ruled out. They belong to the discriminant,
    /// so a view of the receiver it was read from gets none.
    fn ruled_out(&self, site: &ResidualSite) -> BTreeSet<narrowing::LiteralValue> {
        if !matches!(site, ResidualSite::Scrutinee { .. }) {
            return BTreeSet::new();
        }
        let mut ruled_out = self.literals_ruled_out();
        ruled_out.extend(self.named_members.iter().map(|(_, value)| value.clone()));
        ruled_out
    }
}

/// What makes two `case` labels the same: a member of an enum, or a literal
/// value. `case E.A` beside `case 0` or `case F.X` of the same value is no
/// duplicate, as each matches a value the other doesn't name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum CaseKey {
    Member(crate::MangledName, String),
    Literal(narrowing::LiteralValue),
    Null,
}

fn case_key(value: &TypedSwitchValue) -> CaseKey {
    match value {
        TypedSwitchValue::Enum {
            enum_name, member, ..
        } => CaseKey::Member(enum_name.clone(), member.name.clone()),
        _ => switch_value_to_literal_value(value).map_or(CaseKey::Null, CaseKey::Literal),
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
        TypedSwitchValue::Expr { literal, .. } => literal.clone(),
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
