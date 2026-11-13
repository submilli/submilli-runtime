//! Result shapes for object statics whose properties depend on their arguments.
use std::collections::BTreeMap;

use super::Inferer;
use super::assignable::{assignable, expand_interface_data_shape};
use crate::compiler_error::CompilerFailure;
use crate::type_size::{TypeBudget, TypeTooLarge};
use crate::{ExprId, IndexSignature, ObjectField, Span, Type, TypedExprKind};

impl Inferer<'_> {
    pub(super) fn object_assign_result(
        &mut self,
        args: &[ExprId],
    ) -> Result<Type, CompilerFailure> {
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
            if self.index_composition_conflicts(&result, &shape) {
                let span = self
                    .typed_ast
                    .try_expr(id)
                    .map_err(super::arena_failure)?
                    .span;
                self.error(span, "Object.assign cannot combine index signatures with different key and value types".into());
                return Ok(Type::Error);
            }
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

    fn index_composition_conflicts(&self, left: &Type, right: &Type) -> bool {
        if let Type::Union(members) = left.peel() {
            return members
                .iter()
                .any(|member| self.index_composition_conflicts(member, right));
        }
        if let Type::Union(members) = right.peel() {
            return members
                .iter()
                .any(|member| self.index_composition_conflicts(left, member));
        }
        self.resolver()
            .index_signature(left)
            .zip(self.resolver().index_signature(right))
            .is_some_and(|(left, right)| {
                left.key.peel() != right.key.peel() && left.value.peel() != right.value.peel()
            })
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
                    string_keyof_includes_number: false,
                    key: Box::new(crate::Type::String),
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
    /// Primitive disjoint properties become never; other incomparable values
    /// retain both constraints, including empty arrays and callable overloads.
    pub(super) fn assign_intersection(
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
            budget.charge_copy(left, depth + 1)?;
            budget.charge_copy(right, depth + 1)?;
            return Ok(Type::intersection(vec![left.clone(), right.clone()]));
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
                string_keyof_includes_number: a.string_keyof_includes_number
                    || b.string_keyof_includes_number,
                key: Box::new(Type::union(vec![(*a.key).clone(), (*b.key).clone()])),
                value: Box::new(self.assign_intersection(&a.value, &b.value, depth + 1, budget)?),
                readonly: a.readonly && b.readonly,
            }),
            (index, None) | (None, index) => index,
        };
        Ok(Type::Object { fields, index })
    }
}

/// What `Object.values` or `Object.entries` lists for one argument.
struct EnumeratedValues {
    /// The union of the listed values' types.
    ty: Type,
    /// Whether some are read from an object's fields. Those can hold values
    /// of other types at run time: a record is covariant in its value type,
    /// and an object keeps the fields its static type leaves out. An array's
    /// elements and a string's units are what their types say.
    from_fields: bool,
}

impl Inferer<'_> {
    /// Type the `call` of `Object.values(arg)`, or of `Object.entries(arg)`
    /// when `lists_entries`, by what `arg` holds, as tsc's overload
    /// `values<T>(o: { [s: string]: T } | ArrayLike<T>): T[]` types it. Where
    /// tsc falls back to `values(o: {}): any[]`, the call keeps its `declared`
    /// `unknown` values. Values read from fields are checked at run time, as
    /// `call as T[]` checks them, so one outside its declared type throws a
    /// `TypeError` instead of reaching code typed for `T`. A `T` that check
    /// can't verify keeps `unknown` too.
    pub(super) fn type_object_enumeration(
        &mut self,
        call: TypedExprKind,
        declared: Type,
        lists_entries: bool,
        arg: ExprId,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let arg_ty = &self
            .typed_ast
            .try_expr(arg)
            .map_err(super::arena_failure)?
            .ty;
        let Some(values) = self.enumerated_values(arg_ty) else {
            return Ok((call, declared));
        };
        let element = if lists_entries {
            Type::tuple(vec![Type::String, values.ty])
        } else {
            values.ty
        };
        let result = Type::Array(Box::new(element));
        if !values.from_fields {
            return Ok((call, result));
        }
        let Ok(check) = self.as_check_shape(&result) else {
            return Ok((call, declared));
        };
        self.checked_call(call, declared, &result, check, span)
    }

    /// What `Object.values` lists for an argument of type `ty`, or `None` to
    /// keep the declared `unknown`: where tsc types the values `any`, or where
    /// nothing more precise is sound.
    fn enumerated_values(&self, ty: &Type) -> Option<EnumeratedValues> {
        let members = match ty.peel() {
            Type::Union(members) => members.as_slice(),
            _ => std::slice::from_ref(ty),
        };
        // The values of each member, from which tsc infers `T`.
        let mut candidates = Vec::new();
        let mut from_fields = false;
        for member in members {
            // A match array also lists its `index`, `input` and `groups`.
            if member.is_regexp_match_array() {
                return None;
            }
            let candidate = match member.peel() {
                // `Object.values` throws on these rather than listing anything.
                Type::Null | Type::Undefined | Type::Void => continue,
                Type::Array(element) => {
                    // So does a match array widened to this array type, or
                    // bound to a type parameter of its element.
                    if assignable(&Type::regexp_element(), element, self.resolver())
                        || super::narrowing::has_type_parameter_member(element)
                    {
                        return None;
                    }
                    (**element).clone()
                }
                Type::Tuple(tuple) => Type::union(tuple.elements.clone()),
                Type::String | Type::StringLiteral(_) | Type::TemplateLiteral { .. } => {
                    Type::String
                }
                // An object type lists its index signature's and fields'
                // values: tsc's implicit index signature. `{}` lists none
                // statically, but may hold any object, so it still needs the
                // check.
                Type::Object { fields, index } => {
                    from_fields = true;
                    if fields.is_empty() && index.is_none() {
                        continue;
                    }
                    Type::union(field_value_types(fields, index.as_ref()).collect())
                }
                // Unlike an object type, an interface has no implicit index
                // signature, so only one that declares one is enumerated.
                interface @ Type::InterfaceRef { .. } => {
                    let Some(Type::Object {
                        fields,
                        index: Some(index),
                    }) = expand_interface_data_shape(interface, self.resolver())
                    else {
                        return None;
                    };
                    from_fields = true;
                    Type::union(field_value_types(&fields, Some(&index)).collect())
                }
                _ => return None,
            };
            candidates.push(candidate);
        }
        let ty = self.inferred_candidate(&candidates)?;
        if matches!(ty.peel(), Type::Unknown | Type::Error) {
            return None;
        }
        Some(EnumeratedValues { ty, from_fields })
    }

    /// The `T` tsc infers from `candidates`: the one the others fit, or the
    /// union of literals of one primitive or enum type. `None` where neither
    /// holds: tsc's inference fails and the call falls back to `any`.
    fn inferred_candidate(&self, candidates: &[Type]) -> Option<Type> {
        let union = Type::union(candidates.to_vec());
        if let Some(supertype) = candidates
            .iter()
            .find(|candidate| assignable(&union, candidate, self.resolver()))
        {
            return Some(supertype.clone());
        }
        let mut bases = candidates.iter().map(Type::widen_literal);
        let first = bases.next()?;
        let is_primitive_or_enum = matches!(
            first.peel(),
            Type::Number
                | Type::String
                | Type::Boolean
                | Type::BigInt
                | Type::NumberEnum { .. }
                | Type::StringEnum { .. }
        );
        (is_primitive_or_enum && bases.all(|base| base == first)).then_some(union)
    }
}

/// The types of the values an object type lists: its index signature's and
/// its fields'. A present optional field may hold `undefined`, which is
/// listed, so it adds `undefined` as its read does, where tsc leaves it out.
fn field_value_types<'a>(
    fields: &'a BTreeMap<String, ObjectField>,
    index: Option<&'a IndexSignature>,
) -> impl Iterator<Item = Type> + 'a {
    let field_types = fields.values().map(|field| {
        if field.optional {
            Type::union(vec![field.ty.clone(), Type::Undefined])
        } else {
            field.ty.clone()
        }
    });
    index
        .map(|index| (*index.value).clone())
        .into_iter()
        .chain(field_types)
}
