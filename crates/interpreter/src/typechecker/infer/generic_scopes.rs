//! Generic-scope stack management.

use std::collections::BTreeMap;

use crate::Type;
use crate::compiler_error::{CompilerFailure, CompilerStage};

use super::Inferer;

impl<'a> Inferer<'a> {
    pub(super) fn is_generic_in_scope(&self, text: &str) -> bool {
        self.generics_in_scope
            .iter()
            .rev()
            .any(|frame| frame.iter().any(|n| n == text))
    }

    pub(super) fn fresh_generic_param(&mut self, name: &str) -> Result<Type, CompilerFailure> {
        let id = self.next_generic_param_id;
        self.next_generic_param_id = id.checked_add(1).ok_or_else(|| CompilerFailure::Limit {
            stage: CompilerStage::Infer,
            span: None,
            message: "generic instantiations exceed the compiler's identifier limit".into(),
            help: vec!["split the program into smaller modules".into()],
        })?;
        Ok(Type::GenericParam {
            id,
            name: name.to_string(),
        })
    }

    pub(super) fn push_signature_generics(&mut self, parameters: crate::GenericParameters) {
        self.generics_in_scope.push(parameters);
    }

    pub(super) fn pop_signature_generics(&mut self) {
        self.generics_in_scope.pop();
    }

    /// Both stacks (`generics_in_scope` and `body_instantiations`) move in
    /// lockstep; always pop via [`pop_body_generics`].
    pub(super) fn push_body_generics(
        &mut self,
        parameters: crate::GenericParameters,
    ) -> Result<BTreeMap<String, Type>, CompilerFailure> {
        let mut map = BTreeMap::new();
        for name in &parameters {
            map.insert(name.clone(), self.fresh_generic_param(name)?);
        }
        let mut bindings = BTreeMap::new();
        for frame in &self.body_instantiations {
            bindings.extend(frame.clone());
        }
        bindings.extend(map.clone());
        for (name, constraint) in parameters.constraints.iter() {
            if let Some(Type::GenericParam { id, .. }) = map.get(name) {
                let bound =
                    super::generic::substitute_typevars(constraint, &bindings, &self.type_limits)
                        .map_err(super::type_limit_unlocated)?;
                self.generic_constraints.insert(*id, bound);
            }
        }
        self.generics_in_scope.push(parameters);
        self.body_instantiations.push(map.clone());
        Ok(map)
    }

    pub(super) fn pop_body_generics(&mut self) {
        self.generics_in_scope.pop();
        self.body_instantiations.pop();
    }

    /// A proof over a signature must retain its bounds without treating its
    /// TypeVars as inference wildcards. The temporary frames also substitute
    /// outer parameters into inner bounds before being removed.
    pub(super) fn rigid_signature_bindings(
        &mut self,
    ) -> Result<BTreeMap<String, Type>, CompilerFailure> {
        let scopes = self.generics_in_scope.clone();
        let signature_depth = self.generics_in_scope.len();
        let body_depth = self.body_instantiations.len();
        let result = (|| {
            let mut bindings = BTreeMap::new();
            for scope in scopes {
                bindings.extend(self.push_body_generics(scope)?);
            }
            Ok(bindings)
        })();
        self.generics_in_scope.truncate(signature_depth);
        self.body_instantiations.truncate(body_depth);
        result
    }

    /// Register the whole list before resolving bounds, including forward
    /// references to another parameter in the same list.
    pub(super) fn push_declared_generics(
        &mut self,
        annotations: &[crate::TypeParameterAnnotation],
    ) -> Result<crate::GenericParameters, CompilerFailure> {
        let mut parameters = crate::GenericParameters::unconstrained(
            annotations.iter().map(|p| p.name.clone()).collect(),
        );
        self.push_signature_generics(parameters.clone());
        let pending_start = self.pending_constraint_checks.as_ref().map_or(0, Vec::len);
        for parameter in annotations {
            if let Some(annotation) = &parameter.constraint {
                let constraint = self.resolve_type(annotation)?;
                parameters
                    .constraints
                    .insert(parameter.name.clone(), constraint);
            }
        }
        for parameter in annotations {
            if has_circular_constraint(&parameters, &parameter.name, &self.type_limits)
                .map_err(super::type_limit_at(parameter.span))?
            {
                self.error_with_help(
                    parameter.span,
                    format!(
                        "type parameter `{}` has a circular constraint",
                        parameter.name
                    ),
                    vec![
                        "use a structural constraint instead of a cycle of type parameters".into(),
                    ],
                );
                parameters
                    .constraints
                    .insert(parameter.name.clone(), Type::Error);
            }
        }
        if let Some(frame) = self.generics_in_scope.last_mut() {
            *frame = parameters.clone();
        }
        if let Some(pending) = &mut self.pending_constraint_checks {
            for check in pending.iter_mut().skip(pending_start) {
                let belongs_to_constraint = annotations
                    .iter()
                    .filter_map(|p| p.constraint.as_ref())
                    .any(|annotation| {
                        annotation.span.file == check.span.file
                            && annotation.span.start <= check.span.start
                            && check.span.end <= annotation.span.end
                    });
                if !belongs_to_constraint {
                    continue;
                }
                if let Some(frame) = check.scopes.last_mut() {
                    *frame = parameters.clone();
                }
            }
        }
        Ok(parameters)
    }

    /// Returns `None` during the signature pass (no instantiation pushed).
    pub(super) fn lookup_body_gp(&self, text: &str) -> Option<&Type> {
        self.body_instantiations
            .iter()
            .rev()
            .find_map(|frame| frame.get(text))
    }

    /// Substitute every `TypeVar` naming an active body generic with its
    /// `GenericParam` instantiation (innermost frame wins). Signature-space
    /// types brought into a body (a generic parent's ctor params, a `super`
    /// method sig) go through this so they compare against the body's live
    /// forms.
    pub(super) fn apply_body_instantiations(
        &self,
        ty: &Type,
    ) -> Result<Type, crate::type_size::TypeTooLarge> {
        if self.body_instantiations.is_empty() {
            return Ok(ty.clone());
        }
        let mut merged = BTreeMap::new();
        for frame in &self.body_instantiations {
            merged.extend(frame.clone());
        }
        super::generic::substitute_typevars(ty, &merged, &self.type_limits)
    }
}

/// Only naked parameter/union links form an immediate cycle. A structural
/// bound such as `T extends { next: T }` is a legal recursive constraint.
fn has_circular_constraint(
    parameters: &crate::GenericParameters,
    name: &str,
    limits: &crate::type_size::TypeLimits,
) -> Result<bool, crate::type_size::TypeTooLarge> {
    let mut pending: Vec<_> = parameters.constraints.get(name).into_iter().collect();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(bound) = pending.pop() {
        if !limits.spend_work(1) {
            return Err(crate::type_size::TypeTooLarge::Work);
        }
        match bound.peel() {
            Type::TypeVar(next) => {
                if next == name {
                    return Ok(true);
                }
                if visited.insert(next) {
                    pending.extend(parameters.constraints.get(next));
                }
            }
            Type::Union(members) => pending.extend(members),
            _ => {}
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use crate::Type;
    use crate::compiler_error::CompilerFailure;

    use super::super::test_support::with_inferer;

    #[test]
    fn generic_param_ids_stay_unique_and_exhaustion_is_a_limit() {
        with_inferer(|tc| {
            tc.next_generic_param_id = u32::MAX - 1;
            assert_eq!(
                tc.fresh_generic_param("T").unwrap(),
                Type::GenericParam {
                    id: u32::MAX - 1,
                    name: "T".into()
                }
            );
            let frames = tc.generics_in_scope.len();
            // Like arena IDs, `u32::MAX` itself is never allocated, so the frame
            // fails at `U` and nothing is pushed.
            let error = tc
                .push_body_generics(crate::GenericParameters::unconstrained(vec![
                    "U".into(),
                    "V".into(),
                ]))
                .unwrap_err();
            assert!(matches!(error, CompilerFailure::Limit { .. }), "{error:?}");
            assert_eq!(tc.generics_in_scope.len(), frames);
            assert!(tc.fresh_generic_param("W").is_err());
        });
    }

    #[test]
    fn constraint_proof_depth_is_a_compiler_limit() {
        std::thread::Builder::new()
            .stack_size(crate::compiler_limits::COMPILER_STACK_BYTES)
            .spawn(|| {
                let count = crate::compiler_limits::MAX_TYPE_RESOLUTION_DEPTH as usize + 8;
                let parameters = (0..count).map(|i| {
                    if i + 1 == count { format!("T{i} extends string") }
                    else { format!("T{i} extends T{} | string", i + 1) }
                }).collect::<Vec<_>>().join(", ");
                for (result, expression) in [("string", "value"), ("number", "value.length")] {
                    let source = format!("function f<{parameters}>(value: T0): {result} {{ return {expression}; }}\nfunction main(): void {{}}");
                    let (_, diagnostics) = crate::typecheck_to_typed_ast(&source, crate::FileId(0));
                    assert!(diagnostics.iter().any(|diagnostic| diagnostic.message.contains("limit")), "{expression}: {diagnostics:?}");
                }
                let (_, healthy) = crate::typecheck_to_typed_ast("function main(): number { return 42; }", crate::FileId(0));
                assert!(healthy.iter().all(|diagnostic| diagnostic.severity != crate::Severity::Error), "{healthy:?}");
            }).unwrap().join().unwrap();
    }
}
