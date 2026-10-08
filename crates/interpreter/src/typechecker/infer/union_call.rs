//! Calling a value whose type is a union of function types. tsc calls it
//! through one combined signature: each parameter takes a type every member's
//! parameter there accepts (their intersection), and the result is the union
//! of the members' results.

use std::collections::BTreeMap;

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Type, TypedExpr, TypedExprKind};

use super::Inferer;
use super::assignable::assignable;

/// How many objects deep the fields of a combined parameter type merge.
const MERGE_DEPTH: usize = 8;

/// How many fields, in all, the parameter types of one union may merge:
/// types that share structure would otherwise merge into an object that
/// grows exponentially with their depth.
const MERGE_FIELDS: usize = 256;

impl Inferer<'_> {
    /// The callee `typed_callee`, of type `callee_ty`, cast up to the one
    /// signature its members combine into when it is a union of function
    /// types; otherwise as it is.
    pub(super) fn cast_up_to_union_signature(
        &mut self,
        typed_callee: ExprId,
        callee_ty: Type,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        let Some(signature) = self.union_signature_of(&callee_ty) else {
            return Ok((typed_callee, callee_ty));
        };
        let span = self
            .typed_ast
            .try_expr(typed_callee)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        let cast = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Cast {
                    value: typed_callee,
                    target_ty: signature.clone(),
                    check: None,
                },
                span,
                ty: signature.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok((cast, signature))
    }

    /// The signature a callee of type `ty` is called through when `ty` is a
    /// union of function types that combine.
    pub(super) fn union_signature_of(&self, ty: &Type) -> Option<Type> {
        match ty.peel() {
            Type::Union(members) => self.union_call_signature(members),
            _ => None,
        }
    }

    /// The function type a union of function types `members` is called
    /// through, or `None` when they don't combine: a member that is not a
    /// function or has a rest parameter, members that differ in whether they
    /// return a value, or a position whose parameter types have no common
    /// subtype Submilli can name. A member with fewer parameters ignores the
    /// arguments past them, so the combined signature takes the most.
    pub(super) fn union_call_signature(&self, members: &[Type]) -> Option<Type> {
        let mut signatures = Vec::with_capacity(members.len());
        for member in members {
            match member.peel() {
                Type::Function {
                    params,
                    ret,
                    has_rest: false,
                    ..
                } => signatures.push((params, ret)),
                _ => return None,
            }
        }
        let returns_void = signatures.first()?.1.is_void();
        if signatures
            .iter()
            .any(|(_, ret)| ret.is_void() != returns_void)
        {
            return None;
        }
        let arity = signatures.iter().map(|(params, _)| params.len()).max()?;
        let mut params = Vec::with_capacity(arity);
        let mut fields_left = MERGE_FIELDS;
        for position in 0..arity {
            let at_position: Vec<&Type> = signatures
                .iter()
                .filter_map(|(params, _)| params.get(position))
                .collect();
            params.push(self.common_subtype(&at_position, MERGE_DEPTH, &mut fields_left)?);
        }
        let ret = if returns_void {
            Type::Void
        } else {
            Type::union(
                signatures
                    .iter()
                    .map(|(_, ret)| ret.as_ref().clone())
                    .collect(),
            )
        };
        Some(Type::Function {
            params,
            ret: Box::new(ret),
            predicate: None,
            has_rest: false,
        })
    }

    /// A type assignable to each of `types`: the one of them assignable to
    /// all the others, or, for object types and interfaces, the object with
    /// every field of each, as tsc's intersection of them has. Merging gives
    /// up once it is `depth_left` objects deep, so types that refer back to
    /// themselves don't merge forever, or once it has merged `fields_left`
    /// more fields.
    fn common_subtype(
        &self,
        types: &[&Type],
        depth_left: usize,
        fields_left: &mut usize,
    ) -> Option<Type> {
        let narrowest = types.iter().find(|candidate| {
            types
                .iter()
                .all(|other| assignable(candidate, other, self.resolver()))
        });
        if let Some(narrowest) = narrowest {
            return Some((*narrowest).clone());
        }
        let depth_left = depth_left.checked_sub(1)?;
        let mut merged: BTreeMap<String, Vec<crate::ObjectField>> = BTreeMap::new();
        for ty in types {
            if matches!(ty.peel(), Type::Union(_)) {
                return None;
            }
            let shape = self.sole_object_shape(ty)?;
            for (name, field) in shape {
                merged.entry(name).or_default().push(field);
            }
        }
        let mut fields = BTreeMap::new();
        for (name, declared) in merged {
            *fields_left = fields_left.checked_sub(1)?;
            let field_types: Vec<&Type> = declared.iter().map(|field| &field.ty).collect();
            let ty = self.common_subtype(&field_types, depth_left, fields_left)?;
            fields.insert(
                name,
                crate::ObjectField {
                    ty,
                    optional: declared.iter().all(|field| field.optional),
                    readonly: declared.iter().all(|field| field.readonly),
                    method: false,
                },
            );
        }
        let combined = Type::Object {
            fields,
            index: None,
        };
        types
            .iter()
            .all(|ty| assignable(&combined, ty, self.resolver()))
            .then_some(combined)
    }
}
