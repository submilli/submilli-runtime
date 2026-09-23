//! An assignment used as a value: `b = (a = 3)`, `while ((m = next()) !== null)`.

use crate::{
    BinOp, ExprId, ExprKind, Ident, Span, StmtId, Type, TypedExpr, TypedExprKind, TypedStmt,
    TypedStmtKind,
};

use super::Inferer;

impl Inferer<'_> {
    /// Infers the assignment exactly as its statement form, so it gets every
    /// check and narrowing that statement gets, then yields the assigned value.
    ///
    /// A binding target is read back after the write, which gives the value
    /// with the type the write narrowed the binding to. A field or index
    /// target is not read back: that would evaluate its receiver twice, and a
    /// setter's getter need not return what was set. Its receiver, index, and
    /// value are held in temporaries instead, and the value is yielded.
    pub(super) fn infer_assign_expr(
        &mut self,
        target: ExprId,
        op: Option<(BinOp, Span)>,
        value: ExprId,
        span: Span,
    ) -> (TypedExprKind, Type) {
        let assignment = self.infer_assignment(target, op, value, span);
        let mut stmts = Vec::new();
        let result = match assignment {
            TypedStmtKind::AssignLocal {
                target_ty: Type::Error,
                ..
            } => {
                // An unresolved target, already reported: reading it back would
                // report it again.
                stmts.push(self.push_typed_stmt(assignment, span));
                return (
                    TypedExprKind::Sequence {
                        stmts,
                        result: self.poison(span),
                    },
                    Type::Error,
                );
            }
            TypedStmtKind::AssignLocal { .. } | TypedStmtKind::AssignGlobal { .. } => {
                stmts.push(self.push_typed_stmt(assignment, span));
                self.infer_expr(target, None).0
            }
            TypedStmtKind::AssignField {
                receiver,
                name,
                value,
            } => {
                let held_receiver = self.hold_in_temp(receiver, "receiver", &mut stmts);
                self.redirect_compound_read(value, receiver, held_receiver, None);
                let held_value = self.hold_in_temp(value, "value", &mut stmts);
                let assignment = TypedStmtKind::AssignField {
                    receiver: held_receiver,
                    name,
                    value: held_value,
                };
                stmts.push(self.push_typed_stmt(assignment, span));
                self.reread_temp(held_value)
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                elem_ty,
            } => {
                let held_receiver = self.hold_in_temp(receiver, "receiver", &mut stmts);
                let held_index = self.hold_in_temp(index, "index", &mut stmts);
                self.redirect_compound_read(
                    value,
                    receiver,
                    held_receiver,
                    Some((index, held_index)),
                );
                let held_value = self.hold_in_temp(value, "value", &mut stmts);
                let assignment = TypedStmtKind::AssignIndex {
                    receiver: held_receiver,
                    index: held_index,
                    value: held_value,
                    elem_ty,
                };
                stmts.push(self.push_typed_stmt(assignment, span));
                self.reread_temp(held_value)
            }
            other => unreachable!("an assignment infers to an assignment statement, got {other:?}"),
        };
        let ty = self.typed_ast.expr(result).ty.clone();
        (TypedExprKind::Sequence { stmts, result }, ty)
    }

    fn infer_assignment(
        &mut self,
        target: ExprId,
        op: Option<(BinOp, Span)>,
        value: ExprId,
        span: Span,
    ) -> TypedStmtKind {
        match (self.ast.expr(target).kind.clone(), op) {
            (ExprKind::Identifier(ident), None) => self.infer_assign(ident, value, span),
            (ExprKind::Identifier(ident), Some((op, op_span))) => {
                self.infer_compound_assign(ident, op, op_span, value, span)
            }
            (ExprKind::FieldAccess { receiver, name }, None) => {
                self.infer_assign_field(receiver, name, value)
            }
            (ExprKind::FieldAccess { receiver, name }, Some((op, op_span))) => {
                self.infer_compound_assign_field(receiver, name, op, op_span, value)
            }
            (ExprKind::IndexAccess { receiver, index }, None) => {
                self.infer_assign_index(receiver, index, value, span)
            }
            (ExprKind::IndexAccess { receiver, index }, Some((op, op_span))) => {
                self.infer_compound_assign_index(receiver, index, op, op_span, value, span)
            }
            _ => unreachable!("the parser only builds assignments to valid targets"),
        }
    }

    fn push_typed_stmt(&mut self, kind: TypedStmtKind, span: Span) -> StmtId {
        self.typed_ast.push_stmt(TypedStmt { kind, span })
    }

    fn poison(&mut self, span: Span) -> ExprId {
        self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::Null,
            span,
            ty: Type::Error,
        })
    }

    /// Declares a `const` temporary holding `expr`, and returns a read of it.
    fn hold_in_temp(&mut self, expr: ExprId, role: &str, stmts: &mut Vec<StmtId>) -> ExprId {
        let TypedExpr { span, ty, .. } = self.typed_ast.expr(expr).clone();
        // `#` cannot occur in a source identifier, and the expression id is
        // unique, so the name cannot collide.
        let name = Ident {
            name: format!("#assign_{role}_{}", expr.0),
            span,
        };
        stmts.push(self.push_typed_stmt(
            TypedStmtKind::Const {
                name: name.clone(),
                ty: ty.clone(),
                value: expr,
                doc: None,
            },
            span,
        ));
        self.typed_ast.push_expr(TypedExpr {
            kind: TypedExprKind::LocalRef {
                ident: name,
                boxed: false,
            },
            span,
            ty,
        })
    }

    fn reread_temp(&mut self, held: ExprId) -> ExprId {
        let read = self.typed_ast.expr(held).clone();
        self.typed_ast.push_expr(read)
    }

    /// A compound assignment's value reads the target through the same
    /// receiver (and index) expression it writes. Point that read at the held
    /// temporaries, so the receiver is evaluated once.
    fn redirect_compound_read(
        &mut self,
        value: ExprId,
        receiver: ExprId,
        held_receiver: ExprId,
        index: Option<(ExprId, ExprId)>,
    ) {
        let TypedExprKind::Binary { lhs, .. } = self.typed_ast.expr(value).kind else {
            return;
        };
        match &mut self.typed_ast.expr_mut(lhs).kind {
            TypedExprKind::FieldAccess { receiver: read, .. } if *read == receiver => {
                *read = held_receiver;
            }
            TypedExprKind::IndexAccess {
                receiver: read,
                index: read_index,
            } if *read == receiver => {
                *read = held_receiver;
                if let Some((index, held_index)) = index
                    && *read_index == index
                {
                    *read_index = held_index;
                }
            }
            _ => {}
        }
    }
}
