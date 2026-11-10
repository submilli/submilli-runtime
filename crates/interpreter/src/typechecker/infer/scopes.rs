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
    /// A typed local without an initializer must be assigned before a read.
    pub(super) requires_assignment: bool,
    pub(super) is_const: bool,
    pub(super) decl_span: Span,
    /// Disambiguates shadowed bindings for the narrowing engine.
    pub(super) decl_scope: narrowing::ScopeId,
    /// For a function declared inside another function, its index in
    /// `Inferer::nested_functions`.
    pub(super) nested_function: Option<usize>,
    /// Where the literal types in `ty` came from; see `literal_freshness`.
    pub(super) literal_origin: LiteralOrigin,
    /// For a local holding a read that may find no value, the span of that
    /// read; see `missing_reads`.
    pub(super) missing_read: Option<Span>,
    /// Whether a `let` or `const` statement declares it, rather than a
    /// parameter or loop: only such a local can hold a maybe-missing read,
    /// since codegen tracks it from its declaration.
    pub(super) is_statement_local: bool,
}

impl ScopeEntry {
    pub(super) fn read_type(&self) -> &Type {
        if self.requires_assignment {
            self.write_type()
        } else {
            &self.ty
        }
    }

    pub(super) fn write_type(&self) -> &Type {
        self.declared_ty.as_ref().unwrap_or(&self.ty)
    }

    pub(super) fn storage_type(&self) -> &Type {
        self.storage_ty.as_ref().unwrap_or(&self.ty)
    }

    /// The widened storage of an unannotated local holding a maybe-missing
    /// read, `T | undefined`. A local whose own type names `undefined` needs
    /// no widening.
    pub(super) fn maybe_missing_storage(&self) -> Option<Type> {
        let storage = self.storage_type();
        (self.missing_read.is_some() && !storage.holds_undefined())
            .then(|| Type::union(vec![storage.clone(), Type::Undefined]))
    }

    /// Whether this local can hold a maybe-missing read: a `let`/`const`
    /// local no closure captures. One typed with a type parameter, whose
    /// instantiation may hold `undefined`, tells a missing value from a
    /// present `undefined` by a present flag codegen keeps beside it.
    pub(super) fn can_hold_missing_read(&self, captured: bool) -> bool {
        self.is_statement_local && !captured
    }
}

impl Scopes {
    /// Puts back the bindings of `saved`, a copy taken earlier. Scope ids
    /// handed out since stay used, so nothing recorded under one of them can
    /// match a later scope.
    pub(super) fn restore(&mut self, saved: Scopes) {
        self.stack = saved.stack;
    }

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

    pub(super) fn insert_uninitialized(&mut self, name: String, declared: Type, span: Span) {
        let storage = Type::union(vec![declared.clone(), Type::Undefined]);
        self.insert(name.clone(), storage, false, span);
        if let Some(entry) = self
            .stack
            .last_mut()
            .and_then(|scope| scope.bindings.get_mut(&name))
        {
            entry.declared_ty = Some(declared);
            entry.requires_assignment = true;
        }
    }

    pub(super) fn set_parameter_read_type(&mut self, name: &str, ty: Type) {
        if let Some(entry) = self
            .stack
            .iter_mut()
            .rev()
            .find_map(|scope| scope.bindings.get_mut(name))
        {
            entry.ty = ty;
        }
    }

    /// A write invalidates the initial refinement established by a default.
    pub(super) fn reset_parameter_read_type(&mut self, name: &str) {
        if let Some(entry) = self
            .stack
            .iter_mut()
            .rev()
            .find_map(|scope| scope.bindings.get_mut(name))
            && !entry.requires_assignment
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
                    requires_assignment: false,
                    is_const,
                    decl_span,
                    decl_scope,
                    nested_function,
                    literal_origin,
                    missing_read: None,
                    is_statement_local: false,
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

    /// Marks the binding `name` just declared in the innermost scope as a
    /// `let`/`const` statement's.
    pub(super) fn mark_statement_local(&mut self, name: &str) {
        if let Some(entry) = self
            .stack
            .last_mut()
            .and_then(|scope| scope.bindings.get_mut(name))
        {
            entry.is_statement_local = true;
        }
    }

    pub(super) fn get_mut(&mut self, name: &str) -> Option<&mut ScopeEntry> {
        self.stack
            .iter_mut()
            .rev()
            .find_map(|s| s.bindings.get_mut(name))
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
