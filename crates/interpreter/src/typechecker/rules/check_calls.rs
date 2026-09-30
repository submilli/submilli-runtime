//! Finds the `check(...)` calls of `submilli:security` under a body.
//!
//! `check` is always a direct call: a named, aliased or namespace import all
//! resolve to the same mangled name.

use crate::compiler_error::CompilerFailure;
use crate::{
    ClosureBody, ExprId, PostfixTarget, Span, StmtId, TypedAst, TypedChainPart, TypedExprKind,
    TypedStmtKind,
};

/// Where a search starts: the root of a body, or of code outside one.
#[derive(Clone, Copy, Debug)]
pub(super) enum SearchRoot {
    Stmt(StmtId),
    /// An expression that belongs to no statement: the result of an
    /// expression-bodied arrow function, or a class field initializer.
    Expr(ExprId),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CheckCall {
    pub(super) span: Span,
    pub(super) args: Vec<ExprId>,
    /// The closures the call is nested in, outermost first. Empty when the
    /// call is directly in the searched body.
    pub(super) closures: Vec<ExprId>,
}

/// Appends the calls under `root` in source order: a call comes before the
/// calls nested in its arguments.
pub(super) fn collect(
    ta: &TypedAst,
    root: SearchRoot,
    out: &mut Vec<CheckCall>,
) -> Result<(), CompilerFailure> {
    let mut collector = Collector {
        ta,
        closures: Vec::new(),
        out,
    };
    match root {
        SearchRoot::Stmt(id) => collector.stmt(id),
        SearchRoot::Expr(id) => collector.expr(id),
    }
}

struct Collector<'a> {
    ta: &'a TypedAst,
    closures: Vec<ExprId>,
    out: &'a mut Vec<CheckCall>,
}

impl Collector<'_> {
    fn stmt(&mut self, id: StmtId) -> Result<(), CompilerFailure> {
        let ta = self.ta;
        let _: () = match &ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedStmtKind::Let { value, .. }
            | TypedStmtKind::Const { value, .. }
            | TypedStmtKind::Expr(value)
            | TypedStmtKind::Throw { value }
            | TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => self.expr(*value)?,
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(*condition)?;
                self.stmt(*then_block)?;
                if let Some(else_block) = else_block {
                    self.stmt(*else_block)?;
                }
            }
            TypedStmtKind::While { condition, body }
            | TypedStmtKind::DoWhile { body, condition } => {
                self.expr(*condition)?;
                self.stmt(*body)?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(*init)?;
                }
                if let Some(condition) = condition {
                    self.expr(*condition)?;
                }
                if let Some(update) = update {
                    self.stmt(*update)?;
                }
                self.stmt(*body)?;
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                self.expr(*iter)?;
                self.stmt(*body)?;
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.expr(*discriminant)?;
                for case in cases {
                    self.stmt(case.body)?;
                }
                if let Some(default) = default {
                    self.stmt(*default)?;
                }
            }
            TypedStmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(*value)?;
                }
            }
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.stmt(*body)?;
                for clause in catches {
                    self.stmt(clause.body)?;
                }
                if let Some(finally) = finally {
                    self.stmt(*finally)?;
                }
            }
            TypedStmtKind::Block(stmts) => {
                for stmt in stmts {
                    self.stmt(*stmt)?;
                }
            }
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                self.expr(*receiver)?;
                self.expr(*value)?;
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.expr(*receiver)?;
                self.expr(*index)?;
                self.expr(*value)?;
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                self.expr(*source)?;
                self.stmt(*body)?;
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        };
        Ok(())
    }

    fn expr(&mut self, id: ExprId) -> Result<(), CompilerFailure> {
        let ta = self.ta;
        let expr = ta.try_expr(id).map_err(crate::typechecker::arena_failure)?;
        let _: () = match &expr.kind {
            TypedExprKind::Call { mangled, args, .. } => {
                if crate::stdlib::security::is_check(mangled) {
                    self.record(expr.span, args.clone());
                }
                self.exprs(args)?;
            }
            TypedExprKind::GenericCall { mangled, args, .. } => {
                if crate::stdlib::security::is_check(mangled) {
                    self.record(expr.span, args.iter().map(|arg| arg.expr).collect());
                }
                for arg in args {
                    self.expr(arg.expr)?;
                }
            }
            TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. } => self.exprs(args)?,
            TypedExprKind::CallClosure { callee, args } => {
                self.expr(*callee)?;
                self.exprs(args)?;
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.expr(*receiver)?;
                self.exprs(args)?;
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.expr(*receiver)?;
                for arg in args {
                    self.expr(arg.expr)?;
                }
            }
            TypedExprKind::Binary { lhs, rhs, .. }
            | TypedExprKind::NullishCoalesce { lhs, rhs } => {
                self.expr(*lhs)?;
                self.expr(*rhs)?;
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.expr(*effect)?;
                self.expr(*result)?;
            }
            TypedExprKind::Sequence { stmts, result } => {
                for stmt in stmts {
                    self.stmt(*stmt)?;
                }
                self.expr(*result)?;
            }
            TypedExprKind::Unary { operand, .. }
            | TypedExprKind::FieldAccess {
                receiver: operand, ..
            }
            | TypedExprKind::InterfacePropertyAccess {
                receiver: operand, ..
            }
            | TypedExprKind::TypeofTag { value: operand, .. }
            | TypedExprKind::InstanceOf { value: operand, .. }
            | TypedExprKind::NonNullAssert { value: operand }
            | TypedExprKind::Cast { value: operand, .. } => self.expr(*operand)?,
            TypedExprKind::IndexAccess { receiver, index } => {
                self.expr(*receiver)?;
                self.expr(*index)?;
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        self.expr(expression)?;
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    self.expr(element.expr_id())?;
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => self.exprs(elements)?,
            TypedExprKind::Closure { body, .. } => self.closure(id, body)?,
            TypedExprKind::Narrowed { source, inner, .. } => {
                self.expr(*source)?;
                self.expr(*inner)?;
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.expr(*cond)?;
                self.expr(*then_)?;
                self.expr(*else_)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.expr(*base)?;
                for part in parts {
                    self.chain_part(part)?;
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                PostfixTarget::Field { receiver, .. } => self.expr(*receiver)?,
                PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    self.expr(*receiver)?;
                    self.expr(*index)?;
                }
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {}
            },
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

    fn exprs(&mut self, ids: &[ExprId]) -> Result<(), CompilerFailure> {
        for id in ids {
            self.expr(*id)?;
        }
        Ok(())
    }

    fn closure(&mut self, id: ExprId, body: &ClosureBody) -> Result<(), CompilerFailure> {
        self.closures.push(id);
        let walked = match body {
            ClosureBody::Expr(expr) => self.expr(*expr),
            ClosureBody::Block(stmt) => self.stmt(*stmt),
        };
        self.closures.pop();
        walked
    }

    fn chain_part(&mut self, part: &TypedChainPart) -> Result<(), CompilerFailure> {
        match part {
            TypedChainPart::Index { idx, .. } => self.expr(*idx),
            TypedChainPart::Call { args, .. } | TypedChainPart::MethodCall { args, .. } => {
                self.exprs(args)
            }
            TypedChainPart::Field { .. }
            | TypedChainPart::InterfaceProperty { .. }
            | TypedChainPart::NonNull { .. } => Ok(()),
        }
    }

    fn record(&mut self, span: Span, args: Vec<ExprId>) {
        self.out.push(CheckCall {
            span,
            args,
            closures: self.closures.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckCall, SearchRoot, collect};
    use crate::{TypedAst, TypedExprKind, TypedTypeDecl};

    fn typed(source: &str) -> TypedAst {
        let (ta, diags) = super::super::test_util::infer_script(source);
        assert!(diags.is_empty(), "{diags:?}");
        ta
    }

    fn calls_in_function(ta: &TypedAst, name: &str) -> Vec<CheckCall> {
        let function = ta
            .functions
            .iter()
            .find(|f| f.name.name == name)
            .expect("function under test");
        let mut calls = Vec::new();
        collect(ta, SearchRoot::Stmt(function.body), &mut calls).unwrap();
        calls
    }

    fn capability(ta: &TypedAst, call: &CheckCall) -> String {
        match &ta.try_expr(call.args[0]).unwrap().kind {
            TypedExprKind::String(value) => value.clone(),
            other => panic!("capability is not a literal: {other:?}"),
        }
    }

    #[test]
    fn finds_direct_aliased_and_namespace_calls() {
        let ta = typed(
            "import { check, check as gate } from \"submilli:security\";\n\
             import * as security from \"submilli:security\";\n\
             function f(): void {\n\
               check(\"x/direct\", {});\n\
               gate(\"x/aliased\", {});\n\
               security.check(\"x/namespace\", {});\n\
             }\n\
             function main(): void { }\n",
        );
        let calls = calls_in_function(&ta, "f");
        let capabilities: Vec<_> = calls.iter().map(|call| capability(&ta, call)).collect();
        assert_eq!(capabilities, ["x/direct", "x/aliased", "x/namespace"]);
        assert!(calls.iter().all(|call| call.args.len() == 2), "{calls:?}");
        assert!(
            calls.iter().all(|call| call.closures.is_empty()),
            "{calls:?}"
        );
    }

    #[test]
    fn records_the_enclosing_closures_outermost_first() {
        let ta = typed(
            "import { check } from \"submilli:security\";\n\
             function f(): void {\n\
               check(\"x/body\", {});\n\
               const outer = (): void => {\n\
                 check(\"x/outer\", {});\n\
                 const inner = (): void => { check(\"x/inner\", {}); };\n\
                 inner();\n\
               };\n\
               outer();\n\
               check(\"x/after\", {});\n\
             }\n\
             function main(): void { }\n",
        );
        let calls = calls_in_function(&ta, "f");
        let depths: Vec<_> = calls
            .iter()
            .map(|call| (capability(&ta, call), call.closures.len()))
            .collect();
        assert_eq!(
            depths,
            [
                ("x/body".to_string(), 0),
                ("x/outer".to_string(), 1),
                ("x/inner".to_string(), 2),
                ("x/after".to_string(), 0),
            ]
        );
        let inner = &calls[2];
        assert_eq!(inner.closures[0], calls[1].closures[0]);
        assert!(inner.closures.iter().all(|id| matches!(
            ta.try_expr(*id).unwrap().kind,
            TypedExprKind::Closure { .. }
        )));
    }

    #[test]
    fn a_call_precedes_the_calls_in_its_arguments() {
        let ta = typed(
            "import { check } from \"submilli:security\";\n\
             function inner(): string { check(\"x/unrelated\", {}); return \"v\"; }\n\
             function f(): void {\n\
               check(\"x/outer\", { nested: ((): string => { check(\"x/arg\", {}); return \"v\"; })() });\n\
             }\n\
             function main(): void { }\n",
        );
        let calls = calls_in_function(&ta, "f");
        let capabilities: Vec<_> = calls.iter().map(|call| capability(&ta, call)).collect();
        assert_eq!(capabilities, ["x/outer", "x/arg"]);
    }

    #[test]
    fn searches_an_expression_root() {
        let ta = typed(
            "import { check } from \"submilli:security\";\n\
             function gate(): number { check(\"x/helper\", {}); return 1; }\n\
             class Holder {\n\
               value: number = ((): number => { check(\"x/initializer\", {}); return 1; })();\n\
             }\n\
             function main(): void { }\n",
        );
        let initializer = ta
            .types
            .iter()
            .find_map(|decl| match decl {
                TypedTypeDecl::Class(class) => class.fields.iter().find_map(|f| f.initializer),
                _ => None,
            })
            .expect("field initializer");
        let mut calls = Vec::new();
        collect(&ta, SearchRoot::Expr(initializer), &mut calls).unwrap();
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(capability(&ta, &calls[0]), "x/initializer");
        assert_eq!(calls[0].closures.len(), 1);
    }

    #[test]
    fn an_invalid_node_id_is_an_internal_failure() {
        let ta = typed("function main(): void { }\n");
        for root in [
            SearchRoot::Stmt(crate::StmtId(u32::MAX)),
            SearchRoot::Expr(crate::ExprId(u32::MAX)),
        ] {
            let failure = collect(&ta, root, &mut Vec::new()).unwrap_err();
            assert!(
                matches!(
                    failure,
                    crate::compiler_error::CompilerFailure::Internal { .. }
                ),
                "{failure}"
            );
        }
    }
}
