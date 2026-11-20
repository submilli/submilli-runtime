//! User-defined type guard machinery.

use crate::compiler_error::CompilerFailure;

use crate::{ExprId, Param, Type, TypedExpr, types::ObjectField};

use super::{Inferer, assignable, narrowing};

fn compose_param_narrowing(
    env: &narrowing::NarrowEnv,
    path: &narrowing::ReferencePath,
    param_ty: &Type,
) -> Type {
    let base = env
        .get(path)
        .map_or_else(|| param_ty.clone(), |view| view.narrowed_ty.clone());
    let Type::Object {
        fields,
        index,
        more_indexes,
    } = base.peel().clone()
    else {
        return base;
    };
    let mut refined = fields;
    for (entry_path, view) in env {
        if entry_path.root != path.root {
            continue;
        }
        if entry_path.chain.len() != 1 {
            continue;
        }
        let narrowing::PathElem::Field(field_name) = &entry_path.chain[0] else {
            continue;
        };
        match refined.get_mut(field_name) {
            Some(existing) => existing.ty = view.narrowed_ty.clone(),
            None => {
                refined.insert(
                    field_name.clone(),
                    ObjectField::required(view.narrowed_ty.clone()),
                );
            }
        }
    }
    Type::Object {
        more_indexes,
        index,
        fields: refined,
    }
}

impl Inferer<'_> {
    pub(super) fn resolve_type_predicate(
        &mut self,
        pred: &crate::TypePredicateAnnotation,
        params: &[Param],
    ) -> Result<Option<crate::TypePredicate>, CompilerFailure> {
        let idx = params.iter().position(|p| p.name == pred.param.name);
        let Some(idx) = idx else {
            self.error(
                pred.param.span,
                format!(
                    "type predicate references unknown parameter `{}`",
                    pred.param.name,
                ),
            );
            return Ok(None);
        };
        // `asserts x` asserts no type, only that the argument is truthy.
        let asserted_type = match &pred.asserted {
            Some(asserted) => {
                let asserted_type = self.resolve_type(asserted)?;
                if matches!(asserted_type, Type::Error) {
                    return Ok(None);
                }
                self.check_predicate_fits_parameter(asserted, &asserted_type, &params[idx].ty);
                asserted_type
            }
            None => Type::Unknown,
        };
        Ok(Some(crate::TypePredicate {
            parameter_index: idx as u32,
            asserted_type,
            kind: pred.kind(),
        }))
    }

    /// A predicate's type must be assignable to its parameter's type, as tsc
    /// requires: `x is boolean` for `x: string | number` can never hold, and
    /// an array `filter` with it would narrow to elements it never keeps. A
    /// type parameter on either side is left to the call that binds it.
    fn check_predicate_fits_parameter(
        &mut self,
        annotation: &crate::TypeAnnotation,
        asserted: &Type,
        param: &Type,
    ) {
        if matches!(param.peel(), Type::Error)
            || super::expr::type_contains_type_var(param)
            || super::expr::type_contains_type_var(asserted)
            || super::assignable(asserted, param, self.resolver())
        {
            return;
        }
        self.error(
            annotation.span,
            format!("a type predicate's type must be assignable to its parameter's type: `{asserted}` is not a `{param}`"),
        );
    }

    pub(super) fn predicate_envs_user_guard(
        &mut self,
        predicate: &crate::TypePredicate,
        args: &[ExprId],
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        let idx = predicate.parameter_index as usize;
        let arg_id = *match args.get(idx) {
            Some(value) => value,
            None => return Ok(None),
        };
        self.narrow_path_to_asserted(arg_id, &predicate.asserted_type)
    }

    /// Build the (true, false) narrow envs for refining the binding referenced by `target`
    /// to `asserted`. Shared by user-defined `s is T` guards and `x instanceof Foo`: the
    /// true branch keeps the union members assignable to `asserted` (collapsing to
    /// `asserted` for a proven subtype), the false branch drops them. Returns empty envs
    /// when `target` isn't a narrowable reference path, and a `Type::Error` narrowed type
    /// (skipped by `wrap_narrow_regions`) when the predicate eliminates every member.
    pub(super) fn narrow_path_to_asserted(
        &mut self,
        target: ExprId,
        asserted: &Type,
    ) -> Result<
        Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>,
        crate::compiler_error::CompilerFailure,
    > {
        let arg_expr = self
            .typed_ast
            .try_expr(target)
            .map_err(crate::typechecker::arena_failure)?;
        if matches!(arg_expr.kind, crate::TypedExprKind::OptionalChain { .. }) {
            return self.narrow_optional_chain_to_asserted(target, asserted);
        }
        let Some(path) = self.expr_to_reference_path(arg_expr)? else {
            return Ok(None);
        };
        if self.path_root_is_captured_mutator(&path) {
            return Ok(Some((
                narrowing::NarrowEnv::new(),
                narrowing::NarrowEnv::new(),
            )));
        }
        let from_ty = self.narrowing_source_ty(arg_expr)?;
        let arg_span = arg_expr.span;
        let fallback_kind = arg_expr.kind.clone();

        // prefer un-narrowed source so NarrowRegion materialization doesn't depend on a chain-only shadow binding.
        let arg_kind = self
            .synthesize_unnarrowed_source(&path, arg_span)?
            .unwrap_or(fallback_kind);

        // Intersect to keep element-type precision: Array.isArray(x: number[] | string) → number[], not Array<unknown>.
        let true_ty = self.intersect_asserted(&from_ty, asserted);
        let false_ty = self.subtract_asserted(&from_ty, asserted);

        let source_true = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: arg_kind.clone(),
                span: arg_span,
                ty: from_ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let source_false = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: arg_kind,
                span: arg_span,
                ty: from_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        // Either way the value keeps the literals it was already known not to be.
        let exclusions = self.known_exclusions(&path);
        let mut true_env = narrowing::NarrowEnv::new();
        let mut false_env = narrowing::NarrowEnv::new();
        true_env.insert(
            path.clone(),
            narrowing::NarrowedView {
                evolving: None,
                narrowed_ty: true_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: exclusions.clone(),
                binding: self.mint_narrow_binding(arg_span)?,
                source: source_true,
            },
        );
        false_env.insert(
            path,
            narrowing::NarrowedView {
                evolving: None,
                narrowed_ty: false_ty,
                facts: narrowing::TypeFacts::EMPTY,
                excluded_literals: exclusions,
                binding: self.mint_narrow_binding(arg_span)?,
                source: source_false,
            },
        );
        Ok(Some((true_env, false_env)))
    }

    fn intersect_asserted(&self, ty: &Type, asserted: &Type) -> Type {
        if let Type::Refined {
            original,
            ty: shape,
        } = ty.without_aliases()
        {
            return narrowing::preserve_refinement(
                original,
                self.intersect_asserted(shape, asserted),
            );
        }
        match ty.peel() {
            Type::Union(members) => Type::union(
                members
                    .iter()
                    .map(|member| self.intersect_asserted(member, asserted))
                    .filter(|ty| !matches!(ty, Type::Error))
                    .collect(),
            ),
            // A type parameter, or a form over one (`Readonly<T>`), narrows by
            // refinement: `T & C`.
            erased if erased.is_type_operator() || matches!(erased, Type::GenericParam { .. }) => {
                Type::Refined {
                    original: Box::new(ty.clone()),
                    ty: Box::new(asserted.clone()),
                }
            }
            _ if assignable(ty, asserted, self.resolver()) => ty.clone(),
            // A readonly array is still an array at runtime, so a guard for `T[]`
            // (`Array.isArray`) keeps it, readonly. The false branch keeps it too,
            // as `tsc` does: it is not assignable to the asserted mutable type.
            _ if assignable::drops_readonly(ty, asserted)
                && assignable(
                    ty,
                    &Type::Readonly(Box::new(asserted.peel().clone())),
                    self.resolver(),
                ) =>
            {
                ty.clone()
            }
            _ if assignable(asserted, ty, self.resolver()) => asserted.clone(),
            _ => Type::Error,
        }
    }

    fn subtract_asserted(&self, ty: &Type, asserted: &Type) -> Type {
        if let Type::Refined {
            original,
            ty: shape,
        } = ty.without_aliases()
        {
            return narrowing::preserve_refinement(
                original,
                self.subtract_asserted(shape, asserted),
            );
        }
        match ty.peel() {
            Type::Union(members) => Type::union(
                members
                    .iter()
                    .filter(|member| !assignable(member, asserted, self.resolver()))
                    .cloned()
                    .collect(),
            ),
            shape if assignable(shape, asserted, self.resolver()) => Type::Error,
            _ => ty.clone(),
        }
    }

    /// Checks that a type guard's returned value proves its predicate, and
    /// returns what to return in its place: the value itself or, for a
    /// [`GuardFacts::plain_object_gap`], the value with the rest of the predicate checked at
    /// run time.
    pub(super) fn check_type_guard_return(
        &mut self,
        value_id: ExprId,
        span: crate::Span,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        let Some((predicate, param)) = self.current_type_predicate.clone() else {
            return Ok(value_id);
        };
        // As in tsc, an assertion's body is not checked to assert: it returns
        // `void`, and a call that returns at all is taken to prove it.
        if predicate.kind.is_assertion() {
            return Ok(value_id);
        }
        let Some(scope_entry) = self.scopes.get(&param.name).cloned() else {
            return Ok(value_id);
        };
        // x is T only attaches on true returns; return false makes no narrowing claim.
        if matches!(
            self.typed_ast
                .try_expr(value_id)
                .map_err(crate::typechecker::arena_failure)?
                .kind,
            crate::TypedExprKind::Boolean(false)
        ) {
            return Ok(value_id);
        }
        // A local of the same name would be what the body's checks narrow, not
        // the argument the caller's narrowing applies to.
        if scope_entry.decl_span != param.span {
            let name = &param.name;
            self.error_with_help(
                span,
                format!("a local `{name}` shadows the type guard's parameter at this return"),
                vec![format!(
                    "rename the local, so the checks this return relies on are about the \
                     parameter `{name}`"
                )],
            );
            return Ok(value_id);
        }
        // Likewise once the body writes the parameter, directly or from a
        // closure: its checks are then about the written value.
        if self.last_assignments.contains_key(&param.span)
            || self
                .captured_mutators
                .contains(&(param.name.clone(), param.span))
        {
            let name = &param.name;
            self.error_with_help(
                span,
                format!("the type guard assigns its parameter `{name}`"),
                vec![format!(
                    "a guard's checks must be about the argument it was called with: copy \
                     `{name}` into a new local instead of assigning it"
                )],
            );
            return Ok(value_id);
        }
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
            name: param.name.clone(),
            decl_scope: scope_entry.decl_scope,
        });
        let (return_true_env, _return_false_env) = self.predicate_envs(value_id)?;
        // Merge enclosing if-branch narrowings (narrow_scopes) with the return expr's env;
        // outer-frames-first so the return value's env is the final (innermost) override.
        let mut combined_env = narrowing::NarrowEnv::new();
        for frame in &self.narrow_scopes {
            for (p, v) in frame {
                combined_env.insert(p.clone(), v.clone());
            }
        }
        for (p, v) in return_true_env {
            combined_env.insert(p, v);
        }
        // Compose root + field-path narrowings: in `if (x !== null && "f" in x && typeof x.f === "T")`,
        // neither x→Object{f:unknown} nor x.f→T alone matches the asserted shape, but together they do.
        let narrowed_ty = compose_param_narrowing(&combined_env, &path, &scope_entry.ty);
        // Substitute TypeVars → body GenericParams; without this, assignable treats TypeVar as a wildcard
        // and generic guards like `f<T>(x: T | null): x is T` slip through validation.
        let asserted_body = match self.body_instantiations.last() {
            Some(frame) if !frame.is_empty() => super::generic::substitute_typevars(
                &predicate.asserted_type,
                frame,
                &self.type_limits,
            )
            .map_err(super::type_limit_at(span))?,
            _ => predicate.asserted_type.clone(),
        };
        if assignable(&narrowed_ty, &asserted_body, self.resolver()) {
            return Ok(value_id);
        }
        let declared = scope_entry.declared_ty.as_ref().unwrap_or(&scope_entry.ty);
        let root_ty = combined_env
            .get(&path)
            .map_or(declared, |view| &view.narrowed_ty);
        let facts = GuardFacts {
            declared_untyped: matches!(declared.peel(), Type::Unknown)
                || matches!(
                    narrowing::strip_nullish(declared).peel(),
                    Type::NonPrimitive
                ),
            unnarrowed: is_unnarrowed(root_ty),
            narrowed_to_object: is_object_narrowing(root_ty),
            narrowed_to_nullable_object: is_object_narrowing(&narrowing::strip_nullish(root_ty)),
            asserts_object: is_object_like(&asserted_body),
            refines: assignable(&asserted_body, &narrowed_ty, self.resolver()),
        };
        let uncheckable = if facts.plain_object_gap() {
            match self.as_check_shape(&asserted_body) {
                Ok(shape) => {
                    return self.checked_guard_return(
                        value_id,
                        &param,
                        &scope_entry.ty,
                        &asserted_body,
                        shape,
                        span,
                    );
                }
                Err(reason) => Some(reason),
            }
        } else {
            None
        };
        let name = &param.name;
        let asserted = &predicate.asserted_type;
        self.error_with_help(
            span,
            format!("type-guard return value does not narrow parameter `{name}` to `{asserted}`"),
            vec![
                format!("at this return, `{name}` is inferred as `{narrowed_ty}`"),
                format!(
                    "Submilli checks that a type guard's body proves its predicate (tsc trusts \
                     it): a `return` that can be true must narrow `{name}` to `{asserted}`"
                ),
                type_guard_hint(name, asserted, &narrowed_ty, &facts, uncheckable),
            ],
        );
        Ok(value_id)
    }

    /// `value ? (param as asserted, true) : false`: a guard whose body narrowed
    /// an untyped parameter only to `object` proves the rest when it returns
    /// true, the way `param as asserted` would, throwing a `TypeError` on a
    /// mismatch (which a `try` around the `return` in the guard itself catches).
    /// Plain data then behaves as in tsc, and anything else (a `Map`, a missing
    /// field) fails loudly instead of narrowing.
    fn checked_guard_return(
        &mut self,
        value_id: ExprId,
        param: &crate::Ident,
        param_ty: &Type,
        asserted: &Type,
        shape: Type,
        span: crate::Span,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        let read = crate::TypedExprKind::LocalRef {
            ident: param.clone(),
            boxed: false,
        };
        let (kind, ty) = self.checked_call(read, param_ty.clone(), asserted, shape, span)?;
        let cast = self.push_guard_expr(kind, ty, span)?;
        let yes = self.push_guard_expr(crate::TypedExprKind::Boolean(true), Type::Boolean, span)?;
        let checked = self.push_guard_expr(
            crate::TypedExprKind::EffectThen {
                effect: cast,
                result: yes,
            },
            Type::Boolean,
            span,
        )?;
        let no = self.push_guard_expr(crate::TypedExprKind::Boolean(false), Type::Boolean, span)?;
        self.push_guard_expr(
            crate::TypedExprKind::Ternary {
                cond: value_id,
                then_: checked,
                else_: no,
            },
            Type::Boolean,
            span,
        )
    }

    fn push_guard_expr(
        &mut self,
        kind: crate::TypedExprKind,
        ty: Type,
        span: crate::Span,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        self.typed_ast
            .try_push_expr(TypedExpr { kind, span, ty })
            .map_err(crate::typechecker::arena_failure)
    }
}

/// What a type guard's failing `return` tells about its parameter and
/// predicate, which decides between a run-time check and the error's hint.
struct GuardFacts {
    /// The parameter takes untyped data: `unknown`, or `object` possibly with
    /// `null` or `undefined`.
    declared_untyped: bool,
    /// The checks reaching the return left it `unknown`, or ruled out only
    /// `null` and `undefined`.
    unnarrowed: bool,
    /// The body narrowed it to `object`, or to shapes `"f" in x` built on it.
    narrowed_to_object: bool,
    /// The same, except that it may still be `null` or `undefined`.
    narrowed_to_nullable_object: bool,
    /// The predicate is an object shape, record or interface, or a union of these.
    asserts_object: bool,
    /// The predicate fits inside what the body narrowed the parameter to.
    refines: bool,
}

impl GuardFacts {
    /// What's left to prove is that the value is plain data of the predicate's
    /// shape, which `typeof` can't tell and `as` checks at run time.
    fn plain_object_gap(&self) -> bool {
        self.declared_untyped && self.narrowed_to_object && self.asserts_object && self.refines
    }
}

/// The help line telling how to make a type guard's body prove its predicate.
/// `uncheckable` is why a plain-object gap's predicate can't be checked at run
/// time.
fn type_guard_hint(
    param: &str,
    asserted: &Type,
    narrowed: &Type,
    facts: &GuardFacts,
    uncheckable: Option<&str>,
) -> String {
    if super::expr::type_contains_type_var(asserted) {
        return format!(
            "`{asserted}` names a type parameter, which can't be checked at run time: the \
             body must prove it with checks on the value itself, e.g. `{param} !== null` for \
             a parameter typed `T | null`"
        );
    }
    if let Some(reason) = uncheckable {
        return format!(
            "`{asserted}` can't be checked at run time ({reason}), so the body must narrow \
             `{param}` to it with its own checks"
        );
    }
    if let Type::ClassRef { args, .. } = asserted.peel()
        && args.is_empty()
    {
        return format!("return `{param} instanceof {asserted}`");
    }
    if facts.declared_untyped && facts.asserts_object && facts.unnarrowed {
        return format!(
            "narrow `{param}` to an object with `typeof {param} === \"object\" && {param} !== \
             null` (add `&& !Array.isArray({param})` for a record), in the returned \
             expression or an enclosing `if`: narrowing doesn't flow through `? true : false` \
             or `Boolean(…)`. Submilli then checks the rest of `{asserted}` at run time when \
             the guard returns true, and throws a `TypeError` if it doesn't match"
        );
    }
    if facts.declared_untyped
        && facts.asserts_object
        && facts.refines
        && facts.narrowed_to_nullable_object
    {
        let checks = nullish_members(narrowed)
            .map(|nullish| format!("`{param} !== {nullish}`"))
            .collect::<Vec<_>>()
            .join(" and ");
        return format!(
            "rule out {checks} too; Submilli then checks the rest of `{asserted}` at run time \
             when the guard returns true"
        );
    }
    if !facts.refines {
        return format!(
            "the checks before this return leave `{param}` as `{narrowed}`, which \
             `{asserted}` doesn't fit inside: re-check them"
        );
    }
    if facts.asserts_object {
        return format!(
            "add checks that narrow `{param}` before returning true: a discriminant \
             comparison (`{param}.kind === \"…\"`), or `\"field\" in {param}` followed by a \
             `typeof` test on that field"
        );
    }
    format!(
        "add checks that narrow `{param}` before returning true, e.g. \
         `return typeof {param} === \"…\"` or `Array.isArray({param})`"
    )
}

/// `unknown`, or `{}` that may still be `null` or `undefined`: what is left
/// when the checks prove nothing about the value's kind.
fn is_unnarrowed(ty: &Type) -> bool {
    match narrowing::strip_nullish(ty).peel() {
        Type::Unknown => true,
        Type::Object {
            fields,
            index,
            more_indexes,
        } => fields.is_empty() && index.is_none() && more_indexes.is_empty(),
        _ => false,
    }
}

/// `object`, or a shape `"f" in x` built on it (or a union of these). An empty `{}` is not one: what
/// `x !== null && x !== undefined` leaves still holds primitives.
fn is_object_narrowing(ty: &Type) -> bool {
    match ty.peel() {
        Type::NonPrimitive => true,
        Type::Object { .. } => !is_unnarrowed(ty),
        Type::Union(members) => members.iter().all(is_object_narrowing),
        _ => false,
    }
}

/// `null` and `undefined`, those of them `ty` still holds.
fn nullish_members(ty: &Type) -> impl Iterator<Item = &'static str> + '_ {
    let members = match ty.peel() {
        Type::Union(members) => members.as_slice(),
        _ => std::slice::from_ref(ty),
    };
    [(Type::Null, "null"), (Type::Undefined, "undefined")]
        .into_iter()
        .filter(move |(nullish, _)| members.iter().any(|m| m.peel() == nullish))
        .map(|(_, name)| name)
}

/// An object shape, record or interface, or a union of only these.
fn is_object_like(ty: &Type) -> bool {
    match ty.peel() {
        Type::Object { .. } | Type::InterfaceRef { .. } => true,
        Type::Union(members) => members.iter().all(is_object_like),
        _ => false,
    }
}
