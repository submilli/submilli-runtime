//! tsc's checks for code that type-checks but is almost certainly a mistake: a
//! condition whose truthiness its syntax already decides (TS2872, TS2873), a
//! `??` whose left side is never or always nullish (TS2869, TS2871), and a
//! function or enum member tested where a call or comparison was meant (TS2774,
//! TS2845). Like tsc, the first two look only at the syntax of the operand, not
//! its type, so `if (flag)` on a `true`-typed `flag` stays legal.

use crate::compiler_error::CompilerFailure;
use crate::{BinOp, ChainPart, ExprId, ExprKind, PostfixOp, Span, Type};

use super::Inferer;

/// What the syntax of an expression decides about a test of it, as a set:
/// `ALWAYS | NEVER` is "sometimes", which decides nothing.
type Outcomes = u8;
const ALWAYS: Outcomes = 1;
const NEVER: Outcomes = 2;
const SOMETIMES: Outcomes = ALWAYS | NEVER;

/// A read the uncalled-function check compares by name: a binding or `this`,
/// then the fields read from it.
#[derive(PartialEq, Eq)]
enum PathRoot {
    Binding(String),
    This,
}

impl Inferer<'_> {
    /// TS2872 and TS2873: reports a condition operand whose syntax makes it
    /// always truthy (an object, array or function literal, a non-empty string,
    /// a number other than `0` or `1`) or always falsy (`null`, `undefined`,
    /// `void`, `""`).
    pub(super) fn check_constant_truthiness(
        &mut self,
        operand: ExprId,
    ) -> Result<(), CompilerFailure> {
        let span = self
            .ast
            .try_expr(operand)
            .map_err(super::arena_failure)?
            .span;
        match self.syntactic_truthiness(operand)? {
            ALWAYS => self.error_with_help(
                span,
                "this kind of expression is always truthy".to_string(),
                vec!["the test can never fail; test a value that can change".to_string()],
            ),
            NEVER => self.error_with_help(
                span,
                "this kind of expression is always falsy".to_string(),
                vec!["the test can never pass; test a value that can change".to_string()],
            ),
            _ => {}
        }
        Ok(())
    }

    /// TS2869 and TS2871: reports the left side of `??` when its syntax makes it
    /// never nullish (a literal, an operator that yields a non-nullish value),
    /// so the right side never runs, or always nullish, so the left is wasted.
    /// Returns whether it reported.
    pub(super) fn check_constant_nullishness(
        &mut self,
        lhs: ExprId,
    ) -> Result<bool, CompilerFailure> {
        let target = self.skip_outer_expressions(lhs)?;
        let span = self
            .ast
            .try_expr(target)
            .map_err(super::arena_failure)?
            .span;
        match self.syntactic_nullishness(target)? {
            ALWAYS => self.error(span, "this expression is always nullish".to_string()),
            NEVER => self.error_with_help(
                span,
                "right operand of `??` is unreachable because the left operand is never nullish"
                    .to_string(),
                vec!["remove the `??` and its right operand".to_string()],
            ),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// TS2774 and TS2845 for the condition of an `if` or a conditional: reports
    /// a function tested for truthiness that `body`, the code the condition
    /// guards, never uses, and an enum member, whose value is fixed. The
    /// operands of a `||` or `??` chain are tested too, as tsc does.
    pub(super) fn check_tested_functions(
        &mut self,
        condition: ExprId,
        body: Span,
    ) -> Result<(), CompilerFailure> {
        let mut pending = vec![condition];
        while let Some(expr) = pending.pop() {
            let expr = self.skip_parens(expr)?;
            match self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::Binary {
                    op: BinOp::Or | BinOp::NullishCoalesce,
                    lhs,
                    rhs,
                } => {
                    pending.push(lhs);
                    pending.push(rhs);
                }
                _ => self.check_tested_function(expr, None, Some(body))?,
            }
        }
        Ok(())
    }

    /// TS2774 and TS2845 for the left operand of `a && b`, which `b` may use.
    /// When the `&&` is (part of) an `if` condition, the `if`'s body may use it
    /// too.
    fn check_tested_function_in_and(
        &mut self,
        and: ExprId,
        lhs: ExprId,
        rhs: ExprId,
    ) -> Result<(), CompilerFailure> {
        let rhs_span = self.ast.try_expr(rhs).map_err(super::arena_failure)?.span;
        let body = self.if_condition_ands.get(&and).copied();
        let lhs = self.skip_parens(lhs)?;
        if let ExprKind::Binary {
            op: BinOp::Or | BinOp::NullishCoalesce,
            ..
        } = self.ast.try_expr(lhs).map_err(super::arena_failure)?.kind
        {
            return Ok(());
        }
        self.check_tested_function(lhs, Some(rhs_span), body)
    }

    /// Notes, for the `if` about to check `condition`, the `&&` operators its
    /// truthiness reaches through parentheses, `&&` and `||`: their left
    /// operands may be used in `body`.
    pub(super) fn note_if_condition_ands(
        &mut self,
        condition: ExprId,
        body: Span,
    ) -> Result<(), CompilerFailure> {
        let mut pending = vec![condition];
        while let Some(expr) = pending.pop() {
            let expr = self.skip_parens(expr)?;
            if let ExprKind::Binary {
                op: op @ (BinOp::And | BinOp::Or),
                lhs,
                rhs,
            } = self.ast.try_expr(expr).map_err(super::arena_failure)?.kind
            {
                if op == BinOp::And {
                    self.if_condition_ands.insert(expr, body);
                }
                pending.push(lhs);
                pending.push(rhs);
            }
        }
        Ok(())
    }

    /// After inferring `expr`: records a read the condition checks look at, and
    /// checks the left operand of an `&&`.
    pub(super) fn check_tested_read(
        &mut self,
        expr: ExprId,
        ty: &Type,
    ) -> Result<(), CompilerFailure> {
        match self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
            ExprKind::Identifier(_)
            | ExprKind::FieldAccess { .. }
            | ExprKind::OptionalChain { .. } => {
                self.note_tested_read(expr, ty);
            }
            ExprKind::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } => self.check_tested_function_in_and(expr, lhs, rhs)?,
            _ => {}
        }
        Ok(())
    }

    /// Records `expr` as a read of a function or of one enum member, for the
    /// checks on a condition that reads it.
    fn note_tested_read(&mut self, expr: ExprId, ty: &Type) {
        if matches!(ty.peel(), Type::Function { .. }) {
            self.function_reads.insert(expr);
        } else {
            self.function_reads.remove(&expr);
        }
        let member_truthiness = match ty.peel() {
            Type::NumberEnum {
                member: Some(member),
                ..
            } => Some(member.value.0 != 0.0 && !member.value.0.is_nan()),
            Type::StringEnum {
                member: Some(member),
                ..
            } => Some(!member.value.is_empty()),
            _ => None,
        };
        match member_truthiness {
            Some(truthy) => self.enum_member_reads.insert(expr, truthy),
            None => self.enum_member_reads.remove(&expr),
        };
    }

    fn check_tested_function(
        &mut self,
        tested: ExprId,
        chain_rest: Option<Span>,
        body: Option<Span>,
    ) -> Result<(), CompilerFailure> {
        let span = self
            .ast
            .try_expr(tested)
            .map_err(super::arena_failure)?
            .span;
        if let Some(value) = self.enum_member_truthiness(tested)? {
            self.error(span, format!("this condition will always return `{value}`"));
            return Ok(());
        }
        if !self.function_reads.contains(&tested) {
            return Ok(());
        }
        let Some((root, fields)) = self.read_path(tested)? else {
            return Ok(());
        };
        let used = match chain_rest {
            Some(rest) => self.names_read_within(rest, &root, &fields)?,
            None => false,
        } || match body {
            Some(body) => self.path_read_within(body, &root, &fields)?,
            None => false,
        };
        if !used {
            self.error_with_help(
                span,
                "this condition will always return true since this function is always defined"
                    .to_string(),
                vec!["did you mean to call it instead?".to_string()],
            );
        }
        Ok(())
    }

    /// A member of a numeric or string enum read as `E.A`, tested for
    /// truthiness: its value decides the test.
    fn enum_member_truthiness(&self, tested: ExprId) -> Result<Option<bool>, CompilerFailure> {
        let ExprKind::FieldAccess { receiver, .. } = &self
            .ast
            .try_expr(tested)
            .map_err(super::arena_failure)?
            .kind
        else {
            return Ok(None);
        };
        if !matches!(
            self.ast
                .try_expr(*receiver)
                .map_err(super::arena_failure)?
                .kind,
            ExprKind::Identifier(_)
        ) {
            return Ok(None);
        }
        Ok(self.enum_member_reads.get(&tested).copied())
    }

    /// Whether some expression within `region` reads the same binding or
    /// field `root` and `fields` name: tsc counts any such read in the code a
    /// condition guards as a use of the function it tests.
    fn path_read_within(
        &self,
        region: Span,
        root: &PathRoot,
        fields: &[String],
    ) -> Result<bool, CompilerFailure> {
        for id in self.ast.expr_ids().map_err(super::arena_failure)? {
            let expr = self.ast.try_expr(id).map_err(super::arena_failure)?;
            if !region.encloses(expr.span) {
                continue;
            }
            if let Some((other_root, other_fields)) = self.read_path(id)?
                && other_root == *root
                && other_fields == fields
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether some expression within `region` names the last step of the
    /// tested read: the binding itself, or a field of that name. The right
    /// operands of an `&&` chain count as a use by name alone, as in tsc.
    fn names_read_within(
        &self,
        region: Span,
        root: &PathRoot,
        fields: &[String],
    ) -> Result<bool, CompilerFailure> {
        for id in self.ast.expr_ids().map_err(super::arena_failure)? {
            let expr = self.ast.try_expr(id).map_err(super::arena_failure)?;
            if !region.encloses(expr.span) {
                continue;
            }
            let reads = match (&expr.kind, fields.last(), root) {
                (ExprKind::Identifier(ident), None, PathRoot::Binding(name)) => ident.name == *name,
                (ExprKind::FieldAccess { name, .. }, Some(field), _) => name.name == *field,
                (ExprKind::OptionalChain { parts, .. }, Some(field), _) => parts.iter().any(
                    |part| matches!(part, ChainPart::Field { name, .. } if name.name == *field),
                ),
                _ => false,
            };
            if reads {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `expr` as a binding or `this` followed by field reads, or `None` when
    /// it is any other expression.
    fn read_path(&self, expr: ExprId) -> Result<Option<(PathRoot, Vec<String>)>, CompilerFailure> {
        let mut fields = Vec::new();
        let mut current = expr;
        loop {
            match &self
                .ast
                .try_expr(current)
                .map_err(super::arena_failure)?
                .kind
            {
                ExprKind::Identifier(ident) => {
                    fields.reverse();
                    return Ok(Some((PathRoot::Binding(ident.name.clone()), fields)));
                }
                ExprKind::This => {
                    fields.reverse();
                    return Ok(Some((PathRoot::This, fields)));
                }
                ExprKind::FieldAccess { receiver, name } => {
                    fields.push(name.name.clone());
                    current = *receiver;
                }
                ExprKind::OptionalChain { base, parts } => {
                    for part in parts.iter().rev() {
                        let ChainPart::Field { name, .. } = part else {
                            return Ok(None);
                        };
                        fields.push(name.name.clone());
                    }
                    current = *base;
                }
                ExprKind::Paren(inner) => current = *inner,
                _ => return Ok(None),
            }
        }
    }

    fn syntactic_truthiness(&self, expr: ExprId) -> Result<Outcomes, CompilerFailure> {
        let expr = self.skip_outer_expressions(expr)?;
        Ok(
            match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                // `while (1)` and `while (0)` are idioms, not mistakes.
                ExprKind::Number(value) if *value == 0.0 || *value == 1.0 => SOMETIMES,
                ExprKind::Number(_)
                | ExprKind::BigInt(_)
                | ExprKind::ArrayLiteral { .. }
                | ExprKind::ObjectLiteral { .. }
                | ExprKind::Arrow { .. }
                | ExprKind::FunctionExpression { .. }
                | ExprKind::Regex { .. } => ALWAYS,
                ExprKind::Void { .. } | ExprKind::Null => NEVER,
                ExprKind::String(text) => truthiness_of_text(text),
                ExprKind::TemplateLiteral { parts, exprs, .. } if exprs.is_empty() => {
                    truthiness_of_text(&parts.concat())
                }
                ExprKind::Ternary { then_, else_, .. } => {
                    self.syntactic_truthiness(*then_)? | self.syntactic_truthiness(*else_)?
                }
                ExprKind::Identifier(ident) if self.reads_global_undefined(expr, &ident.name) => {
                    NEVER
                }
                _ => SOMETIMES,
            },
        )
    }

    fn syntactic_nullishness(&self, expr: ExprId) -> Result<Outcomes, CompilerFailure> {
        let expr = self.skip_outer_expressions(expr)?;
        Ok(
            match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::Call { .. }
                | ExprKind::IndexAccess { .. }
                | ExprKind::FieldAccess { .. }
                | ExprKind::OptionalChain { .. }
                | ExprKind::New { .. }
                | ExprKind::This
                | ExprKind::Super => SOMETIMES,
                ExprKind::Binary {
                    op: BinOp::NullishCoalesce | BinOp::Or | BinOp::And,
                    ..
                } => SOMETIMES,
                ExprKind::Binary { .. } => NEVER,
                // A plain or logical assignment yields what it stores; an
                // arithmetic one yields a number or a string.
                ExprKind::Assign { op, .. } => match op {
                    None | Some(BinOp::NullishCoalesce | BinOp::Or | BinOp::And) => SOMETIMES,
                    Some(_) => NEVER,
                },
                ExprKind::Ternary { then_, else_, .. } => {
                    self.syntactic_nullishness(*then_)? | self.syntactic_nullishness(*else_)?
                }
                ExprKind::Null | ExprKind::Void { .. } => ALWAYS,
                ExprKind::Identifier(ident) if self.reads_global_undefined(expr, &ident.name) => {
                    ALWAYS
                }
                ExprKind::Identifier(_) => SOMETIMES,
                _ => NEVER,
            },
        )
    }

    fn reads_global_undefined(&self, expr: ExprId, name: &str) -> bool {
        self.ast.synthetic_undefined.contains(&expr) || self.names_global_undefined(name)
    }

    /// Strips what tsc skips before judging an operand: parentheses, `as` and
    /// `!`, none of which change the value.
    fn skip_outer_expressions(&self, mut expr: ExprId) -> Result<ExprId, CompilerFailure> {
        loop {
            expr = match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::Paren(inner) | ExprKind::As { expr: inner, .. } => *inner,
                ExprKind::PostfixUnary {
                    op: PostfixOp::NonNullAssert,
                    operand,
                } => *operand,
                _ => return Ok(expr),
            };
        }
    }

    fn skip_parens(&self, mut expr: ExprId) -> Result<ExprId, CompilerFailure> {
        while let ExprKind::Paren(inner) =
            &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind
        {
            expr = *inner;
        }
        Ok(expr)
    }
}

fn truthiness_of_text(text: &str) -> Outcomes {
    if text.is_empty() { NEVER } else { ALWAYS }
}
