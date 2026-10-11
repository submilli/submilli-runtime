//! Lexical scope chain for function-local bindings.
//!
//! Top-level bindings live in `Inferer.top_symbols`, not here.

use std::collections::BTreeMap;

use crate::{Span, Type};

use super::literal_freshness::LiteralOrigin;
use super::narrowing;

#[derive(Clone, Default)]
pub(in crate::typechecker) struct Scopes {
    stack: Vec<Scope>,
    next_id: u32,
}

#[derive(Clone)]
struct Scope {
    id: narrowing::ScopeId,
    bindings: BTreeMap<String, ScopeEntry>,
}

#[derive(Clone)]
pub(super) struct ScopeEntry {
    pub(super) ty: Type,
    /// ABI storage can admit undefined even after a default initializes the binding.
    pub(super) storage_ty: Option<Type>,
    /// Assignment contract before a default narrows the readable value.
    pub(super) declared_ty: Option<Type>,
    pub(super) is_const: bool,
    pub(super) decl_span: Span,
    /// Disambiguates shadowed bindings for the narrowing engine.
    pub(super) decl_scope: narrowing::ScopeId,
    /// For a function declared inside another function, its index in
    /// `Inferer::nested_functions`.
    pub(super) nested_function: Option<usize>,
    /// Where the literal types in `ty` came from; see `literal_freshness`.
    pub(super) literal_origin: LiteralOrigin,
}

impl ScopeEntry {
    pub(super) fn write_type(&self) -> &Type {
        self.declared_ty.as_ref().unwrap_or(&self.ty)
    }

    pub(super) fn storage_type(&self) -> &Type {
        self.storage_ty.as_ref().unwrap_or(&self.ty)
    }
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
        self.insert_entry(name, ty, is_const, decl_span, None, LiteralOrigin::Unknown);
    }

    /// Bind a `let`, `const` or loop variable whose literal types came from
    /// `literal_origin`.
    pub(super) fn insert_with_literal_origin(
        &mut self,
        name: String,
        ty: Type,
        is_const: bool,
        decl_span: Span,
        literal_origin: LiteralOrigin,
    ) {
        self.insert_entry(name, ty, is_const, decl_span, None, literal_origin);
    }

    /// Bind a parameter whose type is written out, so every literal type in it
    /// is regular.
    pub(super) fn insert_annotated_param(&mut self, name: String, ty: Type, decl_span: Span) {
        self.insert_entry(name, ty, false, decl_span, None, LiteralOrigin::Declared);
    }

    /// Bind a parameter that reads as `ty` but is stored as `storage_ty`, which
    /// admits the omission a default replaces.
    pub(super) fn insert_parameter(
        &mut self,
        name: String,
        ty: Type,
        storage_ty: Type,
        span: Span,
        literal_origin: LiteralOrigin,
    ) {
        self.insert_entry(name.clone(), ty, false, span, None, literal_origin);
        if let Some(entry) = self
            .stack
            .last_mut()
            .and_then(|scope| scope.bindings.get_mut(&name))
        {
            entry.storage_ty = Some(storage_ty);
        }
    }

    pub(super) fn set_parameter_declared_type(&mut self, name: &str, ty: Type) {
        if let Some(entry) = self
            .stack
            .last_mut()
            .and_then(|scope| scope.bindings.get_mut(name))
        {
            entry.declared_ty = Some(ty);
        }
    }

    /// A write invalidates the initial refinement established by a default.
    pub(super) fn reset_parameter_read_type(&mut self, name: &str) {
        if let Some(entry) = self
            .stack
            .iter_mut()
            .rev()
            .find_map(|scope| scope.bindings.get_mut(name))
            && let Some(ty) = &entry.declared_ty
        {
            entry.ty = ty.clone();
        }
    }

    /// Bind a nested function declaration's name, which cannot be reassigned.
    pub(super) fn insert_nested_function(
        &mut self,
        name: String,
        ty: Type,
        decl_span: Span,
        index: usize,
    ) {
        self.insert_entry(
            name,
            ty,
            true,
            decl_span,
            Some(index),
            LiteralOrigin::Unknown,
        );
    }

    fn insert_entry(
        &mut self,
        name: String,
        ty: Type,
        is_const: bool,
        decl_span: Span,
        nested_function: Option<usize>,
        literal_origin: LiteralOrigin,
    ) {
        if let Some(top) = self.stack.last_mut() {
            let decl_scope = top.id;
            top.bindings.insert(
                name,
                ScopeEntry {
                    ty,
                    storage_ty: None,
                    declared_ty: None,
                    is_const,
                    decl_span,
                    decl_scope,
                    nested_function,
                    literal_origin,
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

    pub(super) fn get_binding(&self, name: &str, scope: narrowing::ScopeId) -> Option<&ScopeEntry> {
        self.stack
            .iter()
            .find(|s| s.id == scope)?
            .bindings
            .get(name)
    }

    pub(super) fn all_names(&self) -> impl Iterator<Item = &str> {
        self.stack
            .iter()
            .rev()
            .flat_map(|s| s.bindings.keys().map(String::as_str))
    }
}
