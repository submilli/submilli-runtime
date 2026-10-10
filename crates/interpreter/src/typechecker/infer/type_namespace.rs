use std::collections::BTreeMap;

use crate::TypeSymbol;

/// A named type plus its owning package. The package is carried explicitly —
/// captured from the source `PackageDeclaration` (or the prelude / user module) at
/// insertion — never parsed back out of the mangled name. Mirrors
/// [`ValueEntry::package_name`](super::ValueEntry::package_name) for the value side.
struct TypeEntry<'a> {
    package_name: TypeEntryPackage<'a>,
    sym: TypeEntrySymbol<'a>,
}

enum TypeEntryPackage<'a> {
    Borrowed(&'a str),
    Owned(String),
}

impl TypeEntryPackage<'_> {
    fn as_str(&self) -> &str {
        match self {
            TypeEntryPackage::Borrowed(name) => name,
            TypeEntryPackage::Owned(name) => name,
        }
    }
}

enum TypeEntrySymbol<'a> {
    Borrowed(&'a TypeSymbol),
    Owned(Box<TypeSymbol>),
}

impl TypeEntrySymbol<'_> {
    fn as_ref(&self) -> &TypeSymbol {
        match self {
            TypeEntrySymbol::Borrowed(sym) => sym,
            TypeEntrySymbol::Owned(sym) => sym,
        }
    }

    fn as_mut(&mut self) -> Option<&mut TypeSymbol> {
        match self {
            TypeEntrySymbol::Borrowed(_) => None,
            TypeEntrySymbol::Owned(sym) => Some(sym),
        }
    }
}

#[derive(Default)]
pub(in crate::typechecker) struct TypeNamespace<'a> {
    entries: BTreeMap<String, TypeEntry<'a>>,
}

impl<'a> TypeNamespace<'a> {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn lookup(&self, name: &str) -> Option<&TypeSymbol> {
        self.entries.get(name).map(|e| e.sym.as_ref())
    }

    /// The owning package of the named type, for stamping onto a by-name
    /// [`Type`](crate::Type) reference during resolution.
    pub(super) fn package_of(&self, name: &str) -> Option<&str> {
        self.entries.get(name).map(|e| e.package_name.as_str())
    }

    /// Mutable lookup — used to fill in a forward-declared alias's body
    /// once it's resolved on demand.
    pub(super) fn lookup_mut(&mut self, name: &str) -> Option<&mut TypeSymbol> {
        self.entries.get_mut(name).and_then(|e| e.sym.as_mut())
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub(super) fn insert(&mut self, name: String, package_name: String, sym: TypeSymbol) {
        self.entries.insert(
            name,
            TypeEntry {
                package_name: TypeEntryPackage::Owned(package_name),
                sym: TypeEntrySymbol::Owned(Box::new(sym)),
            },
        );
    }

    pub(super) fn insert_borrowed(
        &mut self,
        name: String,
        package_name: &'a str,
        sym: &'a TypeSymbol,
    ) {
        self.entries.insert(
            name,
            TypeEntry {
                package_name: TypeEntryPackage::Borrowed(package_name),
                sym: TypeEntrySymbol::Borrowed(sym),
            },
        );
    }

    pub(super) fn iter_names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Resolve a symbol by its mangled identity (linear scan). Used to walk a
    /// class's `extends` chain, whose parents are keyed by mangled name and may
    /// live in the current module (absent from the import-independent registry).
    pub(super) fn lookup_by_mangled(&self, mangled: &crate::MangledName) -> Option<&TypeSymbol> {
        self.entries
            .values()
            .map(|e| e.sym.as_ref())
            .find(|s| &s.mangled_name == mangled)
    }
}
