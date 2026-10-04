//! Rule: every `switch` case body must terminate with `break` or
//! `return` — no fallthrough.

use super::body_walk::{self, Visitor};
use super::control_flow::case_terminates;
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
        &mut Fallthrough {
            ta,
            declarations,
            diags,
        },
    )
}

struct Fallthrough<'a, 'd> {
    ta: &'a TypedAst,
    declarations: &'a TypeDeclarations<'d>,
    diags: &'a mut Vec<Diagnostic>,
}

impl Visitor for Fallthrough<'_, '_> {
    fn visit_stmt(&mut self, kind: &TypedStmtKind) -> Result<(), CompilerFailure> {
        let TypedStmtKind::Switch { cases, .. } = kind else {
            return Ok(());
        };
        for case in cases {
            if case_terminates(self.ta, self.declarations, case.body)? {
                continue;
            }
            self.diags.push(Diagnostic {
                severity: Severity::Error,
                span: case.span,
                message: "`switch` case body must end with `break` or `return` — no fallthrough"
                    .to_string(),
                help: vec![
                    "add `break;` at the end of the case body, or `return …;` if the body returns from the enclosing function"
                        .to_string(),
                ],
                notes: vec![],
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{run, run_lines};

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

    #[test]
    fn class_members_and_closures_are_checked() {
        let source = "function main(): void { }
class C {
  n: number = 0;
  readonly f: (x: number) => void = (x: number): void => {
    switch (x) {
      case 1:
        this.n = 1;
      default:
        break;
    }
  };
  constructor() {
    switch (this.n) {
      case 0:
        this.n = 1;
      default:
        break;
    }
  }
  route(x: number): void {
    const handle = (): void => {
      switch (x) {
        case 1:
          this.n = 2;
        default:
          break;
      }
    };
    handle();
  }
  get label(): string {
    switch (this.n) {
      case 0:
        this.n = 1;
      default:
        return \"x\";
    }
    return \"y\";
  }
  set value(v: number) {
    switch (v) {
      case 0:
        this.n = 0;
      default:
        this.n = v;
    }
  }
}
";
        let lines: Vec<usize> = run_lines(source)
            .into_iter()
            .map(|(message, line)| {
                assert!(message.contains("no fallthrough"), "{message}");
                line
            })
            .collect();
        assert_eq!(lines, [14, 23, 33, 42, 6]);
    }
}
