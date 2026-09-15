//! Rule: every `switch` case body must terminate with `break` or
//! `return` — no fallthrough.

use crate::{Diagnostic, Severity, StmtId, TypedAst, TypedStmtKind};

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
        TypedStmtKind::Switch { cases, default, .. } => {
            for case in cases {
                if !body_terminates(ta, case.body) {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        span: case.span,
                        message:
                            "`switch` case body must end with `break` or `return` — no fallthrough"
                                .to_string(),
                        help: vec![
                            "add `break;` at the end of the case body, or `return …;` if the body returns from the enclosing function"
                                .to_string(),
                        ],
                        notes: vec![],
                    });
                }
                walk(ta, case.body, diags);
            }
            if let Some(d) = default {
                walk(ta, *d, diags);
            }
        }
        TypedStmtKind::Block(stmts) => {
            for &s in stmts {
                walk(ta, s, diags);
            }
        }
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => {
            walk(ta, *then_block, diags);
            if let Some(eb) = else_block {
                walk(ta, *eb, diags);
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForOf { body, .. }
        | TypedStmtKind::DoWhile { body, .. } => walk(ta, *body, diags),
        TypedStmtKind::NarrowRegion { body, .. } => walk(ta, *body, diags),
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
        TypedStmtKind::Let { .. }
        | TypedStmtKind::Const { .. }
        | TypedStmtKind::Return(_)
        | TypedStmtKind::Throw { .. }
        | TypedStmtKind::Expr(_)
        | TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::ReboxLocal { .. }
        | TypedStmtKind::AssignLocal { .. }
        | TypedStmtKind::AssignGlobal { .. }
        | TypedStmtKind::AssignField { .. }
        | TypedStmtKind::AssignIndex { .. } => {}
    }
}

fn body_terminates(ta: &TypedAst, id: StmtId) -> bool {
    match &ta.stmt(id).kind {
        TypedStmtKind::Break
        | TypedStmtKind::Return(_)
        | TypedStmtKind::Continue
        | TypedStmtKind::Throw { .. } => true,
        TypedStmtKind::Block(stmts) => stmts.last().is_some_and(|&s| body_terminates(ta, s)),
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => {
            else_block.is_some_and(|eb| body_terminates(ta, *then_block) && body_terminates(ta, eb))
        }
        TypedStmtKind::NarrowRegion { body, .. } => body_terminates(ta, *body),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::run;

    #[test]
    fn fallthrough_diagnoses() {
        let diags = run(
            "function f(x: number): void { switch (x) { case 1: { let y: number = 2; } case 2: break; } }",
        );
        assert!(
            diags.iter().any(|d| d.message.contains("no fallthrough")),
            "expected fallthrough diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn case_ending_in_break_ok() {
        let diags =
            run("function f(x: number): void { switch (x) { case 1: break; case 2: break; } }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn case_ending_in_return_ok() {
        let diags = run(
            "function f(x: number): number { switch (x) { case 1: return 10; case 2: return 20; default: return 0; } }",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn default_does_not_require_terminator() {
        let diags = run(
            "function f(x: number): void { switch (x) { case 1: break; default: { let y: number = 2; } } }",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn if_with_both_branches_returning_ok() {
        let diags = run(
            "function f(x: number, b: boolean): number { switch (x) { case 1: if (b) { return 1; } else { return 2; } default: return 0; } }",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }
}
