//! Immediately-invoked functions, whose bodies TypeScript checks as code
//! running at the call: narrowing flows into the body, and the body's writes
//! flow out after the call.

use std::collections::BTreeSet;

use crate::compiler_error::CompilerFailure;
use crate::{ArrowBody, Ast, ExprId, ExprKind, Ident, Span, StmtId, StmtKind, Type};

use super::{Inferer, narrowing};

/// What an immediately-invoked body leaves behind for the code after the call.
pub(super) struct InvokedBodyExit {
    /// The outer paths the body writes.
    pub(super) written: BTreeSet<narrowing::ReferencePath>,
    /// The type each written binding holds when the body finishes, when every
    /// way out of the body is its end.
    pub(super) narrowed: Vec<(narrowing::ReferencePath, Type)>,
}

/// The arrow an immediately-invoked call runs: `callee`, through parentheses,
/// is an arrow or an anonymous function expression called with no arguments.
/// Without arguments nothing runs between the call and the body, and without
/// a name the function can't run its own body again from inside it.
pub(super) fn immediately_invoked_arrow(
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
) -> Result<Option<ExprId>, CompilerFailure> {
    if !args.is_empty() {
        return Ok(None);
    }
    Ok(
        match &ast.try_expr(callee).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => immediately_invoked_arrow(ast, *inner, args)?,
            ExprKind::FunctionExpression {
                name: None,
                function,
                ..
            } => Some(*function),
            ExprKind::Arrow { .. } => Some(callee),
            _ => None,
        },
    )
}

/// Whether a function body can return before its last statement, so that its
/// end is not the only way out of it.
pub(super) fn returns_before_end(ast: &Ast, body: &ArrowBody) -> Result<bool, CompilerFailure> {
    let ArrowBody::Block(block) = body else {
        return Ok(false);
    };
    let StmtKind::Block(stmts) = &ast.try_stmt(*block).map_err(super::arena_failure)?.kind else {
        return contains_return(ast, *block);
    };
    let Some((last, leading)) = stmts.split_last() else {
        return Ok(false);
    };
    for &stmt in leading {
        if contains_return(ast, stmt)? {
            return Ok(true);
        }
    }
    if matches!(
        ast.try_stmt(*last).map_err(super::arena_failure)?.kind,
        StmtKind::Return(_)
    ) {
        return Ok(false);
    }
    contains_return(ast, *last)
}

/// Whether `stmt` holds a `return` of the function it is in; one inside a
/// nested function returns from that function instead.
fn contains_return(ast: &Ast, stmt: StmtId) -> Result<bool, CompilerFailure> {
    let nested: Vec<StmtId> = match &ast.try_stmt(stmt).map_err(super::arena_failure)?.kind {
        StmtKind::Return(_) => return Ok(true),
        StmtKind::Block(stmts) => stmts.clone(),
        StmtKind::If {
            then_block,
            else_block,
            ..
        } => std::iter::once(*then_block).chain(*else_block).collect(),
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::ForOf { body, .. }
        | StmtKind::ForOfPattern { body, .. } => vec![*body],
        StmtKind::Switch { cases, default, .. } => cases
            .iter()
            .map(|case| case.body)
            .chain(default.iter().map(|default| default.body))
            .collect(),
        StmtKind::Try {
            body,
            catches,
            finally,
        } => std::iter::once(*body)
            .chain(catches.iter().map(|catch| catch.body))
            .chain(*finally)
            .collect(),
        _ => Vec::new(),
    };
    for stmt in nested {
        if contains_return(ast, stmt)? {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Inferer<'_> {
    /// Infers a call, and when it immediately invokes a function, carries the
    /// narrowing at the call into the function's body and the body's writes
    /// out past the call.
    pub(super) fn infer_call_running_invoked_body(
        &mut self,
        callee: ExprId,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(crate::TypedExprKind, Type), CompilerFailure> {
        let Some(arrow) = immediately_invoked_arrow(self.ast, callee, &args)? else {
            return self.infer_call(callee, type_args, args, expected, span);
        };
        let arrow_span = self.ast.try_expr(arrow).map_err(super::arena_failure)?.span;
        self.immediately_invoked = Some(arrow_span);
        let inferred = self.infer_call(callee, type_args, args, expected, span);
        self.immediately_invoked = None;
        let exit = self.invoked_body_exit.take();
        let inferred = inferred?;
        if let Some(exit) = exit {
            self.apply_invoked_body_exit(exit, span)?;
        }
        Ok(inferred)
    }

    /// What the immediately-invoked body being left hands to the code after
    /// its call: the paths it writes, and what each written binding holds at
    /// its end. Call before leaving the body's narrowing boundary.
    pub(super) fn capture_invoked_body_exit(&self, returns_before_end: bool) -> InvokedBodyExit {
        let written = self.assigned_scopes.last().cloned().unwrap_or_default();
        let ends_normally = !returns_before_end;
        let narrowed = written
            .iter()
            .filter(|path| ends_normally && path.chain.is_empty())
            .filter_map(|path| {
                let view = self.narrow_scopes.last()?.get(path)?;
                Some((path.clone(), view.narrowed_ty.clone()))
            })
            .collect();
        InvokedBodyExit { written, narrowed }
    }

    /// Applies an immediately-invoked body's writes after its call: each
    /// written outer path loses its narrowing, and a binding the body left
    /// narrowed takes that type, as after an assignment.
    pub(super) fn apply_invoked_body_exit(
        &mut self,
        exit: InvokedBodyExit,
        call_span: Span,
    ) -> Result<(), CompilerFailure> {
        for path in exit.written {
            if self.path_root_in_scope(&path) {
                self.invalidate_for_reassignment(path, call_span);
            }
        }
        for (path, narrowed_ty) in exit.narrowed {
            if !self.path_root_in_scope(&path) {
                continue;
            }
            let Some(name) = self.binding_name(&path) else {
                continue;
            };
            let ident = Ident {
                name,
                span: call_span,
            };
            self.install_assignment_narrowing(path, ident, narrowed_ty, call_span)?;
        }
        Ok(())
    }

    /// The source name a root binding is read by.
    fn binding_name(&self, path: &narrowing::ReferencePath) -> Option<String> {
        match &path.root {
            narrowing::BindingId::Local { name, .. } => Some(name.clone()),
            narrowing::BindingId::Global(mangled) => self
                .top_symbols
                .iter()
                .find(|(_, entry)| &entry.mangled_name == mangled)
                .map(|(name, _)| name.clone()),
            narrowing::BindingId::This => None,
        }
    }
}
