//! The element type of an array literal whose elements all fit one another's
//! types, the way tsc picks it: the union of the elements' types with each type
//! that is a subtype of another left out. `[new Dog(), new Animal()]` holds
//! `Animal`, while `[a, b]` with `a: { x: number; y?: number }` and
//! `b: { x: number; z?: number }` holds both types, since neither has the
//! other's optional field.

use crate::TypedArrayElement;
use crate::compiler_error::CompilerFailure;
use crate::types::Type;

use super::Inferer;
use super::assignable::assignable;

/// One distinct type an array literal's elements hold.
struct Candidate {
    ty: Type,
    /// Whether an object literal wrote it, which lists every field it holds,
    /// so it has no field it doesn't name: tsc relates it to an optional field
    /// it lacks.
    fresh: bool,
}

impl Inferer<'_> {
    /// The element type of an array literal whose elements joined as `joined`:
    /// the elements' types reduced to those no other is a supertype of, or
    /// `joined` itself when that is no different or an element's type can't be
    /// read. Literal types are settled elsewhere, so a union of primitives
    /// keeps `joined`. `object_literals` says which elements are object
    /// literals.
    pub(super) fn best_common_element_type(
        &self,
        joined: Type,
        elements: &[TypedArrayElement],
        object_literals: &[bool],
    ) -> Result<Type, CompilerFailure> {
        if super::literal_freshness::is_primitive_union(&joined) {
            return Ok(joined);
        }
        let Some(candidates) = self.element_candidates(elements, object_literals)? else {
            return Ok(joined);
        };
        let kept = self.without_subtypes(&candidates);
        if kept.len() < 2 {
            return Ok(kept.into_iter().next().unwrap_or(joined));
        }
        // A union of function types Submilli can't call through one combined
        // signature (members differing in whether they return a value) keeps
        // the one type the functions all fit.
        if kept
            .iter()
            .any(|ty| matches!(ty.peel(), Type::Function { .. }))
            && self.union_call_signature(&kept).is_none()
        {
            return Ok(joined);
        }
        let union = Type::union(kept);
        let fits = candidates
            .iter()
            .all(|candidate| assignable(&candidate.ty, &union, self.resolver()));
        Ok(if fits { union } else { joined })
    }

    /// The distinct types the elements hold, in source order, with a union's
    /// members counted one by one. `None` when one is an error.
    fn element_candidates(
        &self,
        elements: &[TypedArrayElement],
        object_literals: &[bool],
    ) -> Result<Option<Vec<Candidate>>, CompilerFailure> {
        let mut candidates: Vec<Candidate> = Vec::new();
        for (element, &object_literal) in elements.iter().zip(object_literals) {
            let typed = self
                .typed_ast
                .try_expr(element.expr_id())
                .map_err(crate::typechecker::arena_failure)?;
            let (ty, fresh) = match element {
                TypedArrayElement::Value(_) => (typed.ty.widen_literal(), object_literal),
                TypedArrayElement::Spread(_) => {
                    match super::expr::spread_element_type(typed.ty.peel()) {
                        Some(ty) => (ty, false),
                        None => return Ok(None),
                    }
                }
            };
            let members = match ty.peel() {
                Type::Union(members) => members.clone(),
                _ => vec![ty],
            };
            for member in members {
                match member.peel() {
                    Type::Never => continue,
                    Type::Error => return Ok(None),
                    _ => {}
                }
                match candidates.iter_mut().find(|known| known.ty == member) {
                    Some(known) => known.fresh &= fresh,
                    None => candidates.push(Candidate { ty: member, fresh }),
                }
            }
        }
        Ok(Some(candidates))
    }

    /// `candidates` without each one that is a subtype of another kept one.
    /// Two types that are each other's subtypes keep the one with more fields,
    /// or the earlier one, as tsc's strict subtype relation does.
    fn without_subtypes(&self, candidates: &[Candidate]) -> Vec<Type> {
        let mut kept: Vec<bool> = vec![true; candidates.len()];
        for (index, candidate) in candidates.iter().enumerate() {
            let absorbed = candidates.iter().enumerate().any(|(other_index, other)| {
                other_index != index
                    && kept[other_index]
                    && self.is_subtype(candidate, other)
                    && (!self.is_subtype(other, candidate)
                        || wins_tie(other, other_index, candidate, index))
            });
            if absorbed {
                kept[index] = false;
            }
        }
        candidates
            .iter()
            .zip(kept)
            .filter(|(_, kept)| *kept)
            .map(|(candidate, _)| candidate.ty.clone())
            .collect()
    }

    /// tsc's subtype relation as far as it differs from assignability here: a
    /// type that isn't a fresh object literal must have each optional field
    /// of the other, at any depth.
    fn is_subtype(&self, source: &Candidate, target: &Candidate) -> bool {
        if !assignable(&source.ty, &target.ty, self.resolver()) {
            return false;
        }
        source.fresh
            || !lacks_optional_field(
                &self.reduce_interfaces_to_shapes(&source.ty),
                &self.reduce_interfaces_to_shapes(&target.ty),
            )
    }
}

/// Whether `challenger`, at `challenger_index`, stays over `incumbent` when
/// each is the other's subtype: the one with more fields, or else the first.
fn wins_tie(
    challenger: &Candidate,
    challenger_index: usize,
    incumbent: &Candidate,
    incumbent_index: usize,
) -> bool {
    let fields = |candidate: &Candidate| match candidate.ty.peel() {
        Type::Object { fields, .. } => fields.len(),
        _ => 0,
    };
    match fields(challenger).cmp(&fields(incumbent)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => challenger_index < incumbent_index,
    }
}

/// Whether some object `target` reaches has an optional field the object
/// `source` reaches in the same place lacks. Function parameters are compared
/// the other way round.
fn lacks_optional_field(source: &Type, target: &Type) -> bool {
    match (source.peel(), target.peel()) {
        (
            Type::Object {
                fields: source_fields,
                ..
            },
            Type::Object {
                fields: target_fields,
                ..
            },
        ) => target_fields
            .iter()
            .any(|(name, target_field)| match source_fields.get(name) {
                None => target_field.optional,
                Some(source_field) => lacks_optional_field(&source_field.ty, &target_field.ty),
            }),
        (
            Type::Function {
                params: source_params,
                ret: source_ret,
                ..
            },
            Type::Function {
                params: target_params,
                ret: target_ret,
                ..
            },
        ) => {
            source_params
                .iter()
                .zip(target_params)
                .any(|(source_param, target_param)| {
                    lacks_optional_field(target_param, source_param)
                })
                || lacks_optional_field(source_ret, target_ret)
        }
        (Type::Array(source_element), Type::Array(target_element)) => {
            lacks_optional_field(source_element, target_element)
        }
        _ => false,
    }
}
