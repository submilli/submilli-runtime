use super::control_flow::{ControlFlow, control_flow};
use crate::{
    ClosureBody, Diagnostic, ExprId, Severity, StmtId, TypedAst, TypedExprKind, TypedStmtKind,
};

pub(super) fn run(ta: &TypedAst, diags: &mut Vec<Diagnostic>) {
    for f in &ta.functions {
        walk(ta, f.body, diags);
    }
    for &id in &ta.top_level_statements {
        walk(ta, id, diags);
    }
}

fn walk(ta: &TypedAst, id: StmtId, diags: &mut Vec<Diagnostic>) {
    match &ta.stmt(id).kind {
        TypedStmtKind::Block(stmts) => {
            let mut returned_idx: Option<usize> = None;
            for (i, &s) in stmts.iter().enumerate() {
                if returned_idx.is_none() && control_flow(ta, s) == ControlFlow::Returns {
                    returned_idx = Some(i);
                }
            }
            if let Some(idx) = returned_idx
                && idx + 1 < stmts.len()
            {
                let span = ta.stmt(stmts[idx + 1]).span;
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    span,
                    message: "unreachable code".to_string(),
                    help: vec![],
                    notes: vec![],
                });
            }
            for &s in stmts {
                walk(ta, s, diags);
            }
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            walk_expr(ta, *condition, diags);
            walk(ta, *then_block, diags);
            if let Some(eb) = else_block {
                walk(ta, *eb, diags);
            }
        }
        TypedStmtKind::While { condition, body } => {
            walk_expr(ta, *condition, diags);
            walk(ta, *body, diags);
        }
        TypedStmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                walk(ta, *i, diags);
            }
            if let Some(c) = condition {
                walk_expr(ta, *c, diags);
            }
            if let Some(u) = update {
                walk(ta, *u, diags);
            }
            walk(ta, *body, diags);
        }
        TypedStmtKind::ForOf { iter, body, .. } => {
            walk_expr(ta, *iter, diags);
            walk(ta, *body, diags);
        }
        TypedStmtKind::DoWhile { body, condition } => {
            walk(ta, *body, diags);
            walk_expr(ta, *condition, diags);
        }
        TypedStmtKind::Switch {
            discriminant,
            cases,
            default,
            ..
        } => {
            walk_expr(ta, *discriminant, diags);
            for case in cases {
                walk(ta, case.body, diags);
            }
            if let Some(d) = default {
                walk(ta, *d, diags);
            }
        }
        TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        TypedStmtKind::Return(value) => {
            if let Some(v) = value {
                walk_expr(ta, *v, diags);
            }
        }
        TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => {
            walk_expr(ta, *value, diags);
        }
        TypedStmtKind::AssignLocal { value, .. } | TypedStmtKind::AssignGlobal { value, .. } => {
            walk_expr(ta, *value, diags);
        }
        TypedStmtKind::AssignField {
            receiver, value, ..
        } => {
            walk_expr(ta, *receiver, diags);
            walk_expr(ta, *value, diags);
        }
        TypedStmtKind::AssignIndex {
            receiver,
            index,
            value,
            ..
        } => {
            walk_expr(ta, *receiver, diags);
            walk_expr(ta, *index, diags);
            walk_expr(ta, *value, diags);
        }
        TypedStmtKind::NarrowRegion { source, body, .. } => {
            walk_expr(ta, *source, diags);
            walk(ta, *body, diags);
        }
        TypedStmtKind::Throw { value } => walk_expr(ta, *value, diags),
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk(ta, *body, diags);
            for c in catches {
                walk(ta, c.body, diags);
            }
            if let Some(f) = finally {
                walk(ta, *f, diags);
            }
        }
        TypedStmtKind::Expr(e) => walk_expr(ta, *e, diags),
    }
}

fn walk_expr(ta: &TypedAst, expr_id: ExprId, diags: &mut Vec<Diagnostic>) {
    match &ta.expr(expr_id).kind {
        TypedExprKind::Closure { body, .. } => match body {
            ClosureBody::Expr(e) => walk_expr(ta, *e, diags),
            ClosureBody::Block(b) => walk(ta, *b, diags),
        },
        TypedExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(ta, *lhs, diags);
            walk_expr(ta, *rhs, diags);
        }
        TypedExprKind::EffectThen { effect, result } => {
            walk_expr(ta, *effect, diags);
            walk_expr(ta, *result, diags);
        }
        TypedExprKind::Unary { operand, .. } => walk_expr(ta, *operand, diags),
        TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
            walk_expr(ta, *value, diags);
        }
        TypedExprKind::Call { args, .. }
        | TypedExprKind::McpCall { args, .. }
        | TypedExprKind::SuperCtorCall { args, .. }
        | TypedExprKind::SuperMethodCall { args, .. } => {
            for &a in args {
                walk_expr(ta, a, diags);
            }
        }
        TypedExprKind::CallClosure { callee, args } => {
            walk_expr(ta, *callee, diags);
            for &a in args {
                walk_expr(ta, a, diags);
            }
        }
        TypedExprKind::GenericCall { args, .. } => {
            for a in args {
                walk_expr(ta, a.expr, diags);
            }
        }
        TypedExprKind::MethodCall { receiver, args, .. } => {
            walk_expr(ta, *receiver, diags);
            for &a in args {
                walk_expr(ta, a, diags);
            }
        }
        TypedExprKind::GenericMethodCall { receiver, args, .. } => {
            walk_expr(ta, *receiver, diags);
            for a in args {
                walk_expr(ta, a.expr, diags);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for &a in args {
                walk_expr(ta, a, diags);
            }
        }
        TypedExprKind::ObjectLiteral { members, .. } => {
            for member in members {
                walk_expr(ta, member.expr_id(), diags);
            }
        }
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for e in elements {
                walk_expr(ta, e.expr_id(), diags);
            }
        }
        TypedExprKind::TupleLiteral { elements, .. } => {
            for &e in elements {
                walk_expr(ta, e, diags);
            }
        }
        TypedExprKind::FieldAccess { receiver, .. }
        | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
            walk_expr(ta, *receiver, diags);
        }
        TypedExprKind::IndexAccess { receiver, index } => {
            walk_expr(ta, *receiver, diags);
            walk_expr(ta, *index, diags);
        }
        TypedExprKind::Narrowed { source, inner, .. } => {
            walk_expr(ta, *source, diags);
            walk_expr(ta, *inner, diags);
        }
        TypedExprKind::Ternary { cond, then_, else_ } => {
            walk_expr(ta, *cond, diags);
            walk_expr(ta, *then_, diags);
            walk_expr(ta, *else_, diags);
        }
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            walk_expr(ta, *lhs, diags);
            walk_expr(ta, *rhs, diags);
        }
        TypedExprKind::OptionalChain { base, parts } => {
            walk_expr(ta, *base, diags);
            for part in parts {
                match part {
                    crate::TypedChainPart::Index { idx, .. } => walk_expr(ta, *idx, diags),
                    crate::TypedChainPart::Call { args, .. }
                    | crate::TypedChainPart::MethodCall { args, .. } => {
                        for a in args {
                            walk_expr(ta, *a, diags);
                        }
                    }
                    crate::TypedChainPart::Field { .. }
                    | crate::TypedChainPart::InterfaceProperty { .. }
                    | crate::TypedChainPart::NonNull { .. } => {}
                }
            }
        }
        TypedExprKind::PostfixUnary { target, .. } => match target {
            crate::PostfixTarget::Field { receiver, .. } => walk_expr(ta, *receiver, diags),
            crate::PostfixTarget::Index {
                receiver, index, ..
            } => {
                walk_expr(ta, *receiver, diags);
                walk_expr(ta, *index, diags);
            }
            crate::PostfixTarget::Local { .. } | crate::PostfixTarget::Global { .. } => {}
        },
        TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
            walk_expr(ta, *value, diags);
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
    }
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
