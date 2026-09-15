//! Generic-scope stack management.

use std::collections::BTreeMap;

use crate::Type;

use super::Inferer;

impl<'a> Inferer<'a> {
    pub(super) fn is_generic_in_scope(&self, text: &str) -> bool {
        self.generics_in_scope
            .iter()
            .rev()
            .any(|frame| frame.iter().any(|n| n == text))
    }

    pub(super) fn fresh_generic_param(&mut self, name: &str) -> Type {
        let id = self.next_generic_param_id;
        self.next_generic_param_id += 1;
        Type::GenericParam {
            id,
            name: name.to_string(),
        }
    }

    pub(super) fn push_signature_generics(&mut self, names: Vec<String>) {
        self.generics_in_scope.push(names);
    }

    pub(super) fn pop_signature_generics(&mut self) {
        self.generics_in_scope.pop();
    }

    /// Both stacks (`generics_in_scope` and `body_instantiations`) move in
    /// lockstep; always pop via [`pop_body_generics`].
    pub(super) fn push_body_generics(&mut self, names: Vec<String>) -> BTreeMap<String, Type> {
        let mut map = BTreeMap::new();
        for name in &names {
            map.insert(name.clone(), self.fresh_generic_param(name));
        }
        self.generics_in_scope.push(names);
        self.body_instantiations.push(map.clone());
        map
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
