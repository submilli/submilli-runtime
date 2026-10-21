//! The packages a run links: only the blueprint-listed packages the program
//! imports, with their dependency closure, compiled once and kept until the cache
//! is cleared.
//!
//! Copied from `submilli-server`'s `app.rs` (`prepared_packages_for_imports`,
//! `PreparedBlueprintPackages`, `PreparePackagesError`) and, for the versions a
//! run's finalizers are registered under, `session_manager.rs`
//! (`finalizer_provider`) and `finalizers.rs` (`dependency_names`). The host
//! serves one blueprint, so the cache needs no per-blueprint eviction: anything
//! that may change what a run resolves clears it all.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use interpreter::{PackageDeclaration, ScriptImports};
use submilli_blueprint::Blueprint;
use submilli_build::{ArtifactMetadata, PackageStore, PackageStoreError};
use wasmtime::{Engine, Module};

/// The packages one run links.
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedPackages {
    pub(crate) stdlib_declarations: Vec<PackageDeclaration>,
    /// Declarations of the blueprint-listed packages: the program's importable
    /// surface. Closure-only dependencies are linked but not importable.
    pub(crate) script_declarations: Vec<PackageDeclaration>,
    /// The full dependency closure in instantiation (topological) order.
    pub(crate) modules: Vec<PreparedPackageModule>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedPackageModule {
    pub(crate) module: Module,
    pub(crate) declaration: PackageDeclaration,
    pub(crate) type_info: interpreter::TypeInfoTable,
    pub(crate) metadata: ArtifactMetadata,
    pub(crate) sources: Vec<submilli_build::ArtifactSource>,
}

#[derive(Debug)]
pub(crate) enum PreparePackagesError {
    Store(PackageStoreError),
    Module {
        name: String,
        /// Where the artifact was read from, when the store could still say.
        package_dir: Option<PathBuf>,
        source: wasmtime::Error,
    },
}

impl fmt::Display for PreparePackagesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(err) => err.fmt(f),
            Self::Module {
                name,
                package_dir: Some(package_dir),
                source,
            } => write!(
                f,
                "failed to compile package `{name}` wasm from {}: {source}",
                package_dir.display()
            ),
            Self::Module {
                name,
                package_dir: None,
                source,
            } => write!(f, "failed to compile package `{name}` wasm: {source}"),
        }
    }
}

impl std::error::Error for PreparePackagesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(err) => Some(err),
            Self::Module { .. } => None,
        }
    }
}

impl From<PackageStoreError> for PreparePackagesError {
    fn from(value: PackageStoreError) -> Self {
        Self::Store(value)
    }
}

/// Prepared package sets by what a run imports.
pub(crate) struct PackageCache {
    store: PackageStore,
    sets: Mutex<HashMap<String, Arc<PreparedPackages>>>,
    /// Bumped by every clear, so a set read from the store while one landed is
    /// served once and not kept.
    generation: AtomicU64,
}

impl PackageCache {
    pub(crate) fn new(store: PackageStore) -> Self {
        Self {
            store,
            sets: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
        }
    }

    pub(crate) fn store(&self) -> &PackageStore {
        &self.store
    }

    /// The packages a program with `imports` links under `blueprint`: the
    /// blueprint-listed packages it imports, their closure, and the standard
    /// library modules it imports.
    pub(crate) fn prepare_for_imports(
        &self,
        engine: &Engine,
        blueprint: &Blueprint,
        imports: &ScriptImports,
    ) -> Result<Arc<PreparedPackages>, PreparePackagesError> {
        let registry_roots: BTreeSet<String> = imports
            .registry_packages
            .iter()
            .filter(|package| blueprint.packages.contains(*package))
            .cloned()
            .collect();
        if registry_roots.is_empty() && imports.stdlib.is_empty() {
            return Ok(Arc::new(PreparedPackages::default()));
        }
        let key = format!(
            "{}\u{1e}{}\u{1e}git={}",
            join_key_parts(&registry_roots),
            join_key_parts(&imports.stdlib),
            blueprint.git.is_some()
        );
        if let Some(cached) = self.lock().get(&key) {
            return Ok(Arc::clone(cached));
        }
        let generation = self.generation.load(Ordering::Acquire);
        let mut selected = self.prepare_selected(engine, &registry_roots, &imports.stdlib)?;
        if blueprint.git.is_none() {
            selected
                .stdlib_declarations
                .retain(|decl| decl.package_name != "submilli:git");
        }
        let prepared = Arc::new(selected);
        let mut sets = self.lock();
        if self.generation.load(Ordering::Acquire) != generation {
            return Ok(prepared);
        }
        Ok(Arc::clone(sets.entry(key).or_insert(prepared)))
    }

    /// Drop every prepared set, so the next run loads its packages from the store
    /// again: after an install, or a blueprint change.
    pub(crate) fn clear(&self) {
        let mut sets = self.lock();
        // Under the lock, so a prepare that checks the generation while holding
        // it sees the bump and the cleared map together.
        self.generation.fetch_add(1, Ordering::AcqRel);
        sets.clear();
    }

    fn prepare_selected(
        &self,
        engine: &Engine,
        registry_roots: &BTreeSet<String>,
        stdlib_names: &BTreeSet<String>,
    ) -> Result<PreparedPackages, PreparePackagesError> {
        let stdlib_declarations = interpreter::runtime::stdlib_package_declarations()
            .into_iter()
            .filter(|defs| stdlib_names.contains(&defs.package_name))
            .collect();
        let mut script_declarations = Vec::with_capacity(registry_roots.len());
        let artifacts = self
            .store
            .load_closure(registry_roots.iter().map(String::as_str))?;
        let mut modules = Vec::with_capacity(artifacts.len());
        for artifact in artifacts {
            let name = artifact.metadata.package_name.clone();
            let module = Module::new(engine, &artifact.wasm).map_err(|source| {
                let package_dir = self
                    .store
                    .locate(&name)
                    .ok()
                    .flatten()
                    .map(|located| located.dir);
                PreparePackagesError::Module {
                    name: name.clone(),
                    package_dir,
                    source,
                }
            })?;
            if registry_roots.contains(&name) {
                script_declarations.push(artifact.package_declaration.clone());
            }
            modules.push(PreparedPackageModule {
                module,
                declaration: artifact.package_declaration,
                type_info: artifact.type_info,
                metadata: artifact.metadata,
                sources: artifact.sources,
            });
        }
        Ok(PreparedPackages {
            stdlib_declarations,
            script_declarations,
            modules,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<PreparedPackages>>> {
        // Poison means a panic interrupted an insert or a clear. AGENTS.md permits
        // the poisoned-lock panic rather than serving a set that may be stale.
        self.sets.lock().expect("prepared package cache poisoned")
    }
}

fn join_key_parts(parts: &BTreeSet<String>) -> String {
    parts.iter().cloned().collect::<Vec<_>>().join("\u{1f}")
}

/// For each linked package that exports a `finalizer`: the versions of it and
/// every package it depends on, as the JSON object its registrations are stored
/// with. A finalizer later runs only against those versions.
pub(crate) fn finalizer_package_versions(
    packages: &PreparedPackages,
) -> Result<BTreeMap<String, String>, String> {
    let mut package_versions = BTreeMap::new();
    for module in &packages.modules {
        if !module.declaration.values.contains_key("finalizer") {
            continue;
        }
        let required = dependency_names(packages, &module.metadata.package_name)?;
        let versions: BTreeMap<_, _> = packages
            .modules
            .iter()
            .filter(|module| required.contains(&module.metadata.package_name))
            .map(|module| {
                (
                    module.metadata.package_name.clone(),
                    module.metadata.package_version.clone(),
                )
            })
            .collect();
        let versions = serde_json::to_string(&versions).map_err(|error| error.to_string())?;
        package_versions.insert(module.metadata.package_name.clone(), versions);
    }
    Ok(package_versions)
}

/// `root` and every package it depends on, directly or not.
fn dependency_names(packages: &PreparedPackages, root: &str) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(name) = pending.pop() {
        if !names.insert(name.clone()) {
            continue;
        }
        let module = packages
            .modules
            .iter()
            .find(|module| module.metadata.package_name == name)
            .ok_or_else(|| "missing finalizer dependency".to_owned())?;
        pending.extend(
            module
                .metadata
                .dependencies
                .iter()
                .map(|dependency| dependency.name.clone()),
        );
    }
    Ok(names)
}

#[cfg(test)]
pub(super) mod tests;
