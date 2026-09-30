//! Super-call rule: a subclass constructor must run `super(...)` on every path
//! that returns or reaches its end. Otherwise the instance is built without the
//! parent constructor and its fields read as `null`/`0` at the WasmGC level,
//! where JavaScript would throw a `ReferenceError`.
//!
//! Inference reports a constructor with no `super(...)` at all, and one whose
//! call isn't a statement of its own; this rule reports one whose call statement
//! some path skips. It is conservative: a call inside a loop or a `switch` case
//! doesn't count as made, so the rule never accepts a constructor that can skip it.

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
        let mut walk = SuperCallWalk::default();
        let end = walk.walk(ta, ctor.body, false)?;
        if !walk.has_super_call_statement {
            continue;
        }
        for span in walk.early_returns {
            diags.push(skipped_super_call(
                span,
                "this `return` can run before `super(...)`",
            ));
        }
        if end.can_finish_without_super() {
            let body = ta
                .try_stmt(ctor.body)
                .map_err(crate::typechecker::arena_failure)?;
            diags.push(skipped_super_call(
                body.span,
                "this constructor can finish without calling `super(...)`",
            ));
        }
    }
    Ok(())
}

fn skipped_super_call(span: Span, message: &str) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        span,
        message: format!("a subclass constructor must call `super(...)` on every path: {message}"),
        help: vec!["call `super(...)` unconditionally, before any `return`".to_string()],
        notes: vec![],
    }
}

/// How a statement's paths leave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Completion {
    /// No path completes normally: each returns, throws, breaks or continues.
    Abrupt,
    /// Some path completes normally; `super_called` if `super(...)` has run on
    /// every one that does.
    Normal { super_called: bool },
}

impl Completion {
    /// The completion of two alternative paths: `super(...)` has run only if it
    /// has on both, and a path that doesn't complete doesn't constrain the other.
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Normal { super_called: a }, Self::Normal { super_called: b }) => Self::Normal {
                super_called: a && b,
            },
            (only, Self::Abrupt) | (Self::Abrupt, only) => only,
        }
    }

    /// The completion of `self` followed by `next`, which runs whichever way
    /// `self` completes (a `finally`): abrupt if either is, and `super(...)`
    /// has run if it did in either.
    fn then(self, next: Self) -> Self {
        match (self, next) {
            (Self::Normal { super_called: a }, Self::Normal { super_called: b }) => Self::Normal {
                super_called: a || b,
            },
            (Self::Abrupt, _) | (_, Self::Abrupt) => Self::Abrupt,
        }
    }

    fn can_finish_without_super(self) -> bool {
        self == Self::Normal {
            super_called: false,
        }
    }
}

/// One constructor body's walk, collecting what the rule reports.
#[derive(Default)]
struct SuperCallWalk {
    has_super_call_statement: bool,
    /// `return`s reached on some path before `super(...)` ran.
    early_returns: Vec<Span>,
}

impl SuperCallWalk {
    /// Walk `id`, entered with `super_called` telling whether `super(...)` has
    /// run on every path so far.
    fn walk(
        &mut self,
        ta: &TypedAst,
        id: StmtId,
        super_called: bool,
    ) -> Result<Completion, CompilerFailure> {
        let entry = Completion::Normal { super_called };
        let stmt = ta.try_stmt(id).map_err(crate::typechecker::arena_failure)?;
        Ok(match &stmt.kind {
            TypedStmtKind::Return(_) => {
                if !super_called {
                    self.early_returns.push(stmt.span);
                }
                Completion::Abrupt
            }
            TypedStmtKind::Throw { .. } | TypedStmtKind::Break | TypedStmtKind::Continue => {
                Completion::Abrupt
            }
            TypedStmtKind::Expr(expr) => {
                let is_call = matches!(
                    ta.try_expr(*expr)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind,
                    TypedExprKind::SuperCtorCall { .. }
                );
                self.has_super_call_statement |= is_call;
                Completion::Normal {
                    super_called: super_called || is_call,
                }
            }
            TypedStmtKind::Block(stmts) => {
                let mut state = entry;
                for &s in stmts {
                    match state {
                        Completion::Normal { super_called } => {
                            state = self.walk(ta, s, super_called)?;
                        }
                        // Unreachable, but a call here still means the
                        // constructor has one to check; walked as if it had
                        // run, so its `return`s aren't reported.
                        Completion::Abrupt => {
                            self.walk(ta, s, true)?;
                        }
                    }
                }
                state
            }
            TypedStmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                let then = self.walk(ta, *then_block, super_called)?;
                let els = match else_block {
                    Some(block) => self.walk(ta, *block, super_called)?,
                    None => entry,
                };
                then.join(els)
            }
            TypedStmtKind::NarrowRegion { body, .. } => self.walk(ta, *body, super_called)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                // A `catch` can start before the body's call, and `finally`
                // after any of them, so each is walked from the entry state.
                let mut state = self.walk(ta, *body, super_called)?;
                for c in catches {
                    state = state.join(self.walk(ta, c.body, super_called)?);
                }
                let Some(f) = finally else {
                    return Ok(state);
                };
                state.then(self.walk(ta, *f, super_called)?)
            }
            // The body may run zero times, and a `break` can leave a case before
            // its call, so only the entry state survives.
            TypedStmtKind::While { body, .. }
            | TypedStmtKind::DoWhile { body, .. }
            | TypedStmtKind::ForOf { body, .. } => {
                self.walk(ta, *body, super_called)?;
                entry
            }
            TypedStmtKind::For {
                init, update, body, ..
            } => {
                for s in [*init, *update].into_iter().flatten() {
                    self.walk(ta, s, super_called)?;
                }
                self.walk(ta, *body, super_called)?;
                entry
            }
            TypedStmtKind::Switch { cases, default, .. } => {
                for body in cases.iter().map(|c| c.body).chain(*default) {
                    self.walk(ta, body, super_called)?;
                }
                entry
            }
            TypedStmtKind::ReboxLocal { .. }
            | TypedStmtKind::Let { .. }
            | TypedStmtKind::Const { .. }
            | TypedStmtKind::AssignLocal { .. }
            | TypedStmtKind::AssignGlobal { .. }
            | TypedStmtKind::AssignField { .. }
            | TypedStmtKind::AssignIndex { .. } => entry,
        })
    }
}
