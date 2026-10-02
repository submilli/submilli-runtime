//! The packages a blueprint's rules apply to: the declared ones, which `main`
//! may import, and the packages they depend on, whose calls run as their own
//! callers without a `packages:` entry.

use std::collections::{BTreeMap, BTreeSet};

use submilli_blueprint::Blueprint;
use submilli_build::{Artifact, PackageStore};

#[derive(Default)]
pub(super) struct DeclaredPackages {
    /// Every package that loaded, declared or a dependency, by name.
    pub artifacts: BTreeMap<String, Artifact>,
    /// Undeclared packages a declared one depends on, loaded or not.
    pub dependencies: BTreeSet<String>,
    /// One message per declared package whose dependency closure failed to
    /// load.
    pub errors: Vec<String>,
}

/// Loads every declared package and each installed package they depend on.
/// A declared package whose dependency closure is incomplete gets an entry in
/// `errors`; what did load keeps its checks.
pub(super) fn load(blueprint: &Blueprint, store: &PackageStore) -> DeclaredPackages {
    let mut loaded = DeclaredPackages::default();
    let mut visited = BTreeSet::new();
    for package in &blueprint.packages {
        if let Err(err) = store.load_closure([package.as_str()]) {
            loaded.errors.push(format!(
                "cannot validate package `{package}` capabilities: {err}"
            ));
        }
        loaded.load_reachable(blueprint, store, package, &mut visited);
    }
    loaded
}

/// Whether `artifact` declares `name` as a direct dependency.
pub(super) fn depends_on(artifact: &Artifact, name: &str) -> bool {
    artifact
        .metadata
        .dependencies
        .iter()
        .any(|dependency| dependency.name == name)
}

impl DeclaredPackages {
    fn load_reachable(
        &mut self,
        blueprint: &Blueprint,
        store: &PackageStore,
        root: &str,
        visited: &mut BTreeSet<String>,
    ) {
        let mut pending = vec![root.to_string()];
        while let Some(name) = pending.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            if !blueprint.packages.contains(&name) {
                self.dependencies.insert(name.clone());
            }
            let Ok(artifact) = store.load(&name) else {
                continue;
            };
            pending.extend(
                artifact
                    .metadata
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.name.clone()),
            );
            self.artifacts.insert(name, artifact);
        }
    }
}
