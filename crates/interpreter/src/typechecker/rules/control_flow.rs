//! Control-flow analysis for the `missing_return` and `unreachable` rules.

use crate::{
    EnumVariantPayload, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind, TypedSwitchCase,
    TypedSwitchValue, typechecker::infer::narrowing,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum ControlFlow {
    Falls,
    Returns,
}

pub(super) fn control_flow(ta: &TypedAst, id: StmtId) -> ControlFlow {
    match &ta.stmt(id).kind {
        TypedStmtKind::Return(_) => ControlFlow::Returns,
        TypedStmtKind::Block(stmts) => {
            if stmts
                .iter()
                .any(|&s| control_flow(ta, s) == ControlFlow::Returns)
            {
                ControlFlow::Returns
            } else {
                ControlFlow::Falls
            }
        }
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => match else_block {
            Some(eb) => {
                if control_flow(ta, *then_block) == ControlFlow::Returns
                    && control_flow(ta, *eb) == ControlFlow::Returns
                {
                    ControlFlow::Returns
                } else {
                    ControlFlow::Falls
                }
            }
            None => ControlFlow::Falls,
        },
        TypedStmtKind::NarrowRegion { body, .. } => control_flow(ta, *body),
        TypedStmtKind::Switch {
            discriminant,
            cases,
            default,
            ..
        } => {
            let all_cases_return = cases
                .iter()
                .all(|c| control_flow(ta, c.body) == ControlFlow::Returns);
            if !all_cases_return {
                return ControlFlow::Falls;
            }
            match default {
                Some(d) => {
                    if control_flow(ta, *d) == ControlFlow::Returns {
                        ControlFlow::Returns
                    } else {
                        ControlFlow::Falls
                    }
                }
                None => {
                    if switch_is_exhaustive(ta, *discriminant, cases) {
                        ControlFlow::Returns
                    } else {
                        ControlFlow::Falls
                    }
                }
            }
        }
        // Modelled as Returns so throw-based guards contribute to reachability.
        TypedStmtKind::Throw { .. } => ControlFlow::Returns,
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            if let Some(f) = finally
                && control_flow(ta, *f) == ControlFlow::Returns
            {
                return ControlFlow::Returns;
            }
            let body_returns = control_flow(ta, *body) == ControlFlow::Returns;
            let catch_returns = catches
                .iter()
                .all(|c| control_flow(ta, c.body) == ControlFlow::Returns);
            if body_returns && catch_returns {
                ControlFlow::Returns
            } else {
                ControlFlow::Falls
            }
        }
        // Conservative: a `break` may exit before the body completes.
        TypedStmtKind::While { .. }
        | TypedStmtKind::For { .. }
        | TypedStmtKind::ForOf { .. }
        | TypedStmtKind::DoWhile { .. }
        | TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::ReboxLocal { .. }
        | TypedStmtKind::Let { .. }
        | TypedStmtKind::Const { .. }
        | TypedStmtKind::AssignLocal { .. }
        | TypedStmtKind::AssignGlobal { .. }
        | TypedStmtKind::AssignField { .. }
        | TypedStmtKind::AssignIndex { .. }
        | TypedStmtKind::Expr(_) => ControlFlow::Falls,
    }
}

fn switch_is_exhaustive(
    ta: &TypedAst,
    discriminant: crate::ExprId,
    cases: &[TypedSwitchCase],
) -> bool {
    let mut seen: std::collections::BTreeSet<narrowing::LiteralValue> =
        std::collections::BTreeSet::new();
    for case in cases {
        for value in &case.values {
            if let Some(key) = literal_value_of(value) {
                seen.insert(key);
            }
        }
    }
    let disc_expr = ta.expr(discriminant);
    // A `switch` on an enum value is exhaustive when every declared variant is
    // covered, even without a `default:` — the canonical enum-dispatch pattern.
    if let Type::NumberEnum { name, .. } | Type::StringEnum { name, .. } = disc_expr.ty.peel() {
        return enum_is_covered(ta, name, &seen);
    }
    if let TypedExprKind::FieldAccess { receiver, name } = &disc_expr.kind
        && let Type::Union(members) = ta.expr(*receiver).ty.peel()
        && let Some((disc_key, table)) = narrowing::union_discriminant(members)
        && disc_key == name.name
    {
        return table.iter().all(|(lit, _)| seen.contains(lit));
    }
    matches!(
        narrowing::subtract_literals(&disc_expr.ty, &seen),
        Type::Never
    )
}

/// True when `seen` covers every variant of the enum named `name`. An enum with
/// no declared members is never exhaustively covered (it has no values, but a
/// missing declaration shouldn't silently prove the switch returns).
fn enum_is_covered(
    ta: &TypedAst,
    name: &str,
    seen: &std::collections::BTreeSet<narrowing::LiteralValue>,
) -> bool {
    for decl in &ta.types {
        match decl {
            crate::TypedTypeDecl::NumberEnum(d) if d.name.name == name => {
                return !d.members.is_empty()
                    && d.members.iter().all(|m| {
                        seen.contains(&narrowing::LiteralValue::Number(crate::types::LiteralF64(
                            m.value,
                        )))
                    });
            }
            crate::TypedTypeDecl::StringEnum(d) if d.name.name == name => {
                return !d.members.is_empty()
                    && d.members
                        .iter()
                        .all(|m| seen.contains(&narrowing::LiteralValue::String(m.value.clone())));
            }
            _ => {}
        }
    }
    false
}

fn literal_value_of(value: &TypedSwitchValue) -> Option<narrowing::LiteralValue> {
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
