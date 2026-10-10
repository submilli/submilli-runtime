use super::body_walk::{self, Visitor};
use super::control_flow::{ControlFlow, control_flow};
use super::declarations::TypeDeclarations;
use crate::compiler_error::CompilerFailure;
use crate::{Diagnostic, Severity, TypedAst, TypedStmtKind};

pub(super) fn run(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    body_walk::walk_program(
        ta,
        &mut Unreachable {
            ta,
            declarations,
            diags,
        },
    )
}

struct Unreachable<'a, 'd> {
    ta: &'a TypedAst,
    declarations: &'a TypeDeclarations<'d>,
    diags: &'a mut Vec<Diagnostic>,
}

impl Visitor for Unreachable<'_, '_> {
    fn visit_stmt(&mut self, kind: &TypedStmtKind) -> Result<(), CompilerFailure> {
        let TypedStmtKind::Block(stmts) = kind else {
            return Ok(());
        };
        let mut returned_idx: Option<usize> = None;
        for (i, &s) in stmts.iter().enumerate() {
            if control_flow(self.ta, self.declarations, s)? == ControlFlow::Returns {
                returned_idx = Some(i);
                break;
            }
        }
        let Some(&first_unreachable) = returned_idx.and_then(|idx| stmts.get(idx + 1)) else {
            return Ok(());
        };
        let span = self
            .ta
            .try_stmt(first_unreachable)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        self.diags.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: "unreachable code".to_string(),
            help: vec![],
            notes: vec![],
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{run, run_lines};

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

    #[test]
    fn class_members_and_field_initializers_are_checked() {
        let source = "function main(): void { }
class C {
  n: number = 0;
  readonly f: () => void = () => {
    return;
    this.n = 4;
  };
  constructor() {
    return;
    this.n = 1;
  }
  read(): number {
    return this.n;
    this.n = 2;
  }
  set value(v: number) {
    throw new Error(\"x\");
    this.n = v;
  }
}
";
        let unreachable = |line| ("unreachable code".to_string(), line);
        assert_eq!(
            run_lines(source),
            [
                unreachable(10),
                unreachable(14),
                unreachable(18),
                unreachable(6)
            ]
        );
    }
}
