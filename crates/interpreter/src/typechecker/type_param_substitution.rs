//! Generic type-parameter substitution and structural unification.

use std::collections::BTreeMap;

use crate::type_size::{TypeBudget, TypeLimits, TypeTooLarge, map_children};
use crate::typechecker::infer::assignable::{
    TypeResolver, assignable, drops_readonly, expand_alias_ref, expand_interface_data_shape,
    rest_function_accepts,
};
use crate::typechecker::infer::type_aliases::rehydrate_alias_refs;
use crate::typechecker::infer::variance::Variance;
use crate::{Span, Type};

/// BTreeMap for deterministic ordering (stable snapshots and error messages).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct TypeParamSubstitution {
    bindings: BTreeMap<String, Type>,
    /// Bindings taken from the type a call's result is expected to have, which
    /// an argument may still replace: as in tsc, what the arguments say comes
    /// first. One stays replaceable until an argument agrees with it.
    replaceable: std::collections::BTreeSet<String>,
    /// Bindings taken from an argument at a covariant position, which a later
    /// argument of a wider type may widen: tsc infers the common supertype of
    /// such candidates (`pick(dog, animal)` is an `Animal`). One used at a
    /// contravariant position, as a callback's parameter, stays as it is.
    widenable: std::collections::BTreeSet<String>,
    /// The reverse, for bindings taken only at contravariant positions: tsc
    /// infers the common subtype of those candidates, so a later callback
    /// taking a narrower parameter narrows the binding.
    narrowable: std::collections::BTreeSet<String>,
    /// Candidate bindings taken only from object and array literals. tsc
    /// combines those candidates into their union, which another literal may
    /// join; a non-literal candidate wins over them.
    literal_candidates: std::collections::BTreeSet<String>,
    /// Literal candidates bound by more than one literal argument: tsc
    /// relates them as the literals' union, so no non-literal candidate
    /// takes them over.
    several_literal_candidates: std::collections::BTreeSet<String>,
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

/// An argument member that must fit the union parameter `param` once
/// inference is done, reported against `matched_member` if it does not:
/// the member it closely matched, as `Box<string>` does `Box<number>`, or
/// the type parameter whose fallback it fit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseMatch {
    pub param: Type,
    pub matched_member: Type,
    pub arg: Type,
    /// The fields of the argument, outermost first, it was found in.
    pub field_path: Vec<String>,
    /// Where in the call it came from, once the call has located it.
    pub argument_span: Option<Span>,
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
            widenable: Default::default(),
            narrowable: Default::default(),
            literal_candidates: Default::default(),
            several_literal_candidates: Default::default(),
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
            widenable: Default::default(),
            narrowable: Default::default(),
            literal_candidates: Default::default(),
            several_literal_candidates: Default::default(),
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

    /// Locate the close matches recorded after the first `count` in a call
    /// argument, at the span `locate` gives for each one's field path.
    pub fn locate_close_matches_after(
        &mut self,
        count: usize,
        mut locate: impl FnMut(&[String]) -> Span,
    ) {
        for close_match in self.close_matches.iter_mut().skip(count) {
            close_match.argument_span = Some(locate(&close_match.field_path));
        }
    }

    /// Record that the close matches after the first `count` were found in
    /// field `name` of the argument.
    pub fn nest_close_matches_after(&mut self, count: usize, name: &str) {
        for close_match in self.close_matches.iter_mut().skip(count) {
            close_match.field_path.insert(0, name.to_string());
        }
    }

    /// Keep only the close matches after the first `count` that `keep`
    /// accepts.
    pub fn retain_close_matches_after(
        &mut self,
        count: usize,
        mut keep: impl FnMut(&CloseMatch) -> bool,
    ) {
        let mut index = 0;
        self.close_matches.retain(|close_match| {
            index += 1;
            index <= count || keep(close_match)
        });
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

    /// Whether a whole union argument stands in for `name`.
    pub fn has_whole_union_fallback(&self, name: &str) -> bool {
        self.whole_union_fallbacks.contains_key(name)
    }

    /// `ty` with every unbound type parameter that has a whole-union
    /// fallback replaced by `never`, so only the rest of `ty` can take a
    /// value; None when `ty` names no such type parameter.
    pub fn without_fallback_type_params(&self, ty: &Type, limits: &TypeLimits) -> Option<Type> {
        let names: Vec<&String> = self
            .whole_union_fallbacks
            .keys()
            .filter(|name| {
                self.is_unbound(name)
                    && super::infer::expr::mentions_type_var(ty, &|var| var == name.as_str())
            })
            .collect();
        if names.is_empty() {
            return None;
        }
        let mut concrete = self.clone();
        for name in names {
            concrete.bindings.insert(name.clone(), Type::Never);
        }
        Some(concrete.apply_or_record(ty, limits))
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

    /// The type parameters bound to something other than themselves.
    fn bound_names(&self) -> impl Iterator<Item = String> + '_ {
        self.bindings
            .keys()
            .filter(|name| !self.is_unbound(name))
            .cloned()
    }

    /// Whether `name` has no binding yet, or is bound only to itself.
    pub(crate) fn is_unbound(&self, name: &str) -> bool {
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

    /// Whether `ty` mentions a type parameter bound to a candidate that a
    /// later argument may still widen or narrow.
    pub fn mentions_candidate_binding(&self, ty: &Type) -> bool {
        super::infer::expr::mentions_type_var(ty, &|name| self.is_candidate_binding(name))
    }

    /// Whether `ty` mentions a candidate binding that some argument other
    /// than an object or array literal gave.
    pub fn mentions_non_literal_candidate_binding(&self, ty: &Type) -> bool {
        super::infer::expr::mentions_type_var(ty, &|name| {
            self.is_candidate_binding(name) && !self.literal_candidates.contains(name)
        })
    }

    /// These bindings without the candidates a later argument may still
    /// widen or narrow.
    pub fn without_candidate_bindings(&self) -> TypeParamSubstitution {
        let mut loose = self.clone();
        loose
            .bindings
            .retain(|name, _| !self.is_candidate_binding(name));
        loose
    }

    /// Whether `name` is bound to a candidate a later argument may widen.
    pub fn is_widenable(&self, name: &str) -> bool {
        self.widenable.contains(name)
    }

    fn is_candidate_binding(&self, name: &str) -> bool {
        self.widenable.contains(name) || self.narrowable.contains(name)
    }

    /// Make the replaceable bindings that `ty` mentions final, so no argument
    /// replaces them.
    pub fn keep_replaceable_bindings(&mut self, ty: &Type) {
        let mentioned =
            |name: &String| super::infer::expr::mentions_type_var(ty, &|var| var == name);
        self.replaceable.retain(|name| !mentioned(name));
    }

    /// Take each type parameter `should_restore` accepts back to what
    /// `before` held for it: its binding and how it may still change. Close
    /// matches stay, as their callers forget or check them on their own.
    pub fn restore_bindings(
        &mut self,
        before: &TypeParamSubstitution,
        should_restore: impl Fn(&str) -> bool,
    ) {
        let TypeParamSubstitution {
            bindings,
            replaceable,
            widenable,
            narrowable,
            literal_candidates,
            several_literal_candidates,
            whole_union_fallbacks,
            close_matches: _,
        } = before;
        let names: std::collections::BTreeSet<String> = self
            .bindings
            .keys()
            .chain(bindings.keys())
            .chain(self.whole_union_fallbacks.keys())
            .chain(whole_union_fallbacks.keys())
            .filter(|name| should_restore(name))
            .cloned()
            .collect();
        for name in names {
            restore_entry(&mut self.bindings, bindings, &name);
            restore_entry(
                &mut self.whole_union_fallbacks,
                whole_union_fallbacks,
                &name,
            );
            for (set, was) in [
                (&mut self.replaceable, replaceable),
                (&mut self.widenable, widenable),
                (&mut self.narrowable, narrowable),
                (&mut self.literal_candidates, literal_candidates),
                (
                    &mut self.several_literal_candidates,
                    several_literal_candidates,
                ),
            ] {
                if was.contains(&name) {
                    set.insert(name.clone());
                } else {
                    set.remove(&name);
                }
            }
        }
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
        let open_before = substituting.len();
        let applied = self
            .chase_binding(ty, budget, substituting)
            .and_then(|bound| {
                budget.charge(depth)?;
                let child = depth.saturating_add(1);
                // An unbound or re-entered variable stays a leaf. GenericParam is
                // opaque here: body-form, not substituted.
                map_children(bound, |inner| {
                    self.apply_rec(inner, child, budget, substituting)
                })
            });
        substituting.truncate(open_before);
        applied
    }

    fn chase_binding<'a>(
        &'a self,
        mut ty: &'a Type,
        budget: &mut TypeBudget<'_>,
        substituting: &mut Vec<String>,
    ) -> Result<&'a Type, TypeTooLarge> {
        while let Type::TypeVar(name) = ty {
            // Charge the lookup and linear cycle check before doing them.
            // Chasing does not add result nodes or native recursion frames.
            budget.charge_work(1 + substituting.len() as u64)?;
            let Some(bound) = self.bindings.get(name) else {
                break;
            };
            if substituting.iter().any(|open| open == name) {
                break;
            }
            substituting.push(name.clone());
            ty = bound;
        }
        Ok(ty)
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
        self.unify_argument_as(param_ty, arg_ty, types, false)
    }

    /// [`Self::unify_argument`] for an argument that is an object or array
    /// literal, whose type joins the other literals' candidates.
    #[allow(clippy::result_large_err)]
    pub(crate) fn unify_literal_argument(
        &mut self,
        param_ty: &Type,
        arg_ty: &Type,
        types: TypeResolver<'_>,
    ) -> Result<(), UnifyError> {
        self.unify_argument_as(param_ty, arg_ty, types, true)
    }

    #[allow(clippy::result_large_err)]
    fn unify_argument_as(
        &mut self,
        param_ty: &Type,
        arg_ty: &Type,
        types: TypeResolver<'_>,
        from_literal: bool,
    ) -> Result<(), UnifyError> {
        let mentions_literal_candidate = !from_literal
            && super::infer::expr::mentions_type_var(param_ty, &|name| {
                self.literal_candidates.contains(name)
            });
        // Marked before unifying, since a name the literal binds first
        // becomes a literal candidate only afterwards.
        if from_literal {
            let joined: Vec<String> = self
                .literal_candidates
                .iter()
                .filter(|name| {
                    super::infer::expr::mentions_type_var(param_ty, &|var| var == name.as_str())
                })
                .cloned()
                .collect();
            self.several_literal_candidates.extend(joined);
        }
        let bound_before: std::collections::BTreeSet<String> = self.bound_names().collect();
        let resolved = self.apply_or_record(param_ty, types.limits);
        let fits_binding = !super::infer::expr::type_contains_type_var(&resolved)
            && assignable(arg_ty, &resolved, types);
        // An argument that fits the expected result's binding still replaces
        // it, so the arguments, not the expected type, decide the inference.
        // So does one that fits only literals' candidates.
        if fits_binding
            && !self.mentions_replaceable_binding(param_ty)
            && !mentions_literal_candidate
        {
            return Ok(());
        }
        // Unless the argument gives no one binding (`[["a", 1], ["b", 2]]` is
        // a union of tuples); then the binding it fits stands, as tsc falls
        // back to the contextual type, and is final, since a later argument
        // narrowing it would no longer fit this one.
        let before = fits_binding.then(|| self.clone());
        let unified = self.keeping_close_matches_on_success(|sub| {
            let mut unifier = Unifier::new(sub, Some(types), types.limits);
            unifier.subtype_widening = true;
            unifier.is_argument = true;
            unifier.combines_literals = from_literal;
            unifier.unify(param_ty, arg_ty)
        });
        if from_literal {
            // Only a binding to the literal's own object or array type is a
            // literal candidate, as in tsc; `T` bound to `Mode` from `[m]`
            // against `T[]` is an ordinary one.
            let newly_bound: Vec<String> = self
                .bound_names()
                .filter(|name| !bound_before.contains(name))
                .filter(|name| {
                    self.bindings.get(name).is_some_and(|bound| {
                        matches!(
                            bound.peel(),
                            Type::Object { .. } | Type::Array(_) | Type::Tuple(_)
                        )
                    })
                })
                .collect();
            self.literal_candidates.extend(newly_bound);
        } else {
            self.literal_candidates.retain(|name| {
                !super::infer::expr::mentions_type_var(param_ty, &|var| var == name)
            });
        }
        if let (Err(_), Some(before)) = (&unified, before) {
            *self = before;
            self.keep_replaceable_bindings(param_ty);
            return Ok(());
        }
        unified
    }
}

/// A [`Unifier`]'s state before a speculative attempt.
struct Snapshot {
    bindings: BTreeMap<String, Type>,
    replaceable: std::collections::BTreeSet<String>,
    widenable: std::collections::BTreeSet<String>,
    narrowable: std::collections::BTreeSet<String>,
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
    /// Whether the argument is an object or array literal, which widens a
    /// binding other literals gave to the union of the two.
    combines_literals: bool,
    /// Whether the walk is inside a function type's result, where a
    /// candidate never takes over literal candidates (see `takes_over` in
    /// [`Self::unify`]).
    inside_function_result: bool,
    /// The object fields, outermost first, the walk is inside.
    field_path: Vec<String>,
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
        // A readonly array never stands for a mutable one; in a callback's
        // parameter the slot's array is the one passed to the callback.
        let drops_readonly = if self.contravariant {
            drops_readonly(param_ty, arg_ty)
        } else {
            drops_readonly(arg_ty, param_ty)
        };
        if drops_readonly {
            return Err(UnifyError::Mismatch {
                expected: param_ty.clone(),
                got: arg_ty.clone(),
            });
        }
        let unpeeled_arg = arg_ty;
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
                let from_callback_parameter = self.sub.narrowable.contains(name);
                let covariant = self.infers_from_covariant_argument();
                // The expected result's binding is only a hint, as tsc gives a
                // return type's inference the lowest priority: the first
                // argument replaces it as the first candidate, which later
                // ones widen, and the result is checked against the expected
                // type afterwards.
                let replaces_hint = replaceable && covariant;
                // A non-literal candidate wins over object and array literal
                // candidates, as in tsc: it takes the binding, and the
                // literals are checked against it once inference is done, the
                // name no longer a literal candidate. It doesn't take over
                // when it is:
                // - `null`, which only makes the binding nullable, or `never`,
                //   which adds nothing;
                // - holding a class instance, which no literal would fit, as
                //   classes are nominal;
                // - a callback's result, checked against the type parameters
                //   tsc fixed from the candidates before it for its parameters;
                // - against several literals, which tsc relates as their
                //   union rather than as the one type Submilli binds;
                // - a subtype of the literal's type, which tsc's common
                //   supertype keeps (see `is_subtype_of_literal`).
                let takes_over =
                    !matches!(arg_resolved.peel(), Type::Null | Type::Never | Type::Error)
                        && !holds_class_instance(&arg_resolved)
                        && !self.inside_function_result
                        && !self.sub.several_literal_candidates.contains(name)
                        && !self.is_subtype_of_literal(&arg_resolved, &resolved);
                let mut replaces_literals = false;
                if covariant && takes_over && !self.combines_literals {
                    replaces_literals = self.sub.literal_candidates.remove(name);
                }
                if replaces_hint || replaces_literals {
                    self.sub.bindings.insert(name.clone(), arg_ty.clone());
                    self.sub.widenable.insert(name.clone());
                    return Ok(());
                }
                if self.contravariant {
                    self.sub.widenable.remove(name);
                } else {
                    self.sub.narrowable.remove(name);
                }
                if self
                    .unify_bound_by_assignability(name, &resolved, &arg_resolved, arg_ty)
                    .is_some()
                {
                    return Ok(());
                }
                // Recurse instead of `==` to peel aliases at every level; remap to Conflict to pin the offending param.
                return match self.unify(&resolved, &arg_resolved) {
                    Ok(()) => Ok(()),
                    Err(_) if self.accepts_as_subtype(&arg_resolved, &resolved) => Ok(()),
                    Err(_) if self.accepts_as_supertype(&arg_resolved, &resolved) => Ok(()),
                    Err(_) if replaceable => {
                        self.sub.bindings.insert(name.clone(), arg_ty.clone());
                        Ok(())
                    }
                    // An argument that neither fits a candidate binding nor
                    // widens it is reported against it, as tsc reports it,
                    // and so is one that doesn't fit what a callback's
                    // parameter bound.
                    Err(_)
                        if self.subtype_widening
                            && (self.sub.widenable.contains(name) || from_callback_parameter) =>
                    {
                        Err(UnifyError::Mismatch {
                            expected: resolved,
                            got: arg_resolved,
                        })
                    }
                    Err(_) => Err(UnifyError::Conflict {
                        name: name.clone(),
                        prev: resolved,
                        new: arg_resolved,
                    }),
                };
            }
            self.sub.bindings.insert(name.clone(), arg_ty.clone());
            if self.infers_from_covariant_argument() {
                self.sub.widenable.insert(name.clone());
            } else if self.is_argument {
                self.sub.narrowable.insert(name.clone());
            }
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
                self.in_function_result(|u| u.unify(ra, rb))
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
                        self.in_field(name, |u| u.unify(&expected.ty, &actual.ty))?;
                    }
                    return Ok(());
                }
                if a.len() != b.len() || !a.keys().eq(b.keys()) {
                    // A shape with nothing left to infer is a plain assignability
                    // check, so a wider argument fits it: `Array.from(al)` with
                    // `al: { length: number; extra: number }`.
                    if self.accepts_as_concrete_supertype_of(arg_ty, param_ty) {
                        return Ok(());
                    }
                    return Err(UnifyError::Mismatch {
                        expected: param_ty.clone(),
                        got: arg_ty.clone(),
                    });
                }
                for ((name, va), vb) in a.iter().zip(b.values()) {
                    if va.optional != vb.optional {
                        return Err(UnifyError::Mismatch {
                            expected: param_ty.clone(),
                            got: arg_ty.clone(),
                        });
                    }
                    self.in_field(name, |u| u.unify(&va.ty, &vb.ty))?;
                }
                Ok(())
            }
            (
                Type::InterfaceRef {
                    mangled: ma,
                    name,
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
                    name,
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
                let variances = self.types.map_or_else(
                    || vec![Variance::Covariant; aa.len()],
                    |types| types.variances_or_covariant(ma, name, aa.len()),
                );
                for ((a, b), variance) in aa.iter().zip(ab.iter()).zip(variances) {
                    // An argument the declaration only takes in (a callback
                    // field's parameter) infers as a function parameter would.
                    if variance == Variance::Contravariant {
                        self.in_function_parameter(|u| u.unify(a, b))?;
                    } else {
                        self.unify(a, b)?;
                    }
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
                // Every member fits a concrete sibling of type parameters
                // already bound, so the argument changes nothing: `2 | 3` for
                // `T | number` once `T` is bound, as tsc matches it.
                if self.fits_concrete_members(pa, pb) {
                    return Ok(());
                }
                if let Some(unified) = self.unify_union_into_lone_type_var(pa, pb) {
                    return unified;
                }
                if let Some(unified) = self.unify_union_into_lone_candidate(pa, pb) {
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
                for a in self.fallback_type_vars_last(pa) {
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
                // Each member sees the argument as written: a readonly array
                // must not fit a mutable member.
                let arg_ty = unpeeled_arg;
                // Bottom fits every arm. Give an unbound variable a chance to
                // infer from it before accepting an unrelated concrete arm.
                if matches!(arg_ty.peel(), Type::Never) && self.subtype_widening {
                    let snap = self.snapshot();
                    if self
                        .without_subtype_widening(|u| u.unify(param_ty, arg_ty))
                        .is_ok()
                    {
                        return Ok(());
                    }
                    self.restore(snap);
                }
                if self.infer_from_readonly_into_mutable_member(pa, arg_ty) {
                    return Ok(());
                }
                if self.unify_by_lone_type_var_rule(pa, arg_ty) {
                    return Ok(());
                }
                if self.defer_to_fitting_fallback(pa, arg_ty) {
                    return Ok(());
                }
                if self.infer_through_structured_member(pa, arg_ty) {
                    return Ok(());
                }
                if self.defer_to_identical_member(pa, arg_ty) {
                    return Ok(());
                }
                for m in self.fallback_type_vars_last(pa) {
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
            combines_literals: false,
            inside_function_result: false,
            field_path: Vec::new(),
        }
    }

    /// Whether `arg` is a subtype of the object or array literal type
    /// `literal` as tsc's subtype relation has it, which unlike assignability
    /// lets an object have no property the object literal lacks: a `Pt` is
    /// one of `{ x: number; y: number | null }`, but a `Dog` is not one of
    /// `{ name: string }`. The rule applies to an object literal itself
    /// only: its fields may hold values of declared types, which ordinary
    /// subtyping relates, so below the top level, and for an array literal,
    /// assignability decides.
    fn is_subtype_of_literal(&self, arg: &Type, literal: &Type) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        if !assignable(arg, literal, types) {
            return false;
        }
        match (object_fields(arg, types), object_fields(literal, types)) {
            (Some(arg_fields), Some(literal_fields)) => arg_fields
                .keys()
                .all(|name| literal_fields.contains_key(name)),
            _ => true,
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

    /// What `name`'s binding widens to for a covariant argument it doesn't
    /// take, as tsc infers the common supertype of an argument's candidates:
    /// the argument's own type when it is a supertype of the binding, and
    /// with `null` set aside and added back otherwise (`pick(1, null)` is a
    /// `number | null`). `None` when the binding is not a candidate an
    /// argument may widen, or the two have no common supertype.
    fn widened_binding(
        &self,
        name: &str,
        arg: &Type,
        bound: &Type,
        unresolved_arg: &Type,
    ) -> Option<Type> {
        let types = self.types?;
        if !self.subtype_widening || !self.sub.widenable.contains(name) {
            return None;
        }
        if assignable(bound, arg, types) {
            return Some(unresolved_arg.clone());
        }
        if self.combines_literals && self.sub.literal_candidates.contains(name) {
            return Some(Type::union(vec![bound.clone(), unresolved_arg.clone()]));
        }
        // Literal candidates of one primitive form their union, as tsc infers
        // where they are kept: `pick(1, 2)` is a `1 | 2`.
        if bound.literal_base().is_some() && bound.literal_base() == arg.literal_base() {
            return Some(Type::union(vec![bound.clone(), arg.clone()]));
        }
        // tsc sets a `null` candidate aside and adds it back to the common
        // supertype of the rest; `None` stands for a candidate that is just
        // `null`.
        let non_null = |ty: &Type| {
            (!matches!(ty.peel(), Type::Null)).then(|| super::infer::narrowing::strip_null(ty))
        };
        let involves_null = |ty: &Type| non_null(ty).as_ref() != Some(ty);
        if !involves_null(bound) && !involves_null(arg) {
            return None;
        }
        let supertype = match (non_null(bound), non_null(arg)) {
            (None, None) => return None,
            (Some(only), None) | (None, Some(only)) => only,
            (Some(bound), Some(arg)) if assignable(&arg, &bound, types) => bound,
            (Some(bound), Some(arg)) if assignable(&bound, &arg, types) => arg,
            _ => return None,
        };
        Some(Type::union(vec![supertype, Type::Null]))
    }

    /// An argument against the type parameter `name` once it is bound to
    /// `bound`, both fully known, decided by assignability as tsc checks
    /// arguments: a covariant argument fits a supertype binding, or widens a
    /// candidate binding to its own type; a callback's parameter must accept
    /// the binding, or narrows a binding every use of which only passes values
    /// in. `None` when neither holds, or outside an argument, for structural
    /// unification to decide.
    fn unify_bound_by_assignability(
        &mut self,
        name: &str,
        bound: &Type,
        arg: &Type,
        unresolved_arg: &Type,
    ) -> Option<()> {
        let types = self.types?;
        if !self.is_argument
            || super::infer::expr::type_contains_type_var(bound)
            || super::infer::expr::type_contains_type_var(arg)
        {
            return None;
        }
        let fits = if self.contravariant {
            assignable(bound, arg, types)
        } else {
            self.subtype_widening && assignable(arg, bound, types)
        };
        if fits {
            return Some(());
        }
        let rebound = if self.contravariant {
            self.narrows_to_subtype(name, arg, bound)
                .then(|| unresolved_arg.clone())
        } else {
            self.widened_binding(name, arg, bound, unresolved_arg)
        }?;
        self.sub.bindings.insert(name.to_string(), rebound);
        Some(())
    }

    /// Whether a callback's parameter that failed to unify with `name`'s
    /// binding is a subtype of it that the binding may narrow to: every use
    /// of the binding so far only passes values in, so a narrower one still
    /// fits them all.
    fn narrows_to_subtype(&self, name: &str, arg: &Type, bound: &Type) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        self.is_argument
            && self.contravariant
            && self.sub.narrowable.contains(name)
            && assignable(arg, bound, types)
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
        self.in_function_result(|u| u.unify(ret, arg_ret))
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

    /// Whether `param`, with no type variable left to bind, accepts `arg` by
    /// assignability.
    fn accepts_as_concrete_supertype_of(&self, arg: &Type, param: &Type) -> bool {
        let known_param = self.sub.apply_or_record(param, self.limits);
        !super::infer::expr::type_contains_type_var(&known_param)
            && self.accepts_as_subtype(arg, &known_param)
    }

    /// Unify within a function type's result.
    #[allow(clippy::result_large_err)]
    fn in_function_result(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<(), UnifyError>,
    ) -> Result<(), UnifyError> {
        let outer = std::mem::replace(&mut self.inside_function_result, true);
        let out = f(self);
        self.inside_function_result = outer;
        out
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

    /// Whether every member of the argument union `args` is assignable to a
    /// member of `params` that names no type parameter, while each member
    /// that names one is bound already.
    fn fits_concrete_members(&mut self, params: &[Type], args: &[Type]) -> bool {
        let Some(types) = self.types else {
            return false;
        };
        let (generic, concrete): (Vec<&Type>, Vec<&Type>) = params
            .iter()
            .partition(|member| super::infer::expr::type_contains_type_var(member));
        let generic_bound = generic.iter().all(|member| {
            let resolved = self.sub.apply_or_record(member, self.limits);
            !super::infer::expr::type_contains_type_var(&resolved)
        });
        !generic.is_empty()
            && generic_bound
            && args
                .iter()
                .all(|arg| concrete.iter().any(|member| assignable(arg, member, types)))
    }

    /// A union argument against a union whose only type parameter is bound to
    /// a candidate a later argument may still widen, or to the expected
    /// result's hint an argument replaces: the members that fit
    /// none of the concrete siblings are one more candidate, as tsc infers
    /// them. `orDefault(5, pick)` with `orDefault<T>(fallback: T, value: T | null)`
    /// and `pick: 1 | 2` widens `T` to `number`
    /// from `1 | 2`, rather than pairing `2` with `null`. `None` when the
    /// parameter has another shape or every member fits a sibling.
    #[allow(clippy::result_large_err)]
    fn unify_union_into_lone_candidate(
        &mut self,
        params: &[Type],
        args: &[Type],
    ) -> Option<Result<(), UnifyError>> {
        let (type_vars, others): (Vec<&Type>, Vec<&Type>) = params
            .iter()
            .partition(|member| super::infer::expr::type_contains_type_var(member));
        let [type_var] = type_vars[..] else {
            return None;
        };
        let Type::TypeVar(name) = type_var.peel() else {
            return None;
        };
        let replaceable = self.sub.replaceable.contains(name) && self.is_argument;
        if !self.sub.is_candidate_binding(name) && !replaceable {
            return None;
        }
        let rest: Vec<Type> = args
            .iter()
            .filter(|arg| !others.iter().any(|other| self.would_unify(other, arg)))
            .cloned()
            .collect();
        if rest.is_empty() {
            return None;
        }
        Some(self.unify(type_var, &Type::union(rest)))
    }

    /// Defer `arg` when it fits the whole-union fallback of a member that is
    /// an unbound type parameter: tsc takes it as one more candidate of the
    /// fallback's priority, which the fallback already covers, so it binds
    /// nothing now and is checked against `params` once inference is done.
    /// Returns whether it deferred `arg`.
    fn defer_to_fitting_fallback(&mut self, params: &[Type], arg: &Type) -> bool {
        if !self.infers_from_covariant_argument() {
            return false;
        }
        let fallbacks: Vec<(&Type, Type)> = params
            .iter()
            .filter_map(|member| Some((member, self.unbound_fallback_of(member)?.clone())))
            .collect();
        let fitting = fallbacks
            .into_iter()
            .find(|(_, fallback)| self.would_unify(fallback, arg));
        let Some((type_var, _)) = fitting else {
            return false;
        };
        self.record_close_match(params, type_var, arg);
        self.offer_to_type_vars_without_fallback(params, arg);
        true
    }

    /// Offer `arg` as the fallback of each unbound type parameter in
    /// `params` without one of its own: tsc gives it the same low-priority
    /// candidate, so a later argument still binds it ahead of `arg`.
    fn offer_to_type_vars_without_fallback(&mut self, params: &[Type], arg: &Type) {
        // Collected first: offering needs `self` mutably.
        let without_fallback: Vec<&Type> = params
            .iter()
            .filter(|member| self.is_unbound_type_var_without_fallback(member))
            .collect();
        for type_var in without_fallback {
            self.offer_whole_union_fallback(type_var, arg.clone());
        }
    }

    /// `members` in order to try an argument against, those that are an
    /// unbound type parameter with a whole-union fallback last: tsc's first
    /// candidate for it is the fallback, so a later argument goes to another
    /// member that takes it first.
    fn fallback_type_vars_last<'m>(&self, members: &'m [Type]) -> Vec<&'m Type> {
        if !self.infers_from_covariant_argument() {
            return members.iter().collect();
        }
        let (last, first): (Vec<&Type>, Vec<&Type>) = members
            .iter()
            .partition(|member| self.is_unbound_fallback_type_var(member));
        first.into_iter().chain(last).collect()
    }

    /// The one member of `params` that is a type parameter not yet bound,
    /// and the other members; None when there is no such member or more
    /// than one. In an argument, unbound type parameters with a whole-union
    /// fallback are left out when another one is unbound: tsc's first
    /// candidate for them is their fallback, so the other one takes what
    /// the remaining members leave.
    fn split_lone_unbound_type_var<'p>(
        &self,
        params: &'p [Type],
    ) -> Option<(&'p Type, Vec<&'p Type>)> {
        let unbound: Vec<&Type> = params
            .iter()
            .filter(|member| self.is_unbound_type_var(member))
            .collect();
        let candidates: Vec<&Type> = if unbound.len() > 1 && self.infers_from_covariant_argument() {
            unbound
                .into_iter()
                .filter(|member| self.is_unbound_type_var_without_fallback(member))
                .collect()
        } else {
            unbound
        };
        let [type_var] = candidates[..] else {
            return None;
        };
        let others = params
            .iter()
            .filter(|member| {
                !std::ptr::eq(*member, type_var) && !self.is_unbound_fallback_type_var(member)
            })
            .collect();
        Some((type_var, others))
    }

    /// tsc's rule for a non-union argument against a union parameter whose
    /// members include exactly one unbound type parameter. Returns true when
    /// it unified `arg`, false when the caller should try each member in
    /// order. It unifies `arg` when either:
    /// - the type parameter has a fallback and another member takes `arg`
    ///   (see [`Self::unify_with_member_beside_fallback`]); or
    /// - `arg` [`closely_matches`] another member without unifying with any,
    ///   as `Box<boolean>` does `Box<number>` in `T | Box<number>`. It is then
    ///   treated like a union argument whose members all closely matched:
    ///   recorded as a close match, checked once inference is done, and
    ///   offered as the type parameter's fallback.
    fn unify_by_lone_type_var_rule(&mut self, params: &[Type], arg: &Type) -> bool {
        if !self.infers_from_covariant_argument() {
            return false;
        }
        let Some((type_var, others)) = self.split_lone_unbound_type_var(params) else {
            return false;
        };
        if self.unify_with_member_beside_fallback(type_var, &others, arg) {
            return true;
        }
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

    /// tsc infers from an argument to a union parameter's members that name
    /// a type parameter inside them before the bare type parameter, which
    /// takes the whole argument only at a lower priority: `{ v: m }` for
    /// `A | Box<A>` binds `A` to `m`'s type. Returns whether such a member
    /// took the argument and bound the lone unbound type parameter.
    fn infer_through_structured_member(&mut self, params: &[Type], arg: &Type) -> bool {
        if !self.infers_from_covariant_argument() {
            return false;
        }
        let Some((Type::TypeVar(name), others)) = self
            .split_lone_unbound_type_var(params)
            .map(|(type_var, others)| (type_var.peel().clone(), others))
        else {
            return false;
        };
        for other in others {
            if !super::infer::expr::mentions_type_var(other, &|var| var == name) {
                continue;
            }
            let snap = self.snapshot();
            if self.unify(other, arg).is_ok() && !self.sub.is_unbound(&name) {
                return true;
            }
            self.restore(snap);
        }
        false
    }

    /// An argument identical to a member of its union parameter that names
    /// no type parameter gives the unbound bare type parameters beside it
    /// only a whole-union fallback, as tsc gives a naked type parameter the
    /// lowest priority: `f(new Box(1), "s")` with `a: T | Box<number>` and
    /// `c: T` binds `T` to `string`, and `h("x")` with `a: T | U | "x"`
    /// binds both to `"x"`.
    fn defer_to_identical_member(&mut self, params: &[Type], arg: &Type) -> bool {
        if !self.infers_from_covariant_argument() {
            return false;
        }
        let identical = params.iter().any(|member| {
            !super::infer::expr::type_contains_type_var(member) && member.peel() == arg.peel()
        });
        if !identical || !params.iter().any(|member| self.is_unbound_type_var(member)) {
            return false;
        }
        self.offer_to_type_vars_without_fallback(params, arg);
        true
    }

    /// Unify `arg` with each of `others`, keeping the bindings of those that
    /// take it, when `type_var` has a whole-union fallback; returns whether
    /// one took it.
    /// tsc counts such an argument at most as another candidate of the
    /// fallback's priority, so it must not bind the type parameter ahead of
    /// the fallback.
    fn unify_with_member_beside_fallback(
        &mut self,
        type_var: &Type,
        others: &[&Type],
        arg: &Type,
    ) -> bool {
        if !self.is_unbound_fallback_type_var(type_var) {
            return false;
        }
        // Every member infers from it, as in tsc: `Box<number>` for
        // `T | Box<number> | Box<U>` binds `U` though the first member takes
        // it as is.
        let mut took = false;
        for other in others {
            took |= self.unifies_or_rolls_back(other, arg);
        }
        took
    }

    /// tsc infers from a readonly array argument through a mutable array
    /// member, as `readonly Mode[]` binds `T` to `Mode` in `T | T[]`, and then
    /// rejects the argument, which no member takes. Infer the same way and
    /// leave the argument to be reported once inference is done.
    fn infer_from_readonly_into_mutable_member(&mut self, params: &[Type], arg: &Type) -> bool {
        if !self.infers_from_covariant_argument() || !arg.is_readonly_array() {
            return false;
        }
        let Some(member) = params.iter().find(|member| {
            matches!(member.peel(), Type::Array(_) | Type::Tuple(_))
                && !member.is_readonly_array()
                && super::infer::expr::type_contains_type_var(member)
        }) else {
            return false;
        };
        if !self.unifies_or_rolls_back(member, arg.peel()) {
            return false;
        }
        self.record_close_match(params, member, arg);
        true
    }

    /// Record that `arg` must fit the union parameter `params` once
    /// inference is done, to be reported against `matched_member` if not.
    fn record_close_match(&mut self, params: &[Type], matched_member: &Type, arg: &Type) {
        self.sub.close_matches.push(CloseMatch {
            param: Type::union(params.to_vec()),
            matched_member: matched_member.clone(),
            arg: arg.clone(),
            field_path: self.field_path.clone(),
            argument_span: None,
        });
    }

    fn in_field<R>(&mut self, name: &str, walk: impl FnOnce(&mut Self) -> R) -> R {
        self.field_path.push(name.to_string());
        let walked = walk(self);
        self.field_path.pop();
        walked
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

    /// Whether `ty` is a type parameter with no binding yet that a whole
    /// union argument stands in for.
    fn is_unbound_fallback_type_var(&self, ty: &Type) -> bool {
        self.unbound_fallback_of(ty).is_some()
    }

    /// The whole-union fallback of `ty` when it is a type parameter with no
    /// binding yet.
    fn unbound_fallback_of(&self, ty: &Type) -> Option<&Type> {
        if !self.is_unbound_type_var(ty) {
            return None;
        }
        match ty.peel() {
            Type::TypeVar(name) => self.sub.whole_union_fallback(name),
            _ => None,
        }
    }

    /// Whether `ty` is a type parameter with no binding yet and no
    /// whole-union fallback.
    fn is_unbound_type_var_without_fallback(&self, ty: &Type) -> bool {
        self.is_unbound_type_var(ty) && !self.is_unbound_fallback_type_var(ty)
    }

    /// Whether the unifier is inferring from a call argument outside any
    /// callback parameter, where tsc's candidate priorities apply.
    fn infers_from_covariant_argument(&self) -> bool {
        self.is_argument && !self.contravariant
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
            widenable: self.sub.widenable.clone(),
            narrowable: self.sub.narrowable.clone(),
            whole_union_fallbacks: self.sub.whole_union_fallbacks.clone(),
            close_matches_len: self.sub.close_matches.len(),
            assumed_len: self.assumed_pairs.len(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.sub.bindings = snapshot.bindings;
        self.sub.replaceable = snapshot.replaceable;
        self.sub.widenable = snapshot.widenable;
        self.sub.narrowable = snapshot.narrowable;
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
        Type::NumberLiteral(_)
            | Type::StringLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
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

/// The fields of `ty` when it is an object type or a data-only interface.
fn object_fields(
    ty: &Type,
    types: TypeResolver<'_>,
) -> Option<BTreeMap<String, crate::ObjectField>> {
    match expand_interface_data_shape(ty, types) {
        Some(Type::Object { fields, .. }) => Some(fields),
        _ => match ty.peel() {
            Type::Object { fields, .. } => Some(fields.clone()),
            _ => None,
        },
    }
}

/// Whether a value of type `ty` is, or structurally holds, a class instance:
/// in an element, a field, a union member or a function's result.
fn holds_class_instance(ty: &Type) -> bool {
    match ty {
        Type::ClassRef { .. } => true,
        Type::Array(elem) | Type::Readonly(elem) => holds_class_instance(elem),
        Type::Tuple(elems) | Type::Union(elems) => elems.iter().any(holds_class_instance),
        Type::Function { ret, .. } => holds_class_instance(ret),
        Type::Object { fields, index } => {
            index
                .as_ref()
                .is_some_and(|i| holds_class_instance(&i.value))
                || fields.values().any(|f| holds_class_instance(&f.ty))
        }
        Type::Alias { ty: inner, .. } => holds_class_instance(inner),
        _ => false,
    }
}

/// Set `map`'s entry for `name` to what `before` held for it.
fn restore_entry(map: &mut BTreeMap<String, Type>, before: &BTreeMap<String, Type>, name: &str) {
    match before.get(name) {
        Some(ty) => map.insert(name.to_string(), ty.clone()),
        None => map.remove(name),
    };
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
    fn binding_chases_spend_work_before_following_the_chain() {
        let mut substitution = TypeParamSubstitution::new();
        substitution.insert("T".into(), t("U"));
        substitution.insert("U".into(), Type::Number);
        // Two lookups, a cycle comparison, and one output node cost four.
        assert_eq!(
            substitution.apply(&t("T"), &TypeLimits::with_work_allowance(3)),
            Err(TypeTooLarge::Work)
        );
        assert_eq!(
            substitution.apply(&t("T"), &TypeLimits::with_work_allowance(4)),
            Ok(Type::Number)
        );
        let limits = TypeLimits::with_work_allowance(1);
        assert_eq!(substitution.apply_or_record(&t("T"), &limits), Type::Error);
        assert_eq!(limits.take(), Err(TypeTooLarge::Work));
        assert_eq!(
            substitution.apply(&t("T"), &TypeLimits::default()),
            Ok(Type::Number)
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
