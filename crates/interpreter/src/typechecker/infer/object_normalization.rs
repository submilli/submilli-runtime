//! tsc's normalization of fresh object literal types. When object literals
//! join into one type, in an array literal or a conditional, each member gains
//! every field only its siblings declare as an optional field tsc types
//! `undefined` (here an optional `never`, which reads as `null`), so any field
//! reads from the union: `[{ a: 0 }, { b: "x" }]` holds
//! `{ a: number; b?: never } | { b: string; a?: never }`. A field that holds
//! object literals in every member normalizes the same way against the objects
//! that field holds in the others, at every depth.
//!
//! Only fresh object literals normalize: their types list every field they
//! hold. A value typed with fewer fields may hold the others, with any type, so
//! the syntax decides which types are fresh.

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler_error::CompilerFailure;
use crate::types::Type;
use crate::{ExprId, ExprKind};

use super::expr::peel_parens;

type ObjectFields = BTreeMap<String, crate::ObjectField>;

/// The fields of a set of sibling fresh object literals that hold fresh object
/// literals themselves, and so normalize one level down, each with its own
/// nested fields.
#[derive(Debug, Default, PartialEq)]
pub(super) struct NormalizingFields {
    nested: BTreeMap<String, NormalizingFields>,
}

/// When every element is a fresh object literal, or a conditional choosing
/// between them, the fields that normalize below the top level. `None`
/// otherwise.
pub(super) fn array_normalization(
    ast: &crate::Ast,
    elements: &[crate::ArrayLiteralElement],
) -> Result<Option<NormalizingFields>, CompilerFailure> {
    let mut choices = Vec::new();
    for element in elements {
        let crate::ArrayLiteralElement::Value(id) = element else {
            return Ok(None);
        };
        let Some(literals) = fresh_object_choices(ast, *id)? else {
            return Ok(None);
        };
        choices.extend(literals);
    }
    normalizing_fields(ast, choices).map(Some)
}

/// When both branches of a conditional are fresh object literals, `null`, or
/// conditionals choosing between them, the fields that normalize below the
/// top level. `None` otherwise.
pub(super) fn conditional_normalization(
    ast: &crate::Ast,
    then_: ExprId,
    else_: ExprId,
) -> Result<Option<NormalizingFields>, CompilerFailure> {
    match branch_choices(ast, then_, else_)? {
        Some(choices) => normalizing_fields(ast, choices).map(Some),
        None => Ok(None),
    }
}

/// Whether every element is an array literal whose elements are all fresh
/// object literals, or conditionals choosing between them. An empty one holds
/// no element to say otherwise.
pub(super) fn every_array_of_object_literals(
    ast: &crate::Ast,
    elements: &[crate::ArrayLiteralElement],
) -> Result<bool, CompilerFailure> {
    for element in elements {
        let crate::ArrayLiteralElement::Value(id) = element else {
            return Ok(false);
        };
        let id = peel_parens(ast, *id)?;
        let ExprKind::ArrayLiteral { elements: inner } =
            &ast.try_expr(id).map_err(super::arena_failure)?.kind
        else {
            return Ok(false);
        };
        if !inner.is_empty() && array_normalization(ast, inner)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `expr`'s type lists every field it holds, at the top level and in
/// the objects an array of it holds: an object literal, or an array literal of
/// them. tsc checks such a type for excess fields when it reduces subtypes; a
/// literal that only spreads takes its fields from values, which it doesn't
/// check.
pub(super) fn is_fresh_literal(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    Ok(
        match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
            ExprKind::ObjectLiteral { members } => {
                members.is_empty()
                    || members
                        .iter()
                        .any(|member| !matches!(member, crate::ObjectLiteralMember::Spread { .. }))
            }
            ExprKind::ArrayLiteral { elements } => {
                for element in elements {
                    let crate::ArrayLiteralElement::Value(id) = element else {
                        return Ok(false);
                    };
                    if !is_fresh_literal(ast, *id)? {
                        return Ok(false);
                    }
                }
                true
            }
            _ => false,
        },
    )
}

/// Whether the object literal `expr` names exactly the fields of `running`, a
/// single object type, and so does each object literal it holds directly.
pub(super) fn has_running_shape(
    ast: &crate::Ast,
    expr: ExprId,
    running: Option<&Type>,
) -> Result<bool, CompilerFailure> {
    let Some(Type::Object { fields, .. }) = running else {
        return Ok(false);
    };
    let Some(literal_fields) = fresh_object_fields(ast, expr)? else {
        return Ok(false);
    };
    let literal_names = literal_fields
        .iter()
        .map(|field| &field.name.name)
        .collect::<BTreeSet<_>>();
    if literal_names != fields.keys().collect() {
        return Ok(false);
    }
    for field in literal_fields {
        let running_field = fields.get(&field.name.name).map(|running| &running.ty);
        if matches!(running_field, Some(Type::Object { .. }))
            && fresh_object_fields(ast, field.value)?.is_some()
            && !has_running_shape(ast, field.value, running_field)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The fields of a fresh object literal: one that only names its fields, with
/// no spread or computed key that could bring in fields its type doesn't list.
pub(super) fn fresh_object_fields(
    ast: &crate::Ast,
    expr: ExprId,
) -> Result<Option<Vec<&crate::ObjectLiteralField>>, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    let ExprKind::ObjectLiteral { members } = &ast.try_expr(id).map_err(super::arena_failure)?.kind
    else {
        return Ok(None);
    };
    Ok(members
        .iter()
        .map(|member| match member {
            crate::ObjectLiteralMember::Field(field) => Some(field),
            _ => None,
        })
        .collect())
}

/// The nested fields of sibling literals: those that hold a fresh object
/// literal in every literal that names them, apart from `null` and primitive
/// literals, which tsc leaves out of a field's siblings.
fn normalizing_fields(
    ast: &crate::Ast,
    siblings: Vec<Vec<&crate::ObjectLiteralField>>,
) -> Result<NormalizingFields, CompilerFailure> {
    let mut values: BTreeMap<&String, Vec<Vec<&crate::ObjectLiteralField>>> = BTreeMap::new();
    let mut stale = BTreeSet::new();
    for field in siblings.into_iter().flatten() {
        let name = &field.name.name;
        match fresh_object_choices(ast, field.value)? {
            Some(choices) => values.entry(name).or_default().extend(choices),
            None if is_primitive_literal(ast, field.value)? => {}
            None => {
                stale.insert(name);
            }
        }
    }
    let mut nested = BTreeMap::new();
    for (name, choices) in values {
        if !stale.contains(name) {
            nested.insert(name.clone(), normalizing_fields(ast, choices)?);
        }
    }
    Ok(NormalizingFields { nested })
}

/// Whether `expr` is `null` or a primitive literal, signed or negated.
fn is_primitive_literal(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    Ok(
        match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
            ExprKind::Null
            | ExprKind::Number(_)
            | ExprKind::BigInt(_)
            | ExprKind::String(_)
            | ExprKind::Boolean(_) => true,
            ExprKind::Unary { operand, .. } => is_primitive_literal(ast, *operand)?,
            _ => false,
        },
    )
}

/// The fields of each fresh object literal `expr` may evaluate to: itself, or
/// each branch of a conditional choosing between such literals or `null`,
/// which tsc leaves out of the siblings. `None` when it may evaluate to
/// anything else.
fn fresh_object_choices(
    ast: &crate::Ast,
    expr: ExprId,
) -> Result<Option<Vec<Vec<&crate::ObjectLiteralField>>>, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    let kind = &ast.try_expr(id).map_err(super::arena_failure)?.kind;
    if matches!(kind, ExprKind::Null) {
        return Ok(Some(Vec::new()));
    }
    if let ExprKind::Ternary { then_, else_, .. } = kind {
        return branch_choices(ast, *then_, *else_);
    }
    Ok(fresh_object_fields(ast, id)?.map(|fields| vec![fields]))
}

/// The choices of both branches of a conditional, or `None` when either may
/// evaluate to anything else.
fn branch_choices(
    ast: &crate::Ast,
    then_: ExprId,
    else_: ExprId,
) -> Result<Option<Vec<Vec<&crate::ObjectLiteralField>>>, CompilerFailure> {
    let (Some(mut choices), Some(others)) = (
        fresh_object_choices(ast, then_)?,
        fresh_object_choices(ast, else_)?,
    ) else {
        return Ok(None);
    };
    choices.extend(others);
    Ok(Some(choices))
}

/// `ty`, the join of sibling fresh object literals, with each object member
/// normalized against the others, and the `normalizing` fields against the
/// objects they hold in the others.
pub(super) fn normalized(ty: &Type, normalizing: &NormalizingFields) -> Type {
    with_sibling_fields(ty, &object_parts(ty), normalizing)
}

/// Whether `source`, a fresh literal's type, has a field `target` lacks in an
/// object both reach in the same place: tsc's excess field check, which keeps
/// it from being `target`'s subtype.
pub(super) fn has_excess_field(source: &Type, target: &Type) -> bool {
    match (source.peel(), target.peel()) {
        (
            Type::Object {
                fields: source_fields,
                ..
            },
            Type::Object {
                fields: target_fields,
                index,
            },
        ) => source_fields
            .iter()
            .filter(|(_, field)| !is_added_missing_field(field))
            .any(|(name, source_field)| match target_fields.get(name) {
                None => index.is_none(),
                Some(target_field) => has_excess_field(&source_field.ty, &target_field.ty),
            }),
        (Type::Array(source_element), Type::Array(target_element)) => {
            has_excess_field(source_element, target_element)
        }
        _ => false,
    }
}

/// A field normalization added. It says nothing about the field's type.
pub(super) fn is_added_missing_field(field: &crate::ObjectField) -> bool {
    field.optional && field.ty == Type::Never
}

fn with_sibling_fields(ty: &Type, siblings: &[&Type], normalizing: &NormalizingFields) -> Type {
    match ty {
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|member| with_sibling_fields(member, siblings, normalizing))
                .collect(),
        ),
        Type::Object {
            fields,
            index: None,
        } => {
            let mut fields = fields.clone();
            for (name, nested) in &normalizing.nested {
                let Some(field) = fields.get_mut(name) else {
                    continue;
                };
                let field_siblings = siblings
                    .iter()
                    .filter_map(|sibling| object_fields(sibling)?.get(name))
                    .filter(|field| !is_added_missing_field(field))
                    .flat_map(|field| object_parts(&field.ty))
                    .collect::<Vec<_>>();
                field.ty = with_sibling_fields(&field.ty, &field_siblings, nested);
            }
            let names = siblings
                .iter()
                .filter_map(|sibling| object_fields(sibling))
                .flat_map(|fields| fields.keys());
            Type::Object {
                fields: fields_with_missing(fields, names),
                index: None,
            }
        }
        _ => ty.clone(),
    }
}

/// The object members of `ty`: itself, or the objects in a union. The rest,
/// `null` among them, have no fields to share.
fn object_parts(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Object { .. } => vec![ty],
        Type::Union(members) => members
            .iter()
            .filter(|member| matches!(member, Type::Object { .. }))
            .collect(),
        _ => Vec::new(),
    }
}

fn object_fields(ty: &Type) -> Option<&ObjectFields> {
    match ty {
        Type::Object { fields, .. } => Some(fields),
        _ => None,
    }
}

fn fields_with_missing<'a>(
    mut fields: ObjectFields,
    names: impl Iterator<Item = &'a String>,
) -> ObjectFields {
    for name in names {
        fields
            .entry(name.clone())
            .or_insert_with(|| crate::ObjectField::optional(Type::Never));
    }
    fields
}
