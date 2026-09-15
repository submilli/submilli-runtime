//! Lexical scope chain for function-local bindings.
//!
//! Top-level bindings live in `Inferer.top_symbols`, not here.

use std::collections::BTreeMap;

use crate::{Span, Type};

use super::narrowing;

#[derive(Default)]
pub(in crate::typechecker) struct Scopes {
    stack: Vec<Scope>,
    next_id: u32,
}

struct Scope {
    id: narrowing::ScopeId,
    bindings: BTreeMap<String, ScopeEntry>,
}

#[derive(Clone)]
pub(super) struct ScopeEntry {
    pub(super) ty: Type,
    pub(super) is_const: bool,
    pub(super) decl_span: Span,
    /// Disambiguates shadowed bindings for the narrowing engine.
    pub(super) decl_scope: narrowing::ScopeId,
}

impl Scopes {
    pub(super) fn push(&mut self) {
        let id = narrowing::ScopeId(self.next_id);
        self.next_id += 1;
        self.stack.push(Scope {
            id,
            bindings: BTreeMap::new(),
        });
    }

    pub(super) fn pop(&mut self) {
        self.stack.pop();
    }

    pub(super) fn insert(&mut self, name: String, ty: Type, is_const: bool, decl_span: Span) {
        if let Some(top) = self.stack.last_mut() {
            let decl_scope = top.id;
            top.bindings.insert(
                name,
                ScopeEntry {
                    ty,
                    is_const,
                    decl_span,
                    decl_scope,
                },
            );
        }
    }

    /// The id the next `push` will assign. Scopes created at or inside a
    /// construct all get ids >= the value read just before entering it.
    pub(super) fn next_scope_id(&self) -> narrowing::ScopeId {
        narrowing::ScopeId(self.next_id)
    }

    pub(super) fn get(&self, name: &str) -> Option<&ScopeEntry> {
        self.stack.iter().rev().find_map(|s| s.bindings.get(name))
    }

    pub(super) fn all_names(&self) -> impl Iterator<Item = &str> {
        self.stack
            .iter()
            .rev()
            .flat_map(|s| s.bindings.keys().map(String::as_str))
    }
}
