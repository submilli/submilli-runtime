use std::collections::{BTreeMap, BTreeSet};

use crate::{
    BinOp, Diagnostic, EnumVariantPayload, ExprId, Severity, Span, SwitchCase, SwitchDefault, Type,
    TypedExpr, TypedExprKind, TypedStmtKind, TypedSwitchCase, TypedSwitchValue,
};

use super::{Inferer, assignable, narrowing};

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
    pub(super) fn infer_switch(
        &mut self,
        discriminant: ExprId,
        cases: Vec<SwitchCase>,
        default: Option<SwitchDefault>,
        switch_span: Span,
    ) -> TypedStmtKind {
        let (entry_env, _) = self.snapshot_active_narrowings(0);
        let disc_source_span = self.ast.expr(discriminant).span;
        let (typed_disc, disc_ty) = self.infer_expr(discriminant, None);
        // A `void` discriminant has nothing to compare against, and an empty
        // switch reaches codegen with no case to report a type error first.
        if disc_ty.carries_void() {
            self.error_non_comparable_type(
                disc_source_span,
                &disc_ty,
                super::diagnostics::ComparisonPosition::SwitchDiscriminant,
            );
        }

        let label_hint = switch_label_hint(&disc_ty);
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
                let value_span = self.ast.expr(*value_expr).span;
                let (typed_val, val_ty) = self.infer_expr(*value_expr, Some(&label_hint));
                let val_kind = self.typed_ast.expr(typed_val).kind.clone();
                if !matches!(disc_ty, Type::Error)
                    && !matches!(val_ty, Type::Error)
                    && !assignable(&val_ty, &label_hint, self.resolver())
                {
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
                        self.predicate_env_for_case_value(typed_disc, disc_source_span, first);
                    for value in iter {
                        let next =
                            self.predicate_env_for_case_value(typed_disc, disc_source_span, value);
                        let (joined, _) =
                            narrowing::union_envs(acc, BTreeSet::new(), next, BTreeSet::new());
                        acc = joined;
                    }
                    acc
                }
            };

            let body_span = self.ast.stmt(case.body).span;
            self.push_narrow_frame(true_env.clone());
            self.switch_depth += 1;
            self.reachable = entry_reachable;
            let typed_body = self
                .infer_stmt(case.body)
                .expect("switch case body is a Block, never a type-only decl");
            let body_reachable = self.reachable;
            self.switch_depth -= 1;
            let (_n, body_assigned) = self.pop_narrow_frame_capture();
            let typed_body = self.wrap_narrow_regions(typed_body, &true_env, body_span);
            all_assigned.extend(body_assigned);
            any_arm_reachable_exit |= body_reachable;

            typed_cases.push(TypedSwitchCase {
                values: typed_values,
                body: typed_body,
                span: case_span,
            });
        }

        let covered: BTreeSet<narrowing::LiteralValue> = seen.keys().cloned().collect();
        let (residual, site) = self.compute_switch_residual(typed_disc, &disc_ty, &covered);

        let typed_default = if let Some(d) = default {
            let body_span = self.ast.stmt(d.body).span;
            let default_residual =
                if saw_null.is_some() && matches!(site, ResidualSite::Scrutinee { .. }) {
                    narrowing::strip_null(&residual)
                } else {
                    residual.clone()
                };
            let env = self.build_default_narrow_env(&default_residual, &site, body_span);
            self.push_narrow_frame(env.clone());
            self.switch_depth += 1;
            self.reachable = entry_reachable;
            let typed_body = self
                .infer_stmt(d.body)
                .expect("switch default body is a Block, never a type-only decl");
            let body_reachable = self.reachable;
            self.switch_depth -= 1;
            let (_n, body_assigned) = self.pop_narrow_frame_capture();
            let typed_body = self.wrap_narrow_regions(typed_body, &env, body_span);
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
        let frame = self.pop_pending_join_frame();
        if !frame.breaks.is_empty() {
            any_arm_reachable_exit = true;
        }
        self.merge_assigned_into_outer(all_assigned, switch_span);
        let natural =
            (typed_default.is_none() && !matches!(residual, Type::Never)).then_some(entry_env);
        self.fold_exits_into_outer(natural, frame.breaks, switch_span);

        self.reachable = any_arm_reachable_exit;

        TypedStmtKind::Switch {
            discriminant: typed_disc,
            discriminant_ty: disc_ty,
            cases: typed_cases,
            default: typed_default,
        }
    }

    fn push_switch_value_expr(&mut self, value: &TypedSwitchValue) -> ExprId {
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
        self.typed_ast.push_expr(TypedExpr { kind, span, ty })
    }

    fn predicate_env_for_case_value(
        &mut self,
        typed_disc: ExprId,
        disc_source_span: Span,
        value: &TypedSwitchValue,
    ) -> narrowing::NarrowEnv {
        let lit_expr_id = self.push_switch_value_expr(value);
        let synth = self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::Binary {
                op: BinOp::Eq,
                lhs: typed_disc,
                rhs: lit_expr_id,
            },
            span: disc_source_span,
            ty: Type::Boolean,
        });
        self.predicate_envs(synth).0
    }

    fn compute_switch_residual(
        &self,
        typed_disc: ExprId,
        disc_ty: &Type,
        covered: &BTreeSet<narrowing::LiteralValue>,
    ) -> (Type, ResidualSite) {
        let disc_expr = self.typed_ast.expr(typed_disc);
        if let TypedExprKind::FieldAccess { receiver, name } = &disc_expr.kind {
            let receiver_expr = self.typed_ast.expr(*receiver);
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
                if let Some(receiver_path) = self.expr_to_reference_path(receiver_expr) {
                    return (
                        residual,
                        ResidualSite::DiscriminatedReceiver {
                            path: receiver_path,
                            disc_key,
                        },
                    );
                }
                return (residual, ResidualSite::Anonymous);
            }
        }
        if let TypedExprKind::IndexAccess { receiver, index } = &disc_expr.kind {
            let receiver_expr = self.typed_ast.expr(*receiver);
            let index_expr = self.typed_ast.expr(*index);
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
                if let Some(receiver_path) = self.expr_to_reference_path(receiver_expr) {
                    return (
                        residual,
                        ResidualSite::DiscriminatedReceiver {
                            path: receiver_path,
                            disc_key: format!("[{position}]"),
                        },
                    );
                }
                return (residual, ResidualSite::Anonymous);
            }
        }
        let residual = narrowing::subtract_literals(disc_ty, covered);
        if let Some(path) = self.expr_to_reference_path(disc_expr) {
            (residual, ResidualSite::Scrutinee { path })
        } else {
            (residual, ResidualSite::Anonymous)
        }
    }

    fn build_default_narrow_env(
        &mut self,
        residual: &Type,
        site: &ResidualSite,
        body_span: Span,
    ) -> narrowing::NarrowEnv {
        let mut env = narrowing::NarrowEnv::new();
        let path = match site {
            ResidualSite::DiscriminatedReceiver { path, .. } => path.clone(),
            ResidualSite::Scrutinee { path } => path.clone(),
            ResidualSite::Anonymous => return env,
        };
        let source = match self.synthesize_unnarrowed_source(&path, body_span) {
            Some(kind) => self.typed_ast.push_expr(TypedExpr {
                kind,
                span: body_span,
                ty: residual.clone(),
            }),
            None => return env,
        };
        let binding = self.mint_narrow_binding(body_span);
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
        env
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

/// Labels compare runtime values; they do not need the generic identity carried
/// by the discriminant's refinement.
fn switch_label_hint(ty: &Type) -> Type {
    match ty.without_aliases() {
        Type::Refined { ty, .. } => switch_label_hint(ty),
        Type::Union(members) => Type::union(members.iter().map(switch_label_hint).collect()),
        _ => ty.clone(),
    }
}
