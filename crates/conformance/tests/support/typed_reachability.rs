//! Retained typed-AST nodes, excluding abandoned speculative inference passes.
use interpreter::{
    ClosureBody, ExprId, PostfixTarget, StmtId, TypedAst, TypedChainPart, TypedExprKind,
    TypedStmtKind,
};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Reachable {
    pub expressions: BTreeSet<ExprId>,
    pub statements: BTreeSet<StmtId>,
}

impl Reachable {
    pub fn collect(typed: &TypedAst) -> Self {
        let mut nodes = Self::default();
        for function in &typed.functions {
            nodes.walk_stmt(typed, function.body);
        }
        for &statement in &typed.top_level_statements {
            nodes.walk_stmt(typed, statement);
        }
        for statement in typed.class_body_roots() {
            nodes.walk_stmt(typed, statement);
        }
        for expression in typed.class_field_initializers() {
            nodes.walk_expr(typed, expression);
        }
        nodes
    }

    fn walk_stmt(&mut self, ta: &TypedAst, id: StmtId) {
        if !self.statements.insert(id) {
            return;
        }
        match &ta.try_stmt(id).unwrap().kind {
            TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => {
                self.walk_expr(ta, *value);
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.walk_expr(ta, *condition);
                self.walk_stmt(ta, *then_block);
                if let Some(else_block) = else_block {
                    self.walk_stmt(ta, *else_block);
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.walk_expr(ta, *condition);
                self.walk_stmt(ta, *body);
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.walk_stmt(ta, *init);
                }
                if let Some(condition) = condition {
                    self.walk_expr(ta, *condition);
                }
                if let Some(update) = update {
                    self.walk_stmt(ta, *update);
                }
                self.walk_stmt(ta, *body);
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                self.walk_expr(ta, *iter);
                self.walk_stmt(ta, *body);
            }
            TypedStmtKind::DoWhile { body, condition } => {
                self.walk_stmt(ta, *body);
                self.walk_expr(ta, *condition);
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.walk_expr(ta, *discriminant);
                for case in cases {
                    self.walk_stmt(ta, case.body);
                }
                if let Some(default) = default {
                    self.walk_stmt(ta, *default);
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::Return(value) => {
                if let Some(value) = value {
                    self.walk_expr(ta, *value);
                }
            }
            TypedStmtKind::Expr(expr) => self.walk_expr(ta, *expr),
            TypedStmtKind::Block(stmts) => {
                for &stmt in stmts {
                    self.walk_stmt(ta, stmt);
                }
            }
            TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => self.walk_expr(ta, *value),
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                self.walk_expr(ta, *receiver);
                self.walk_expr(ta, *value);
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.walk_expr(ta, *receiver);
                self.walk_expr(ta, *index);
                self.walk_expr(ta, *value);
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                self.walk_expr(ta, *source);
                self.walk_stmt(ta, *body);
            }
            TypedStmtKind::Throw { value } => self.walk_expr(ta, *value),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.walk_stmt(ta, *body);
                for clause in catches {
                    self.walk_stmt(ta, clause.body);
                }
                if let Some(finally) = finally {
                    self.walk_stmt(ta, *finally);
                }
            }
        }
    }

    fn walk_expr(&mut self, ta: &TypedAst, id: ExprId) {
        if !self.expressions.insert(id) {
            return;
        }
        match &ta.try_expr(id).unwrap().kind {
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. } => {
                for &arg in args {
                    self.walk_expr(ta, arg);
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.walk_expr(ta, *callee);
                for &arg in args {
                    self.walk_expr(ta, arg);
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for arg in args {
                    self.walk_expr(ta, arg.expr);
                }
            }
            TypedExprKind::Closure { body, .. } => match *body {
                ClosureBody::Expr(expr) => self.walk_expr(ta, expr),
                ClosureBody::Block(stmt) => self.walk_stmt(ta, stmt),
            },
            TypedExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(ta, *lhs);
                self.walk_expr(ta, *rhs);
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.walk_expr(ta, *effect);
                self.walk_expr(ta, *result);
            }
            TypedExprKind::Sequence { stmts, result } => {
                for &stmt in stmts {
                    self.walk_stmt(ta, stmt);
                }
                self.walk_expr(ta, *result);
            }
            TypedExprKind::Unary { operand, .. }
            | TypedExprKind::TypeofTag { value: operand, .. }
            | TypedExprKind::InstanceOf { value: operand, .. }
            | TypedExprKind::NonNullAssert { value: operand } => self.walk_expr(ta, *operand),
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.walk_expr(ta, *receiver);
                for &arg in args {
                    self.walk_expr(ta, arg);
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.walk_expr(ta, *receiver);
                for arg in args {
                    self.walk_expr(ta, arg.expr);
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    self.walk_expr(ta, member.expr_id());
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    self.walk_expr(ta, elem.expr_id());
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for &elem in elements {
                    self.walk_expr(ta, elem);
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                self.walk_expr(ta, *receiver);
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                self.walk_expr(ta, *receiver);
                self.walk_expr(ta, *index);
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                self.walk_expr(ta, *source);
                self.walk_expr(ta, *inner);
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.walk_expr(ta, *cond);
                self.walk_expr(ta, *then_);
                self.walk_expr(ta, *else_);
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                self.walk_expr(ta, *lhs);
                self.walk_expr(ta, *rhs);
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.walk_expr(ta, *base);
                for part in parts {
                    match part {
                        TypedChainPart::Index { idx, .. } => self.walk_expr(ta, *idx),
                        TypedChainPart::Call { args, .. }
                        | TypedChainPart::MethodCall { args, .. } => {
                            for &arg in args {
                                self.walk_expr(ta, arg);
                            }
                        }
                        TypedChainPart::Field { .. }
                        | TypedChainPart::InterfaceProperty { .. }
                        | TypedChainPart::NonNull { .. } => {}
                    }
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                PostfixTarget::Field { receiver, .. } => self.walk_expr(ta, *receiver),
                PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    self.walk_expr(ta, *receiver);
                    self.walk_expr(ta, *index);
                }
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {}
            },
            TypedExprKind::Cast { value, .. } => self.walk_expr(ta, *value),
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::Undefined
            | TypedExprKind::This
            | TypedExprKind::Regex { .. }
            | TypedExprKind::LocalRef { .. }
            | TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => {}
        }
    }
}
