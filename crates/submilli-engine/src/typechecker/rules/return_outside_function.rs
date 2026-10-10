use crate::{Diagnostic, Severity, StmtId, TypedAst, TypedStmtKind};

pub(super) fn run(
    ta: &TypedAst,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for &id in &ta.top_level_statements {
        walk(ta, id, diags)?;
    }
    Ok(())
}

fn walk(
    ta: &TypedAst,
    id: StmtId,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match &ta
        .try_stmt(id)
        .map_err(crate::typechecker::arena_failure)?
        .kind
    {
        TypedStmtKind::Return(_) => {
            diags.push(Diagnostic {
                severity: Severity::Error,
                span: ta
                    .try_stmt(id)
                    .map_err(crate::typechecker::arena_failure)?
                    .span,
                message: "`return` outside function".to_string(),
                help: vec![
                    "wrap the body in a function: `function name(): R { return …; }`".to_string(),
                ],
                notes: vec![],
            });
        }
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => {
            walk(ta, *then_block, diags)?;
            if let Some(eb) = else_block {
                walk(ta, *eb, diags)?;
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForOf { body, .. }
        | TypedStmtKind::DoWhile { body, .. } => walk(ta, *body, diags)?,
        TypedStmtKind::Switch { cases, default, .. } => {
            for case in cases {
                walk(ta, case.body, diags)?;
            }
            if let Some(d) = default {
                walk(ta, *d, diags)?;
            }
        }
        TypedStmtKind::Block(stmts) => {
            for &s in stmts {
                walk(ta, s, diags)?;
            }
        }
        TypedStmtKind::NarrowRegion { body, .. } => walk(ta, *body, diags)?,
        TypedStmtKind::Throw { .. } => {}
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk(ta, *body, diags)?;
            for c in catches {
                walk(ta, c.body, diags)?;
            }
            if let Some(f) = finally {
                walk(ta, *f, diags)?;
            }
        }
        TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        TypedStmtKind::Let { .. }
        | TypedStmtKind::Const { .. }
        | TypedStmtKind::AssignLocal { .. }
        | TypedStmtKind::AssignGlobal { .. }
        | TypedStmtKind::AssignField { .. }
        | TypedStmtKind::AssignIndex { .. }
        | TypedStmtKind::Expr(_) => {}
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_util::run_raw;

    #[test]
    fn return_at_top_level_diagnoses() {
        let diags = run_raw("function main(): void { } return 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "`return` outside function");
    }

    #[test]
    fn return_inside_top_level_if_diagnoses() {
        let diags = run_raw("function main(): void { } if (true) { return 1; }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "`return` outside function");
    }

    #[test]
    fn return_inside_top_level_while_diagnoses() {
        let diags = run_raw("function main(): void { } while (true) { return 1; }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "`return` outside function");
    }

    #[test]
    fn return_inside_function_is_fine() {
        let diags = run_raw("function main(): number { return 1; }");
        assert!(diags.is_empty(), "{diags:?}");
    }
}
