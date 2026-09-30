//! Super-call rule: a subclass constructor must run `super(...)` on every path
//! that returns or reaches its end. Otherwise the instance is built without the
//! parent constructor and its fields read as `null`/`0` at the WasmGC level,
//! where JavaScript would throw a `ReferenceError`.
//!
//! A constructor with no `super(...)` statement at all is reported during
//! inference; this rule reports only one whose call some path skips. It is
//! conservative: a call inside a loop, a `switch` case or an expression doesn't
//! count as made, so the rule never accepts a constructor that can skip it.

use crate::compiler_error::CompilerFailure;
use crate::{
    Diagnostic, Severity, Span, StmtId, TypedAst, TypedExprKind, TypedStmtKind, TypedTypeDecl,
};

pub(super) fn run(ta: &TypedAst, diags: &mut Vec<Diagnostic>) -> Result<(), CompilerFailure> {
    for decl in &ta.types {
        let TypedTypeDecl::Class(class) = decl else {
            continue;
        };
        let (Some(_), Some(ctor)) = (&class.extends, &class.constructor) else {
            continue;
        };
        let mut paths = Paths::default();
        let called = paths.walk(ta, ctor.body, false)?;
        if !paths.found_call {
            continue;
        }
        for span in paths.early_returns {
            diags.push(skipped(span, "this `return` can run before `super(...)`"));
        }
        if called == Some(false) {
            let body = ta
                .try_stmt(ctor.body)
                .map_err(crate::typechecker::arena_failure)?;
            diags.push(skipped(
                body.span,
                "this constructor can finish without calling `super(...)`",
            ));
        }
    }
    Ok(())
}

fn skipped(span: Span, message: &str) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        span,
        message: format!("a subclass constructor must call `super(...)` on every path: {message}"),
        help: vec!["call `super(...)` unconditionally, before any `return`".to_string()],
        notes: vec![],
    }
}

#[derive(Default)]
struct Paths {
    /// Whether any statement in the body is a `super(...)` call.
    found_call: bool,
    /// `return`s reached on some path before `super(...)` ran.
    early_returns: Vec<Span>,
}

impl Paths {
    /// Walk `id` entered with `called` telling whether `super(...)` has run on
    /// every path so far. Returns the same for the paths that complete
    /// normally, or `None` when none do.
    fn walk(
        &mut self,
        ta: &TypedAst,
        id: StmtId,
        called: bool,
    ) -> Result<Option<bool>, CompilerFailure> {
        let stmt = ta.try_stmt(id).map_err(crate::typechecker::arena_failure)?;
        Ok(match &stmt.kind {
            TypedStmtKind::Return(_) => {
                if !called {
                    self.early_returns.push(stmt.span);
                }
                None
            }
            TypedStmtKind::Throw { .. } | TypedStmtKind::Break | TypedStmtKind::Continue => None,
            TypedStmtKind::Expr(expr) => {
                let is_call = matches!(
                    ta.try_expr(*expr)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind,
                    TypedExprKind::SuperCtorCall { .. }
                );
                self.found_call |= is_call;
                Some(called || is_call)
            }
            TypedStmtKind::Block(stmts) => {
                let mut state = Some(called);
                for &s in stmts {
                    let Some(now) = state else { break };
                    state = self.walk(ta, s, now)?;
                }
                state
            }
            TypedStmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                let then = self.walk(ta, *then_block, called)?;
                let els = match else_block {
                    Some(block) => self.walk(ta, *block, called)?,
                    None => Some(called),
                };
                join(then, els)
            }
            TypedStmtKind::NarrowRegion { body, .. } => self.walk(ta, *body, called)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                // A `catch` can start before the body's call, and `finally`
                // after any of them, so each is walked from the entry state.
                let mut state = self.walk(ta, *body, called)?;
                for c in catches {
                    state = join(state, self.walk(ta, c.body, called)?);
                }
                match finally {
                    Some(f) => match self.walk(ta, *f, called)? {
                        None => None,
                        Some(in_finally) => state.map(|after| after || in_finally),
                    },
                    None => state,
                }
            }
            // The body may run zero times, and a `break` can leave a case before
            // its call, so only the entry state survives.
            TypedStmtKind::While { body, .. }
            | TypedStmtKind::DoWhile { body, .. }
            | TypedStmtKind::ForOf { body, .. } => {
                self.walk(ta, *body, called)?;
                Some(called)
            }
            TypedStmtKind::For {
                init, update, body, ..
            } => {
                for s in [*init, *update].into_iter().flatten() {
                    self.walk(ta, s, called)?;
                }
                self.walk(ta, *body, called)?;
                Some(called)
            }
            TypedStmtKind::Switch { cases, default, .. } => {
                for body in cases.iter().map(|c| c.body).chain(*default) {
                    self.walk(ta, body, called)?;
                }
                Some(called)
            }
            TypedStmtKind::ReboxLocal { .. }
            | TypedStmtKind::Let { .. }
            | TypedStmtKind::Const { .. }
            | TypedStmtKind::AssignLocal { .. }
            | TypedStmtKind::AssignGlobal { .. }
            | TypedStmtKind::AssignField { .. }
            | TypedStmtKind::AssignIndex { .. } => Some(called),
        })
    }
}

/// The state after two alternative paths: `super(...)` has run only if it has
/// on both, and a path that doesn't complete doesn't constrain the other.
fn join(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a && b),
        (a, None) => a,
        (None, b) => b,
    }
}
