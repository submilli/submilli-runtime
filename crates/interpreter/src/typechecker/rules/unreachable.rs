use super::control_flow::{ControlFlow, control_flow};
use super::declarations::TypeDeclarations;
use crate::{
    ClosureBody, Diagnostic, ExprId, Severity, StmtId, TypedAst, TypedExprKind, TypedStmtKind,
};

pub(super) fn run(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for f in &ta.functions {
        walk(ta, declarations, f.body, diags)?;
    }
    for &id in &ta.top_level_statements {
        walk(ta, declarations, id, diags)?;
    }
    Ok(())
}

fn walk(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    id: StmtId,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match &ta
        .try_stmt(id)
        .map_err(crate::typechecker::arena_failure)?
        .kind
    {
        TypedStmtKind::Block(stmts) => {
            let mut returned_idx: Option<usize> = None;
            for (i, &s) in stmts.iter().enumerate() {
                if returned_idx.is_none()
                    && control_flow(ta, declarations, s)? == ControlFlow::Returns
                {
                    returned_idx = Some(i);
                }
            }
            if let Some(idx) = returned_idx
                && idx + 1 < stmts.len()
            {
                let span = ta
                    .try_stmt(stmts[idx + 1])
                    .map_err(crate::typechecker::arena_failure)?
                    .span;
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    span,
                    message: "unreachable code".to_string(),
                    help: vec![],
                    notes: vec![],
                });
            }
            for &s in stmts {
                walk(ta, declarations, s, diags)?;
            }
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            walk_expr(ta, declarations, *condition, diags)?;
            walk(ta, declarations, *then_block, diags)?;
            if let Some(eb) = else_block {
                walk(ta, declarations, *eb, diags)?;
            }
        }
        TypedStmtKind::While { condition, body } => {
            walk_expr(ta, declarations, *condition, diags)?;
            walk(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                walk(ta, declarations, *i, diags)?;
            }
            if let Some(c) = condition {
                walk_expr(ta, declarations, *c, diags)?;
            }
            if let Some(u) = update {
                walk(ta, declarations, *u, diags)?;
            }
            walk(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::ForOf { iter, body, .. } => {
            walk_expr(ta, declarations, *iter, diags)?;
            walk(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::DoWhile { body, condition } => {
            walk(ta, declarations, *body, diags)?;
            walk_expr(ta, declarations, *condition, diags)?;
        }
        TypedStmtKind::Switch {
            discriminant,
            cases,
            default,
            ..
        } => {
            walk_expr(ta, declarations, *discriminant, diags)?;
            for case in cases {
                walk(ta, declarations, case.body, diags)?;
            }
            if let Some(d) = default {
                walk(ta, declarations, *d, diags)?;
            }
        }
        TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        TypedStmtKind::Return(value) => {
            if let Some(v) = value {
                walk_expr(ta, declarations, *v, diags)?;
            }
        }
        TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => {
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedStmtKind::AssignLocal { value, .. } | TypedStmtKind::AssignGlobal { value, .. } => {
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedStmtKind::AssignField {
            receiver, value, ..
        } => {
            walk_expr(ta, declarations, *receiver, diags)?;
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedStmtKind::AssignIndex {
            receiver,
            index,
            value,
            ..
        } => {
            walk_expr(ta, declarations, *receiver, diags)?;
            walk_expr(ta, declarations, *index, diags)?;
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedStmtKind::NarrowRegion { source, body, .. } => {
            walk_expr(ta, declarations, *source, diags)?;
            walk(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::Throw { value } => walk_expr(ta, declarations, *value, diags)?,
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk(ta, declarations, *body, diags)?;
            for c in catches {
                walk(ta, declarations, c.body, diags)?;
            }
            if let Some(f) = finally {
                walk(ta, declarations, *f, diags)?;
            }
        }
        TypedStmtKind::Expr(e) => walk_expr(ta, declarations, *e, diags)?,
    };
    Ok(())
}

fn walk_expr(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    expr_id: ExprId,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match &ta
        .try_expr(expr_id)
        .map_err(crate::typechecker::arena_failure)?
        .kind
    {
        TypedExprKind::Closure { body, .. } => match body {
            ClosureBody::Expr(e) => walk_expr(ta, declarations, *e, diags)?,
            ClosureBody::Block(b) => walk(ta, declarations, *b, diags)?,
        },
        TypedExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(ta, declarations, *lhs, diags)?;
            walk_expr(ta, declarations, *rhs, diags)?;
        }
        TypedExprKind::EffectThen { effect, result } => {
            walk_expr(ta, declarations, *effect, diags)?;
            walk_expr(ta, declarations, *result, diags)?;
        }
        TypedExprKind::Sequence { stmts, result } => {
            for &stmt in stmts {
                walk(ta, declarations, stmt, diags)?;
            }
            walk_expr(ta, declarations, *result, diags)?;
        }
        TypedExprKind::Unary { operand, .. } => walk_expr(ta, declarations, *operand, diags)?,
        TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedExprKind::Call { args, .. }
        | TypedExprKind::McpCall { args, .. }
        | TypedExprKind::SuperCtorCall { args, .. }
        | TypedExprKind::SuperMethodCall { args, .. } => {
            for &a in args {
                walk_expr(ta, declarations, a, diags)?;
            }
        }
        TypedExprKind::CallClosure { callee, args } => {
            walk_expr(ta, declarations, *callee, diags)?;
            for &a in args {
                walk_expr(ta, declarations, a, diags)?;
            }
        }
        TypedExprKind::GenericCall { args, .. } => {
            for a in args {
                walk_expr(ta, declarations, a.expr, diags)?;
            }
        }
        TypedExprKind::MethodCall { receiver, args, .. } => {
            walk_expr(ta, declarations, *receiver, diags)?;
            for &a in args {
                walk_expr(ta, declarations, a, diags)?;
            }
        }
        TypedExprKind::GenericMethodCall { receiver, args, .. } => {
            walk_expr(ta, declarations, *receiver, diags)?;
            for a in args {
                walk_expr(ta, declarations, a.expr, diags)?;
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for &a in args {
                walk_expr(ta, declarations, a, diags)?;
            }
        }
        TypedExprKind::ObjectLiteral { members, .. } => {
            for member in members {
                for expression in member.expressions() {
                    walk_expr(ta, declarations, expression, diags)?;
                }
            }
        }
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for e in elements {
                walk_expr(ta, declarations, e.expr_id(), diags)?;
            }
        }
        TypedExprKind::TupleLiteral { elements, .. } => {
            for &e in elements {
                walk_expr(ta, declarations, e, diags)?;
            }
        }
        TypedExprKind::FieldAccess { receiver, .. }
        | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
            walk_expr(ta, declarations, *receiver, diags)?;
        }
        TypedExprKind::IndexAccess { receiver, index } => {
            walk_expr(ta, declarations, *receiver, diags)?;
            walk_expr(ta, declarations, *index, diags)?;
        }
        TypedExprKind::Narrowed { source, inner, .. } => {
            walk_expr(ta, declarations, *source, diags)?;
            walk_expr(ta, declarations, *inner, diags)?;
        }
        TypedExprKind::Ternary { cond, then_, else_ } => {
            walk_expr(ta, declarations, *cond, diags)?;
            walk_expr(ta, declarations, *then_, diags)?;
            walk_expr(ta, declarations, *else_, diags)?;
        }
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            walk_expr(ta, declarations, *lhs, diags)?;
            walk_expr(ta, declarations, *rhs, diags)?;
        }
        TypedExprKind::OptionalChain { base, parts } => {
            walk_expr(ta, declarations, *base, diags)?;
            for part in parts {
                match part {
                    crate::TypedChainPart::Index { idx, .. } => {
                        walk_expr(ta, declarations, *idx, diags)?;
                    }
                    crate::TypedChainPart::Call { args, .. }
                    | crate::TypedChainPart::MethodCall { args, .. } => {
                        for a in args {
                            walk_expr(ta, declarations, *a, diags)?;
                        }
                    }
                    crate::TypedChainPart::Field { .. }
                    | crate::TypedChainPart::InterfaceProperty { .. }
                    | crate::TypedChainPart::NonNull { .. } => {}
                }
            }
        }
        TypedExprKind::PostfixUnary { target, .. } => match target {
            crate::PostfixTarget::Field { receiver, .. } => {
                walk_expr(ta, declarations, *receiver, diags)?;
            }
            crate::PostfixTarget::Index {
                receiver, index, ..
            } => {
                walk_expr(ta, declarations, *receiver, diags)?;
                walk_expr(ta, declarations, *index, diags)?;
            }
            crate::PostfixTarget::Local { .. } | crate::PostfixTarget::Global { .. } => {}
        },
        TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedExprKind::Number(_)
        | TypedExprKind::BigInt(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Boolean(_)
        | TypedExprKind::Null
        | TypedExprKind::This
        | TypedExprKind::Regex { .. }
        | TypedExprKind::LocalRef { .. }
        | TypedExprKind::LocalNarrowRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::NumberEnumMember { .. }
        | TypedExprKind::StringEnumMember { .. } => {}
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_util::run;

    #[test]
    fn unreachable_after_return() {
        let diags = run("function f(): number { return 1; let x: number = 2; }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unreachable code");
    }

    #[test]
    fn unreachable_after_if_that_returns_both_branches() {
        let diags = run(
            "function f(b: boolean): number { if (b) { return 1; } else { return 2; } let x: number = 3; }",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unreachable code");
    }

    #[test]
    fn no_unreachable_after_partial_if_return() {
        let diags = run(
            "function f(b: boolean): number { if (b) { return 1; } let x: number = 2; return x; }",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unreachable_inside_nested_block() {
        let diags = run("function f(): number { { return 1; let x: number = 2; } }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unreachable code");
    }

    #[test]
    fn unreachable_after_throw() {
        let diags = run(r#"function f(): void { throw new Error("x"); let y: number = 1; }"#);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unreachable code");
    }
}
