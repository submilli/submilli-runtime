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

    pub(super) fn push_signature_generics(&mut self, names: Vec<String>) {
        self.generics_in_scope.push(names);
    }

    pub(super) fn pop_signature_generics(&mut self) {
        self.generics_in_scope.pop();
    }

    /// Both stacks (`generics_in_scope` and `body_instantiations`) move in
    /// lockstep; always pop via [`pop_body_generics`].
    pub(super) fn push_body_generics(
        &mut self,
        names: Vec<String>,
    ) -> Result<BTreeMap<String, Type>, CompilerFailure> {
        let mut map = BTreeMap::new();
        for name in &names {
            map.insert(name.clone(), self.fresh_generic_param(name)?);
        }
        self.generics_in_scope.push(names);
        self.body_instantiations.push(map.clone());
        Ok(map)
    }

    pub(super) fn pop_body_generics(&mut self) {
        self.generics_in_scope.pop();
        self.body_instantiations.pop();
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
    pub(super) fn apply_body_instantiations(&self, ty: &Type) -> Type {
        if self.body_instantiations.is_empty() {
            return ty.clone();
        }
        let mut merged = BTreeMap::new();
        for frame in &self.body_instantiations {
            merged.extend(frame.clone());
        }
        super::generic::substitute_typevars(ty, &merged)
    }
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
                .push_body_generics(vec!["U".into(), "V".into()])
                .unwrap_err();
            assert!(matches!(error, CompilerFailure::Limit { .. }), "{error:?}");
            assert_eq!(tc.generics_in_scope.len(), frames);
            assert!(tc.fresh_generic_param("W").is_err());
        });
    }
}
