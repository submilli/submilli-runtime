//! The operands of `===`, `!==` and a `case` label as `tsc` compares them:
//! literals keep their literal types, an enum member is the literal of its
//! value, and a template of literals is the string it spells.

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, ExprKind, MangledName, Type, TypedExprKind, UnOp};

use super::assignable::TypeResolver;
use super::comparable::comparable;

/// An equality operand or `case` label as it is compared: its type, the enum it
/// is a member of, and how a diagnostic names it.
pub(super) struct ComparisonOperand {
    pub(super) ty: Type,
    /// The enum an enum member belongs to. A member compares as its value, but
    /// never with a value of a different enum, whatever the values.
    member_of: Option<MangledName>,
    pub(super) label: String,
}

/// Whether the two operands can ever be equal.
pub(super) fn operands_comparable(
    left: &ComparisonOperand,
    right: &ComparisonOperand,
    types: TypeResolver,
) -> bool {
    if let (Some(left_enum), Some(right_enum)) = (&left.member_of, &right.member_of)
        && left_enum != right_enum
    {
        return false;
    }
    let (Some(left_ty), Some(right_ty)) = (
        without_other_enums(&left.ty, right.member_of.as_ref()),
        without_other_enums(&right.ty, left.member_of.as_ref()),
    ) else {
        return false;
    };
    comparable(&left_ty, &right_ty, types)
}

/// `ty` without the union members that are values of an enum other than
/// `member_of`, which a member of `member_of` never equals even where the values
/// match. `None` when nothing but `null` is left to compare.
fn without_other_enums(ty: &Type, member_of: Option<&MangledName>) -> Option<Type> {
    let Some(member_of) = member_of else {
        return Some(ty.clone());
    };
    let other_enum = |member: &Type| enum_name(member).is_some_and(|name| name != member_of);
    let members = match ty.peel() {
        Type::Union(members) => members.as_slice(),
        _ => std::slice::from_ref(ty),
    };
    if !members.iter().any(other_enum) {
        return Some(ty.clone());
    }
    let kept: Vec<Type> = members
        .iter()
        .filter(|member| !other_enum(member))
        .cloned()
        .collect();
    if kept
        .iter()
        .all(|member| matches!(member.peel(), Type::Null))
    {
        return None;
    }
    Some(Type::union(kept))
}

fn enum_name(ty: &Type) -> Option<&MangledName> {
    match ty.peel() {
        Type::NumberEnum { mangled, .. } | Type::StringEnum { mangled, .. } => Some(mangled),
        _ => None,
    }
}

impl super::Inferer<'_> {
    /// The operand `source` (typed as `typed`) as it is compared: a template
    /// whose substitutions are all string or number literals is the string it
    /// spells, as `tsc` folds it (`` `abc${0}` `` compares as `"abc0"`); an enum
    /// member is the literal of its value; anything else keeps its literal type.
    pub(super) fn comparison_operand(
        &self,
        source: ExprId,
        typed: ExprId,
    ) -> Result<ComparisonOperand, CompilerFailure> {
        let typed = self
            .typed_ast
            .try_expr(typed)
            .map_err(crate::typechecker::arena_failure)?;
        if let Some(text) = constant_template(self.ast, source)? {
            return Ok(ComparisonOperand::of_type(Type::StringLiteral(text)));
        }
        let member_value = match &typed.kind {
            TypedExprKind::NumberEnumMember { value, variant, .. } => {
                Some((super::expr::number_literal_type(*value), variant))
            }
            TypedExprKind::StringEnumMember { value, variant, .. } => {
                Some((Type::StringLiteral(value.clone()), variant))
            }
            _ => None,
        };
        let Some((ty, variant)) = member_value else {
            let ty = super::expr::literal_comparison_type(&self.typed_ast, typed)?;
            return Ok(ComparisonOperand::of_type(ty));
        };
        Ok(ComparisonOperand {
            ty,
            member_of: enum_name(&typed.ty).cloned(),
            label: format!("{}.{}", typed.ty, variant.name),
        })
    }
}

impl super::Inferer<'_> {
    /// Whether `typed` reads the global `NaN`, not a local of that name. `tsc`
    /// reports comparing it with `===` or `!==` (TS2845), since `NaN` equals
    /// nothing, itself included. Like `tsc`, `Number.NaN` is not checked.
    pub(super) fn is_global_nan(&self, typed: ExprId) -> Result<bool, CompilerFailure> {
        let typed = self
            .typed_ast
            .try_expr(typed)
            .map_err(crate::typechecker::arena_failure)?;
        Ok(matches!(
            &typed.kind,
            TypedExprKind::GlobalRef { mangled, name }
                if name.name == "NaN" && crate::mangle::is_builtin(mangled)
        ))
    }

    pub(super) fn error_nan_comparison(&mut self, op: crate::BinOp, span: crate::Span) {
        let always = if op == crate::BinOp::NotEq { "true" } else { "false" };
        self.error_with_help(
            span,
            format!("this comparison is always `{always}`: `NaN` is not equal to any value, itself included"),
            vec!["use `Number.isNaN(x)` to test for `NaN`".to_string()],
        );
    }
}

impl ComparisonOperand {
    /// An operand that is not an enum member, compared as its type.
    fn of_type(ty: Type) -> Self {
        Self {
            label: ty.to_string(),
            member_of: None,
            ty,
        }
    }
}

/// The string a template literal spells when every substitution is a string or
/// number literal, else `None`.
fn constant_template(ast: &crate::Ast, id: ExprId) -> Result<Option<String>, CompilerFailure> {
    let (parts, exprs) = match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
        ExprKind::Paren(inner) => return constant_template(ast, *inner),
        ExprKind::TemplateLiteral { parts, exprs, .. } => (parts, exprs),
        _ => return Ok(None),
    };
    let mut text = String::new();
    for (index, part) in parts.iter().enumerate() {
        text.push_str(part);
        let Some(expr) = exprs.get(index) else {
            continue;
        };
        let Some(value) = constant_substitution(ast, *expr)? else {
            return Ok(None);
        };
        text.push_str(&value);
    }
    Ok(Some(text))
}

fn constant_substitution(ast: &crate::Ast, id: ExprId) -> Result<Option<String>, CompilerFailure> {
    Ok(
        match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => constant_substitution(ast, *inner)?,
            ExprKind::String(value) => Some(value.clone()),
            ExprKind::TemplateLiteral { .. } => constant_template(ast, id)?,
            _ => constant_number(ast, id)?.map(crate::runtime::number::format_number_js),
        },
    )
}

pub(super) fn constant_number(ast: &crate::Ast, id: ExprId) -> Result<Option<f64>, CompilerFailure> {
    Ok(
        match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => constant_number(ast, *inner)?,
            ExprKind::Number(value) => Some(*value),
            ExprKind::Unary {
                op: UnOp::Neg,
                operand,
            } => constant_number(ast, *operand)?.map(|value| -value),
            ExprKind::Unary {
                op: UnOp::Pos,
                operand,
            } => constant_number(ast, *operand)?,
            _ => None,
        },
    )
}
