//! The rule's own lexical scopes.
//!
//! A reference carries the identifier at its use, and the inferer's scope
//! numbers restart in every module, so neither identifies a binding.

use std::collections::BTreeMap;

/// Names in scope, each with what a walk records about its binding.
pub(super) struct Scopes<B> {
    /// Innermost last.
    scopes: Vec<BTreeMap<String, B>>,
}

impl<B> Default for Scopes<B> {
    fn default() -> Self {
        Self { scopes: Vec::new() }
    }
}

impl<B> Scopes<B> {
    pub(super) fn push(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn pop(&mut self) {
        self.scopes.pop();
    }

    /// Declares `name` in the innermost scope.
    pub(super) fn declare(&mut self, name: &str, binding: B) {
        match self.scopes.last_mut() {
            Some(scope) => {
                scope.insert(name.to_string(), binding);
            }
            None => self
                .scopes
                .push(BTreeMap::from([(name.to_string(), binding)])),
        }
    }

    pub(super) fn resolve(&self, name: &str) -> Option<&B> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }
}
