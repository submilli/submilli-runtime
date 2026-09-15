//! Import-independent registry of every prelude + loaded-package type, keyed by
//! the declaring symbol's mangled name.
//!
//! Structural resolution of an *already-typed* value — member access on something
//! the typechecker already knows is `InterfaceRef { mangled, .. }`, or
//! `format_definition` lifting a type into a diagnostic — reads from here. Because
//! the key carries the owning package, that resolution never depends on what the
//! current module imported, fixing the class of bug where a library return type
//! couldn't be inspected unless the user also imported the interface name.
//!
//! Contrast [`TypeNamespace`](super::type_namespace::TypeNamespace), which stays
//! import-scoped and is the right table for resolving a *name written in source*.

use std::collections::BTreeMap;

use crate::mangle::MangledName;
use crate::package_declaration::TypeSymbol;

#[derive(Default)]
pub(in crate::typechecker) struct TypeRegistry<'a> {
    entries: BTreeMap<MangledName, TypeRegistryEntry<'a>>,
}

enum TypeRegistryEntry<'a> {
    Borrowed(&'a TypeSymbol),
    Owned(Box<TypeSymbol>),
}

impl<'a> TypeRegistry<'a> {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn insert_borrowed(&mut self, sym: &'a TypeSymbol) {
        self.entries
            .insert(sym.mangled_name.clone(), TypeRegistryEntry::Borrowed(sym));
    }

    pub(super) fn insert_owned(&mut self, sym: TypeSymbol) {
        self.entries.insert(
            sym.mangled_name.clone(),
            TypeRegistryEntry::Owned(Box::new(sym)),
        );
    }

    pub(super) fn lookup(&self, mangled: &MangledName) -> Option<&TypeSymbol> {
        self.entries.get(mangled).map(|entry| match entry {
            TypeRegistryEntry::Borrowed(sym) => *sym,
            TypeRegistryEntry::Owned(sym) => sym,
        })
    }
}
