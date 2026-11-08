//! Upper bounds preserve a parameter's identity while exposing its members.

use std::collections::BTreeSet;

use crate::compiler_error::CompilerFailure;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{ExprId, GenericParameters, Span, Type, TypeKind, TypedExprKind};

use super::{Inferer, assignable, type_limit_at};

pub(in crate::typechecker) struct PendingConstraintCheck {
    pub ty: Type,
    pub span: Span,
    pub scopes: Vec<GenericParameters>,
}

impl Inferer<'_> {
    pub(super) fn apparent_type(&self, ty: &Type) -> Type {
        self.apparent_type_at_depth(ty, 0)
    }

    fn apparent_type_at_depth(&self, ty: &Type, depth: u32) -> Type {
        if !contains_body_parameter(ty) {
            return ty.clone();
        }
        if depth >= crate::compiler_limits::MAX_TYPE_RESOLUTION_DEPTH {
            self.type_limits
                .record(crate::type_size::TypeTooLarge::Depth);
            return Type::Error;
        }
        let mut current = ty;
        let mut visited = BTreeSet::new();
        while let Type::GenericParam { id, .. } = current.without_aliases() {
            if !self.type_limits.spend_work(1) {
                return Type::Error;
            }
            if !visited.insert(*id) {
                return Type::Error;
            }
            let Some(bound) = self.generic_constraints.get(id) else {
                break;
            };
            current = bound;
        }
        if let Type::Refined {
            original,
            ty: shape,
        } = current.without_aliases()
        {
            let bound = self.apparent_type_at_depth(original, depth + 1);
            if let Type::Union(members) = bound.peel() {
                let members: Vec<_> = members
                    .iter()
                    .filter(|member| assignable(member, shape, self.resolver()))
                    .cloned()
                    .collect();
                if !members.is_empty() {
                    return Type::union(members);
                }
            } else if assignable(&bound, shape, self.resolver()) {
                return bound;
            }
            return shape.as_ref().clone();
        }
        // A form reads through its base constraint: `p.a` on `Partial<T>`
        // with `T extends { a: number }` is `number | undefined` (tsc keeps
        // `T["a"] | undefined`).
        if current.without_aliases().is_type_operator() {
            return super::assignable::base_constraint(current, self.resolver());
        }
        if let Type::Union(members) = current.without_aliases() {
            let mut budget = self.type_limits.budget();
            let mut apparent_members = Vec::new();
            for member in members {
                let apparent = self.apparent_type_at_depth(member, depth + 1);
                if let Err(exceeded) = budget.charge_copy(&apparent, 1) {
                    self.type_limits.record(exceeded);
                    return Type::Error;
                }
                apparent_members.push(apparent);
            }
            return Type::union(apparent_members);
        }
        current.clone()
    }

    /// The static proof permits a representation-only cast. The original
    /// expression still has its erased slot, including for primitive bounds.
    pub(super) fn constraint_receiver(
        &mut self,
        value: ExprId,
        ty: Type,
        span: Span,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        let apparent = self.constraint_receiver_type(&ty, span)?;
        if apparent == ty {
            return Ok((value, ty));
        }
        let value = self.push_synthetic_expr(
            TypedExprKind::Cast {
                value,
                target_ty: apparent.clone(),
                check: None,
            },
            apparent.clone(),
            span,
        )?;
        Ok((value, apparent))
    }

    /// Both direct member reads and optional chains expose the bound's shape
    /// while retaining the source identity for later discriminant narrowing.
    pub(super) fn constraint_receiver_type(
        &self,
        ty: &Type,
        span: Span,
    ) -> Result<Type, CompilerFailure> {
        let apparent = self.apparent_type(ty);
        if &apparent == ty {
            return Ok(apparent);
        }
        let original = match ty.without_aliases() {
            Type::Refined { original, .. } => original.as_ref(),
            _ => ty,
        };
        let mut budget = self.type_limits.budget();
        budget
            .charge_copy(original, 1)
            .map_err(type_limit_at(span))?;
        budget
            .charge_copy(&apparent, 1)
            .map_err(type_limit_at(span))?;
        budget.charge(1).map_err(type_limit_at(span))?;
        Ok(super::narrowing::preserve_refinement(original, apparent))
    }

    pub(super) fn check_generic_constraints(
        &mut self,
        parameters: &GenericParameters,
        substitution: &TypeParamSubstitution,
        inferred_fallbacks: &[String],
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if parameters.constraints.is_empty() {
            return Ok(());
        }
        // Signature TypeVars are inference wildcards. Turn active parameters
        // into rigid identities for this proof, just as at body entry.
        let rigid = self.rigid_signature_bindings()?;
        for (name, bound) in parameters.constraints.iter() {
            if inferred_fallbacks.contains(name) {
                continue;
            }
            let actual = substitution
                .apply(&Type::TypeVar(name.clone()), &self.type_limits)
                .map_err(type_limit_at(span))?;
            let expected = substitution
                .apply(bound, &self.type_limits)
                .map_err(type_limit_at(span))?;
            let actual = super::generic::substitute_typevars(&actual, &rigid, &self.type_limits)
                .map_err(type_limit_at(span))?;
            let expected =
                super::generic::substitute_typevars(&expected, &rigid, &self.type_limits)
                    .map_err(type_limit_at(span))?;
            // `keyof T` and utilities in a bound evaluate once `T` is known,
            // though the message names the bound as written (`keyof User`).
            let evaluated = super::utility_types::normalize(&expected, self.resolver())
                .map_err(type_limit_at(span))?;
            if !self.satisfies_generic_constraint(&actual, &evaluated) {
                self.error_with_help(
                    span,
                    format!("Type `{actual}` does not satisfy the constraint `{expected}`"),
                    vec![format!(
                        "choose a type argument for `{name}` assignable to `{expected}`"
                    )],
                );
            }
        }
        Ok(())
    }

    pub(super) fn satisfies_generic_constraint(&self, actual: &Type, expected: &Type) -> bool {
        if let Type::Union(members) = actual.peel() {
            return members
                .iter()
                .all(|member| self.satisfies_generic_constraint(member, expected));
        }
        // This is a type-level proof, not an assignment of one closure ABI to
        // another. A `never[]` rest accepts every fixed parameter type by
        // contravariance, as used by ReturnType/Parameters without `any`.
        if let Type::Function {
            params,
            ret,
            has_rest: true,
            predicate: None,
            ..
        } = expected.peel()
            && matches!(params.as_slice(), [Type::Array(element)] if element.peel() == &Type::Never)
            && let Type::Function {
                ret: actual_ret, ..
            } = self.apparent_type(actual).peel()
        {
            return ret.peel() == &Type::Void || assignable(actual_ret, ret, self.resolver());
        }
        assignable(actual, expected, self.resolver())
    }

    /// An invalid inference candidate falls back to its bound. Arguments are
    /// then checked against the resulting parameter type at their own spans;
    /// explicit type arguments instead retain the constraint diagnostic.
    pub(super) fn constrain_inferred_arguments(
        &self,
        parameters: &GenericParameters,
        substitution: &mut TypeParamSubstitution,
        inferred_fallbacks: &mut Vec<String>,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        for name in &parameters.names {
            if inferred_fallbacks.contains(name) {
                continue;
            }
            let Some(bound) = parameters.constraints.get(name) else {
                continue;
            };
            let actual = substitution
                .apply(&Type::TypeVar(name.clone()), &self.type_limits)
                .map_err(type_limit_at(span))?;
            let bound = substitution
                .apply(bound, &self.type_limits)
                .map_err(type_limit_at(span))?;
            let bound = super::utility_types::normalize(&bound, self.resolver())
                .map_err(type_limit_at(span))?;
            if !self.satisfies_generic_constraint(&actual, &bound) {
                substitution.insert(name.clone(), bound);
                inferred_fallbacks.push(name.clone());
            }
        }
        Ok(())
    }

    /// Check a written named type's arguments against its parameters'
    /// bounds. A utility application is checked once, where it is resolved
    /// ([`Self::check_application_constraints`]), so it is skipped here.
    pub(super) fn check_named_type_constraints(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if ty.is_utility_application()
            || matches!(ty, Type::Alias { mangled, .. } if super::utility_types::utility(mangled).is_some())
        {
            return Ok(());
        }
        self.check_application_constraints(ty, span)
    }

    /// Check an application's arguments against its parameters' bounds, and
    /// a `T[K]` form's key against `keyof T`. Inside a declaration the check
    /// waits until every bound is known.
    pub(super) fn check_application_constraints(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        match ty {
            // A concrete form (over a later declaration) reports a missing
            // key where it evaluates.
            Type::IndexedAccess { object, index }
                if super::utility_types::holds_type_parameter(ty) =>
            {
                if !self.defer_constraint_check(ty, span) {
                    self.check_index_key(object, index, span)?;
                }
                Ok(())
            }
            Type::InterfaceRef { .. }
            | Type::ClassRef { .. }
            | Type::Alias { .. }
            | Type::AliasRef { .. } => {
                if !self.defer_constraint_check(ty, span) {
                    self.check_argument_bounds(ty, span)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Queue the check of `ty` while a declaration binds, so every bound is
    /// known when it runs. Whether it was queued.
    fn defer_constraint_check(&mut self, ty: &Type, span: Span) -> bool {
        let Some(pending) = &mut self.pending_constraint_checks else {
            return false;
        };
        pending.push(PendingConstraintCheck {
            ty: ty.clone(),
            span,
            scopes: self.generics_in_scope.clone(),
        });
        true
    }

    /// Check the arguments of an interface, class or alias application
    /// against its parameters' bounds.
    fn check_argument_bounds(&mut self, ty: &Type, span: Span) -> Result<(), CompilerFailure> {
        let (Type::InterfaceRef {
            mangled,
            name,
            args,
            ..
        }
        | Type::ClassRef {
            mangled,
            name,
            args,
            ..
        }
        | Type::Alias {
            mangled,
            name,
            args,
            ..
        }
        | Type::AliasRef {
            mangled,
            name,
            args,
            ..
        }) = ty
        else {
            return Ok(());
        };
        let Some(symbol) = self.lookup_structural_type(mangled, name) else {
            return Ok(());
        };
        let parameters = match &symbol.kind {
            TypeKind::Interface { generics, .. }
            | TypeKind::Class { generics, .. }
            | TypeKind::Alias { generics, .. } => generics.clone(),
            _ => return Ok(()),
        };
        let substitution = TypeParamSubstitution::from_pairs(&parameters, args);
        self.check_generic_constraints(&parameters, &substitution, &[], span)
    }

    /// tsc's TS2536: `T["a"]` needs `"a"` among the keys of `T`'s bound.
    fn check_index_key(
        &mut self,
        object: &Type,
        index: &Type,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let rigid = self.rigid_signature_bindings()?;
        let rigid_object = super::generic::substitute_typevars(object, &rigid, &self.type_limits)
            .map_err(type_limit_at(span))?;
        let rigid_index = super::generic::substitute_typevars(index, &rigid, &self.type_limits)
            .map_err(type_limit_at(span))?;
        let keys =
            super::utility_types::normalize(&Type::KeyOf(Box::new(rigid_object)), self.resolver())
                .map_err(type_limit_at(span))?;
        if !self.satisfies_generic_constraint(&rigid_index, &keys) {
            self.error(
                span,
                format!("Type `{index}` cannot be used to index type `{object}`"),
            );
        }
        Ok(())
    }

    pub(super) fn check_pending_constraints(&mut self) -> Result<(), CompilerFailure> {
        let pending = self.pending_constraint_checks.take().unwrap_or_default();
        let scopes = std::mem::take(&mut self.generics_in_scope);
        let result = (|| {
            for check in pending {
                self.generics_in_scope = check.scopes;
                self.check_application_constraints(&check.ty, check.span)?;
            }
            Ok(())
        })();
        self.generics_in_scope = scopes;
        result
    }
}

fn contains_body_parameter(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if matches!(ty, Type::GenericParam { .. }) {
            return true;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    false
}
