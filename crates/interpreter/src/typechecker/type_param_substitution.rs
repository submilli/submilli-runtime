//! Generic type-parameter substitution and structural unification.

use std::collections::BTreeMap;

use crate::Type;
use crate::type_size::{TypeBudget, TypeLimits, TypeTooLarge, map_children};
use crate::typechecker::infer::assignable::{
    TypeResolver, assignable, expand_alias_ref, expand_interface_data_shape,
};
use crate::typechecker::infer::type_aliases::rehydrate_alias_refs;

/// BTreeMap for deterministic ordering (stable snapshots and error messages).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct TypeParamSubstitution {
    bindings: BTreeMap<String, Type>,
    /// Bindings taken from the type a call's result is expected to have, which
    /// an argument may still replace: as in tsc, what the arguments say comes
    /// first. One stays replaceable until an argument agrees with it.
    from_expected_result: std::collections::BTreeSet<String>,
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
            from_expected_result: Default::default(),
        }
    }

    /// The table member resolution already computed, adopted as-is. Contrast
    /// [`from_pairs`](Self::from_pairs), which zips a declaration against a
    /// receiver and is correct only when that declaration is the one the member
    /// was found on.
    pub fn from_bindings(bindings: BTreeMap<String, Type>) -> Self {
        Self {
            bindings,
            from_expected_result: Default::default(),
        }
    }

    /// Mark the bindings made since `before` as taken from the call's expected
    /// result type, so an argument may replace them.
    pub fn mark_from_expected_result(&mut self, before: &TypeParamSubstitution) {
        for name in self.bindings.keys() {
            if !before.bindings.contains_key(name) {
                self.from_expected_result.insert(name.clone());
            }
        }
    }

    /// Whether `ty` mentions a type parameter bound only from the call's
    /// expected result type.
    pub fn mentions_expected_result_binding(&self, ty: &Type) -> bool {
        super::infer::expr::mentions_type_var(ty, &|name| self.from_expected_result.contains(name))
    }

    /// Make the bindings from the expected result type that `ty` mentions
    /// final, so no argument replaces them.
    pub fn keep_expected_result_bindings(&mut self, ty: &Type) {
        self.from_expected_result
            .retain(|name| !super::infer::expr::mentions_type_var(ty, &|var| var == name));
    }

    pub fn insert(&mut self, name: String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    pub fn get(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Replace `TypeVar`s in `ty` with bound types; chases var-to-var chains.
    /// Fails, without building the rest, once the result passes a type limit.
    pub fn apply(&self, ty: &Type, limits: &TypeLimits) -> Result<Type, TypeTooLarge> {
        self.apply_rec(ty, 1, &mut limits.budget(), &mut Vec::new())
    }

    /// [`apply`](Self::apply) where the caller cannot return an error: a type
    /// past a limit is recorded in `limits` and comes back as `Type::Error`.
    pub fn apply_or_record(&self, ty: &Type, limits: &TypeLimits) -> Type {
        limits.type_or_error(self.apply(ty, limits))
    }

    /// [`apply`](Self::apply) for the node at `depth` of the result, tracking
    /// which variables' bindings are open on the current path in `substituting`.
    ///
    /// A binding is allowed to mention the very variable it binds — a recursive
    /// alias's back-edge rehydrated to its inline form carries the alias's own
    /// parameter inside its body — and chasing that mention would re-enter the
    /// same binding forever. Re-entry yields the variable itself, the finite
    /// spelling of that fixpoint. The degenerate case is a `T → T` self-binding
    /// from two nested generics sharing a name, which re-enters at depth zero.
    fn apply_rec(
        &self,
        ty: &Type,
        depth: u32,
        budget: &mut TypeBudget<'_>,
        substituting: &mut Vec<String>,
    ) -> Result<Type, TypeTooLarge> {
        if let Type::TypeVar(name) = ty
            && let Some(bound) = self.bindings.get(name)
            && !substituting.iter().any(|open| open == name)
        {
            // The bound type takes the variable's place, at the same depth.
            substituting.push(name.clone());
            let applied = self.apply_rec(bound, depth, budget, substituting);
            substituting.pop();
            return applied;
        }
        budget.charge(depth)?;
        let child = depth.saturating_add(1);
        // An unbound or re-entered variable is a leaf and stays itself.
        // GenericParam is opaque here — body-form, not substituted.
        map_children(ty, |inner| {
            self.apply_rec(inner, child, budget, substituting)
        })
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
        Unifier::new(self, Some(types), types.limits).unify(param_ty, arg_ty)
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
        let resolved = self.apply_or_record(param_ty, types.limits);
        if !super::infer::expr::type_contains_type_var(&resolved)
            && assignable(arg_ty, &resolved, types)
        {
            return Ok(());
        }
        let mut unifier = Unifier::new(self, Some(types), types.limits);
        unifier.subtype_widening = true;
        unifier.is_argument = true;
        unifier.unify(param_ty, arg_ty)
    }
}

/// A [`Unifier`]'s state before a speculative attempt.
struct Snapshot {
    bindings: BTreeMap<String, Type>,
    from_expected_result: std::collections::BTreeSet<String>,
    assumed_len: usize,
}

/// The mutable state of one `unify` call: the bindings being built, the
/// registry its [`Type::AliasRef`] arm expands against, and the coinductive
/// assumption set that stops two recursive aliases expanding forever.
struct Unifier<'a> {
    sub: &'a mut TypeParamSubstitution,
    types: Option<TypeResolver<'a>>,
    /// Where a substitution that passes a type limit is recorded; the walk
    /// continues with `Type::Error`, which unifies with anything.
    limits: &'a TypeLimits,
    assumed_pairs: Vec<(Type, Type)>,
    /// Whether an argument assignable to an already-bound type parameter is
    /// accepted rather than reported as a conflict. Set by
    /// [`unify_argument`](TypeParamSubstitution::unify_argument), and cleared
    /// while descending into a function *parameter*, which is contravariant.
    subtype_widening: bool,
    /// Whether this unifies a call argument, which replaces a binding taken
    /// from the expected result type when it doesn't fit it.
    is_argument: bool,
    /// Whether the walk is inside a function type's parameter, where an
    /// argument may be wider than the type parameter's binding.
    contravariant: bool,
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
                let resolved = self.sub.apply_or_record(&existing, self.limits);
                // A `T → T` self-binding (two nested generics sharing a name, e.g. the
                // unresolved vars of `new Map()` flowing into `.set`'s receiver) is
                // effectively unbound: recursing here would unify `T` against the arg
                // forever. Treat it as a fresh binding instead — same hazard the `apply`
                // guard above handles.
                if matches!(&resolved, Type::TypeVar(other) if other == name) {
                    self.sub.bindings.insert(name.clone(), arg_ty.clone());
                    return Ok(());
                }
                let arg_resolved = self.sub.apply_or_record(arg_ty, self.limits);
                // Recurse instead of `==` to peel aliases at every level; remap to Conflict to pin the offending param.
                let replaceable = self.is_argument && self.sub.from_expected_result.remove(name);
                return match self.unify(&resolved, &arg_resolved) {
                    Ok(()) => Ok(()),
                    Err(_) if self.accepts_as_subtype(&arg_resolved, &resolved) => Ok(()),
                    Err(_) if self.accepts_as_supertype(&arg_resolved, &resolved) => Ok(()),
                    Err(_) if replaceable => {
                        self.sub.bindings.insert(name.clone(), arg_ty.clone());
                        Ok(())
                    }
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
                    self.in_function_parameter(|u| u.unify(p, a))?;
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
            // A data-only interface and an object type are mutually assignable
            // (spec §2.3), so one infers from the other through its fields.
            (Type::InterfaceRef { .. }, Type::Object { .. })
            | (Type::Object { .. }, Type::InterfaceRef { .. }) => {
                let expanded = self.types.and_then(|types| {
                    let param = expand_interface_data_shape(param_ty, types);
                    let arg = expand_interface_data_shape(arg_ty, types);
                    Some((
                        param.unwrap_or_else(|| param_ty.clone()),
                        arg.unwrap_or_else(|| arg_ty.clone()),
                    ))
                    .filter(|(param, arg)| param != param_ty || arg != arg_ty)
                });
                match expanded {
                    Some((param, arg)) => self.unify(&param, &arg),
                    None => Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    }),
                }
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
                    let resolved_a = self.sub.apply_or_record(a, self.limits);
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
                    || self.is_literal_of(arg_ty, param_ty)
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
    fn new(
        sub: &'a mut TypeParamSubstitution,
        types: Option<TypeResolver<'a>>,
        limits: &'a TypeLimits,
    ) -> Self {
        Unifier {
            sub,
            types,
            limits,
            assumed_pairs: Vec::new(),
            subtype_widening: false,
            is_argument: false,
            contravariant: false,
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

    /// Whether `arg` is a literal type of the primitive `param`, which binds
    /// nothing and is assignable to it: `{ length: n }` with `const n = 4` is
    /// a `{ length: number }`.
    fn is_literal_of(&self, arg: &Type, param: &Type) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        matches!(param, Type::Number | Type::String | Type::Boolean)
            && is_primitive_literal(arg)
            && assignable(arg, param, types)
    }

    /// Whether an argument's function parameter that failed to unify with an
    /// already-bound type parameter is still acceptable, because the binding
    /// is assignable to it: `[5].map((a: unknown) => ...)` passes each `number`
    /// to a parameter that takes any value.
    fn accepts_as_supertype(&self, arg: &Type, bound: &Type) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        self.is_argument && self.contravariant && assignable(bound, arg, types)
    }

    /// Unify within a function type's parameter: subtype-widening stops (see
    /// [`Self::without_subtype_widening`]), and the variance flips.
    #[allow(clippy::result_large_err)]
    fn in_function_parameter(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<(), UnifyError>,
    ) -> Result<(), UnifyError> {
        self.contravariant = !self.contravariant;
        let out = self.without_subtype_widening(f);
        self.contravariant = !self.contravariant;
        out
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
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            bindings: self.sub.bindings.clone(),
            from_expected_result: self.sub.from_expected_result.clone(),
            assumed_len: self.assumed_pairs.len(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.sub.bindings = snapshot.bindings;
        self.sub.from_expected_result = snapshot.from_expected_result;
        self.assumed_pairs.truncate(snapshot.assumed_len);
    }

    /// Rehydrate any recursion back-edge in `ty` to its inline form. A no-op
    /// without a registry to resolve the back-edge against.
    fn inline_alias_refs(&self, ty: &Type) -> Type {
        match self.types {
            Some(types) => types.limits.type_or_error(rehydrate_alias_refs(ty, types)),
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
    /// Fails outright when applying a binding passes a type limit.
    pub fn resolve_all(
        &self,
        generics: &[String],
        limits: &TypeLimits,
    ) -> Result<Result<Vec<Type>, Vec<String>>, TypeTooLarge> {
        let mut resolved = Vec::with_capacity(generics.len());
        let mut unbound = Vec::new();
        for name in generics {
            match self.bindings.get(name) {
                Some(t) => {
                    let r = self.apply(t, limits)?;
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
            Ok(Ok(resolved))
        } else {
            Ok(Err(unbound))
        }
    }
}

fn is_primitive_literal(ty: &Type) -> bool {
    matches!(
        ty,
        Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)
    )
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
        let limits = TypeLimits::default();
        let result = Unifier::new(sub, None, &limits).unify(param_ty, arg_ty);
        assert_eq!(limits.take(), Ok(()));
        result
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
        assert_eq!(
            s.apply(&t("T"), &crate::type_size::TypeLimits::default())
                .unwrap(),
            Type::Number
        );
    }

    #[test]
    fn apply_passes_through_unbound_var() {
        let s = TypeParamSubstitution::new();
        assert_eq!(
            s.apply(&t("T"), &crate::type_size::TypeLimits::default())
                .unwrap(),
            t("T")
        );
    }

    #[test]
    fn apply_chases_var_to_var_chain() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), t("U"));
        s.insert("U".to_string(), Type::Number);
        assert_eq!(
            s.apply(&t("T"), &crate::type_size::TypeLimits::default())
                .unwrap(),
            Type::Number
        );
    }

    #[test]
    fn apply_recurses_into_array() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), Type::Number);
        let arr_t = Type::Array(Box::new(t("T")));
        assert_eq!(
            s.apply(&arr_t, &crate::type_size::TypeLimits::default())
                .unwrap(),
            Type::Array(Box::new(Type::Number))
        );
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
            s.apply(&f, &crate::type_size::TypeLimits::default())
                .unwrap(),
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
            s.apply(&o, &crate::type_size::TypeLimits::default())
                .unwrap(),
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
            .resolve_all(
                &["T".to_string(), "U".to_string()],
                &crate::type_size::TypeLimits::default(),
            )
            .unwrap()
            .expect("all bound");
        assert_eq!(resolved, vec![Type::Number, Type::String]);
    }

    #[test]
    fn resolve_all_returns_unbound_names() {
        let s = TypeParamSubstitution::new();
        let unbound = s
            .resolve_all(
                &["T".to_string(), "U".to_string()],
                &crate::type_size::TypeLimits::default(),
            )
            .unwrap()
            .expect_err("none bound");
        assert_eq!(unbound, vec!["T".to_string(), "U".to_string()]);
    }

    #[test]
    fn resolve_all_chases_var_to_var() {
        let mut s = TypeParamSubstitution::new();
        s.insert("T".to_string(), t("U"));
        s.insert("U".to_string(), Type::Number);
        let resolved = s
            .resolve_all(&["T".to_string()], &crate::type_size::TypeLimits::default())
            .unwrap()
            .expect("T resolves through U");
        assert_eq!(resolved, vec![Type::Number]);
    }
}
