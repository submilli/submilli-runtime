//! Generic type-parameter substitution and structural unification.

use std::collections::BTreeMap;

use crate::Type;
use crate::typechecker::infer::assignable::{TypeResolver, assignable, expand_alias_ref};
use crate::typechecker::infer::type_aliases::rehydrate_alias_refs;

/// BTreeMap for deterministic ordering (stable snapshots and error messages).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct TypeParamSubstitution {
    bindings: BTreeMap<String, Type>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnifyError {
    Mismatch {
        expected: Type,
        got: Type,
    },
    /// `T` bound to different types from different arg positions.
    Conflict {
        name: String,
        prev: Type,
        new: Type,
    },
}

impl TypeParamSubstitution {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mismatched lengths silently truncate to the shorter — caller validates arity.
    pub fn from_pairs(generics: &[String], type_args: &[Type]) -> Self {
        Self {
            bindings: generics
                .iter()
                .zip(type_args.iter())
                .map(|(name, ty)| (name.clone(), ty.clone()))
                .collect(),
        }
    }

    /// The table member resolution already computed, adopted as-is. Contrast
    /// [`from_pairs`](Self::from_pairs), which zips a declaration against a
    /// receiver and is correct only when that declaration is the one the member
    /// was found on.
    pub fn from_bindings(bindings: BTreeMap<String, Type>) -> Self {
        Self { bindings }
    }

    pub fn insert(&mut self, name: String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    pub fn get(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Replace `TypeVar`s in `ty` with bound types; chases var-to-var chains.
    pub fn apply(&self, ty: &Type) -> Type {
        self.apply_rec(ty, &mut Vec::new())
    }

    /// [`apply`](Self::apply), tracking which variables' bindings are open on
    /// the current path in `substituting`.
    ///
    /// A binding is allowed to mention the very variable it binds — a recursive
    /// alias's back-edge rehydrated to its inline form carries the alias's own
    /// parameter inside its body — and chasing that mention would re-enter the
    /// same binding forever. Re-entry yields the variable itself, the finite
    /// spelling of that fixpoint. The degenerate case is a `T → T` self-binding
    /// from two nested generics sharing a name, which re-enters at depth zero.
    fn apply_rec(&self, ty: &Type, substituting: &mut Vec<String>) -> Type {
        match ty {
            Type::Refined { original, ty } => Type::Refined {
                original: Box::new(self.apply_rec(original, substituting)),
                ty: Box::new(self.apply_rec(ty, substituting)),
            },
            Type::TypeVar(name) => match self.bindings.get(name) {
                Some(bound) => {
                    if substituting.iter().any(|open| open == name) {
                        return ty.clone();
                    }
                    substituting.push(name.clone());
                    let applied = self.apply_rec(bound, substituting);
                    substituting.pop();
                    applied
                }
                None => ty.clone(),
            },
            Type::Array(elem) => Type::Array(Box::new(self.apply_rec(elem, substituting))),
            Type::Readonly(inner) => Type::Readonly(Box::new(self.apply_rec(inner, substituting))),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|e| self.apply_rec(e, substituting))
                    .collect(),
            ),
            Type::Function {
                params,
                ret,
                predicate,
                has_rest,
            } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.apply_rec(p, substituting))
                    .collect(),
                ret: Box::new(self.apply_rec(ret, substituting)),
                has_rest: *has_rest,
                // substitute so `x is T` predicates resolve T at the call site.
                predicate: predicate.as_ref().map(|p| {
                    Box::new(crate::TypePredicate {
                        parameter_index: p.parameter_index,
                        asserted_type: self.apply_rec(&p.asserted_type, substituting),
                    })
                }),
            },
            Type::Object { fields, index } => Type::Object {
                index: index
                    .as_ref()
                    .map(|i| i.map_value(|v| self.apply_rec(v, substituting))),
                fields: fields
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            crate::ObjectField {
                                ty: self.apply_rec(&v.ty, substituting),
                                optional: v.optional,
                                readonly: v.readonly,
                            },
                        )
                    })
                    .collect(),
            },
            Type::InterfaceRef {
                mangled,
                package,
                name,
                args,
            } => Type::interface_ref(
                package.clone(),
                name.clone(),
                mangled.clone(),
                args.iter()
                    .map(|a| self.apply_rec(a, substituting))
                    .collect(),
            ),
            Type::ClassRef {
                mangled,
                package,
                name,
                args,
            } => Type::class_ref(
                package.clone(),
                name.clone(),
                mangled.clone(),
                args.iter()
                    .map(|a| self.apply_rec(a, substituting))
                    .collect(),
            ),
            // Use `Type::union` so collapsing substitutions preserve canonical form.
            Type::Union(members) => Type::union(
                members
                    .iter()
                    .map(|m| self.apply_rec(m, substituting))
                    .collect(),
            ),
            // Alias label preserved; substitute body and args so `Box<T>` resolves T.
            Type::Alias {
                mangled,
                package,
                name,
                args,
                ty: inner,
            } => Type::alias_ty(
                package.clone(),
                name.clone(),
                mangled.clone(),
                args.iter()
                    .map(|a| self.apply_rec(a, substituting))
                    .collect(),
                Box::new(self.apply_rec(inner, substituting)),
            ),
            // Recursion back-edge: no inline body, substitute args only.
            Type::AliasRef {
                mangled,
                package,
                name,
                args,
            } => Type::alias_ref(
                package.clone(),
                name.clone(),
                mangled.clone(),
                args.iter()
                    .map(|a| self.apply_rec(a, substituting))
                    .collect(),
            ),
            // GenericParam is opaque here — body-form, not substituted.
            Type::Number
            | Type::BigInt
            | Type::NumberLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Void
            | Type::Never
            | Type::Unknown
            | Type::Error
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
            | Type::GenericParam { .. } => ty.clone(),
        }
    }

    /// Structural unification of `param_ty` against `arg_ty`, binding `TypeVar`s
    /// on the param side.
    ///
    /// Takes the type registry because a recursion back-edge
    /// ([`Type::AliasRef`]) is a name, not a body: without somewhere to resolve
    /// it, a recursive alias and its own expansion compare as two different
    /// types.
    #[allow(clippy::result_large_err)]
    pub(crate) fn unify(
        &mut self,
        param_ty: &Type,
        arg_ty: &Type,
        types: TypeResolver<'_>,
    ) -> Result<(), UnifyError> {
        Unifier::new(self, Some(types)).unify(param_ty, arg_ty)
    }

    /// [`unify`](Self::unify) for a **call-argument** position.
    ///
    /// Differs in one place: when a type parameter is already bound and the
    /// argument doesn't unify with the binding, an argument at a covariant
    /// position is accepted if it is *assignable* to the binding. Passing a
    /// `Sub` where the receiver fixed `T = Base` is the assignability
    /// question, not an identity one — `Map<Base, V>#set`, `Array<Base>#push`
    /// and `Set<Base>#add` all read as invariant without it. The binding is
    /// left at `Base`, so the call's result type is unchanged.
    #[allow(clippy::result_large_err)]
    pub(crate) fn unify_argument(
        &mut self,
        param_ty: &Type,
        arg_ty: &Type,
        types: TypeResolver<'_>,
    ) -> Result<(), UnifyError> {
        let resolved = self.apply(param_ty);
        if !super::infer::expr::type_contains_type_var(&resolved)
            && assignable(arg_ty, &resolved, types)
        {
            return Ok(());
        }
        let mut unifier = Unifier::new(self, Some(types));
        unifier.subtype_widening = true;
        unifier.unify(param_ty, arg_ty)
    }
}

/// The mutable state of one `unify` call: the bindings being built, the
/// registry its [`Type::AliasRef`] arm expands against, and the coinductive
/// assumption set that stops two recursive aliases expanding forever.
struct Unifier<'a> {
    sub: &'a mut TypeParamSubstitution,
    types: Option<TypeResolver<'a>>,
    assumed_pairs: Vec<(Type, Type)>,
    /// Whether an argument assignable to an already-bound type parameter is
    /// accepted rather than reported as a conflict. Set by
    /// [`unify_argument`](TypeParamSubstitution::unify_argument), and cleared
    /// while descending into a function *parameter*, which is contravariant.
    subtype_widening: bool,
}

impl<'a> Unifier<'a> {
    /// Structural unification of `param_ty` against `arg_ty`. Binds `TypeVar`s on the
    /// param side; `Type::Error` on either side silently succeeds (upstream already reported).
    #[allow(clippy::result_large_err)]
    fn unify(&mut self, param_ty: &Type, arg_ty: &Type) -> Result<(), UnifyError> {
        // peel aliases — unification is structural, alias label doesn't participate.
        // A type variable still binds to a readonly argument as readonly, or the
        // call's result would hand back a writable view of it.
        let bindable_arg = arg_ty.peel_preserving_readonly();
        let param_ty = param_ty.peel();
        let arg_ty = arg_ty.peel();
        if matches!(param_ty, Type::Error) || matches!(arg_ty, Type::Error) {
            return Ok(());
        }

        if let Some(types) = self.alias_ref_expansion(param_ty, arg_ty) {
            return self.unify_expanded(param_ty, arg_ty, types);
        }

        if let Type::TypeVar(name) = param_ty {
            // What a parameter binds to is what the call's result type is built
            // from, and codegen lowers a bare back-edge to the universal
            // `$Object` where the inline `Alias` form gives the narrower
            // `$ObjectShape` — bind the inline form so the two agree.
            let arg_ty = &self.inline_alias_refs(bindable_arg);
            if let Some(existing) = self.sub.bindings.get(name).cloned() {
                let resolved = self.sub.apply(&existing);
                // A `T → T` self-binding (two nested generics sharing a name, e.g. the
                // unresolved vars of `new Map()` flowing into `.set`'s receiver) is
                // effectively unbound: recursing here would unify `T` against the arg
                // forever. Treat it as a fresh binding instead — same hazard the `apply`
                // guard above handles.
                if matches!(&resolved, Type::TypeVar(other) if other == name) {
                    self.sub.bindings.insert(name.clone(), arg_ty.clone());
                    return Ok(());
                }
                let arg_resolved = self.sub.apply(arg_ty);
                // Recurse instead of `==` to peel aliases at every level; remap to Conflict to pin the offending param.
                return match self.unify(&resolved, &arg_resolved) {
                    Ok(()) => Ok(()),
                    Err(_) if self.accepts_as_subtype(&arg_resolved, &resolved) => Ok(()),
                    Err(_) => Err(UnifyError::Conflict {
                        name: name.clone(),
                        prev: resolved,
                        new: arg_resolved,
                    }),
                };
            }
            self.sub.bindings.insert(name.clone(), arg_ty.clone());
            return Ok(());
        }

        match (param_ty, arg_ty) {
            (Type::Array(a), Type::Array(b)) => self.unify(a, b),
            // A tuple *is* an array at runtime and routes to `Array` for member dispatch
            // (`Type::interface_routing`), so it infers `T[]`'s element as the union of
            // its positions — the same element type that routing gives it. Without this,
            // `Array.from(pair)` and `xs.concat(pair)` reject a value `const a: number[] =
            // pair` accepts.
            (Type::Array(a), Type::Tuple(elems)) => self.unify(a, &Type::union(elems.clone())),
            (Type::Tuple(aa), Type::Tuple(ab)) => {
                if aa.len() != ab.len() {
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                for (a, b) in aa.iter().zip(ab.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            }
            (
                Type::Function {
                    params: pa,
                    ret: ra,
                    has_rest: rest_a,
                    ..
                },
                Type::Function {
                    params: pb,
                    ret: rb,
                    has_rest: rest_b,
                    ..
                },
            ) => {
                if !Type::function_arity_fits(pb.len(), pa.len(), *rest_a || *rest_b) {
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                for (p, a) in pa.iter().zip(pb.iter()) {
                    self.without_subtype_widening(|u| u.unify(p, a))?;
                }
                self.unify(ra, rb)
            }
            (
                Type::Object {
                    fields: a,
                    index: ai,
                },
                Type::Object {
                    fields: b,
                    index: bi,
                },
            ) => {
                if let Some(index) = ai {
                    if let Some(actual) = bi {
                        self.unify(&index.value, &actual.value)?;
                    }
                    for field in b.values() {
                        self.unify(&index.value, &field.ty)?;
                    }
                    for (name, expected) in a {
                        let Some(actual) = b.get(name) else {
                            if expected.optional {
                                continue;
                            }
                            return Err(UnifyError::Mismatch {
                                expected: param_ty.clone(),
                                got: arg_ty.clone(),
                            });
                        };
                        self.unify(&expected.ty, &actual.ty)?;
                    }
                    return Ok(());
                }
                if a.len() != b.len() || !a.keys().eq(b.keys()) {
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                for (va, vb) in a.values().zip(b.values()) {
                    if va.optional != vb.optional {
                        return Err(UnifyError::Mismatch {
                            expected: param_ty.clone(),
                            got: arg_ty.clone(),
                        });
                    }
                    self.unify(&va.ty, &vb.ty)?;
                }
                Ok(())
            }
            (
                Type::InterfaceRef {
                    mangled: ma,
                    args: aa,
                    ..
                },
                Type::InterfaceRef {
                    mangled: mb,
                    args: ab,
                    ..
                },
            )
            | (
                Type::ClassRef {
                    mangled: ma,
                    args: aa,
                    ..
                },
                Type::ClassRef {
                    mangled: mb,
                    args: ab,
                    ..
                },
            ) => {
                if ma != mb || aa.len() != ab.len() {
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                for (a, b) in aa.iter().zip(ab.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            }
            // Two-pass union-vs-union: pair matching members first, then unify leftovers in order.
            (Type::Union(pa), Type::Union(pb)) => {
                if pa.len() != pb.len() {
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                let mut leftover_pa: Vec<&Type> = Vec::new();
                let mut leftover_pb: Vec<Type> = pb.to_vec();
                for a in pa {
                    let resolved_a = self.sub.apply(a);
                    // Snapshot and roll back on failure — speculative pairing.
                    let mut paired: Option<usize> = None;
                    for (i, b) in leftover_pb.iter().enumerate() {
                        let snap = self.snapshot();
                        let b_clone = b.clone();
                        if self.unify(&resolved_a, &b_clone).is_ok() {
                            paired = Some(i);
                            break;
                        }
                        self.restore(snap);
                    }
                    match paired {
                        Some(idx) => {
                            leftover_pb.remove(idx);
                        }
                        None => leftover_pa.push(a),
                    }
                }
                if leftover_pa.len() != leftover_pb.len() {
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                for (a, b) in leftover_pa.iter().zip(leftover_pb.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            }
            // Union param against a single (non-union) arg: try each member.
            (Type::Union(pa), _) => {
                // Bottom fits every arm. Give an unbound variable a chance to
                // infer from it before accepting an unrelated concrete arm.
                if matches!(arg_ty, Type::Never) && self.subtype_widening {
                    let snap = self.snapshot();
                    if self
                        .without_subtype_widening(|u| u.unify(param_ty, arg_ty))
                        .is_ok()
                    {
                        return Ok(());
                    }
                    self.restore(snap);
                }
                for m in pa {
                    let snap = self.snapshot();
                    if self.unify(m, arg_ty).is_ok() {
                        return Ok(());
                    }
                    self.restore(snap);
                }
                Err(UnifyError::Mismatch {
                    expected: param_ty.clone(),
                    got: arg_ty.clone(),
                })
            }
            // A param slot resolving to `unknown` accepts any argument — the top
            // type binds nothing. This is how an erased-args receiver
            // (`instanceof Map` narrows to `Map<unknown, unknown>`) accepts
            // `.set("k", v)`. The reverse stays a mismatch: an `unknown` arg
            // doesn't fit a concrete param.
            //
            // The widening is not limited to that case: a parameter already
            // bound to `unknown` now accepts anything, so `f(a: unknown[])`
            // called with a `number[]` typechecks and any write through it is
            // caught at run time instead of compile time. TypeScript accepts
            // the same program (its array covariance is unsound too), and the
            // failure is a trap rather than a bad read, so this matches the
            // language we are a subset of rather than tightening past it.
            (Type::Unknown, _) => Ok(()),
            _ => {
                if param_ty == arg_ty
                    || (matches!(arg_ty, Type::Never) && self.accepts_as_subtype(arg_ty, param_ty))
                {
                    Ok(())
                } else {
                    Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    })
                }
            }
        }
    }
    fn new(sub: &'a mut TypeParamSubstitution, types: Option<TypeResolver<'a>>) -> Self {
        Unifier {
            sub,
            types,
            assumed_pairs: Vec::new(),
            subtype_widening: false,
        }
    }

    /// Whether an argument that failed to unify with an already-bound type
    /// parameter is still acceptable, because it is assignable to the binding.
    fn accepts_as_subtype(&self, arg: &Type, bound: &Type) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        self.subtype_widening && assignable(arg, bound, types)
    }

    /// Run `f` with subtype-widening off — function parameters are
    /// contravariant, so widening stops here and does not resume deeper.
    ///
    /// Defensive rather than load-bearing today: a closure argument narrower
    /// than its parameter is reported by argument inference before unification
    /// is consulted, so no program currently reaches this with the flag set.
    /// It keeps the rule local to the walk that would otherwise apply it.
    #[allow(clippy::result_large_err)]
    fn without_subtype_widening(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<(), UnifyError>,
    ) -> Result<(), UnifyError> {
        let saved = self.subtype_widening;
        self.subtype_widening = false;
        let out = f(self);
        self.subtype_widening = saved;
        out
    }

    /// A speculative attempt's rollback point. The assumption set rolls back
    /// with the bindings: a pair assumed to hold inside an attempt that failed
    /// was never proved, and leaving it behind would let a later mismatch pass.
    fn snapshot(&self) -> (BTreeMap<String, Type>, usize) {
        (self.sub.bindings.clone(), self.assumed_pairs.len())
    }

    fn restore(&mut self, (bindings, assumed_len): (BTreeMap<String, Type>, usize)) {
        self.sub.bindings = bindings;
        self.assumed_pairs.truncate(assumed_len);
    }

    /// Rehydrate any recursion back-edge in `ty` to its inline form. A no-op
    /// without a registry to resolve the back-edge against.
    fn inline_alias_refs(&self, ty: &Type) -> Type {
        match self.types {
            Some(types) => rehydrate_alias_refs(ty, types),
            None => ty.clone(),
        }
    }

    /// The registry to expand with when one side is a recursion back-edge, and
    /// `None` otherwise. A back-edge is a *name*, so `peel` stops at it and the
    /// structural arms never see the body — left alone, the pair falls through
    /// to the nominal `param_ty == arg_ty` comparison, which reads a recursive
    /// alias and its own expansion as two different types.
    ///
    /// A `TypeVar` param is excluded: binding it to the back-edge is what the
    /// caller wants, and the expansion happens when that binding is compared
    /// against the next argument.
    fn alias_ref_expansion(&self, param_ty: &Type, arg_ty: &Type) -> Option<TypeResolver<'a>> {
        let is_back_edge =
            matches!(param_ty, Type::AliasRef { .. }) || matches!(arg_ty, Type::AliasRef { .. });
        if !is_back_edge || matches!(param_ty, Type::TypeVar(_)) {
            return None;
        }
        self.types
    }

    /// Expand the back-edge(s) and retry, modelled on `assignable`'s coinductive
    /// arm: two back-edges to the same alias with unifiable args agree without
    /// expanding, and a pair already under proof is assumed to hold so the
    /// recursion terminates.
    #[allow(clippy::result_large_err)]
    fn unify_expanded(
        &mut self,
        param_ty: &Type,
        arg_ty: &Type,
        types: TypeResolver<'_>,
    ) -> Result<(), UnifyError> {
        if let (
            Type::AliasRef {
                mangled: mp,
                args: ap,
                ..
            },
            Type::AliasRef {
                mangled: ma,
                args: aa,
                ..
            },
        ) = (param_ty, arg_ty)
            && mp == ma
            && ap.len() == aa.len()
        {
            for (p, a) in ap.iter().zip(aa.iter()) {
                self.unify(p, a)?;
            }
            return Ok(());
        }
        let key = (param_ty.clone(), arg_ty.clone());
        if self.assumed_pairs.contains(&key) {
            return Ok(());
        }
        self.assumed_pairs.push(key);
        let expanded_param = expand_alias_ref(param_ty, types);
        let expanded_arg = expand_alias_ref(arg_ty, types);
        // `expand_alias_ref` returns its input when the name doesn't resolve;
        // retrying then would recurse on the same pair forever.
        if expanded_param == *param_ty && expanded_arg == *arg_ty {
            return Err(UnifyError::Mismatch {
                expected: param_ty.clone(),
                got: arg_ty.clone(),
            });
        }
        self.unify(&expanded_param, &expanded_arg)
    }
}

impl TypeParamSubstitution {
    /// Returns `Ok(resolved)` with fully applied types per `generics` entry, or `Err(unbound_names)`.
    /// A `T → TypeVar(T)` self-binding counts as unbound — no concrete type flowed in.
    pub fn resolve_all(&self, generics: &[String]) -> Result<Vec<Type>, Vec<String>> {
        let mut resolved = Vec::with_capacity(generics.len());
        let mut unbound = Vec::new();
        for name in generics {
            match self.bindings.get(name) {
                Some(t) => {
                    let r = self.apply(t);
                    if matches!(&r, Type::TypeVar(other) if other == name) {
                        unbound.push(name.clone());
                    } else {
                        resolved.push(r);
                    }
                }
                None => unbound.push(name.clone()),
            }
        }
        if unbound.is_empty() {
            Ok(resolved)
        } else {
            Err(unbound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unification with no type registry — recursion back-edges compare
    /// nominally. None of these cases involve a recursive alias; the resolved
    /// path is covered by the `recursive-types` fixtures.
    #[allow(clippy::result_large_err)]
    fn unify_bare(
        sub: &mut TypeParamSubstitution,
        param_ty: &Type,
        arg_ty: &Type,
    ) -> Result<(), UnifyError> {
        Unifier::new(sub, None).unify(param_ty, arg_ty)
    }
    use std::collections::BTreeMap;

    fn t(name: &str) -> Type {
        // TypeVar (signature-form); substitution keys are TypeVar, not GenericParam.
        Type::TypeVar(name.to_string())
    }

    fn obj(pairs: &[(&str, Type)]) -> Type {
        let mut fields = BTreeMap::new();
        for (k, v) in pairs {
            fields.insert((*k).to_string(), crate::ObjectField::required(v.clone()));
        }
        Type::Object {
            index: None,
            fields,
        }
    }

    #[test]
    fn new_substitution_has_no_bindings() {
        let s = TypeParamSubstitution::new();
        assert_eq!(s.get("T"), None);
    }

    #[test]
    fn from_pairs_seeds_bindings() {
        let s = TypeParamSubstitution::from_pairs(
            &["T".to_string(), "U".to_string()],
            &[Type::Number, Type::String],
        );
        assert_eq!(s.get("T"), Some(&Type::Number));
        assert_eq!(s.get("U"), Some(&Type::String));
    }

    #[test]
    fn insert_overrides_previous_binding() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), Type::Number);
        s.insert("T".to_string(), Type::String);
        assert_eq!(s.get("T"), Some(&Type::String));
    }

    #[test]
    fn apply_replaces_bound_var() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), Type::Number);
        assert_eq!(s.apply(&t("T")), Type::Number);
    }

    #[test]
    fn apply_passes_through_unbound_var() {
        let s = TypeParamSubstitution::new();
        assert_eq!(s.apply(&t("T")), t("T"));
    }

    #[test]
    fn apply_chases_var_to_var_chain() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), t("U"));
        s.insert("U".to_string(), Type::Number);
        assert_eq!(s.apply(&t("T")), Type::Number);
    }

    #[test]
    fn apply_recurses_into_array() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), Type::Number);
        let arr_t = Type::Array(Box::new(t("T")));
        assert_eq!(s.apply(&arr_t), Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn apply_recurses_into_function() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), Type::Number);
        s.insert("U".to_string(), Type::String);
        let f = Type::Function {
            params: vec![t("T")],
            ret: Box::new(t("U")),
            predicate: None,
            has_rest: false,
        };
        assert_eq!(
            s.apply(&f),
            Type::Function {
                params: vec![Type::Number],
                ret: Box::new(Type::String),
                predicate: None,
                has_rest: false,
            }
        );
    }

    #[test]
    fn apply_recurses_into_object_fields() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), Type::Number);
        let o = obj(&[("x", t("T")), ("y", Type::String)]);
        assert_eq!(
            s.apply(&o),
            obj(&[("x", Type::Number), ("y", Type::String)])
        );
    }

    #[test]
    fn unify_var_to_concrete_binds() {
        let mut s = TypeParamSubstitution::new();
        assert_eq!(unify_bare(&mut s, &t("T"), &Type::Number), Ok(()));
        assert_eq!(s.get("T"), Some(&Type::Number));
    }

    #[test]
    fn unify_var_to_var_binds_to_other_var() {
        let mut s = TypeParamSubstitution::new();
        assert_eq!(unify_bare(&mut s, &t("T"), &t("T_outer")), Ok(()));
        assert_eq!(s.get("T"), Some(&t("T_outer")));
    }

    #[test]
    fn unify_consistent_rebind_succeeds() {
        let mut s = TypeParamSubstitution::new();
        unify_bare(&mut s, &t("T"), &Type::Number).unwrap();
        assert_eq!(unify_bare(&mut s, &t("T"), &Type::Number), Ok(()));
    }

    #[test]
    fn unify_conflicting_rebind_fails() {
        let mut s = TypeParamSubstitution::new();
        unify_bare(&mut s, &t("T"), &Type::Number).unwrap();
        let err = unify_bare(&mut s, &t("T"), &Type::String).expect_err("conflict");
        assert_eq!(
            err,
            UnifyError::Conflict {
                name: "T".to_string(),
                prev: Type::Number,
                new: Type::String,
            }
        );
    }

    #[test]
    fn unify_self_bound_var_rebinds_without_looping() {
        // `T → T` self-binding must not recurse forever; it rebinds to the arg.
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), t("T"));
        assert_eq!(unify_bare(&mut s, &t("T"), &Type::Number), Ok(()));
        assert_eq!(s.get("T"), Some(&Type::Number));
    }

    #[test]
    fn unify_self_bound_var_inside_interface_ref() {
        // The `new Map().set(...)` shape: receiver carries self-bound K,V; unifying
        // the method return `Map<K,V>` against `Map<string,number>` binds them.
        let mut s = TypeParamSubstitution::new();
        s.insert("K".to_string(), t("K"));
        s.insert("V".to_string(), t("V"));
        let ret = Type::InterfaceRef {
            mangled: crate::mangle::prelude("Map"),
            package: crate::Package::prelude(),
            name: "Map".to_string(),
            args: vec![t("K"), t("V")],
        };
        let want = Type::InterfaceRef {
            mangled: crate::mangle::prelude("Map"),
            package: crate::Package::prelude(),
            name: "Map".to_string(),
            args: vec![Type::String, Type::Number],
        };
        unify_bare(&mut s, &ret, &want).unwrap();
        assert_eq!(s.get("K"), Some(&Type::String));
        assert_eq!(s.get("V"), Some(&Type::Number));
    }

    #[test]
    fn unify_var_against_var_then_concrete_chases_through() {
        let mut s = TypeParamSubstitution::new();
        unify_bare(&mut s, &t("T"), &t("U")).unwrap();
        s.insert("U".to_string(), Type::Number);
        assert_eq!(unify_bare(&mut s, &t("T"), &Type::Number), Ok(()));
    }

    #[test]
    fn unify_array_elements_recurses() {
        let mut s = TypeParamSubstitution::new();
        let param = Type::Array(Box::new(t("T")));
        let arg = Type::Array(Box::new(Type::Number));
        unify_bare(&mut s, &param, &arg).unwrap();
        assert_eq!(s.get("T"), Some(&Type::Number));
    }

    #[test]
    fn unify_function_recurses_per_param_and_ret() {
        let mut s = TypeParamSubstitution::new();
        let param = Type::Function {
            params: vec![t("T")],
            ret: Box::new(t("U")),
            predicate: None,
            has_rest: false,
        };
        let arg = Type::Function {
            params: vec![Type::Number],
            ret: Box::new(Type::String),
            predicate: None,
            has_rest: false,
        };
        unify_bare(&mut s, &param, &arg).unwrap();
        assert_eq!(s.get("T"), Some(&Type::Number));
        assert_eq!(s.get("U"), Some(&Type::String));
    }

    #[test]
    fn unify_function_arity_mismatch_fails() {
        let mut s = TypeParamSubstitution::new();
        let param = Type::Function {
            params: vec![t("T")],
            ret: Box::new(t("U")),
            predicate: None,
            has_rest: false,
        };
        let arg = Type::Function {
            params: vec![Type::Number, Type::String],
            ret: Box::new(Type::String),
            predicate: None,
            has_rest: false,
        };
        let err = unify_bare(&mut s, &param, &arg).expect_err("arity mismatch");
        assert!(matches!(err, UnifyError::Mismatch { .. }));
    }

    #[test]
    fn unify_function_with_fewer_params_binds_the_return() {
        let mut s = TypeParamSubstitution::new();
        let param = Type::Function {
            params: vec![t("T"), Type::Number],
            ret: Box::new(t("U")),
            predicate: None,
            has_rest: false,
        };
        let arg = Type::Function {
            params: vec![Type::String],
            ret: Box::new(Type::Boolean),
            predicate: None,
            has_rest: false,
        };
        unify_bare(&mut s, &param, &arg).unwrap();
        assert_eq!(s.get("T"), Some(&Type::String));
        assert_eq!(s.get("U"), Some(&Type::Boolean));
    }

    #[test]
    fn unify_object_fields_recurses() {
        let mut s = TypeParamSubstitution::new();
        let param = obj(&[("x", t("T"))]);
        let arg = obj(&[("x", Type::Number)]);
        unify_bare(&mut s, &param, &arg).unwrap();
        assert_eq!(s.get("T"), Some(&Type::Number));
    }

    #[test]
    fn unify_object_field_set_mismatch_fails() {
        let mut s = TypeParamSubstitution::new();
        let param = obj(&[("x", t("T"))]);
        let arg = obj(&[("y", Type::Number)]);
        assert!(matches!(
            unify_bare(&mut s, &param, &arg),
            Err(UnifyError::Mismatch { .. })
        ));
    }

    #[test]
    fn unify_concrete_eq_succeeds() {
        let mut s = TypeParamSubstitution::new();
        assert_eq!(unify_bare(&mut s, &Type::Number, &Type::Number), Ok(()));
    }

    #[test]
    fn unify_concrete_mismatch_fails() {
        let mut s = TypeParamSubstitution::new();
        let err = unify_bare(&mut s, &Type::Number, &Type::String).expect_err("mismatch");
        assert_eq!(
            err,
            UnifyError::Mismatch {
                expected: Type::Number,
                got: Type::String,
            }
        );
    }

    #[test]
    fn unify_with_error_on_either_side_succeeds_silently() {
        let mut s = TypeParamSubstitution::new();
        assert_eq!(unify_bare(&mut s, &Type::Error, &Type::Number), Ok(()));
        assert_eq!(unify_bare(&mut s, &Type::Number, &Type::Error), Ok(()));
        assert_eq!(unify_bare(&mut s, &t("T"), &Type::Error), Ok(()));
        assert_eq!(s.get("T"), None);
    }

    #[test]
    fn resolve_all_returns_bindings_in_generics_order() {
        let mut s = TypeParamSubstitution::new();
        s.insert("U".to_string(), Type::String);
        s.insert("T".to_string(), Type::Number);
        let resolved = s
            .resolve_all(&["T".to_string(), "U".to_string()])
            .expect("all bound");
        assert_eq!(resolved, vec![Type::Number, Type::String]);
    }

    #[test]
    fn resolve_all_returns_unbound_names() {
        let s = TypeParamSubstitution::new();
        let unbound = s
            .resolve_all(&["T".to_string(), "U".to_string()])
            .expect_err("none bound");
        assert_eq!(unbound, vec!["T".to_string(), "U".to_string()]);
    }

    #[test]
    fn resolve_all_chases_var_to_var() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), t("U"));
        s.insert("U".to_string(), Type::Number);
        let resolved = s
            .resolve_all(&["T".to_string()])
            .expect("T resolves through U");
        assert_eq!(resolved, vec![Type::Number]);
    }
}
