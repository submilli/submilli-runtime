use super::control_flow::{ControlFlow, control_flow};
use super::declarations::TypeDeclarations;
use crate::{
    ClosureBody, Diagnostic, ExprId, Severity, Span, StmtId, Type, TypedAst, TypedExprKind,
    TypedStmtKind,
};

pub(super) fn run(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for f in &ta.functions {
        let msg = format!(
            "function `{}` does not return a value on all paths",
            f.name.name
        );
        check_returns(
            ta,
            declarations,
            f.body,
            &f.return_type,
            f.name.span,
            msg,
            diags,
        )?;
        walk_stmt(ta, declarations, f.body, diags)?;
    }
    for &stmt_id in &ta.top_level_statements {
        walk_stmt(ta, declarations, stmt_id, diags)?;
    }
    Ok(())
}

fn check_returns(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    body: StmtId,
    return_type: &Type,
    span: Span,
    message: String,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    if matches!(return_type.peel(), Type::Void | Type::Error) {
        return Ok(());
    }
    let _: () = if control_flow(ta, declarations, body)? == ControlFlow::Falls {
        diags.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            help: vec![],
            notes: vec![],
        });
    };
    Ok(())
}

fn walk_stmt(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    stmt_id: StmtId,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match &ta
        .try_stmt(stmt_id)
        .map_err(crate::typechecker::arena_failure)?
        .kind
    {
        TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => {
            walk_expr(ta, declarations, *value, diags)?;
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            walk_expr(ta, declarations, *condition, diags)?;
            walk_stmt(ta, declarations, *then_block, diags)?;
            if let Some(eb) = else_block {
                walk_stmt(ta, declarations, *eb, diags)?;
            }
        }
        TypedStmtKind::While { condition, body } => {
            walk_expr(ta, declarations, *condition, diags)?;
            walk_stmt(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                walk_stmt(ta, declarations, *i, diags)?;
            }
            if let Some(c) = condition {
                walk_expr(ta, declarations, *c, diags)?;
            }
            if let Some(u) = update {
                walk_stmt(ta, declarations, *u, diags)?;
            }
            walk_stmt(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::ForOf { iter, body, .. } => {
            walk_expr(ta, declarations, *iter, diags)?;
            walk_stmt(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::DoWhile { body, condition } => {
            walk_stmt(ta, declarations, *body, diags)?;
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
                walk_stmt(ta, declarations, case.body, diags)?;
            }
            if let Some(d) = default {
                walk_stmt(ta, declarations, *d, diags)?;
            }
        }
        TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        TypedStmtKind::Return(value) => {
            if let Some(v) = value {
                walk_expr(ta, declarations, *v, diags)?;
            }
        }
        TypedStmtKind::Expr(e) => walk_expr(ta, declarations, *e, diags)?,
        TypedStmtKind::Block(stmts) => {
            for &s in stmts {
                walk_stmt(ta, declarations, s, diags)?;
            }
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
            walk_stmt(ta, declarations, *body, diags)?;
        }
        TypedStmtKind::Throw { value } => walk_expr(ta, declarations, *value, diags)?,
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            walk_stmt(ta, declarations, *body, diags)?;
            for c in catches {
                walk_stmt(ta, declarations, c.body, diags)?;
            }
            if let Some(f) = finally {
                walk_stmt(ta, declarations, *f, diags)?;
            }
        }
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
        TypedExprKind::Closure {
            return_type, body, ..
        } => {
            if ta.placeholder_closures.contains(&expr_id) {
                return Ok(());
            }
            if let ClosureBody::Block(b) = body {
                let (span, message) = match ta.nested_function_names.get(&expr_id) {
                    Some(name) => (
                        name.span,
                        format!(
                            "function `{}` does not return a value on all paths",
                            name.name
                        ),
                    ),
                    None => (
                        ta.try_expr(expr_id)
                            .map_err(crate::typechecker::arena_failure)?
                            .span,
                        "arrow function does not return a value on all paths".to_string(),
                    ),
                };
                check_returns(ta, declarations, *b, return_type, span, message, diags)?;
                walk_stmt(ta, declarations, *b, diags)?;
            } else if let ClosureBody::Expr(inner) = body {
                walk_expr(ta, declarations, *inner, diags)?;
            }
        }
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
                walk_stmt(ta, declarations, stmt, diags)?;
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
    use super::super::test_util::{infer_script_with, run, run_package};

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

    #[test]
    fn while_with_return_inside_diagnoses() {
        let diags = run("function f(): number { while (true) { return 1; } }");
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
}
