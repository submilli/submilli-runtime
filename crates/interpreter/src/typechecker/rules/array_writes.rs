use super::body_walk::{self, Visitor};
use crate::compiler_error::CompilerFailure;
use crate::{
    Diagnostic, ExprId, Ident, PostfixTarget, Severity, Type, TypedAst, TypedExprKind, TypedStmt,
    TypedStmtKind,
};

pub(super) fn run(
    ast: &TypedAst,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    body_walk::walk_program(ast, &mut ArrayWrites { ast, diagnostics })
}

pub(in crate::typechecker) fn rejects_named_write(ty: &Type, name: &str) -> bool {
    name != "length"
        && (matches!(ty.peel(), Type::Array(_) | Type::Tuple(_)) || ty.is_array_like_union())
}

struct ArrayWrites<'a> {
    ast: &'a TypedAst,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Visitor for ArrayWrites<'_> {
    fn visit_stmt(&mut self, stmt: &TypedStmt) -> Result<(), CompilerFailure> {
        if let TypedStmtKind::AssignField { receiver, name, .. } = &stmt.kind {
            self.check_write(*receiver, name)?;
        }
        Ok(())
    }

    fn visit_expr(&mut self, _id: ExprId, kind: &TypedExprKind) -> Result<(), CompilerFailure> {
        if let TypedExprKind::PostfixUnary {
            target: PostfixTarget::Field { receiver, name, .. },
            ..
        } = kind
        {
            self.check_write(*receiver, name)?;
        }
        Ok(())
    }
}

impl ArrayWrites<'_> {
    fn check_write(&mut self, receiver: ExprId, name: &Ident) -> Result<(), CompilerFailure> {
        let ty = self
            .ast
            .source_type(receiver)
            .map_err(crate::typechecker::arena_failure)?;
        if rejects_named_write(ty, &name.name) {
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: name.span,
                message: format!(
                    "cannot assign to named property `{}` on an array",
                    name.name
                ),
                help: vec!["use a separate object to store named properties".into()],
                notes: vec![],
            });
        }
        Ok(())
    }
}
