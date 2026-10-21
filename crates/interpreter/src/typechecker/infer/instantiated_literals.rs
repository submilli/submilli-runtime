//! The literal types an argument's array, tuple and object literals keep once
//! a generic call is inferred.
//!
//! An argument is inferred against its parameter with the call's type
//! parameters still open, so a literal in it widens against a bare `V`. tsc
//! then checks the argument again against the parameter instantiated with the
//! inferred type arguments, where a literal keeps its literal type in a slot
//! that names a literal of its kind: `new Map([["", true]])` binds `V` to
//! `boolean`, so the tuple is `[string, true]`.
//!
//! Only the types of the literal nodes change. The binding already happened,
//! codegen builds arrays and tuples from their element types, and lays object
//! literals out from their field origins' declared types, all of which stay.

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Type, TypedArrayElement, TypedExprKind};

use super::Inferer;
use super::expr::number_literal_type;
use super::inference_sources::is_literal_of_context;

impl Inferer<'_> {
    /// Give the literals in `argument` the types they take against `param`,
    /// the parameter with every type parameter bound.
    pub(super) fn retype_argument_literals(
        &mut self,
        argument: ExprId,
        param: &Type,
    ) -> Result<(), CompilerFailure> {
        self.retype_literal(argument, param).map(|_| ())
    }

    /// Re-type `id` against `slot`, returning its new type when it changed.
    fn retype_literal(&mut self, id: ExprId, slot: &Type) -> Result<Option<Type>, CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?;
        let current = expr.ty.clone();
        let retyped = match &expr.kind {
            TypedExprKind::Boolean(value) if current == Type::Boolean => {
                Some(Type::BooleanLiteral(*value))
                    .filter(|literal| is_literal_of_context(literal, slot))
            }
            TypedExprKind::Number(value) if current == Type::Number => {
                Some(number_literal_type(*value))
                    .filter(|literal| is_literal_of_context(literal, slot))
            }
            TypedExprKind::String(value) if current == Type::String => {
                Some(Type::StringLiteral(value.clone()))
                    .filter(|literal| is_literal_of_context(literal, slot))
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                let elements = elements.clone();
                self.retype_tuple(&elements, &current, slot)?
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                let values: Option<Vec<ExprId>> = elements
                    .iter()
                    .map(|element| match element {
                        TypedArrayElement::Value(value) => Some(*value),
                        TypedArrayElement::Spread(_) => None,
                    })
                    .collect();
                match values {
                    Some(values) => self.retype_array(&values, &current, slot)?,
                    None => None,
                }
            }
            TypedExprKind::ObjectLiteral { fields, .. } => {
                let fields = fields.clone();
                self.retype_object(&fields, &current, slot)?
            }
            _ => None,
        };
        if let Some(ty) = &retyped {
            self.typed_ast
                .try_expr_mut(id)
                .map_err(crate::typechecker::arena_failure)?
                .ty = ty.clone();
        }
        Ok(retyped)
    }

    fn retype_tuple(
        &mut self,
        elements: &[ExprId],
        current: &Type,
        slot: &Type,
    ) -> Result<Option<Type>, CompilerFailure> {
        let (Type::Tuple(own), Some(Type::Tuple(expected))) = (
            current,
            sole_member(slot, |member| matches!(member, Type::Tuple(_))),
        ) else {
            return Ok(None);
        };
        if own.elements.len() != elements.len() || expected.elements.len() != elements.len() {
            return Ok(None);
        }
        let mut retyped = own.clone();
        let mut changed = false;
        for (index, (element, element_slot)) in elements.iter().zip(&expected.elements).enumerate()
        {
            if let Some(ty) = self.retype_literal(*element, element_slot)? {
                retyped.elements[index] = ty;
                changed = true;
            }
        }
        Ok(changed.then_some(Type::Tuple(retyped)))
    }

    fn retype_object(
        &mut self,
        fields: &[crate::TypedObjectFieldOrigin],
        current: &Type,
        slot: &Type,
    ) -> Result<Option<Type>, CompilerFailure> {
        let (
            Type::Object {
                fields: own_fields, ..
            },
            Some(Type::Object {
                fields: slot_fields,
                ..
            }),
        ) = (
            current,
            sole_member(slot, |member| matches!(member, Type::Object { .. })),
        )
        else {
            return Ok(None);
        };
        let mut retyped = current.clone();
        let Type::Object {
            fields: retyped_fields,
            ..
        } = &mut retyped
        else {
            return Ok(None);
        };
        let mut changed = false;
        for origin in fields {
            let crate::TypedObjectFieldSource::Literal(value) = origin.source else {
                continue;
            };
            let name = &origin.name.name;
            let (Some(own), Some(field_slot)) = (own_fields.get(name), slot_fields.get(name))
            else {
                continue;
            };
            if let Some(ty) = self.retype_literal(value, &field_slot.ty)? {
                retyped_fields.insert(name.clone(), crate::ObjectField { ty, ..own.clone() });
                changed = true;
            }
        }
        Ok(changed.then_some(retyped))
    }

    fn retype_array(
        &mut self,
        values: &[ExprId],
        current: &Type,
        slot: &Type,
    ) -> Result<Option<Type>, CompilerFailure> {
        let (Type::Array(_), Some(Type::Array(element_slot))) = (
            current,
            sole_member(slot, |member| matches!(member, Type::Array(_))),
        ) else {
            return Ok(None);
        };
        let mut changed = false;
        let mut element_types = Vec::with_capacity(values.len());
        for value in values {
            changed |= self.retype_literal(*value, element_slot)?.is_some();
            element_types.push(
                self.typed_ast
                    .try_expr(*value)
                    .map_err(crate::typechecker::arena_failure)?
                    .ty
                    .clone(),
            );
        }
        Ok(changed.then(|| Type::Array(Box::new(Type::union(element_types)))))
    }
}

/// The one member of `slot`, peeled, that `is_kind` accepts: a parameter such
/// as `Map`'s `readonly [K, V][] | Iterable<[K, V]> | null` holds one array.
fn sole_member(slot: &Type, is_kind: impl Fn(&Type) -> bool) -> Option<&Type> {
    let members: Vec<&Type> = match slot.peel() {
        Type::Union(members) => members.iter().map(Type::peel).collect(),
        other => vec![other],
    };
    match members
        .into_iter()
        .filter(|m| is_kind(m))
        .collect::<Vec<_>>()[..]
    {
        [only] => Some(only),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::run_clean;
    use crate::{ExprId, TypedExprKind};

    /// The type of every tuple and array literal in `source`, in order.
    fn literal_types(source: &str) -> Vec<String> {
        let ta = run_clean(source);
        (0..ta.exprs_len())
            .map(|index| ta.try_expr(ExprId(index as u32)).unwrap())
            .filter(|expr| {
                matches!(
                    expr.kind,
                    TypedExprKind::TupleLiteral { .. } | TypedExprKind::ArrayLiteral { .. }
                )
            })
            .map(|expr| expr.ty.to_string())
            .collect()
    }

    #[test]
    fn argument_literals_keep_what_the_instantiated_parameter_names() {
        let types = literal_types(
            "const m = new Map([[\"\", true]]);\n\
             function main(): void {}",
        );
        assert_eq!(types, ["[string, true]", "[string, true][]"]);
    }

    #[test]
    fn object_argument_literals_keep_what_the_instantiated_parameter_names() {
        let ta = run_clean(
            "function id<V>(z: V): V { return z; }\n\
             const r = id({ n: 1, s: \"a\", b: true });\n\
             function main(): void {}",
        );
        let types: Vec<String> = (0..ta.exprs_len())
            .map(|index| ta.try_expr(ExprId(index as u32)).unwrap())
            .filter(|expr| matches!(expr.kind, TypedExprKind::ObjectLiteral { .. }))
            .map(|expr| expr.ty.to_string())
            .collect();
        assert_eq!(types, ["{ b: true; n: number; s: string }"]);
    }

    #[test]
    fn argument_literals_widen_where_the_instantiated_parameter_names_no_literal() {
        let types = literal_types(
            "function box<T>(xs: T[]): T[] { return xs; }\n\
             const b = box([1, 2]);\n\
             function main(): void {}",
        );
        assert_eq!(types, ["number[]"]);
    }
}
