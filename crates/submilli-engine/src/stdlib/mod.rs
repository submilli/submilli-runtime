//! Standard-library packages user code reaches via `import` —
//! `submilli:crypto` / `submilli:embedding` / `submilli:fs` / `submilli:http` / `submilli:llm` /
//! `submilli:secrets` / `submilli:security` / `submilli:session` /
//! `submilli:url` / `submilli:uuid`.
//!
//! Every package is pure Rust host functions registered directly under its
//! package name; the linker resolves user imports with no Wasm shim modules
//! and no Wasm module instantiation. Host-backed classes initialize their
//! vtables separately for each store.

pub(crate) mod abi;
pub mod agents;
pub mod capabilities;
pub mod code;
pub mod crypto;
pub(crate) mod dot_segments;
pub mod embedding;
pub mod fs;
pub mod git;
pub mod http;
pub mod llm;
pub mod secrets;
pub mod security;
pub mod session;
pub mod shared;
pub mod skills;
/// Test-authoring package. Deliberately absent from
/// [`stdlib_package_declarations`] and [`install_host_functions`]: only
/// `submilli build test` makes it importable (by passing its declaration into
/// the compile and installing it directly), so `submilli run` rejects
/// `import ... from "submilli:test"` as not found.
pub mod test;
pub mod url;
pub mod uuid;

use wasmtime::Linker;

use crate::runtime::StoreData;
use crate::{MangledName, PackageDeclaration, Type};

/// A standard-library package that exists only when the embedder provides it.
///
/// Each one is backed by a provider trait the harness implements. An embedder
/// that does not enable a package gets none of it: the package cannot be
/// imported, discovery and the capability catalog do not list it, and its host
/// functions are not installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum OptionalPackage {
    /// `submilli:agents`, backed by [`AgentProvider`](crate::runtime::AgentProvider).
    Agents,
    /// `submilli:skills`, backed by [`SkillProvider`](crate::runtime::SkillProvider).
    Skills,
}

impl OptionalPackage {
    pub const ALL: &'static [OptionalPackage] = &[Self::Agents, Self::Skills];

    pub const fn module_name(self) -> &'static str {
        match self {
            Self::Agents => agents::MODULE_NAME,
            Self::Skills => skills::MODULE_NAME,
        }
    }

    pub fn from_module_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|package| package.module_name() == name)
    }

    fn package_declaration(self) -> PackageDeclaration {
        match self {
            Self::Agents => agents::package_declaration(),
            Self::Skills => skills::package_declaration(),
        }
    }

    fn install(self, linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
        match self {
            Self::Agents => agents::install(linker),
            Self::Skills => skills::install(linker),
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Agents => 1,
            Self::Skills => 1 << 1,
        }
    }
}

/// A host call the typechecker types from its type argument: the compiler emits
/// a JSON Schema for `T` into the call's trailing `schema` parameter and wraps
/// the result in a structural check against `T`. What differs between such
/// calls is what a bare call returns and how diagnostics name it.
pub(crate) struct SchemaCheckedCall {
    /// The call as diagnostics name it, e.g. `llm.call`.
    pub callee: &'static str,
    /// A typed call to show in a diagnostic.
    pub example: &'static str,
    /// What the structural check is applied to.
    pub checks: &'static str,
    /// Who the schema is sent to.
    pub answerer: &'static str,
    /// The parameters a program writes, in prose; the schema parameter follows.
    pub written_params: &'static str,
    pub written_param_count: usize,
    /// Ends the "cannot be verified" diagnostics: what to do instead of a type
    /// argument the runtime cannot test.
    pub untyped_hint: &'static str,
    /// Ends the `<unknown>` diagnostic.
    pub untyped_hint_for_unknown: &'static str,
    /// Ends the "has no JSON Schema" diagnostic.
    pub untyped_hint_for_no_schema: &'static str,
    /// What `T` binds to when the program wrote no type argument.
    pub untyped_result: Type,
}

impl SchemaCheckedCall {
    /// The schema parameter's position, as an ordinal word.
    pub fn schema_argument_ordinal(&self) -> &'static str {
        match self.written_param_count {
            0 => "first",
            1 => "second",
            2 => "third",
            3 => "fourth",
            _ => "last",
        }
    }
}

/// The schema-checked call `mangled` names, if it is one.
pub(crate) fn schema_checked_call(mangled: &MangledName) -> Option<SchemaCheckedCall> {
    if llm::declaration::is_checked_call(mangled) {
        return Some(SchemaCheckedCall {
            callee: "llm.call",
            example: "llm.call<Severity>(model, prompt)",
            checks: "the model's response",
            answerer: "the model",
            written_params: "the model and the prompt",
            written_param_count: 2,
            untyped_hint: "read the `Completion` envelope yourself",
            untyped_hint_for_unknown: "read the `Completion` envelope's `ok` and `text` yourself",
            untyped_hint_for_no_schema: "parse the `Completion` text yourself",
            untyped_result: llm::declaration::untyped_result_type(mangled),
        });
    }
    if agents::declaration::is_checked_run(mangled) {
        return Some(SchemaCheckedCall {
            callee: "agents.run",
            example: "agents.run<Report>(agent, input)",
            checks: "the agent's result",
            answerer: "the agent",
            written_params: "the agent and the input",
            written_param_count: 2,
            untyped_hint: "read the returned text yourself",
            untyped_hint_for_unknown: "read the returned text yourself",
            untyped_hint_for_no_schema: "parse the returned text yourself",
            untyped_result: Type::String,
        });
    }
    None
}

/// The standard library an embedder offers its programs: the core packages
/// every embedder has, plus the [`OptionalPackage`]s it enables.
///
/// Compiling, discovery, the capability catalog and host-function installation
/// all read the same set, so a package is either fully present or fully absent.
/// The discovery methods (`docs`, `search`, `resolve`, …) are in
/// [`crate::packages`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stdlib {
    optional: u8,
}

impl Stdlib {
    /// The core packages only, what `submilli run` and the server offer.
    pub const fn core() -> Self {
        Stdlib { optional: 0 }
    }

    #[must_use]
    pub const fn with(self, package: OptionalPackage) -> Self {
        Stdlib {
            optional: self.optional | package.bit(),
        }
    }

    pub const fn enables(self, package: OptionalPackage) -> bool {
        self.optional & package.bit() != 0
    }

    pub fn optional_packages(self) -> impl Iterator<Item = OptionalPackage> {
        OptionalPackage::ALL
            .iter()
            .copied()
            .filter(move |package| self.enables(*package))
    }

    /// Whether `name` is a package of this set.
    pub fn contains_module(self, name: &str) -> bool {
        OptionalPackage::from_module_name(name).map_or_else(
            || {
                core_package_declarations()
                    .iter()
                    .any(|defs| defs.package_name == name)
            },
            |package| self.enables(package),
        )
    }

    /// Every package of this set, sorted by name.
    pub fn package_declarations(self) -> Vec<PackageDeclaration> {
        let mut declarations = core_package_declarations();
        declarations.extend(
            self.optional_packages()
                .map(OptionalPackage::package_declaration),
        );
        // Codegen emits imports in this order, so sorting keeps builds byte-for-byte stable.
        declarations.sort_by(|a, b| a.package_name.cmp(&b.package_name));
        declarations
    }

    pub fn install_host_functions(self, linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
        install_core_host_functions(linker)?;
        for package in self.optional_packages() {
            package.install(linker)?;
        }
        Ok(())
    }
}

/// Why `module` cannot be imported though it names a standard-library package,
/// as a message and its help lines. `None` when `module` is no such package.
///
/// Reached only for a package missing from the declarations the compile was
/// given, so an optional package here is one the embedder did not enable.
pub(crate) fn unavailable_import(module: &str) -> Option<(String, Vec<String>)> {
    if module == test::MODULE_NAME {
        return Some((
            format!("package `{module}` not found"),
            vec![
                "`submilli:test` is only available to test files run via `submilli build test`; it is not importable from a program run with `submilli run`"
                    .to_string(),
            ],
        ));
    }
    let package = OptionalPackage::from_module_name(module)?;
    Some((
        format!("package `{module}` is not available in this harness"),
        vec![format!(
            "`{}` exists only where the harness running the program provides it",
            package.module_name()
        )],
    ))
}

/// The core packages: what [`Stdlib::core`] offers.
pub fn stdlib_package_declarations() -> Vec<PackageDeclaration> {
    Stdlib::core().package_declarations()
}

pub fn install_host_functions(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    Stdlib::core().install_host_functions(linker)
}

fn core_package_declarations() -> Vec<PackageDeclaration> {
    vec![
        // Alphabetical by package name; codegen import-emission relies on this order.
        code::package_declaration(),
        crypto::package_declaration(),
        embedding::package_declaration(),
        fs::package_declaration(),
        git::package_declaration(),
        http::package_declaration(),
        llm::package_declaration(),
        secrets::package_declaration(),
        security::package_declaration(),
        session::package_declaration(),
        url::package_declaration(),
        uuid::package_declaration(),
    ]
}

fn install_core_host_functions(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    code::install(linker)?;
    crypto::install(linker)?;
    embedding::install(linker)?;
    fs::install(linker)?;
    git::install(linker)?;
    http::install(linker)?;
    llm::install(linker)?;
    secrets::install(linker)?;
    security::install(linker)?;
    session::install(linker)?;
    url::install(linker)?;
    uuid::install(linker)?;
    Ok(())
}

/// Initialize host-backed standard-library classes for this store.
pub(crate) fn install_store_bound(
    linker: &mut Linker<StoreData>,
    store: &mut wasmtime::Store<StoreData>,
) -> wasmtime::Result<()> {
    git::class::install(linker, store)
}

#[cfg(test)]
mod optional_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every subset of the optional packages.
    fn every_set() -> Vec<Stdlib> {
        let mut sets = vec![Stdlib::core()];
        for package in OptionalPackage::ALL {
            let with: Vec<Stdlib> = sets.iter().map(|set| set.with(*package)).collect();
            sets.extend(with);
        }
        sets
    }

    #[test]
    fn declarations_are_sorted_for_every_set() {
        for set in every_set() {
            let names: Vec<String> = set
                .package_declarations()
                .into_iter()
                .map(|defs| defs.package_name)
                .collect();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted, "{set:?}");
        }
    }

    #[test]
    fn a_set_contains_exactly_its_packages() {
        for set in every_set() {
            let names: Vec<String> = set
                .package_declarations()
                .into_iter()
                .map(|defs| defs.package_name)
                .collect();
            for name in &names {
                assert!(set.contains_module(name), "{set:?} misses {name}");
            }
            for package in OptionalPackage::ALL {
                assert_eq!(
                    names.iter().any(|name| name == package.module_name()),
                    set.enables(*package),
                    "{set:?} and {package:?}"
                );
            }
        }
    }

    #[test]
    fn core_is_the_global_list() {
        let core: Vec<String> = Stdlib::core()
            .package_declarations()
            .into_iter()
            .map(|defs| defs.package_name)
            .collect();
        let global: Vec<String> = stdlib_package_declarations()
            .into_iter()
            .map(|defs| defs.package_name)
            .collect();
        assert_eq!(core, global);
        assert!(!Stdlib::core().contains_module(test::MODULE_NAME));
    }
}
