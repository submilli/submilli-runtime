//! Calling a value whose type is a union of function types. tsc calls it
//! through one combined signature: each parameter takes a type every member's
//! parameter there accepts (their intersection), and the result is the union
//! of the members' results.

use std::collections::BTreeMap;

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Type, TypedExpr, TypedExprKind};

use super::Inferer;
use super::assignable::{assignable, strict_subtype_union};

/// How many objects deep the fields of a combined parameter type merge.
const MERGE_DEPTH: usize = 8;

/// How many fields, in all, the parameter types of one union may merge:
/// types that share structure would otherwise merge into an object that
/// grows exponentially with their depth.
const MERGE_FIELDS: usize = 256;

impl Inferer<'_> {
    /// The callee `typed_callee`, of type `callee_ty`, cast up to the signature
    /// a call passing `arg_count` arguments reaches it through, when it is a
    /// union of function types that combine; otherwise as it is.
    pub(super) fn cast_up_to_union_signature(
        &mut self,
        typed_callee: ExprId,
        callee_ty: Type,
        arg_count: usize,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        let Some(signature) = self.union_signature_of(&callee_ty, Some(arg_count)) else {
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

    /// The signature a call passing `arg_count` arguments (any number, when
    /// `None`) reaches a callee of type `ty` through, when `ty` is a union of
    /// function types that combine.
    pub(super) fn union_signature_of(&self, ty: &Type, arg_count: Option<usize>) -> Option<Type> {
        match ty.peel() {
            Type::Union(members) => self.union_call_signature(members, arg_count),
            _ => None,
        }
    }

    /// The function type a union of function types `members` is called
    /// through by a call passing `arg_count` arguments, or `None` when they
    /// don't combine: a member that is not a function or has a rest parameter
    /// that isn't an array, members that differ in whether they return a
    /// value, or a position whose parameter types have no common subtype
    /// Submilli can name.
    ///
    /// As in tsc, a member whose parameters every other member accepts, and
    /// that requires as many arguments as any of them, is a signature of the
    /// union; the call goes through the first that takes `arg_count`
    /// arguments, or the first. When no member is one, the members combine
    /// pairwise: each position takes a type every member's parameter there
    /// accepts, a member with fewer parameters ignores the arguments past
    /// them, and a rest parameter covers every later position.
    pub(super) fn union_call_signature(
        &self,
        members: &[Type],
        arg_count: Option<usize>,
    ) -> Option<Type> {
        let signatures = members
            .iter()
            .map(Signature::of)
            .collect::<Option<Vec<_>>>()?;
        let returns_void = signatures.first()?.ret.is_void();
        if signatures
            .iter()
            .any(|sig| sig.ret.is_void() != returns_void)
        {
            return None;
        }
        let shared = self.shared_signatures(&signatures);
        let chosen = match shared.first() {
            Some(first) => (*arg_count
                .and_then(|count| shared.iter().find(|sig| sig.accepts(count)))
                .unwrap_or(first))
            .clone(),
            None => {
                let mut fields_left = MERGE_FIELDS;
                let mut combined = signatures.first()?.clone();
                for next in &signatures[1..] {
                    combined = self.combine(&combined, next, &mut fields_left)?;
                }
                combined
            }
        };
        let ret = if returns_void {
            Type::Void
        } else {
            strict_subtype_union(
                signatures.iter().map(|sig| sig.ret.clone()).collect(),
                self.resolver(),
            )
        };
        // A closure takes its rest arguments packed after its own fixed
        // parameters, so members whose fixed parameters differ are called
        // through a fixed signature the call's arguments fill.
        let packs_alike = signatures
            .iter()
            .all(|sig| sig.rest && sig.params.len() == chosen.params.len());
        let chosen = match arg_count {
            Some(count) if chosen.rest && !packs_alike => chosen.spread_rest(count),
            _ => chosen,
        };
        Some(chosen.into_function(ret))
    }

    /// The members of a union every other member's signature accepts the
    /// arguments of: each other requires no more arguments and accepts each
    /// parameter type it declares.
    fn shared_signatures<'s>(&self, signatures: &'s [Signature]) -> Vec<&'s Signature> {
        signatures
            .iter()
            .enumerate()
            .filter(|(index, candidate)| {
                signatures.iter().enumerate().all(|(other_index, other)| {
                    other_index == *index
                        || (other.required() <= candidate.required()
                            && (0..candidate.params.len()).all(|position| {
                                match (candidate.type_at(position), other.type_at(position)) {
                                    (Some(param), Some(accepted)) => {
                                        assignable(param, accepted, self.resolver())
                                    }
                                    (_, None) => true,
                                    (None, Some(_)) => false,
                                }
                            }))
                })
            })
            .map(|(_, candidate)| candidate)
            .collect()
    }

    /// One signature that calls both `left` and `right`, as tsc combines the
    /// members of a union neither of which accepts the other's arguments.
    fn combine(
        &self,
        left: &Signature,
        right: &Signature,
        fields_left: &mut usize,
    ) -> Option<Signature> {
        let (longest, shorter) = if left.params.len() >= right.params.len() {
            (left, right)
        } else {
            (right, left)
        };
        let either_rest = left.rest || right.rest;
        let extra_rest = either_rest && !longest.rest;
        let mut params = Vec::with_capacity(longest.params.len() + usize::from(extra_rest));
        for position in 0..longest.params.len() {
            let at_position: Vec<&Type> = [longest.type_at(position), shorter.type_at(position)]
                .into_iter()
                .flatten()
                .collect();
            let param = self.common_subtype(&at_position, MERGE_DEPTH, fields_left)?;
            let is_rest = either_rest && !extra_rest && position + 1 == longest.params.len();
            params.push(if is_rest {
                Type::Array(Box::new(param))
            } else {
                param
            });
        }
        if extra_rest {
            params.push(Type::Array(Box::new(
                shorter.type_at(longest.params.len())?.clone(),
            )));
        }
        let fixed = params.len() - usize::from(either_rest);
        let required = left.required().max(right.required());
        Some(Signature {
            optional: fixed.saturating_sub(required),
            params,
            rest: either_rest,
            ret: Type::Void,
        })
    }

    /// A type assignable to each of `types`, as tsc's intersection of them
    /// is: the one of them assignable to all the others; for unions, the
    /// members each of them holds; for functions, one taking each parameter
    /// any of them takes; for object types and interfaces, the object with
    /// every field of each. Merging gives up once it is `depth_left` objects
    /// deep, so types that refer back to themselves don't merge forever, or
    /// once it has merged `fields_left` more fields.
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
        if types.iter().any(|ty| matches!(ty.peel(), Type::Union(_))) {
            return self.common_members(types);
        }
        let depth_left = depth_left.checked_sub(1)?;
        if types
            .iter()
            .all(|ty| matches!(ty.peel(), Type::Function { .. }))
        {
            return self.common_function_subtype(types, depth_left, fields_left);
        }
        let mut merged: BTreeMap<String, Vec<crate::ObjectField>> = BTreeMap::new();
        for ty in types {
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

    /// The members of `types`' unions that every one of `types` holds:
    /// `"a" | "b"` and `"b" | "c"` share `"b"`. `None` when they share none.
    fn common_members(&self, types: &[&Type]) -> Option<Type> {
        let mut shared: Vec<Type> = Vec::new();
        for ty in types {
            let members = match ty.peel() {
                Type::Union(members) => members.as_slice(),
                _ => std::slice::from_ref(*ty),
            };
            for member in members {
                if !shared.contains(member)
                    && types
                        .iter()
                        .all(|other| assignable(member, other, self.resolver()))
                {
                    shared.push(member.clone());
                }
            }
        }
        (!shared.is_empty()).then(|| Type::union(shared))
    }

    /// A function each of the function types `types` stands for a caller of:
    /// it takes the parameters they all declare, each at the union of their
    /// types there, and returns what each of theirs accepts.
    fn common_function_subtype(
        &self,
        types: &[&Type],
        depth_left: usize,
        fields_left: &mut usize,
    ) -> Option<Type> {
        let mut signatures = Vec::with_capacity(types.len());
        for ty in types {
            match ty.peel() {
                Type::Function {
                    params,
                    ret,
                    predicate: None,
                    has_rest: false,
                    ..
                } => signatures.push((params, ret)),
                _ => return None,
            }
        }
        let arity = signatures.iter().map(|(params, _)| params.len()).min()?;
        let params = (0..arity)
            .map(|position| {
                Type::union(
                    signatures
                        .iter()
                        .map(|(params, _)| params[position].clone())
                        .collect(),
                )
            })
            .collect();
        let returns: Vec<&Type> = signatures.iter().map(|(_, ret)| ret.as_ref()).collect();
        let ret = if returns.iter().all(|ret| ret.is_void()) {
            Type::Void
        } else {
            self.common_subtype(&returns, depth_left, fields_left)?
        };
        let combined = Type::Function {
            params,
            optional: 0,
            ret: Box::new(ret),
            predicate: None,
            has_rest: false,
        };
        types
            .iter()
            .all(|ty| assignable(&combined, ty, self.resolver()))
            .then_some(combined)
    }
}

/// A union member's call signature.
#[derive(Clone)]
struct Signature {
    params: Vec<Type>,
    /// Trailing fixed parameters a caller may omit.
    optional: usize,
    /// Whether the last of `params` is a rest parameter's array.
    rest: bool,
    ret: Type,
}

impl Signature {
    fn of(ty: &Type) -> Option<Self> {
        let Type::Function {
            params,
            optional,
            ret,
            has_rest,
            ..
        } = ty.peel()
        else {
            return None;
        };
        if *has_rest && params.last()?.rest_element().is_none() {
            return None;
        }
        Some(Self {
            params: params.clone(),
            optional: *optional,
            rest: *has_rest,
            ret: ret.as_ref().clone(),
        })
    }

    fn fixed(&self) -> usize {
        self.params.len() - usize::from(self.rest)
    }

    fn required(&self) -> usize {
        self.fixed().saturating_sub(self.optional)
    }

    fn accepts(&self, arg_count: usize) -> bool {
        arg_count >= self.required() && (self.rest || arg_count <= self.params.len())
    }

    /// The type an argument at `position` must have: its parameter's, or the
    /// rest element's past the fixed parameters. `None` past the end of a
    /// signature with no rest parameter, where an argument is ignored.
    fn type_at(&self, position: usize) -> Option<&Type> {
        if self.rest && position >= self.fixed() {
            return self.params.last()?.rest_element();
        }
        self.params.get(position)
    }

    /// This signature with its rest parameter spread into fixed parameters for
    /// a call passing `arg_count` arguments, so a member that has no rest
    /// parameter can stand for it.
    fn spread_rest(self, arg_count: usize) -> Self {
        let Some(element) = self.type_at(self.fixed()).cloned() else {
            return self;
        };
        let required = self.required();
        let mut params = self.params;
        params.pop();
        while params.len() < arg_count {
            params.push(element.clone());
        }
        Self {
            optional: params.len().saturating_sub(required),
            params,
            rest: false,
            ret: self.ret,
        }
    }

    fn into_function(self, ret: Type) -> Type {
        Type::Function {
            params: self.params,
            optional: self.optional,
            ret: Box::new(ret),
            predicate: None,
            has_rest: self.rest,
        }
    }
}
