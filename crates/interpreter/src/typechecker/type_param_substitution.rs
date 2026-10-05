//! Generic type-parameter substitution and structural unification.

use std::collections::BTreeMap;

use crate::Type;
use crate::type_size::{TypeBudget, TypeLimits, TypeTooLarge, map_children};
use crate::typechecker::infer::assignable::{
    TypeResolver, assignable, expand_alias_ref, expand_interface_data_shape, rest_function_accepts,
};
use crate::typechecker::infer::type_aliases::rehydrate_alias_refs;

/// BTreeMap for deterministic ordering (stable snapshots and error messages).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct TypeParamSubstitution {
    bindings: BTreeMap<String, Type>,
    /// Bindings taken from the type a call's result is expected to have, which
    /// an argument may still replace: as in tsc, what the arguments say comes
    /// first. One stays replaceable until an argument agrees with it.
    replaceable: std::collections::BTreeSet<String>,
    /// For a type parameter in a union parameter whose other members took
    /// every member of a union argument, that whole argument: tsc infers it at
    /// the lowest priority, so it binds the type parameter only when nothing
    /// else does (see [`Self::bind_whole_union_fallbacks`]).
    whole_union_fallbacks: BTreeMap<String, Type>,
    /// Union argument members that only closely matched a member of their
    /// union parameter while its type parameter was unbound: each must fit
    /// the parameter once inference is done, as tsc checks the argument then.
    close_matches: Vec<CloseMatch>,
}

/// An argument member that closely matched `sibling`, a member of the union
/// parameter `param`, as `Box<string>` does `Box<number>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseMatch {
    pub param: Type,
    pub sibling: Type,
    pub arg: Type,
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
            replaceable: Default::default(),
            whole_union_fallbacks: Default::default(),
            close_matches: Vec::new(),
        }
    }

    /// The table member resolution already computed, adopted as-is. Contrast
    /// [`from_pairs`](Self::from_pairs), which zips a declaration against a
    /// receiver and is correct only when that declaration is the one the member
    /// was found on.
    pub fn from_bindings(bindings: BTreeMap<String, Type>) -> Self {
        Self {
            bindings,
            replaceable: Default::default(),
            whole_union_fallbacks: Default::default(),
            close_matches: Vec::new(),
        }
    }

    /// Mark the bindings made since `before` as taken from the call's expected
    /// result type, so an argument may replace them.
    pub fn mark_from_expected_result(&mut self, before: &TypeParamSubstitution) {
        for name in self.bindings.keys() {
            if !before.bindings.contains_key(name) {
                self.replaceable.insert(name.clone());
            }
        }
    }

    /// The argument members that closely matched a union parameter member
    /// and wait to be checked once inference is done, each once; none are
    /// left after.
    pub fn take_close_matches(&mut self) -> Vec<CloseMatch> {
        let mut distinct: Vec<CloseMatch> = Vec::new();
        for close_match in std::mem::take(&mut self.close_matches) {
            if !distinct.contains(&close_match) {
                distinct.push(close_match);
            }
        }
        distinct
    }

    /// How many close matches wait to be checked.
    pub fn close_match_count(&self) -> usize {
        self.close_matches.len()
    }

    /// Drop the close matches recorded after the first `count`.
    pub fn forget_close_matches_after(&mut self, count: usize) {
        self.close_matches.truncate(count);
    }

    /// The whole union argument that stands in for `name` when nothing else
    /// binds it.
    pub fn whole_union_fallback(&self, name: &str) -> Option<&Type> {
        self.whole_union_fallbacks.get(name)
    }

    /// Bind each type parameter still unbound that a whole union argument
    /// stands in for, as tsc does with its lowest-priority inference.
    pub fn bind_whole_union_fallbacks(&mut self) {
        self.bind_whole_union_fallbacks_where(|_| true);
    }

    /// [`Self::bind_whole_union_fallbacks`] for the type parameters `names`.
    pub fn bind_whole_union_fallbacks_named(&mut self, names: &[String]) {
        self.bind_whole_union_fallbacks_where(|name| names.iter().any(|each| each == name));
    }

    /// [`Self::bind_whole_union_fallbacks`] for the type parameters `types`
    /// mention.
    pub fn bind_whole_union_fallbacks_in(&mut self, types: &[Type]) {
        self.bind_whole_union_fallbacks_where(|name| {
            types
                .iter()
                .any(|ty| super::infer::expr::mentions_type_var(ty, &|var| var == name))
        });
    }

    fn bind_whole_union_fallbacks_where(&mut self, applies: impl Fn(&str) -> bool) {
        let unbound: Vec<(String, Type)> = self
            .whole_union_fallbacks
            .iter()
            .filter(|(name, _)| applies(name) && self.is_unbound(name))
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        self.bindings.extend(unbound);
    }

    /// Unbind, and return, each type parameter unbound in `before` that is
    /// now bound to just its whole-union fallback, as a callback field typed
    /// from the fallback binds it: the fallback stays the lowest-priority
    /// inference, so a later field may still bind the type parameter.
    pub fn unbind_fallback_echoes(&mut self, before: &TypeParamSubstitution) -> Vec<String> {
        let echoes: Vec<String> = self
            .whole_union_fallbacks
            .iter()
            .filter(|(name, fallback)| {
                before.is_unbound(name) && self.bindings.get(name.as_str()) == Some(fallback)
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in &echoes {
            self.bindings.remove(name);
        }
        echoes
    }

    /// Whether `name` has no binding yet, or is bound only to itself.
    fn is_unbound(&self, name: &str) -> bool {
        match self.bindings.get(name) {
            None => true,
            Some(bound) => matches!(bound.peel(), Type::TypeVar(other) if other == name),
        }
    }

    /// Whether `ty` mentions a type parameter whose binding an argument may
    /// still replace.
    pub fn mentions_replaceable_binding(&self, ty: &Type) -> bool {
        super::infer::expr::mentions_type_var(ty, &|name| self.replaceable.contains(name))
    }

    /// Make the replaceable bindings that `ty` mentions final, so no argument
    /// replaces them.
    pub fn keep_replaceable_bindings(&mut self, ty: &Type) {
        let mentioned =
            |name: &String| super::infer::expr::mentions_type_var(ty, &|var| var == name);
        self.replaceable.retain(|name| !mentioned(name));
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
        self.keeping_close_matches_on_success(|sub| {
            Unifier::new(sub, Some(types), types.limits).unify(param_ty, arg_ty)
        })
    }

    /// Run `attempt` and keep the close matches it records only if it
    /// succeeds: a failed unification's mismatch is reported already, and
    /// its members never took part.
    #[allow(clippy::result_large_err)]
    fn keeping_close_matches_on_success(
        &mut self,
        attempt: impl FnOnce(&mut Self) -> Result<(), UnifyError>,
    ) -> Result<(), UnifyError> {
        let close_matches_before = self.close_match_count();
        let unified = attempt(self);
        if unified.is_err() {
            self.forget_close_matches_after(close_matches_before);
        }
        unified
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
        self.keeping_close_matches_on_success(|sub| {
            let mut unifier = Unifier::new(sub, Some(types), types.limits);
            unifier.subtype_widening = true;
            unifier.is_argument = true;
            unifier.unify(param_ty, arg_ty)
        })
    }
}

/// A [`Unifier`]'s state before a speculative attempt.
struct Snapshot {
    bindings: BTreeMap<String, Type>,
    replaceable: std::collections::BTreeSet<String>,
    whole_union_fallbacks: BTreeMap<String, Type>,
    close_matches_len: usize,
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
                // Unified with an argument, a replaceable binding is the
                // arguments' own from here on, whether or not it is replaced.
                let replaceable = self.sub.replaceable.remove(name) && self.is_argument;
                // Recurse instead of `==` to peel aliases at every level; remap to Conflict to pin the offending param.
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
                if *rest_b && !rest_a {
                    return self.unify_rest_function(param_ty, arg_ty, pa, pb, ra, rb);
                }
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
                let mismatch = || UnifyError::Mismatch {
                    expected: param_ty.clone(),
                    got: arg_ty.clone(),
                };
                if ma != mb {
                    return if self.accepts_as_supertype(arg_ty, param_ty) {
                        Ok(())
                    } else {
                        Err(mismatch())
                    };
                }
                if aa.len() != ab.len() {
                    return Err(mismatch());
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
            // Union-vs-union: a lone unbound type parameter takes what its
            // siblings leave (see `unify_union_into_lone_type_var`); otherwise
            // two passes pair matching members first, then unify leftovers in order.
            (Type::Union(pa), Type::Union(pb)) => {
                if let Some(unified) = self.unify_union_into_lone_type_var(pa, pb) {
                    return unified;
                }
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
                if self.try_defer_close_match_into_lone_type_var(pa, arg_ty) {
                    return Ok(());
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
                    || self.accepts_as_supertype(arg_ty, param_ty)
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

    /// Unify a fixed-arity function type, `param_ty`, with an argument that
    /// has a rest parameter: each fixed position against the argument's
    /// parameter there, and each position past them against the rest
    /// parameter's element type (see [`rest_function_accepts`]).
    #[allow(clippy::result_large_err)]
    fn unify_rest_function(
        &mut self,
        param_ty: &Type,
        arg_ty: &Type,
        params: &[Type],
        arg_params: &[Type],
        ret: &Type,
        arg_ret: &Type,
    ) -> Result<(), UnifyError> {
        let mut result = Ok(());
        let fits = rest_function_accepts(arg_params, params, |passed, declared| {
            let unified = self.in_function_parameter(|u| u.unify(passed, declared));
            let ok = unified.is_ok();
            if result.is_ok() {
                result = unified;
            }
            ok
        });
        if !fits {
            return Err(result.err().unwrap_or(UnifyError::Mismatch {
                expected: param_ty.clone(),
                got: arg_ty.clone(),
            }));
        }
        self.unify(ret, arg_ret)
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

    /// Whether an argument's function parameter `arg` that failed to unify with
    /// the parameter type `param` is still acceptable, because `param` is
    /// assignable to it: `[5].map((a: unknown, i: unknown) => ...)` passes each
    /// `number` to a parameter that takes any value, and `(a: Animal) => ...`
    /// takes each `Dog`. `param` must be fully known: a type parameter still
    /// unbound in it could later bind to something `arg` doesn't accept.
    fn accepts_as_supertype(&self, arg: &Type, param: &Type) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        if !self.is_argument || !self.contravariant {
            return false;
        }
        let known_param = self.sub.apply_or_record(param, self.limits);
        !super::infer::expr::type_contains_type_var(&known_param)
            && assignable(&known_param, arg, types)
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

    /// tsc's union-to-union rule when exactly one of `params` is a type
    /// parameter not yet bound; None otherwise, so the members pair up
    /// instead. Each member of `args` that unifies with one of the other
    /// members of `params` is absorbed by it. A member that only
    /// [`closely_matches`] another member, as `Box<string>` does `Box<number>`,
    /// is not given to the type parameter, as tsc pairs them (a parameter
    /// member identical to some argument member is not closely matched with
    /// others). Then:
    /// - if members are left over, they bind the type parameter (`T | null`
    ///   with `"on" | "off" | null` binds `T` to `"on" | "off"`); once it is
    ///   bound, by them or by absorbing, each closely matched member must fit
    ///   some member of `params`;
    /// - otherwise the whole argument is the type parameter's fallback, used
    ///   only when nothing else binds it, and each closely matched member
    ///   waits to be checked once inference is done (see
    ///   [`TypeParamSubstitution::take_close_matches`]).
    ///
    /// In a callback's parameter (a contravariant position), a closely
    /// matched member is dropped instead: the parameter only has to accept
    /// what the slot passes.
    #[allow(clippy::result_large_err)]
    fn unify_union_into_lone_type_var(
        &mut self,
        params: &[Type],
        args: &[Type],
    ) -> Option<Result<(), UnifyError>> {
        let (type_var, others) = self.split_lone_unbound_type_var(params)?;
        let pairable: Vec<&Type> = others
            .iter()
            .copied()
            .filter(|other| !args.iter().any(|arg| arg.peel() == other.peel()))
            .collect();
        let mut rest = Vec::new();
        let mut closely_matched = Vec::new();
        for arg in args {
            if others
                .iter()
                .any(|other| self.unifies_or_rolls_back(other, arg))
            {
                continue;
            }
            match pairable.iter().find(|other| closely_matches(other, arg)) {
                Some(_) if self.contravariant => {}
                Some(other) => closely_matched.push((*other, arg)),
                None => rest.push(arg.clone()),
            }
        }
        if !rest.is_empty() {
            if let Err(error) = self.unify(type_var, &Type::union(rest)) {
                return Some(Err(error));
            }
            return Some(self.check_closely_matched(params, &closely_matched));
        }
        // Absorbing argument members into another member, as `Box<T>` does
        // `Box<number>`, may have bound the type parameter after all.
        if !self.is_unbound_type_var(type_var) {
            return Some(self.check_closely_matched(params, &closely_matched));
        }
        for (sibling, arg) in closely_matched {
            self.record_close_match(params, sibling, arg);
        }
        self.offer_whole_union_fallback(type_var, Type::union(args.to_vec()));
        Some(Ok(()))
    }

    /// The one member of `params` that is a type parameter not yet bound,
    /// and the other members; None when there is no such member or more
    /// than one.
    fn split_lone_unbound_type_var<'p>(
        &self,
        params: &'p [Type],
    ) -> Option<(&'p Type, Vec<&'p Type>)> {
        let mut unbound = params
            .iter()
            .enumerate()
            .filter(|(_, member)| self.is_unbound_type_var(member));
        let (type_var_index, type_var) = unbound.next()?;
        if unbound.next().is_some() {
            return None;
        }
        let others = params
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != type_var_index)
            .map(|(_, member)| member)
            .collect();
        Some((type_var, others))
    }

    /// tsc's rule for a non-union argument against a union parameter, as
    /// `Box<boolean>` against `T | Box<number>`. When `params` has exactly
    /// one unbound type parameter and `arg` [`closely_matches`] one of the
    /// other members without unifying with any of them, `arg` is deferred as
    /// a union argument whose members are all closely matched is: it is
    /// recorded as a close match, checked once inference is done, and
    /// offered as the type parameter's fallback. Returns true when `arg` was
    /// deferred, false when the caller should try each member in order.
    fn try_defer_close_match_into_lone_type_var(&mut self, params: &[Type], arg: &Type) -> bool {
        if !self.is_argument || self.contravariant {
            return false;
        }
        let Some((type_var, others)) = self.split_lone_unbound_type_var(params) else {
            return false;
        };
        let Some(sibling) = others.iter().find(|other| closely_matches(other, arg)) else {
            return false;
        };
        if others.iter().any(|other| self.would_unify(other, arg)) {
            return false;
        }
        self.record_close_match(params, sibling, arg);
        self.offer_whole_union_fallback(type_var, arg.clone());
        true
    }

    /// Record that `arg` closely matched `sibling`, a member of the union
    /// parameter `params`, to be checked once inference is done.
    fn record_close_match(&mut self, params: &[Type], sibling: &Type, arg: &Type) {
        self.sub.close_matches.push(CloseMatch {
            param: Type::union(params.to_vec()),
            sibling: sibling.clone(),
            arg: arg.clone(),
        });
    }

    /// Offer `candidate` as `type_var`'s whole-union fallback. As tsc picks
    /// the common supertype of candidates of the same priority, `candidate`
    /// replaces the existing fallback when that fallback is assignable to
    /// it; otherwise the existing fallback stays.
    fn offer_whole_union_fallback(&mut self, type_var: &Type, candidate: Type) {
        let Type::TypeVar(name) = type_var.peel() else {
            return;
        };
        if let Some(existing) = self.sub.whole_union_fallbacks.get(name).cloned()
            && !self.would_unify(&candidate, &existing)
        {
            return;
        }
        self.sub
            .whole_union_fallbacks
            .insert(name.clone(), candidate);
    }

    /// Check each closely matched argument member against the whole
    /// parameter now that its type parameter is bound, as tsc checks the
    /// argument once inference is done: one no member takes is reported
    /// against the member it closely matched.
    #[allow(clippy::result_large_err)]
    fn check_closely_matched(
        &mut self,
        params: &[Type],
        closely_matched: &[(&Type, &Type)],
    ) -> Result<(), UnifyError> {
        for (other, arg) in closely_matched {
            if !params
                .iter()
                .any(|param| self.unifies_or_rolls_back(param, arg))
            {
                return self.unify(other, arg);
            }
        }
        Ok(())
    }

    /// Whether `ty` is a type parameter with no binding yet, or bound only to
    /// itself.
    fn is_unbound_type_var(&self, ty: &Type) -> bool {
        matches!(ty.peel(), Type::TypeVar(name) if self.sub.is_unbound(name))
    }

    /// Whether `param_ty` unifies with `arg_ty`, keeping the bindings that
    /// took when it does and undoing them when it doesn't.
    fn unifies_or_rolls_back(&mut self, param_ty: &Type, arg_ty: &Type) -> bool {
        let snap = self.snapshot();
        let unified = self.unify(param_ty, arg_ty).is_ok();
        if !unified {
            self.restore(snap);
        }
        unified
    }

    /// Whether `param_ty` unifies with `arg_ty`, undoing the bindings either
    /// way.
    fn would_unify(&mut self, param_ty: &Type, arg_ty: &Type) -> bool {
        let snap = self.snapshot();
        let unified = self.unify(param_ty, arg_ty).is_ok();
        self.restore(snap);
        unified
    }

    /// A speculative attempt's rollback point. The assumption set rolls back
    /// with the bindings: a pair assumed to hold inside an attempt that failed
    /// was never proved, and leaving it behind would let a later mismatch pass.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            bindings: self.sub.bindings.clone(),
            replaceable: self.sub.replaceable.clone(),
            whole_union_fallbacks: self.sub.whole_union_fallbacks.clone(),
            close_matches_len: self.sub.close_matches.len(),
            assumed_len: self.assumed_pairs.len(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.sub.bindings = snapshot.bindings;
        self.sub.replaceable = snapshot.replaceable;
        self.sub.whole_union_fallbacks = snapshot.whole_union_fallbacks;
        self.sub.close_matches.truncate(snapshot.close_matches_len);
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

/// Whether `arg` names the same declaration as the union member `param`
/// with other type arguments, as `Box<string>` does `Box<number>`: tsc's
/// "closely matched" rule, which pairs such a member with that sibling rather
/// than giving it to the union's type parameter. Arrays match arrays of the
/// same mutability, as `Array` and `ReadonlyArray` are distinct in tsc;
/// tuples, which name no declaration, match nothing.
fn closely_matches(param: &Type, arg: &Type) -> bool {
    match (param.peel(), arg.peel()) {
        (Type::InterfaceRef { mangled: p, .. }, Type::InterfaceRef { mangled: a, .. }) => p == a,
        (Type::ClassRef { mangled: p, .. }, Type::ClassRef { mangled: a, .. }) => p == a,
        (Type::Array(_), Type::Array(_)) => param.is_readonly_array() == arg.is_readonly_array(),
        _ => false,
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
