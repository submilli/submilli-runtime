//! An assignment used as a value: `b = (a = 3)`, `while ((m = next()) !== null)`.

use crate::compiler_error::CompilerFailure;

use crate::{
    BinOp, ExprId, ExprKind, Ident, Span, StmtId, Type, TypedExpr, TypedExprKind, TypedStmt,
    TypedStmtKind,
};

use super::{Inferer, narrowing};

impl Inferer<'_> {
    /// Infers the assignment exactly as its statement form, so it gets every
    /// check and narrowing that statement gets, then yields the assigned value
    /// at the value's own type, as TypeScript types `x = v` by `v`.
    ///
    /// The value is held in a temporary and yielded from there. Reading the
    /// target back would evaluate a field or index receiver twice, and a
    /// setter's getter or a `Uint8Array` store need not return what was set.
    /// A field or index target's receiver and index are held too, so each is
    /// evaluated once, before the value, as in JavaScript.
    pub(super) fn infer_assign_expr(
        &mut self,
        target: ExprId,
        op: Option<(BinOp, Span)>,
        value: ExprId,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let mut assignment = self.infer_assignment(target, op, value, span)?;
        if let TypedStmtKind::AssignLocal {
            target_ty: Type::Error,
            ..
        } = assignment
        {
            // An unresolved target, already reported.
            let stmts = vec![self.push_typed_stmt(assignment, span)];
            let result = self.error_placeholder_expr(span);
            return Ok((TypedExprKind::Sequence { stmts, result }, Type::Error));
        }
        let mut stmts = Vec::new();
        let held_value = if let TypedStmtKind::AssignLocal { value, .. }
        | TypedStmtKind::AssignGlobal { value, .. } = &mut assignment
        {
            let held_value = self.hold_in_temp(*value, "value", &mut stmts);
            *value = held_value;
            stmts.push(self.push_typed_stmt(assignment, span));
            held_value
        } else {
            self.sequence_member_assignment(assignment, span, &mut stmts)
        };
        let result = self.reread_temp(held_value);
        let ty = self.typed_ast.expr(result).ty.clone();
        Ok((TypedExprKind::Sequence { stmts, result }, ty))
    }

    /// The type a condition narrows from when it tests `expr`. An assignment
    /// used as a value has the value's type, as TypeScript gives it, but the
    /// narrowing lands on the binding, which a `readonly` declaration keeps
    /// readonly: `(a = [0]) !== null` must not hand `a` a mutable array.
    pub(super) fn narrowing_source_ty(&self, expr: &TypedExpr) -> Type {
        let TypedExprKind::Sequence { stmts, .. } = &expr.kind else {
            return expr.ty.clone();
        };
        let value_ty = expr.ty.clone();
        let Some(
            TypedStmtKind::AssignLocal { target_ty, .. }
            | TypedStmtKind::AssignGlobal { target_ty, .. },
        ) = stmts.last().map(|&s| &self.typed_ast.stmt(s).kind)
        else {
            return value_ty;
        };
        self.assignment_narrowed_ty(target_ty, value_ty)
    }

    /// A field or index assignment, with its receiver, index, and value held.
    /// Returns the temporary holding the value.
    fn sequence_member_assignment(
        &mut self,
        assignment: TypedStmtKind,
        span: Span,
        stmts: &mut Vec<StmtId>,
    ) -> ExprId {
        match assignment {
            TypedStmtKind::AssignField {
                receiver,
                name,
                value,
            } => self.sequence_field_assignment(receiver, name, value, span, stmts),
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                elem_ty,
            } => self.sequence_index_assignment(receiver, index, value, elem_ty, span, stmts),
            other => unreachable!("an assignment infers to an assignment statement, got {other:?}"),
        }
    }

    fn infer_assignment(
        &mut self,
        target: ExprId,
        op: Option<(BinOp, Span)>,
        value: ExprId,
        span: Span,
    ) -> Result<TypedStmtKind, CompilerFailure> {
        Ok(
            match (
                self.ast
                    .try_expr(target)
                    .map_err(super::arena_failure)?
                    .kind
                    .clone(),
                op,
            ) {
                (ExprKind::Identifier(ident), None) => self.infer_assign(ident, value, span)?,
                (ExprKind::Identifier(ident), Some((op, op_span))) => {
                    self.infer_compound_assign(ident, op, op_span, value, span)?
                }
                (ExprKind::FieldAccess { receiver, name }, None) => {
                    self.infer_assign_field(receiver, name, value)?
                }
                (ExprKind::FieldAccess { receiver, name }, Some((op, op_span))) => {
                    self.infer_compound_assign_field(receiver, name, op, op_span, value)?
                }
                (ExprKind::IndexAccess { receiver, index }, None) => {
                    self.infer_assign_index(receiver, index, value, span)?
                }
                (ExprKind::IndexAccess { receiver, index }, Some((op, op_span))) => {
                    self.infer_compound_assign_index(receiver, index, op, op_span, value, span)?
                }
                _ => {
                    return Err(super::inference_failure(
                        "the parser only builds assignments to valid targets",
                    ));
                }
            },
        )
    }

    /// Returns the temporary holding the assigned value.
    fn sequence_field_assignment(
        &mut self,
        receiver: ExprId,
        name: Ident,
        value: ExprId,
        span: Span,
        stmts: &mut Vec<StmtId>,
    ) -> ExprId {
        let held_receiver = self.hold_in_temp(receiver, "receiver", stmts);
        self.redirect_compound_read(value, receiver, held_receiver, None);
        let held_value = self.hold_in_temp(value, "value", stmts);
        let assignment = TypedStmtKind::AssignField {
            receiver: held_receiver,
            name,
            value: held_value,
        };
        stmts.push(self.push_typed_stmt(assignment, span));
        held_value
    }

    /// Returns the temporary holding the assigned value.
    fn sequence_index_assignment(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        value: ExprId,
        elem_ty: Type,
        span: Span,
        stmts: &mut Vec<StmtId>,
    ) -> ExprId {
        let held_receiver = self.hold_in_temp(receiver, "receiver", stmts);
        let held_index = self.hold_in_temp(index, "index", stmts);
        let held = HeldIndex {
            original: index,
            held: held_index,
        };
        self.redirect_compound_read(value, receiver, held_receiver, Some(held));
        let held_value = self.hold_in_temp(value, "value", stmts);
        let assignment = TypedStmtKind::AssignIndex {
            receiver: held_receiver,
            index: held_index,
            value: held_value,
            elem_ty,
        };
        stmts.push(self.push_typed_stmt(assignment, span));
        held_value
    }

    fn push_typed_stmt(&mut self, kind: TypedStmtKind, span: Span) -> StmtId {
        self.typed_ast.push_stmt(TypedStmt { kind, span })
    }

    fn error_placeholder_expr(&mut self, span: Span) -> ExprId {
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
        index: Option<HeldIndex>,
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
                if let Some(index) = index
                    && *read_index == index.original
                {
                    *read_index = index.held;
                }
            }
            _ => {}
        }
    }

    /// The binding an assignment used as a value writes, when it is a local or
    /// global. `(m = next()) !== null` tests the value just written to `m`, so
    /// it narrows `m`.
    pub(super) fn sequence_binding_path(
        &self,
        stmts: &[StmtId],
    ) -> Option<narrowing::ReferencePath> {
        match &self.typed_ast.stmt(*stmts.last()?).kind {
            TypedStmtKind::AssignLocal { ident, .. } => {
                let entry = self.scopes.get(&ident.name)?;
                Some(narrowing::ReferencePath::root(
                    narrowing::BindingId::Local {
                        name: ident.name.clone(),
                        decl_scope: entry.decl_scope,
                    },
                ))
            }
            TypedStmtKind::AssignGlobal { mangled, .. } => Some(narrowing::ReferencePath::root(
                narrowing::BindingId::Global(mangled.clone()),
            )),
            _ => None,
        }
    }

    /// Removes from `env` what the assignments among the typed expressions
    /// after `read` and up to `end` invalidate. A narrowing is a fact about a
    /// value read at `read`; a write evaluated after it in the same condition
    /// (`x !== null && f(x = null)`) replaces that value before the branch runs.
    /// Expressions are allocated after their operands, so the ids in
    /// `read + 1 ..= end` are the ones inferred after `read` finished.
    pub(super) fn forget_later_writes(
        &self,
        env: &mut narrowing::NarrowEnv,
        read: ExprId,
        end: ExprId,
    ) {
        if env.is_empty() || end.0 <= read.0 {
            return;
        }
        for id in read.0 + 1..=end.0 {
            let TypedExprKind::Sequence { stmts, .. } = &self.typed_ast.expr(ExprId(id)).kind
            else {
                continue;
            };
            let Some(written) = self.sequence_written_path(stmts) else {
                continue;
            };
            env.retain(|path, _| !written.is_prefix_of(path));
        }
    }

    /// The path an assignment sequence writes, or the root it writes under
    /// when the exact path has no reference form (a computed index).
    fn sequence_written_path(&self, stmts: &[StmtId]) -> Option<narrowing::ReferencePath> {
        if let Some(path) = self.sequence_binding_path(stmts) {
            return Some(path);
        }
        let (receiver, field) = match &self.typed_ast.stmt(*stmts.last()?).kind {
            TypedStmtKind::AssignField { receiver, name, .. } => {
                (*receiver, Some(name.name.clone()))
            }
            TypedStmtKind::AssignIndex { receiver, .. } => (*receiver, None),
            _ => return None,
        };
        let original = self.held_temp_source(stmts, receiver)?;
        let mut path = self.expr_to_reference_path(self.typed_ast.expr(original))?;
        if let Some(field) = field {
            path.chain.push(narrowing::PathElem::Field(field));
        }
        Some(path)
    }

    /// The expression a `hold_in_temp` read in `stmts` was initialized from.
    fn held_temp_source(&self, stmts: &[StmtId], read: ExprId) -> Option<ExprId> {
        let TypedExprKind::LocalRef { ident, .. } = &self.typed_ast.expr(read).kind else {
            return None;
        };
        stmts
            .iter()
            .find_map(|&stmt| match &self.typed_ast.stmt(stmt).kind {
                TypedStmtKind::Const { name, value, .. } if name.name == ident.name => Some(*value),
                _ => None,
            })
    }
}

/// An index expression and the temporary now holding its value.
struct HeldIndex {
    original: ExprId,
    held: ExprId,
}
