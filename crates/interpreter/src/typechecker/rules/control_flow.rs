//! Completion analysis shared by return, reachability, and switch-fallthrough rules.

use super::declarations::TypeDeclarations;
use crate::{
    EnumVariantPayload, MangledName, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind,
    TypedSwitchCase, TypedSwitchValue, typechecker::infer::narrowing,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum ControlFlow {
    Falls,
    Returns,
}

pub(super) fn control_flow(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    id: StmtId,
) -> Result<ControlFlow, crate::compiler_error::CompilerFailure> {
    let flow = completion(ta, declarations, id)?;
    Ok(if flow.falls || flow.breaks || flow.continues {
        ControlFlow::Falls
    } else {
        ControlFlow::Returns
    })
}

pub(super) fn case_terminates(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    id: StmtId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    Ok(!completion(ta, declarations, id)?.falls)
}

#[derive(Clone, Copy, Default)]
struct Completion {
    falls: bool,
    breaks: bool,
    continues: bool,
}

impl Completion {
    fn union(self, other: Self) -> Self {
        Self {
            falls: self.falls || other.falls,
            breaks: self.breaks || other.breaks,
            continues: self.continues || other.continues,
        }
    }

    fn then(self, next: Self) -> Self {
        Self {
            falls: self.falls && next.falls,
            breaks: self.breaks || (self.falls && next.breaks),
            continues: self.continues || (self.falls && next.continues),
        }
    }
}

fn completion(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    id: StmtId,
) -> Result<Completion, crate::compiler_error::CompilerFailure> {
    let falls = Completion {
        falls: true,
        ..Default::default()
    };
    Ok(
        match &ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedStmtKind::Break => Completion {
                breaks: true,
                ..Default::default()
            },
            TypedStmtKind::Return(_) | TypedStmtKind::Throw { .. } => Completion::default(),
            TypedStmtKind::Continue => Completion {
                continues: true,
                ..Default::default()
            },
            TypedStmtKind::Block(stmts) => stmts.iter().try_fold(falls, |flow, &stmt| {
                Ok::<_, crate::compiler_error::CompilerFailure>(flow.then(completion(
                    ta,
                    declarations,
                    stmt,
                )?))
            })?,
            TypedStmtKind::If {
                then_block,
                else_block,
                ..
            } => completion(ta, declarations, *then_block)?.union(
                else_block
                    .map(|body| completion(ta, declarations, body))
                    .transpose()?
                    .unwrap_or(falls),
            ),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => try_completion(ta, declarations, *body, catches, *finally)?,
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => switch_completion(ta, declarations, *discriminant, cases, *default)?,
            TypedStmtKind::NarrowRegion { body, .. } => completion(ta, declarations, *body)?,
            TypedStmtKind::While { condition, body } => {
                let runs_forever = is_static_true(ta, *condition)?;
                loop_completion(ta, declarations, *body, runs_forever)?
            }
            TypedStmtKind::For {
                condition, body, ..
            } => {
                let runs_forever = condition.map_or(Ok(true), |c| is_static_true(ta, c))?;
                loop_completion(ta, declarations, *body, runs_forever)?
            }
            TypedStmtKind::DoWhile { body, condition } => {
                let pass = completion(ta, declarations, *body)?;
                let repeats = pass.falls || pass.continues;
                let exits_by_condition = repeats && !is_static_true(ta, *condition)?;
                Completion {
                    falls: exits_by_condition || pass.breaks,
                    ..Default::default()
                }
            }
            TypedStmtKind::ForOf { .. }
            | TypedStmtKind::ReboxLocal { .. }
            | TypedStmtKind::Let { .. }
            | TypedStmtKind::Const { .. }
            | TypedStmtKind::AssignLocal { .. }
            | TypedStmtKind::AssignGlobal { .. }
            | TypedStmtKind::AssignField { .. }
            | TypedStmtKind::AssignIndex { .. }
            | TypedStmtKind::Expr(_) => falls,
        },
    )
}

/// A `while`/`for` loop ends normally when its condition can fail, or when its
/// body breaks out of it. A `break` inside a nested loop or `switch` is that
/// statement's own, so it never reaches here.
fn loop_completion(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    body: StmtId,
    runs_forever: bool,
) -> Result<Completion, crate::compiler_error::CompilerFailure> {
    let breaks = completion(ta, declarations, body)?.breaks;
    Ok(Completion {
        falls: !runs_forever || breaks,
        ..Default::default()
    })
}

/// Whether a loop condition is the literal `true`, as the inferer's
/// reachability also treats it, seen through any narrowing wrappers.
fn is_static_true(
    ta: &TypedAst,
    condition: crate::ExprId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let mut id = condition;
    loop {
        match &ta
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedExprKind::Boolean(value) => return Ok(*value),
            TypedExprKind::Narrowed { inner, .. } => id = *inner,
            _ => return Ok(false),
        }
    }
}

fn try_completion(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    body: StmtId,
    catches: &[crate::TypedCatchClause],
    finally: Option<StmtId>,
) -> Result<Completion, crate::compiler_error::CompilerFailure> {
    let flow = catches
        .iter()
        .try_fold(completion(ta, declarations, body)?, |flow, catch| {
            Ok::<_, crate::compiler_error::CompilerFailure>(flow.union(completion(
                ta,
                declarations,
                catch.body,
            )?))
        })?;
    let Some(finally) = finally else {
        return Ok(flow);
    };
    let cleanup = completion(ta, declarations, finally)?;
    // A control transfer from finally replaces the pending completion.
    let preserved = if cleanup.falls {
        flow
    } else {
        Completion::default()
    };
    Ok(preserved.union(Completion {
        falls: false,
        ..cleanup
    }))
}

fn switch_completion(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    discriminant: crate::ExprId,
    cases: &[crate::TypedSwitchCase],
    default: Option<StmtId>,
) -> Result<Completion, crate::compiler_error::CompilerFailure> {
    let mut flow = cases.iter().try_fold(Completion::default(), |flow, case| {
        Ok::<_, crate::compiler_error::CompilerFailure>(flow.union(completion(
            ta,
            declarations,
            case.body,
        )?))
    })?;
    if let Some(default) = default {
        flow = flow.union(completion(ta, declarations, default)?);
    } else if !switch_is_exhaustive(ta, declarations, discriminant, cases)? {
        flow.falls = true;
    }
    // A break exits this nested switch, not the containing case.
    Ok(Completion {
        falls: flow.falls || flow.breaks,
        breaks: false,
        continues: flow.continues,
    })
}

fn switch_is_exhaustive(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    discriminant: crate::ExprId,
    cases: &[TypedSwitchCase],
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    let mut seen: std::collections::BTreeSet<narrowing::LiteralValue> =
        std::collections::BTreeSet::new();
    for case in cases {
        for value in &case.values {
            if let Some(key) = literal_value_of(value) {
                seen.insert(key);
            }
        }
    }
    let disc_expr = ta
        .try_expr(discriminant)
        .map_err(crate::typechecker::arena_failure)?;
    // A `switch` on an enum value is exhaustive when every declared variant is
    // covered, even without a `default:` — the canonical enum-dispatch pattern.
    if let Type::NumberEnum { mangled, name, .. } | Type::StringEnum { mangled, name, .. } =
        disc_expr.ty.peel()
    {
        return Ok(enum_is_covered(declarations, mangled, name, &seen));
    }
    if let TypedExprKind::FieldAccess { receiver, name } = &disc_expr.kind
        && let Type::Union(members) = ta
            .try_expr(*receiver)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .peel()
        && let Some((disc_key, table)) = narrowing::union_discriminant(members)
        && disc_key == name.name
    {
        return Ok(table.iter().all(|(lit, _)| seen.contains(lit)));
    }
    Ok(matches!(
        narrowing::subtract_literals(&disc_expr.ty, &seen),
        Type::Never
    ))
}

/// True when `seen` covers every variant of the enum. An enum with no declared
/// members is never exhaustively covered (it has no values, but a missing
/// declaration shouldn't silently prove the switch returns).
fn enum_is_covered(
    declarations: &TypeDeclarations<'_>,
    mangled: &MangledName,
    name: &str,
    seen: &std::collections::BTreeSet<narrowing::LiteralValue>,
) -> bool {
    declarations
        .enum_values(mangled, name)
        .is_some_and(|values| !values.is_empty() && values.iter().all(|v| seen.contains(v)))
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
