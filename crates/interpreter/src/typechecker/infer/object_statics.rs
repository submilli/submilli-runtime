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
            more_indexes: Vec::new(),
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
            // Sources may bring signatures a declaration would be refused for:
            // overlapping without nesting, or nested with values that don't.
            let span = self
                .typed_ast
                .try_expr(id)
                .map_err(super::arena_failure)?
                .span;
            match self.refused_signatures(&result) {
                Some(RefusedSignatures::Inconsistent(narrow, wide)) => {
                    self.report_inconsistent_signature(&narrow, &wide, span);
                    return Ok(Type::Error);
                }
                Some(RefusedSignatures::Overlapping(left, right)) => {
                    self.report_overlapping_signatures(&left, &right, span);
                    return Ok(Type::Error);
                }
                None => {}
            }
        }
        if matches!(target.peel(), Type::Array(_) | Type::Tuple(_)) {
            return Ok(target);
        }
        // Properties assigned onto a function make a hybrid: the function and
        // its properties, `F & P`, as TypeScript types `assign<T, U>`.
        if let Some(function) = self.callable_part(&target) {
            return Ok(Type::intersection(vec![function, result]));
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

    /// The first pair of signatures in any alternative of `result` that a
    /// declaration would be refused for.
    fn refused_signatures(&self, result: &Type) -> Option<RefusedSignatures> {
        let alternatives: &[Type] = match result.peel() {
            Type::Union(members) => members,
            _ => std::slice::from_ref(result),
        };
        let types = self.resolver();
        alternatives.iter().find_map(|alternative| {
            let signatures = types.index_signatures(alternative);
            let inconsistent = super::records::inconsistent_signatures(&signatures, types)
                .next()
                .map(|(narrow, wide)| {
                    RefusedSignatures::Inconsistent(narrow.clone(), wide.clone())
                });
            inconsistent.or_else(|| {
                super::records::overlapping_signatures(&signatures, types).map(|(left, right)| {
                    RefusedSignatures::Overlapping(left.clone(), right.clone())
                })
            })
        })
    }

    /// The function a callable `ty` holds: a function type, the function
    /// members of an intersection, or an interface's call signature.
    pub(super) fn callable_part(&self, ty: &Type) -> Option<Type> {
        match ty.peel() {
            Type::Function { .. } => Some(ty.clone()),
            Type::Intersection(_) => {
                super::assignable::hybrid_parts(ty).map(|(function, _)| function)
            }
            Type::InterfaceRef { .. } => self
                .resolver()
                .signature_of(ty, super::assignable::Signature::Call),
            _ => None,
        }
    }

    /// One value is callable and the other isn't, so a property written with
    /// both holds one or the other, never both.
    fn function_object_conflict(&self, left: &Type, right: &Type) -> bool {
        self.callable_part(left).is_some() != self.callable_part(right).is_some()
    }

    fn assignment_input_shape(&self, ty: &Type) -> Type {
        // A hybrid's properties are those of its object members.
        if let Some((_, fields)) = super::assignable::hybrid_parts(ty) {
            return Type::Object {
                fields,
                index: None,
                more_indexes: Vec::new(),
            };
        }
        match ty.peel() {
            Type::Union(members) => Type::union(
                members
                    .iter()
                    .map(|member| self.assignment_input_shape(member))
                    .collect(),
            ),
            Type::Unknown => Type::Unknown,
            // A callable user interface's properties are its named members;
            // its call signature is the function `callable_part` keeps. A
            // prelude constructor's members are no properties of its value.
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } if super::assignable::interface_has_signature(ty, self.resolver())
                && self.resolver().lookup(mangled, name).is_some_and(|symbol| {
                    matches!(
                        symbol.kind,
                        crate::TypeKind::Interface {
                            dispatch: crate::Dispatch::VTable,
                            ..
                        }
                    )
                }) =>
            {
                Type::Object {
                    fields: self
                        .resolver()
                        .interface_full_form(mangled, name, args)
                        .unwrap_or_default(),
                    index: None,
                    more_indexes: Vec::new(),
                }
            }
            Type::String | Type::StringLiteral(_) => Type::Object {
                more_indexes: Vec::new(),
                fields: BTreeMap::new(),
                index: Some(crate::types::IndexSignature {
                    string_keyof_includes_number: false,
                    key: Box::new(crate::Type::String),
                    value: Box::new(Type::String),
                    readonly: false,
                }),
            },
            _ => {
                let (index, more_indexes) =
                    crate::split_index_signatures(self.resolver().index_signatures(ty));
                Type::Object {
                    more_indexes,
                    fields: self.assign_shape(ty).unwrap_or_default(),
                    index,
                }
            }
        }
    }

    fn assign_shape(&self, ty: &Type) -> Option<BTreeMap<String, ObjectField>> {
        match ty.peel() {
            Type::ClassRef { mangled, args, .. } => self.resolver().class_full_form(mangled, args),
            _ => self.sole_object_shape(ty),
        }
    }

    /// Intersect property types under the compiler's normal size/work budget.
    /// Primitive disjoint properties become never; a function and an object
    /// take the later source's type, as `Object.assign` copies values; other
    /// incomparable values retain both constraints, including empty arrays and
    /// callable overloads.
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
            // A function and an object can't be one value: a property both
            // sources write holds the later one's, as `Object.assign` copies
            // values rather than merging them.
            if self.function_object_conflict(left, right) {
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
                    if field.optional && self.function_object_conflict(&existing.ty, &field.ty) {
                        // The later source may leave the earlier value in place.
                        budget.charge_copy(&existing.ty, depth + 1)?;
                        budget.charge_copy(&field.ty, depth + 1)?;
                        Type::union(vec![existing.ty.clone(), field.ty.clone()])
                    } else {
                        self.assign_intersection(&existing.ty, &field.ty, depth + 1, budget)?
                    };
                existing.optional &= field.optional;
                existing.readonly &= field.readonly;
            } else {
                budget.charge_copy(&field.ty, depth + 1)?;
                fields.insert(name, field);
            }
        }
        // The result is both sources at once (tsc's `T & U`): it has each
        // one's signatures, and a key both have reads as both values.
        let mut indexes = self.resolver().index_signatures(left);
        for b in self.resolver().index_signatures(right) {
            if let Some(a) = indexes.iter_mut().find(|a| a.key.peel() == b.key.peel()) {
                a.string_keyof_includes_number |= b.string_keyof_includes_number;
                *a.value = self.assign_intersection(&a.value, &b.value, depth + 1, budget)?;
                a.readonly &= b.readonly;
            } else {
                budget.charge_copy(&b.value, depth + 1)?;
                indexes.push(b);
            }
        }
        let (index, more_indexes) = crate::split_index_signatures(indexes);
        Ok(Type::Object {
            fields,
            index,
            more_indexes,
        })
    }
}

/// Signatures an `Object.assign` result would hold that a declaration is
/// refused for.
enum RefusedSignatures {
    /// A key within another's whose value doesn't fit (TS2413).
    Inconsistent(IndexSignature, IndexSignature),
    /// Keys that overlap without nesting.
    Overlapping(IndexSignature, IndexSignature),
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
                Type::Object {
                    fields,
                    index,
                    more_indexes,
                } => {
                    from_fields = true;
                    if fields.is_empty() && index.is_none() {
                        continue;
                    }
                    Type::union(field_value_types(fields, index, more_indexes).collect())
                }
                // Unlike an object type, an interface has no implicit index
                // signature, so only one that declares one is enumerated.
                interface @ Type::InterfaceRef { .. } => {
                    let Some(Type::Object {
                        fields,
                        index: index @ Some(_),
                        more_indexes,
                    }) = expand_interface_data_shape(interface, self.resolver())
                    else {
                        return None;
                    };
                    from_fields = true;
                    Type::union(field_value_types(&fields, &index, &more_indexes).collect())
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

/// The types of the values an object type lists: its index signatures' and
/// its fields'. A present optional field may hold `undefined`, which is
/// listed, so it adds `undefined` as its read does, where tsc leaves it out.
fn field_value_types<'a>(
    fields: &'a BTreeMap<String, ObjectField>,
    index: &'a Option<IndexSignature>,
    more_indexes: &'a [IndexSignature],
) -> impl Iterator<Item = Type> + 'a {
    let field_types = fields.values().map(|field| {
        if field.optional {
            Type::union(vec![field.ty.clone(), Type::Undefined])
        } else {
            field.ty.clone()
        }
    });
    crate::stored_index_signatures(index, more_indexes)
        .map(|index| (*index.value).clone())
        .chain(field_types)
}
