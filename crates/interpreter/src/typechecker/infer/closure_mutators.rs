//! Pre-inference walk to collect every binding name reassigned inside an arrow body;
//! used by `Inferer.captured_mutators` to refuse narrowing on captured-mutating paths.

use crate::{Ast, ExprId, StmtId};

pub(super) fn collect_closure_mutators(ast: &Ast) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let stmts = ast.top_level.clone();
    for sid in stmts {
        scan_stmt_for_mutators(ast, sid, false, &mut out);
    }
    out
}

/// No `_` arm: a statement kind that stops the walk hides every arrow below it,
/// and the resulting narrowing is unsound rather than merely imprecise — the
/// enclosing frame reads a stale shadow while the closure has already written
/// the slot. A new statement kind should fail the build here, not go unscanned.
fn scan_stmt_for_mutators(
    ast: &Ast,
    id: StmtId,
    in_closure: bool,
    out: &mut std::collections::BTreeSet<String>,
) {
    use crate::StmtKind;
    match &ast.stmt(id).kind {
        StmtKind::Assign { target, value } | StmtKind::CompoundAssign { target, value, .. } => {
            if in_closure {
                out.insert(target.name.clone());
            }
            scan_expr_for_mutators(ast, *value, in_closure, out);
        }
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => {
            scan_expr_for_mutators(ast, *value, in_closure, out);
        }
        StmtKind::Function { body, .. } => {
            // Regular functions run in a fresh frame — don't flip in_closure.
            scan_stmt_for_mutators(ast, *body, in_closure, out);
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            scan_expr_for_mutators(ast, *condition, in_closure, out);
            scan_stmt_for_mutators(ast, *then_block, in_closure, out);
            if let Some(e) = else_block {
                scan_stmt_for_mutators(ast, *e, in_closure, out);
            }
        }
        StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
            scan_expr_for_mutators(ast, *condition, in_closure, out);
            scan_stmt_for_mutators(ast, *body, in_closure, out);
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            for s in [init, update].into_iter().flatten() {
                scan_stmt_for_mutators(ast, *s, in_closure, out);
            }
            if let Some(c) = condition {
                scan_expr_for_mutators(ast, *c, in_closure, out);
            }
            scan_stmt_for_mutators(ast, *body, in_closure, out);
        }
        StmtKind::ForOf { iter, body, .. } => {
            scan_expr_for_mutators(ast, *iter, in_closure, out);
            scan_stmt_for_mutators(ast, *body, in_closure, out);
        }
        StmtKind::Switch {
            discriminant,
            cases,
            default,
        } => {
            scan_expr_for_mutators(ast, *discriminant, in_closure, out);
            for case in cases {
                for &value in &case.values {
                    scan_expr_for_mutators(ast, value, in_closure, out);
                }
                scan_stmt_for_mutators(ast, case.body, in_closure, out);
            }
            if let Some(d) = default {
                scan_stmt_for_mutators(ast, d.body, in_closure, out);
            }
        }
        StmtKind::Try {
            body,
            catches,
            finally,
        } => {
            scan_stmt_for_mutators(ast, *body, in_closure, out);
            for clause in catches {
                scan_stmt_for_mutators(ast, clause.body, in_closure, out);
            }
            if let Some(f) = finally {
                scan_stmt_for_mutators(ast, *f, in_closure, out);
            }
        }
        StmtKind::Return(value) => {
            if let Some(v) = value {
                scan_expr_for_mutators(ast, *v, in_closure, out);
            }
        }
        StmtKind::Throw { value } | StmtKind::Expr(value) => {
            scan_expr_for_mutators(ast, *value, in_closure, out);
        }
        StmtKind::Block(stmts) => {
            for &s in stmts {
                scan_stmt_for_mutators(ast, s, in_closure, out);
            }
        }
        StmtKind::AssignField {
            receiver, value, ..
        }
        | StmtKind::CompoundAssignField {
            receiver, value, ..
        } => {
            scan_expr_for_mutators(ast, *receiver, in_closure, out);
            scan_expr_for_mutators(ast, *value, in_closure, out);
        }
        StmtKind::AssignIndex {
            receiver,
            index,
            value,
        }
        | StmtKind::CompoundAssignIndex {
            receiver,
            index,
            value,
            ..
        } => {
            scan_expr_for_mutators(ast, *receiver, in_closure, out);
            scan_expr_for_mutators(ast, *index, in_closure, out);
            scan_expr_for_mutators(ast, *value, in_closure, out);
        }
        StmtKind::ClassDecl { members, .. } => {
            for member in members {
                match member {
                    crate::ClassMember::Method { body, .. }
                    | crate::ClassMember::Constructor { body, .. }
                    | crate::ClassMember::Accessor { body, .. } => {
                        scan_stmt_for_mutators(ast, *body, in_closure, out);
                    }
                    crate::ClassMember::Field { initializer, .. } => {
                        if let Some(init) = initializer {
                            scan_expr_for_mutators(ast, *init, in_closure, out);
                        }
                    }
                }
            }
        }
        // Type space, control transfer with no operand, or lowered away before
        // inference (`lower_patterns`): nothing to walk.
        StmtKind::Break
        | StmtKind::Continue
        | StmtKind::LetPattern { .. }
        | StmtKind::ConstPattern { .. }
        | StmtKind::ConstRest { .. }
        | StmtKind::ForOfPattern { .. }
        | StmtKind::InterfaceDecl { .. }
        | StmtKind::EnumDecl { .. }
        | StmtKind::TypeAliasDecl { .. }
        | StmtKind::Import { .. }
        | StmtKind::ExportFrom { .. } => {}
    }
}

/// Exhaustive for the same reason as [`scan_stmt_for_mutators`]: an arrow can
/// hide under any sub-expression.
fn scan_expr_for_mutators(
    ast: &Ast,
    id: ExprId,
    in_closure: bool,
    out: &mut std::collections::BTreeSet<String>,
) {
    use crate::{ArrowBody, ChainPart, ExprKind};
    match &ast.expr(id).kind {
        ExprKind::Arrow { body, .. } => match body {
            ArrowBody::Expr(e) => scan_expr_for_mutators(ast, *e, true, out),
            ArrowBody::Block(b) => scan_stmt_for_mutators(ast, *b, true, out),
        },
        // `x++` and `x--` write `x` exactly as `x = x + 1` does. `x!` is the
        // third `PostfixOp` and is a pure read — counting it would refuse
        // narrowing on every binding a closure merely asserts non-null.
        ExprKind::PostfixUnary { op, operand } => {
            if in_closure
                && matches!(op, crate::PostfixOp::Inc | crate::PostfixOp::Dec)
                && let ExprKind::Identifier(ident) = &ast.expr(*operand).kind
            {
                out.insert(ident.name.clone());
            }
            scan_expr_for_mutators(ast, *operand, in_closure, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            scan_expr_for_mutators(ast, *lhs, in_closure, out);
            scan_expr_for_mutators(ast, *rhs, in_closure, out);
        }
        ExprKind::Unary { operand: inner, .. }
        | ExprKind::Typeof { operand: inner }
        | ExprKind::As { expr: inner, .. }
        | ExprKind::InstanceOf { value: inner, .. }
        | ExprKind::Paren(inner)
        | ExprKind::FieldAccess {
            receiver: inner, ..
        } => {
            scan_expr_for_mutators(ast, *inner, in_closure, out);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            scan_expr_for_mutators(ast, *callee, in_closure, out);
            for &a in args {
                scan_expr_for_mutators(ast, a, in_closure, out);
            }
        }
        ExprKind::ObjectLiteral { members } => {
            for m in members {
                scan_expr_for_mutators(ast, m.value(), in_closure, out);
            }
        }
        ExprKind::ArrayLiteral { elements } => {
            for e in elements {
                scan_expr_for_mutators(ast, e.value(), in_closure, out);
            }
        }
        ExprKind::IndexAccess { receiver, index } => {
            scan_expr_for_mutators(ast, *receiver, in_closure, out);
            scan_expr_for_mutators(ast, *index, in_closure, out);
        }
        ExprKind::TemplateLiteral { exprs, .. } => {
            for &e in exprs {
                scan_expr_for_mutators(ast, e, in_closure, out);
            }
        }
        ExprKind::Ternary { cond, then_, else_ } => {
            for &e in [cond, then_, else_] {
                scan_expr_for_mutators(ast, e, in_closure, out);
            }
        }
        ExprKind::OptionalChain { base, parts } => {
            scan_expr_for_mutators(ast, *base, in_closure, out);
            for part in parts {
                match part {
                    ChainPart::Index { idx, .. } => {
                        scan_expr_for_mutators(ast, *idx, in_closure, out);
                    }
                    ChainPart::Call { args, .. } => {
                        for &a in args {
                            scan_expr_for_mutators(ast, a, in_closure, out);
                        }
                    }
                    ChainPart::Field { .. } | ChainPart::NonNull { .. } => {}
                }
            }
        }
        // Leaves: no sub-expression to walk.
        ExprKind::Number(_)
        | ExprKind::BigInt(_)
        | ExprKind::String(_)
        | ExprKind::Boolean(_)
        | ExprKind::Null
        | ExprKind::Identifier(_)
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::Regex { .. } => {}
    }
}
