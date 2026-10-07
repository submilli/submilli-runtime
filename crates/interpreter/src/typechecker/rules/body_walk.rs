//! The traversal the control-flow rules share: every statement a program can
//! run and every closure it creates, wherever they are nested.

use std::collections::BTreeSet;

use crate::compiler_error::CompilerFailure;
use crate::{
    ClosureBody, ExprId, PostfixTarget, StmtId, Type, TypedAst, TypedChainPart, TypedExprKind,
    TypedStmtKind,
};

/// Receives each node [`walk_program`] reaches, before the walk descends
/// into it.
pub(crate) trait Visitor {
    fn visit_stmt(&mut self, _kind: &TypedStmtKind) -> Result<(), CompilerFailure> {
        Ok(())
    }

    fn visit_closure(
        &mut self,
        _id: ExprId,
        _return_type: &Type,
        _body: &ClosureBody,
    ) -> Result<(), CompilerFailure> {
        Ok(())
    }

    fn visit_expr(&mut self, _id: ExprId, _kind: &TypedExprKind) -> Result<(), CompilerFailure> {
        Ok(())
    }

    fn descend_into_closures(&self) -> bool {
        true
    }
}

/// Walks every body of the program: functions, class constructors, methods
/// and accessors, field initializers, and top-level statements.
pub(crate) fn walk_program(
    ta: &TypedAst,
    visitor: &mut impl Visitor,
) -> Result<(), CompilerFailure> {
    let mut walk = Walk {
        ta,
        visitor,
        seen: BTreeSet::new(),
    };
    for function in &ta.functions {
        walk.stmt(function.body)?;
    }
    for body in ta.class_body_roots() {
        walk.stmt(body)?;
    }
    for initializer in ta.class_field_initializers() {
        walk.expr(initializer)?;
    }
    for &stmt_id in &ta.top_level_statements {
        walk.stmt(stmt_id)?;
    }
    Ok(())
}

/// Walk selected roots with one shared seen set. Authority analysis uses this
/// to keep nested closure bodies separate from the callable that creates them.
pub(crate) fn walk_roots(
    ta: &TypedAst,
    statement_roots: impl IntoIterator<Item = StmtId>,
    expression_roots: impl IntoIterator<Item = ExprId>,
    visitor: &mut impl Visitor,
) -> Result<(), CompilerFailure> {
    let mut walk = Walk {
        ta,
        visitor,
        seen: BTreeSet::new(),
    };
    for root in statement_roots {
        walk.stmt(root)?;
    }
    for root in expression_roots {
        walk.expr(root)?;
    }
    Ok(())
}

struct Walk<'a, V> {
    ta: &'a TypedAst,
    visitor: &'a mut V,
    seen: BTreeSet<ExprId>,
}

impl<V: Visitor> Walk<'_, V> {
    fn stmt(&mut self, stmt_id: StmtId) -> Result<(), CompilerFailure> {
        let kind = &self
            .ta
            .try_stmt(stmt_id)
            .map_err(crate::typechecker::arena_failure)?
            .kind;
        self.visitor.visit_stmt(kind)?;
        let _: () = match kind {
            TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => {
                self.expr(*value)?;
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(*condition)?;
                self.stmt(*then_block)?;
                if let Some(eb) = else_block {
                    self.stmt(*eb)?;
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.expr(*condition)?;
                self.stmt(*body)?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(*i)?;
                }
                if let Some(c) = condition {
                    self.expr(*c)?;
                }
                if let Some(u) = update {
                    self.stmt(*u)?;
                }
                self.stmt(*body)?;
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                self.expr(*iter)?;
                self.stmt(*body)?;
            }
            TypedStmtKind::DoWhile { body, condition } => {
                self.stmt(*body)?;
                self.expr(*condition)?;
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.expr(*discriminant)?;
                for comparison in cases
                    .iter()
                    .flat_map(crate::TypedSwitchCase::label_comparisons)
                {
                    self.expr(comparison)?;
                }
                for case in cases {
                    self.stmt(case.body)?;
                }
                if let Some(d) = default {
                    self.stmt(*d)?;
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::Return(value) => {
                if let Some(v) = value {
                    self.expr(*v)?;
                }
            }
            TypedStmtKind::Expr(e) => self.expr(*e)?,
            TypedStmtKind::Block(stmts) => {
                for &s in stmts {
                    self.stmt(s)?;
                }
            }
            TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => {
                self.expr(*value)?;
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
            TypedStmtKind::Throw { value } => self.expr(*value)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.stmt(*body)?;
                for c in catches {
                    self.stmt(c.body)?;
                }
                if let Some(f) = finally {
                    self.stmt(*f)?;
                }
            }
        };
        Ok(())
    }

    fn expr(&mut self, expr_id: ExprId) -> Result<(), CompilerFailure> {
        // Lowering shares a node between two parents — a compound assignment's
        // receiver is also inside its value — so a second visit would report the
        // same defect twice.
        if !self.seen.insert(expr_id) {
            return Ok(());
        }
        let kind = &self
            .ta
            .try_expr(expr_id)
            .map_err(crate::typechecker::arena_failure)?
            .kind;
        self.visitor.visit_expr(expr_id, kind)?;
        let _: () = match kind {
            TypedExprKind::Closure {
                return_type, body, ..
            } => {
                self.visitor.visit_closure(expr_id, return_type, body)?;
                if !self.visitor.descend_into_closures() {
                    return Ok(());
                }
                match body {
                    ClosureBody::Expr(e) => self.expr(*e)?,
                    ClosureBody::Block(b) => self.stmt(*b)?,
                }
            }
            TypedExprKind::Binary { lhs, rhs, .. } => {
                self.expr(*lhs)?;
                self.expr(*rhs)?;
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.expr(*effect)?;
                self.expr(*result)?;
            }
            TypedExprKind::Sequence { stmts, result } => {
                for &stmt in stmts {
                    self.stmt(stmt)?;
                }
                self.expr(*result)?;
            }
            TypedExprKind::Unary { operand, .. } => self.expr(*operand)?,
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                self.expr(*value)?;
            }
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. } => {
                for &a in args {
                    self.expr(a)?;
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.expr(*callee)?;
                for &a in args {
                    self.expr(a)?;
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for a in args {
                    self.expr(a.expr)?;
                }
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.expr(*receiver)?;
                for &a in args {
                    self.expr(a)?;
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.expr(*receiver)?;
                for a in args {
                    self.expr(a.expr)?;
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        self.expr(expression)?;
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for e in elements {
                    self.expr(e.expr_id())?;
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for &e in elements {
                    self.expr(e)?;
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                self.expr(*receiver)?;
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                self.expr(*receiver)?;
                self.expr(*index)?;
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                self.expr(*source)?;
                self.expr(*inner)?;
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.expr(*cond)?;
                self.expr(*then_)?;
                self.expr(*else_)?;
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                self.expr(*lhs)?;
                self.expr(*rhs)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.expr(*base)?;
                for part in parts {
                    match part {
                        TypedChainPart::Index { idx, .. } => self.expr(*idx)?,
                        TypedChainPart::Call { args, .. }
                        | TypedChainPart::MethodCall { args, .. } => {
                            for a in args {
                                self.expr(*a)?;
                            }
                        }
                        TypedChainPart::Field { .. }
                        | TypedChainPart::InterfaceProperty { .. }
                        | TypedChainPart::NonNull { .. } => {}
                    }
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
            TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
                self.expr(*value)?;
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
}
