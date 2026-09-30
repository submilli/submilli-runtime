//! Rule: every `switch` case body must terminate with `break` or
//! `return` — no fallthrough.

use super::declarations::TypeDeclarations;
use crate::{Diagnostic, Severity, StmtId, TypedAst, TypedStmtKind};

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
        TypedStmtKind::Switch { cases, default, .. } => {
            for case in cases {
                if !body_terminates(ta, declarations, case.body)? {
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
                walk(ta, declarations, case.body, diags)?;
            }
            if let Some(d) = default {
                walk(ta, declarations, *d, diags)?;
            }
        }
        TypedStmtKind::Block(stmts) => {
            for &s in stmts {
                walk(ta, declarations, s, diags)?;
            }
        }
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => {
            walk(ta, declarations, *then_block, diags)?;
            if let Some(eb) = else_block {
                walk(ta, declarations, *eb, diags)?;
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForOf { body, .. }
        | TypedStmtKind::DoWhile { body, .. } => walk(ta, declarations, *body, diags)?,
        TypedStmtKind::NarrowRegion { body, .. } => walk(ta, declarations, *body, diags)?,
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
    };
    Ok(())
}

fn body_terminates(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    id: StmtId,
) -> Result<bool, crate::compiler_error::CompilerFailure> {
    super::control_flow::case_terminates(ta, declarations, id)
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
