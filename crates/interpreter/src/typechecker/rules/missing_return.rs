use super::body_walk::{self, Visitor};
use super::control_flow::{ControlFlow, control_flow};
use super::declarations::TypeDeclarations;
use crate::compiler_error::CompilerFailure;
use crate::{
    ClosureBody, Diagnostic, ExprId, Severity, Span, StmtId, Type, TypedAst, TypedClassAccessor,
    TypedClassDecl, TypedStmtKind, TypedTypeDecl,
};

/// Named functions and class members are checked here; the closures nested
/// anywhere in the program are found by the shared walk.
pub(super) fn run(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    let mut rule = MissingReturn {
        ta,
        declarations,
        diags,
    };
    for f in &ta.functions {
        let subject = format!("function `{}`", f.name.name);
        rule.check(f.body, &f.return_type, f.name.span, &subject)?;
    }
    for decl in &ta.types {
        if let TypedTypeDecl::Class(class) = decl {
            rule.check_class_members(class)?;
        }
    }
    body_walk::walk_program(ta, &mut rule)
}

struct MissingReturn<'a, 'd> {
    ta: &'a TypedAst,
    declarations: &'a TypeDeclarations<'d>,
    diags: &'a mut Vec<Diagnostic>,
}

impl MissingReturn<'_, '_> {
    /// Methods and getters; a constructor or setter returns no value.
    fn check_class_members(&mut self, class: &TypedClassDecl) -> Result<(), CompilerFailure> {
        let class_name = &class.name.name;
        for method in &class.methods {
            let subject = format!("method `{class_name}.{}`", method.name.name);
            self.check(method.body, &method.return_type, method.name.span, &subject)?;
        }
        for accessor in &class.accessors {
            let TypedClassAccessor::Getter {
                name, ret_ty, body, ..
            } = accessor
            else {
                continue;
            };
            let subject = format!("getter `{class_name}.{}`", name.name);
            self.check(*body, ret_ty, name.span, &subject)?;
        }
        Ok(())
    }

    /// Reports `subject` — "function `f`", "method `C.m`" — when `body` can
    /// end without returning the value `return_type` requires.
    fn check(
        &mut self,
        body: StmtId,
        return_type: &Type,
        span: Span,
        subject: &str,
    ) -> Result<(), CompilerFailure> {
        let return_type = return_type.peel();
        if matches!(return_type, Type::Void | Type::Error) {
            return Ok(());
        }
        if control_flow(self.ta, self.declarations, body)? != ControlFlow::Falls {
            return Ok(());
        }
        // `unknown` admits the `undefined` that falling off the end returns
        // (`null` here), so as in TypeScript only a body that never returns
        // is a mistake.
        let returns_unknown = matches!(return_type, Type::Unknown);
        if returns_unknown && contains_return(self.ta, body)? {
            return Ok(());
        }
        let (message, help) = if returns_unknown {
            (
                format!("{subject} returns `unknown` but has no `return`"),
                vec![
                    "add a `return` with a value, or a bare `return;`, which yields `null`"
                        .to_string(),
                ],
            )
        } else {
            (
                format!("{subject} does not return a value on all paths"),
                vec![],
            )
        };
        self.diags.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            help,
            notes: vec![],
        });
        Ok(())
    }
}

impl Visitor for MissingReturn<'_, '_> {
    fn visit_closure(
        &mut self,
        id: ExprId,
        return_type: &Type,
        body: &ClosureBody,
    ) -> Result<(), CompilerFailure> {
        let ClosureBody::Block(body) = body else {
            return Ok(());
        };
        if self.ta.placeholder_closures.contains(&id) {
            return Ok(());
        }
        let (span, subject) = match self.ta.nested_function_names.get(&id) {
            Some(name) => (name.span, format!("function `{}`", name.name)),
            None => (
                self.ta
                    .try_expr(id)
                    .map_err(crate::typechecker::arena_failure)?
                    .span,
                "arrow function".to_string(),
            ),
        };
        self.check(*body, return_type, span, &subject)
    }
}

/// Whether `stmt_id` holds a `return` of its own body; one inside a nested
/// closure returns from the closure instead.
fn contains_return(ta: &TypedAst, stmt_id: StmtId) -> Result<bool, CompilerFailure> {
    let kind = &ta
        .try_stmt(stmt_id)
        .map_err(crate::typechecker::arena_failure)?
        .kind;
    let found = match kind {
        TypedStmtKind::Return(_) => true,
        TypedStmtKind::Block(stmts) => any_contains_return(ta, stmts.iter().copied())?,
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => any_contains_return(ta, std::iter::once(*then_block).chain(*else_block))?,
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::DoWhile { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForOf { body, .. }
        | TypedStmtKind::NarrowRegion { body, .. } => contains_return(ta, *body)?,
        TypedStmtKind::Switch { cases, default, .. } => {
            any_contains_return(ta, cases.iter().map(|case| case.body).chain(*default))?
        }
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            let catches = catches.iter().map(|catch| catch.body);
            any_contains_return(ta, std::iter::once(*body).chain(catches).chain(*finally))?
        }
        TypedStmtKind::Let { .. }
        | TypedStmtKind::Const { .. }
        | TypedStmtKind::Expr(_)
        | TypedStmtKind::Throw { .. }
        | TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::ReboxLocal { .. }
        | TypedStmtKind::AssignLocal { .. }
        | TypedStmtKind::AssignGlobal { .. }
        | TypedStmtKind::AssignField { .. }
        | TypedStmtKind::AssignIndex { .. } => false,
    };
    Ok(found)
}

fn any_contains_return(
    ta: &TypedAst,
    stmts: impl IntoIterator<Item = StmtId>,
) -> Result<bool, CompilerFailure> {
    for stmt in stmts {
        if contains_return(ta, stmt)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{infer_script_with, run, run_lines, run_package};

    #[test]
    fn void_no_return() {
        let diags = run("function f(): void { }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn void_with_return() {
        let diags = run("function f(): void { return; }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_void_returns_all_paths() {
        let diags = run("function f(): number { return 1; }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_void_missing_return() {
        let diags = run("function f(): number { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `f` does not return a value on all paths"
        );
    }

    #[test]
    fn if_else_both_return() {
        let diags =
            run("function f(b: boolean): number { if (b) { return 1; } else { return 2; } }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn if_else_only_then_returns() {
        let diags = run("function f(b: boolean): number { if (b) { return 1; } else { } }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `f` does not return a value on all paths"
        );
    }

    #[test]
    fn if_else_only_else_returns() {
        let diags = run("function f(b: boolean): number { if (b) { } else { return 2; } }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `f` does not return a value on all paths"
        );
    }

    #[test]
    fn if_no_else_with_return() {
        let diags = run("function f(b: boolean): number { if (b) { return 1; } }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `f` does not return a value on all paths"
        );
    }

    /// A `while (true)` that never breaks can only leave by its `return`, as tsc
    /// also concludes.
    #[test]
    fn while_true_with_return_inside_returns_on_all_paths() {
        let diags = run("function f(): number { while (true) { return 1; } }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn while_true_that_breaks_diagnoses() {
        let diags =
            run("function f(b: boolean): number { while (true) { if (b) { break; } return 1; } }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `f` does not return a value on all paths"
        );
    }

    #[test]
    fn while_with_a_condition_diagnoses() {
        let diags = run("function f(b: boolean): number { while (b) { return 1; } }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `f` does not return a value on all paths"
        );
    }

    #[test]
    fn nested_block_with_return() {
        let diags = run("function f(): number { { return 1; } }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn error_return_type_no_cascade() {
        let diags = run("function f(): foo { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unknown type `foo`");
    }

    #[test]
    fn multiple_functions_independent() {
        let diags = run("function ok(): number { return 1; } function bad(): number { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "function `bad` does not return a value on all paths"
        );
    }

    #[test]
    fn arrow_block_body_missing_return_diagnoses() {
        let diags = run("function host(): void { let f = (x: number): number => { let y = x; }; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("arrow function does not return")),
            "expected arrow missing-return diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_block_body_with_return_ok() {
        let diags = run("function host(): void { let f = (x: number): number => { return x; }; }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn arrow_expression_body_skipped() {
        let diags = run("function host(): void { let f = (x: number): number => x * 2; }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn arrow_void_block_body_skipped() {
        let diags =
            run("function host(): void { let f = (x: number) => { console.log(x.toString()); }; }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    const LEVELS: &str = "/** Levels. */\nexport enum Level { Low, High }\n";

    const SWITCH_OVER_LEVEL: &str = "function name(level: Level): string {\n\
           switch (level) {\n\
             case Level.Low: return \"low\";\n\
             case Level.High: return \"high\";\n\
           }\n\
         }\n";

    fn package_messages(
        modules: &[(&str, &str)],
        dependencies: &[crate::PackageDeclaration],
    ) -> Vec<String> {
        let (_, _, diags) = run_package("@test/package", modules, dependencies);
        diags.into_iter().map(|diag| diag.message).collect()
    }

    #[test]
    fn a_package_function_missing_a_return_diagnoses() {
        let messages = package_messages(
            &[(
                "lib",
                "function f(b: boolean): number { if (b) { return 1; } }\n",
            )],
            &[],
        );
        assert_eq!(
            messages,
            ["function `f` does not return a value on all paths"]
        );
    }

    #[test]
    fn a_switch_over_an_enum_of_another_module_is_exhaustive() {
        let lib = format!("import {{ Level }} from \"./levels\";\n{SWITCH_OVER_LEVEL}");
        let messages = package_messages(
            &[
                ("lib", &lib),
                ("levels", LEVELS),
                // Same name, more variants: must not be the one looked up.
                ("other", "export enum Level { Low, High, Extreme }\n"),
            ],
            &[],
        );
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn a_switch_missing_a_variant_of_another_modules_enum_diagnoses() {
        let messages = package_messages(
            &[
                (
                    "lib",
                    "import { Level } from \"./levels\";\n\
                     function name(level: Level): string {\n\
                       switch (level) {\n\
                         case Level.Low: return \"low\";\n\
                       }\n\
                     }\n",
                ),
                ("levels", LEVELS),
            ],
            &[],
        );
        assert_eq!(
            messages,
            ["function `name` does not return a value on all paths"]
        );
    }

    #[test]
    fn a_switch_over_an_enum_of_a_dependency_is_exhaustive() {
        let (_, dependency, diags) = run_package("@test/levels", &[("lib", LEVELS)], &[]);
        assert!(diags.is_empty(), "{diags:?}");
        let lib = format!("import {{ Level }} from \"@test/levels\";\n{SWITCH_OVER_LEVEL}");
        let messages = package_messages(&[("lib", &lib)], std::slice::from_ref(&dependency));
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn a_script_switch_over_an_imported_enum_is_exhaustive() {
        let (_, dependency, diags) = run_package("@test/levels", &[("lib", LEVELS)], &[]);
        assert!(diags.is_empty(), "{diags:?}");
        let source = format!(
            "import {{ Level }} from \"@test/levels\";\n{SWITCH_OVER_LEVEL}function main(): void {{ }}\n"
        );
        let (ta, diags) = infer_script_with(&source, std::slice::from_ref(&dependency));
        assert!(diags.is_empty(), "{diags:?}");
        let resolved = crate::typechecker::rules::check_script(&ta, &[&dependency]).unwrap();
        assert!(resolved.is_empty(), "{resolved:?}");
        // Without the declaration the enum's variants are unknown.
        let unresolved = crate::check(&ta).unwrap();
        assert_eq!(unresolved.len(), 1, "{unresolved:?}");
    }

    #[test]
    fn class_members_and_their_closures_are_checked() {
        let source = "function main(): void { }
class C {
  n: number = 0;
  readonly f: () => number = () => {
    if (this.n > 0) {
      return 1;
    }
  };
  pick(flag: boolean): number {
    const inner = (): number => {
      if (flag) {
        return 1;
      }
    };
    if (flag) {
      return inner();
    }
  }
  get sign(): number {
    if (this.n > 0) {
      return 1;
    }
  }
  set value(v: number) {
    this.n = v;
  }
}
";
        let reported = |message: &str, line| (message.to_string(), line);
        assert_eq!(
            run_lines(source),
            [
                reported("method `C.pick` does not return a value on all paths", 9),
                reported("getter `C.sign` does not return a value on all paths", 19),
                reported("arrow function does not return a value on all paths", 10),
                reported("arrow function does not return a value on all paths", 4),
            ]
        );
    }

    #[test]
    fn an_unknown_body_that_returns_somewhere_may_fall_off_the_end() {
        let diags = run(
            "function f(flag: boolean): unknown { if (flag) { return 1; } }\n\
             class C { m(flag: boolean): unknown { if (flag) { return 1; } } }\n\
             const g = (flag: boolean): unknown => { if (flag) { return 1; } };\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn an_unknown_body_that_never_returns_diagnoses() {
        let messages: Vec<String> = run(
            "function f(): unknown { const g = (): number => { return 1; }; g(); }\n\
             class C { get v(): unknown { console.log(\"x\"); } }\n",
        )
        .into_iter()
        .map(|diag| diag.message)
        .collect();
        assert_eq!(
            messages,
            [
                "function `f` returns `unknown` but has no `return`",
                "getter `C.v` returns `unknown` but has no `return`",
            ]
        );
    }
}
