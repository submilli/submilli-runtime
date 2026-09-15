use std::collections::BTreeMap;

use crate::{MangledName, NamespaceSymbol, PackageDeclaration, TypeSymbol, ValueSymbol};

pub(in crate::typechecker) type ModuleValueSymbol = (bool, ValueSymbol);
pub(in crate::typechecker) type ModuleTypeSymbol = (bool, TypeSymbol);

/// Symbol table for a source module during package inference.
///
/// This is deliberately not a [`PackageDeclaration`]: packages have a public API
/// boundary, while source modules are compiler-internal units that can expose a
/// module surface to sibling modules without becoming packages themselves.
#[derive(Clone, Debug, Default)]
pub(in crate::typechecker) struct ModuleSymbols {
    pub(in crate::typechecker) values: BTreeMap<String, ModuleValueSymbol>,
    pub(in crate::typechecker) types: BTreeMap<String, ModuleTypeSymbol>,
}

impl ModuleSymbols {
    pub(in crate::typechecker) fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name) || self.types.contains_key(name)
    }

    pub(in crate::typechecker) fn contains_exported(&self, name: &str) -> bool {
        self.values.get(name).is_some_and(|(exported, _)| *exported)
            || self.types.get(name).is_some_and(|(exported, _)| *exported)
    }

    pub(in crate::typechecker) fn value(&self, name: &str) -> Option<&ValueSymbol> {
        self.values.get(name).map(|(_, sym)| sym)
    }

    pub(in crate::typechecker) fn exported_value(&self, name: &str) -> Option<&ValueSymbol> {
        self.values
            .get(name)
            .and_then(|(exported, sym)| exported.then_some(sym))
    }

    pub(in crate::typechecker) fn type_symbol(&self, name: &str) -> Option<&TypeSymbol> {
        self.types.get(name).map(|(_, sym)| sym)
    }

    pub(in crate::typechecker) fn exported_type(&self, name: &str) -> Option<&TypeSymbol> {
        self.types
            .get(name)
            .and_then(|(exported, sym)| exported.then_some(sym))
    }

    pub(in crate::typechecker) fn exported_values(
        &self,
    ) -> impl Iterator<Item = &ValueSymbol> + Clone {
        self.values
            .values()
            .filter_map(|(exported, sym)| exported.then_some(sym))
    }

    pub(in crate::typechecker) fn exported_types(
        &self,
    ) -> impl Iterator<Item = &TypeSymbol> + Clone {
        self.types
            .values()
            .filter_map(|(exported, sym)| exported.then_some(sym))
    }

    pub(in crate::typechecker) fn exported_value_map(&self) -> BTreeMap<String, ValueSymbol> {
        self.values
            .iter()
            .filter(|(_, (exported, _))| *exported)
            .map(|(name, (_, sym))| (name.clone(), sym.clone()))
            .collect()
    }

    pub(in crate::typechecker) fn exported_type_map(&self) -> BTreeMap<String, TypeSymbol> {
        self.types
            .iter()
            .filter(|(_, (exported, _))| *exported)
            .map(|(name, (_, sym))| (name.clone(), sym.clone()))
            .collect()
    }

    pub(in crate::typechecker) fn all_types(&self) -> impl Iterator<Item = &TypeSymbol> {
        self.types.values().map(|(_, sym)| sym)
    }

    pub(in crate::typechecker) fn exports_help(&self) -> Vec<String> {
        let mut names: Vec<&str> = self
            .values
            .iter()
            .filter(|(_, (exported, _))| *exported)
            .map(|(name, _)| name.as_str())
            .chain(
                self.types
                    .iter()
                    .filter(|(_, (exported, _))| *exported)
                    .map(|(name, _)| name.as_str()),
            )
            .collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "exports: {}",
                names
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )]
        }
    }
}

/// Namespace-import view used by expression inference. It covers external
/// packages and relative module namespace imports without storing a
/// `PackageDeclaration` for source modules.
#[derive(Debug)]
pub(in crate::typechecker) enum NamespaceMembers<'a> {
    Package(&'a PackageDeclaration),
    Module {
        package_name: String,
        values: BTreeMap<String, ValueSymbol>,
        types: BTreeMap<String, TypeSymbol>,
    },
}

impl<'a> NamespaceMembers<'a> {
    pub(in crate::typechecker) fn from_package(package: &'a PackageDeclaration) -> Self {
        Self::Package(package)
    }

    pub(in crate::typechecker) fn from_module(package_name: &str, module: &ModuleSymbols) -> Self {
        Self::Module {
            package_name: package_name.to_string(),
            values: module.exported_value_map(),
            types: module.exported_type_map(),
        }
    }

    pub(in crate::typechecker) fn package_name(&self) -> &str {
        match self {
            Self::Package(package) => &package.package_name,
            Self::Module { package_name, .. } => package_name,
        }
    }

    pub(in crate::typechecker) fn value(&self, name: &str) -> Option<&ValueSymbol> {
        match self {
            Self::Package(package) => package.values.get(name),
            Self::Module { values, .. } => values.get(name),
        }
    }

    pub(in crate::typechecker) fn type_symbol(&self, name: &str) -> Option<&TypeSymbol> {
        match self {
            Self::Package(package) => package.types.get(name),
            Self::Module { types, .. } => types.get(name),
        }
    }

    pub(in crate::typechecker) fn exports_help(&self) -> Vec<String> {
        let mut names: Vec<&str> = match self {
            Self::Package(package) => package
                .values
                .keys()
                .map(String::as_str)
                .chain(package.types.keys().map(String::as_str))
                .collect(),
            Self::Module { values, types, .. } => values
                .keys()
                .map(String::as_str)
                .chain(types.keys().map(String::as_str))
                .collect(),
        };
        names.sort();
        names.dedup();
        if names.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "exports: {}",
                names
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )]
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(in crate::typechecker) struct NamespaceSymbolSet<'a> {
    entries: Vec<&'a NamespaceSymbol>,
}

impl<'a> NamespaceSymbolSet<'a> {
    pub(in crate::typechecker) fn new(ns: &'a NamespaceSymbol) -> Self {
        Self { entries: vec![ns] }
    }

    pub(in crate::typechecker) fn push(&mut self, ns: &'a NamespaceSymbol) {
        self.entries.push(ns);
    }

    pub(in crate::typechecker) fn child(&self, name: &str) -> Option<Self> {
        let entries: Vec<&'a NamespaceSymbol> = self
            .entries
            .iter()
            .filter_map(|ns| ns.namespaces.get(name))
            .collect();
        (!entries.is_empty()).then_some(Self { entries })
    }

    pub(in crate::typechecker) fn value(&self, name: &str) -> Option<&ValueSymbol> {
        self.entries.iter().find_map(|ns| ns.values.get(name))
    }

    pub(in crate::typechecker) fn type_symbol(&self, name: &str) -> Option<&TypeSymbol> {
        self.entries.iter().find_map(|ns| ns.types.get(name))
    }

    pub(in crate::typechecker) fn mangled_prefix(&self) -> MangledName {
        self.entries
            .first()
            .expect("namespace symbol set must not be empty")
            .mangled_prefix
            .clone()
    }

    pub(in crate::typechecker) fn exports(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries
            .iter()
            .flat_map(|ns| {
                ns.values
                    .keys()
                    .chain(ns.types.keys())
                    .chain(ns.namespaces.keys())
                    .cloned()
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
}
