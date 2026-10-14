//! Result shapes for object statics whose properties depend on their arguments.
use std::collections::BTreeMap;

use super::{Inferer, assignable::assignable};
use crate::compiler_error::CompilerFailure;
use crate::type_size::{TypeBudget, TypeTooLarge};
use crate::{ExprId, ObjectField, Type};

impl Inferer<'_> {
    pub(super) fn object_assign_result(&self, args: &[ExprId]) -> Result<Type, CompilerFailure> {
        let Some(first) = args.first() else {
            return Ok(Type::Error);
        };
        let target = self
            .typed_ast
            .try_expr(*first)
            .map_err(super::arena_failure)?
            .ty
            .clone();
        if args.len() == 1 {
            return Ok(target);
        }
        let mut budget = self.type_limits.budget();
        let mut result = Type::Object {
            fields: BTreeMap::new(),
            index: None,
        };
        for &id in args {
            let ty = &self
                .typed_ast
                .try_expr(id)
                .map_err(super::arena_failure)?
                .ty;
            let shape = self.assignment_input_shape(ty);
            result = match self.assign_intersection(&result, &shape, 1, &mut budget) {
                Ok(result) => result,
                Err(exceeded) => {
                    self.type_limits.record(exceeded);
                    return Ok(Type::Error);
                }
            };
        }
        if matches!(target.peel(), Type::Array(_) | Type::Tuple(_)) {
            return Ok(target);
        }
        if matches!(
            target.peel(),
            Type::ClassRef { .. } | Type::GenericParam { .. }
        ) {
            Ok(Type::Refined {
                original: Box::new(target),
                ty: Box::new(result),
            })
        } else {
            Ok(result)
        }
    }

    fn assignment_input_shape(&self, ty: &Type) -> Type {
        match ty.peel() {
            Type::Union(members) => Type::union(
                members
                    .iter()
                    .map(|member| self.assignment_input_shape(member))
                    .collect(),
            ),
            Type::Unknown => Type::Unknown,
            Type::String | Type::StringLiteral(_) => Type::Object {
                fields: BTreeMap::new(),
                index: Some(crate::types::IndexSignature {
                    value: Box::new(Type::String),
                    readonly: false,
                }),
            },
            _ => Type::Object {
                fields: self.assign_shape(ty).unwrap_or_default(),
                index: self.resolver().index_signature(ty),
            },
        }
    }

    fn assign_shape(&self, ty: &Type) -> Option<BTreeMap<String, ObjectField>> {
        match ty.peel() {
            Type::ClassRef { mangled, args, .. } => self.resolver().class_full_form(mangled, args),
            _ => self.sole_object_shape(ty),
        }
    }

    /// Intersect property types under the compiler's normal size/work budget.
    /// A disjoint property is never, rather than making the native call diverge.
    fn assign_intersection(
        &self,
        left: &Type,
        right: &Type,
        depth: u32,
        budget: &mut TypeBudget<'_>,
    ) -> Result<Type, TypeTooLarge> {
        budget.charge(depth)?;
        if left.peel() == &Type::Unknown {
            budget.charge_copy(right, depth)?;
            return Ok(right.clone());
        }
        if right.peel() == &Type::Unknown {
            budget.charge_copy(left, depth)?;
            return Ok(left.clone());
        }
        if let Type::Union(members) = left.peel() {
            return members
                .iter()
                .map(|member| self.assign_intersection(member, right, depth + 1, budget))
                .collect::<Result<Vec<_>, _>>()
                .map(Type::union);
        }
        if let Type::Union(members) = right.peel() {
            return members
                .iter()
                .map(|member| self.assign_intersection(left, member, depth + 1, budget))
                .collect::<Result<Vec<_>, _>>()
                .map(Type::union);
        }
        let (Some(mut fields), Some(other)) = (self.assign_shape(left), self.assign_shape(right))
        else {
            if assignable(left, right, self.resolver()) {
                budget.charge_copy(left, depth)?;
                return Ok(left.clone());
            }
            if assignable(right, left, self.resolver()) {
                budget.charge_copy(right, depth)?;
                return Ok(right.clone());
            }
            return Ok(Type::Never);
        };
        for (name, field) in other {
            if let Some(existing) = fields.get_mut(&name) {
                existing.ty =
                    self.assign_intersection(&existing.ty, &field.ty, depth + 1, budget)?;
                existing.optional &= field.optional;
                existing.readonly &= field.readonly;
            } else {
                budget.charge_copy(&field.ty, depth + 1)?;
                fields.insert(name, field);
            }
        }
        let index = match (
            self.resolver().index_signature(left),
            self.resolver().index_signature(right),
        ) {
            (Some(a), Some(b)) => Some(crate::types::IndexSignature {
                value: Box::new(self.assign_intersection(&a.value, &b.value, depth + 1, budget)?),
                readonly: a.readonly && b.readonly,
            }),
            (index, None) | (None, index) => index,
        };
        Ok(Type::Object { fields, index })
    }
}
