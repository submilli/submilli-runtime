use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::artifact::{ArtifactReadBudget, ArtifactReadLimits, read_package_artifact_with_budget};
use crate::{Artifact, ArtifactError};

// Bound recursive dependency traversal before it can exhaust the host stack.
const MAX_DEPENDENCY_DEPTH: usize = 128;

/// An on-disk package store: one `@scope/name` directory per artifact under a
/// root the store owns, optionally layered over read-only fallback roots.
///
/// Writes (`package_dir`) always target the owned root. Reads search the owned
/// root first, then each fallback in order, so an owned copy shadows a fallback
/// copy. The server uses this to read packages the CLI published locally
/// without ever writing into the CLI's store.
#[derive(Clone, Debug)]
pub struct PackageStore {
    root: PathBuf,
    fallbacks: Vec<PathBuf>,
}

/// A package directory found by a search across the store's roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedPackage {
    pub name: String,
    /// The root it was found under: the owned root or one of the fallbacks.
    pub root: PathBuf,
    pub dir: PathBuf,
}

impl PackageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            fallbacks: Vec::new(),
        }
    }

    /// Add a read-only root searched after the owned root (and any fallback
    /// added earlier). Nothing is ever written under it. A root spelled
    /// identically to one the store already searches is ignored, so an
    /// operator who points the owned store at the fallback's path does not get
    /// every lookup and listing doubled.
    pub fn with_fallback(mut self, root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if !self.roots().any(|known| known == root) {
            self.fallbacks.push(root);
        }
        self
    }

    /// The root writes go to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every root in search order: the owned root, then the fallbacks.
    pub fn roots(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.root.as_path()).chain(self.fallbacks.iter().map(PathBuf::as_path))
    }

    /// Whether `root` is the owned root rather than a fallback.
    pub fn owns(&self, root: &Path) -> bool {
        root == self.root
    }

    /// The directory `name` is written to — always under the owned root.
    pub fn package_dir(&self, name: &str) -> Result<PathBuf, PackageStoreError> {
        package_dir_under(&self.root, name)
    }

    /// The first root, in search order, holding a directory for `name`.
    /// An inaccessible or non-directory entry is an error, not a fallback miss.
    pub fn locate(&self, name: &str) -> Result<Option<LocatedPackage>, PackageStoreError> {
        for root in self.roots() {
            let dir = package_dir_under(root, name)?;
            if package_directory_exists(name, &dir)? {
                return Ok(Some(LocatedPackage {
                    name: name.to_string(),
                    root: root.to_path_buf(),
                    dir,
                }));
            }
        }
        Ok(None)
    }

    /// Load `name` from the first root that has it. A copy that exists but
    /// fails to load is an error, never a reason to fall through to the next
    /// root: silently running a different copy than the one on disk would be
    /// worse than refusing.
    pub fn load(&self, name: &str) -> Result<Artifact, PackageStoreError> {
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
        let mut rebuild = RebuildState::default();
        self.load_with_rebuild(name, &mut budget, &mut rebuild)
    }

    pub(crate) fn load_with_budget(
        &self,
        name: &str,
        budget: &mut ArtifactReadBudget,
    ) -> Result<Artifact, PackageStoreError> {
        let mut rebuild = RebuildState::default();
        self.load_with_rebuild(name, budget, &mut rebuild)
    }

    /// Rebuild every installed package whose wasm was produced by another
    /// compiler, in dependency order. A package that cannot be rebuilt is
    /// reported and skipped; packages that do not depend on it are still rebuilt.
    ///
    /// Does not execute stale wasm. A fallback package is published into the
    /// owned root — the fallback itself is never written.
    pub fn reconcile_installed(&self) -> PackageReconcile {
        let mut state = RebuildState::default();
        let mut failures = Vec::new();
        for name in self.available_packages() {
            if let Err(error) = self.ensure_current(&name, &mut state) {
                failures.push(PackageUnavailable {
                    name,
                    compiler: crate::rebuild::current_compiler().to_string(),
                    detail: error.to_string(),
                });
            }
        }
        PackageReconcile {
            rebuilt: state.rebuilt,
            failures,
        }
    }

    fn load_with_rebuild(
        &self,
        name: &str,
        budget: &mut ArtifactReadBudget,
        rebuild: &mut RebuildState,
    ) -> Result<Artifact, PackageStoreError> {
        self.ensure_current(name, rebuild)?;
        // Charge the caller's budget by reading the store. The rebuild cache is
        // only a substitute when the current wasm lives outside the package
        // roots, as a fallback rebuild does.
        let mut saw_stale = false;
        for root in self.roots() {
            if let Some(artifact) = self.load_from(root, name, budget)? {
                if !crate::rebuild::needs_rebuild(&artifact.metadata) {
                    return Ok(artifact);
                }
                saw_stale = true;
                break;
            }
        }
        if let Some(artifact) = rebuild.artifacts.get(name)
            && !crate::rebuild::needs_rebuild(&artifact.metadata)
        {
            let cache = crate::rebuild::cache_dir(self.root(), name)?;
            return crate::artifact::read_package_artifact_with_budget(&cache, budget).map_err(
                |source| PackageStoreError::Artifact {
                    name: name.to_string(),
                    package_dir: cache,
                    source,
                },
            );
        }
        if saw_stale {
            return Err(self.unavailable(
                name,
                "installed wasm does not match the compiler; refusing to run it",
            ));
        }
        Err(self.missing(name, self.roots().map(Path::to_path_buf).collect()))
    }

    /// Load `name` from the owned root only, ignoring fallbacks. Install-time
    /// decisions (conflicts, upgrades, lockfile satisfaction) use this so the
    /// owned store stays self-contained.
    pub fn load_owned(&self, name: &str) -> Result<Artifact, PackageStoreError> {
        crate::rebuild::recover_exclusive(self.root(), name)?;
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
        match self.load_from(&self.root, name, &mut budget)? {
            Some(artifact) => Ok(artifact),
            None => Err(self.missing(name, vec![self.root.clone()])),
        }
    }

    /// Read `name` under one root. `Ok(None)` only when that root has no
    /// directory for the package at all — the same test [`Self::locate`]
    /// applies. A directory that exists but is missing files (an interrupted
    /// install, a hand-deleted artifact) is the artifact's own error, so a
    /// half-written owned copy is never silently replaced by a fallback one.
    fn load_from(
        &self,
        root: &Path,
        name: &str,
        budget: &mut ArtifactReadBudget,
    ) -> Result<Option<Artifact>, PackageStoreError> {
        let dir = package_dir_under(root, name)?;
        if !package_directory_exists(name, &dir)? {
            return Ok(None);
        }
        let artifact = read_package_artifact_with_budget(&dir, budget).map_err(|source| {
            PackageStoreError::Artifact {
                name: name.to_string(),
                package_dir: dir.clone(),
                source,
            }
        })?;
        validate_artifact_name(name, &dir, artifact).map(Some)
    }

    /// The hint lists only what the failed lookup could have found: an
    /// owned-only lookup must not advertise fallback packages.
    fn missing(&self, name: &str, searched_roots: Vec<PathBuf>) -> PackageStoreError {
        let available = searched_roots
            .iter()
            .flat_map(|root| packages_under(root))
            .map(|(name, _)| name)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        PackageStoreError::MissingPackage {
            name: name.to_string(),
            searched_roots,
            available,
        }
    }

    pub fn load_many<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<Artifact>, PackageStoreError> {
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
        names
            .into_iter()
            .map(|name| self.load_with_budget(name, &mut budget))
            .collect()
    }

    /// Load the named packages plus their transitive dependencies (from
    /// artifact metadata), topologically ordered: dependencies before
    /// dependents. This is the order package modules must be instantiated
    /// into the linker, since a package's wasm imports resolve against its
    /// dependencies' already-registered instances.
    pub fn load_closure<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<Artifact>, PackageStoreError> {
        self.load_closure_with_limits(names, ArtifactReadLimits::default())
    }

    fn load_closure_with_limits<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
        limits: ArtifactReadLimits,
    ) -> Result<Vec<Artifact>, PackageStoreError> {
        let mut walk = ClosureWalk {
            marks: BTreeMap::new(),
            stack: Vec::new(),
            order: Vec::new(),
            budget: ArtifactReadBudget::new(limits),
            rebuild: RebuildState::default(),
        };
        for name in names {
            self.visit_closure(name, None, &mut walk)?;
        }
        Ok(walk.order)
    }

    fn visit_closure(
        &self,
        name: &str,
        required_by: Option<(&str, &str)>,
        walk: &mut ClosureWalk,
    ) -> Result<(), PackageStoreError> {
        match walk.marks.get(name) {
            Some(ClosureMark::Done { version }) => {
                return check_edge_version(name, required_by, version);
            }
            Some(ClosureMark::Visiting) => {
                let start = walk.stack.iter().position(|n| n == name).unwrap_or(0);
                let mut cycle: Vec<_> = walk.stack.iter().skip(start).cloned().collect();
                cycle.push(name.to_string());
                return Err(PackageStoreError::DependencyCycle { cycle });
            }
            None => {}
        }
        if walk.stack.len() >= MAX_DEPENDENCY_DEPTH {
            return Err(PackageStoreError::DependencyDepthExceeded {
                name: name.to_string(),
                limit: MAX_DEPENDENCY_DEPTH,
            });
        }
        let artifact = self
            .load_with_rebuild(name, &mut walk.budget, &mut walk.rebuild)
            .map_err(|err| match (err, required_by) {
                (
                    PackageStoreError::MissingPackage {
                        searched_roots,
                        available,
                        ..
                    },
                    Some((dependent, _)),
                ) => PackageStoreError::MissingDependency {
                    name: name.to_string(),
                    required_by: dependent.to_string(),
                    searched_roots,
                    available,
                },
                (err, _) => err,
            })?;
        check_edge_version(name, required_by, &artifact.metadata.package_version)?;
        walk.marks.insert(name.to_string(), ClosureMark::Visiting);
        walk.stack.push(name.to_string());
        let dependencies = artifact.metadata.dependencies.clone();
        for dep in &dependencies {
            self.visit_closure(&dep.name, Some((name, &dep.version)), walk)?;
        }
        walk.stack.pop();
        walk.marks.insert(
            name.to_string(),
            ClosureMark::Done {
                version: artifact.metadata.package_version.clone(),
            },
        );
        walk.order.push(artifact);
        Ok(())
    }

    /// Every package name present under any root, in name order, each once.
    pub fn available_packages(&self) -> Vec<String> {
        self.locate_all()
            .into_iter()
            .map(|located| located.name)
            .collect()
    }

    /// Every package with the root it resolves from, in name order. A name
    /// present under several readable roots is reported once, from the first
    /// root in search order. Unlike [`Self::load`], this listing is best-effort: a root or
    /// scope directory that cannot be read contributes nothing rather than
    /// hiding the rest: the fallback is a directory the server does not own,
    /// and one bad entry there must not empty the listing.
    pub fn locate_all(&self) -> Vec<LocatedPackage> {
        let mut located: BTreeMap<String, LocatedPackage> = BTreeMap::new();
        for root in self.roots() {
            for (name, dir) in packages_under(root) {
                located
                    .entry(name.clone())
                    .or_insert_with(|| LocatedPackage {
                        name,
                        root: root.to_path_buf(),
                        dir,
                    });
            }
        }
        located.into_values().collect()
    }

    fn ensure_current(
        &self,
        name: &str,
        state: &mut RebuildState,
    ) -> Result<Ensure, PackageStoreError> {
        if let Some(finished) = state.finished.get(name) {
            return match finished {
                Finished::Current => Ok(Ensure::Current),
                Finished::Rebuilt => Ok(Ensure::Rebuilt),
                Finished::Failed(detail) => Err(self.unavailable(name, detail.clone())),
            };
        }
        if state.visiting.iter().any(|visiting| visiting == name) {
            let start = state
                .visiting
                .iter()
                .position(|visiting| visiting == name)
                .unwrap_or(0);
            let mut cycle: Vec<_> = state.visiting.iter().skip(start).cloned().collect();
            cycle.push(name.to_string());
            return Err(PackageStoreError::DependencyCycle { cycle });
        }
        if state.visiting.len() >= MAX_DEPENDENCY_DEPTH {
            return Err(PackageStoreError::DependencyDepthExceeded {
                name: name.to_string(),
                limit: MAX_DEPENDENCY_DEPTH,
            });
        }
        crate::rebuild::recover_exclusive(self.root(), name)?;
        let Some(located) = self.locate(name)? else {
            return Err(self.missing(name, self.roots().map(Path::to_path_buf).collect()));
        };
        let identity = match crate::artifact::read_package_identity(&located.dir) {
            Ok(identity) => identity,
            // A directory with no metadata is an interrupted install, not a stale
            // wasm. Keep that as an artifact error so a new install can replace it.
            Err(source) if is_missing_file(&source) => {
                return Err(PackageStoreError::Artifact {
                    name: name.to_string(),
                    package_dir: located.dir,
                    source,
                });
            }
            Err(source) => {
                return self.fail_rebuild(
                    name,
                    state,
                    format!("could not read retained sources: {source}"),
                );
            }
        };
        if identity.metadata.package_name != name {
            return Err(PackageStoreError::NameMismatch {
                requested: name.to_string(),
                package_dir: located.dir,
                metadata_name: identity.metadata.package_name.clone(),
                declaration_name: identity.metadata.package_name,
            });
        }
        if !crate::rebuild::needs_rebuild(&identity.metadata) {
            crate::rebuild::clear_failure(self.root(), name)?;
            self.remember(name, state)?;
            state.finished.insert(name.to_string(), Finished::Current);
            return Ok(Ensure::Current);
        }
        // Stay on the visiting stack through the closure walk. A current
        // package does not walk its own dependencies, so this name must still
        // be visible when that walk re-enters it.
        state.visiting.push(name.to_string());
        let result = self.rebuild_visiting(name, &located, &identity, state);
        state.visiting.pop();
        result
    }

    fn rebuild_visiting(
        &self,
        name: &str,
        located: &LocatedPackage,
        identity: &crate::artifact::PackageIdentity,
        state: &mut RebuildState,
    ) -> Result<Ensure, PackageStoreError> {
        if let Err(error) = self.ensure_dependencies(name, identity, state) {
            return self.keep_structural(name, state, error);
        }
        let dependencies = match self.artifacts_for_compile(&identity.metadata.dependencies, state)
        {
            Ok(dependencies) => dependencies,
            Err(error) => return self.keep_structural(name, state, error),
        };
        let dependency_key = crate::rebuild::dependency_key(&dependencies);
        let cache_key = crate::rebuild::cache_key(
            &identity.sources,
            &dependency_key,
            &identity.metadata.config.overrides,
        );
        if !self.owns(&located.root)
            && let Some(cached) = self.cached_fallback(name, &cache_key)?
        {
            state.artifacts.insert(name.to_string(), cached);
            state.finished.insert(name.to_string(), Finished::Current);
            return Ok(Ensure::Current);
        }
        if let Some(detail) =
            crate::rebuild::recorded_failure(self.root(), name, &identity.sources, &dependency_key)?
        {
            return self.fail_rebuild(name, state, detail);
        }
        let compiled = match crate::rebuild::recompile(identity, &dependencies) {
            Ok(compiled) => compiled,
            Err(detail) => {
                crate::rebuild::record_failure(
                    self.root(),
                    name,
                    &identity.sources,
                    &dependency_key,
                    &detail,
                )?;
                return self.fail_rebuild(name, state, detail);
            }
        };
        // A fallback package is cached under the owned root. Publishing it as
        // a real package directory would shadow a later CLI upgrade and undo
        // a delete that was supposed to fall back.
        let live = if self.owns(&located.root) {
            self.package_dir(name)?
        } else {
            crate::rebuild::cache_dir(self.root(), name)?
        };
        let slot = if self.owns(&located.root) {
            "owned"
        } else {
            "cache"
        };
        match crate::rebuild::publish(
            self.root(),
            name,
            &live,
            &compiled,
            slot,
            (!self.owns(&located.root)).then_some(cache_key.as_str()),
        )? {
            crate::rebuild::PublishOutcome::Published => {
                crate::rebuild::clear_failure(self.root(), name)?;
                state.rebuilt.push(name.to_string());
            }
            crate::rebuild::PublishOutcome::AlreadyCurrent => {}
            crate::rebuild::PublishOutcome::Removed => {
                return self.fail_rebuild(
                    name,
                    state,
                    "package was removed during rebuild".to_string(),
                );
            }
        }
        let published = crate::artifact::read_package_artifact(&live).map_err(|source| {
            PackageStoreError::Artifact {
                name: name.to_string(),
                package_dir: live,
                source,
            }
        })?;
        state.artifacts.insert(name.to_string(), published);
        state.finished.insert(name.to_string(), Finished::Rebuilt);
        Ok(Ensure::Rebuilt)
    }

    /// Direct dependencies plus the rest of their closure. A current package
    /// does not walk its own dependencies, so a stale dependent must load those
    /// transitive artifacts itself.
    fn artifacts_for_compile(
        &self,
        direct: &[crate::ArtifactDependency],
        state: &mut RebuildState,
    ) -> Result<Vec<Artifact>, PackageStoreError> {
        let mut queue: Vec<String> = direct
            .iter()
            .map(|dependency| dependency.name.clone())
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        let mut artifacts = Vec::new();
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if !state.artifacts.contains_key(&name) {
                self.ensure_current(&name, state)?;
            }
            let Some(artifact) = state.artifacts.get(&name) else {
                return Err(
                    self.unavailable(&name, "dependency is not available to rebuild against")
                );
            };
            queue.extend(
                artifact
                    .metadata
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.name.clone()),
            );
            artifacts.push(artifact.clone());
        }
        Ok(artifacts)
    }

    fn cached_fallback(
        &self,
        name: &str,
        cache_key: &str,
    ) -> Result<Option<Artifact>, PackageStoreError> {
        let cache = crate::rebuild::cache_dir(self.root(), name)?;
        if !crate::artifact::artifact_dir_is_complete(&cache) {
            return Ok(None);
        }
        let Ok(key) = std::fs::read_to_string(cache.join("cache-key")) else {
            return Ok(None);
        };
        if key != cache_key {
            return Ok(None);
        }
        match crate::artifact::read_package_artifact(&cache) {
            Ok(artifact)
                if artifact.metadata.package_name == name
                    && !crate::rebuild::needs_rebuild(&artifact.metadata) =>
            {
                Ok(Some(artifact))
            }
            _ => Ok(None),
        }
    }

    fn ensure_dependencies(
        &self,
        name: &str,
        identity: &crate::artifact::PackageIdentity,
        state: &mut RebuildState,
    ) -> Result<(), PackageStoreError> {
        for dependency in &identity.metadata.dependencies {
            self.ensure_current(&dependency.name, state)?;
            if let Some(artifact) = state.artifacts.get(&dependency.name)
                && artifact.metadata.package_version != dependency.version
            {
                return Err(PackageStoreError::DependencyVersionMismatch {
                    name: dependency.name.clone(),
                    required_by: name.to_string(),
                    required: dependency.version.clone(),
                    found: artifact.metadata.package_version.clone(),
                });
            }
        }
        Ok(())
    }

    fn keep_structural(
        &self,
        name: &str,
        state: &mut RebuildState,
        error: PackageStoreError,
    ) -> Result<Ensure, PackageStoreError> {
        if matches!(
            error,
            PackageStoreError::DependencyCycle { .. }
                | PackageStoreError::DependencyDepthExceeded { .. }
        ) {
            return Err(error);
        }
        self.fail_rebuild(name, state, error.to_string())
    }

    fn fail_rebuild(
        &self,
        name: &str,
        state: &mut RebuildState,
        detail: String,
    ) -> Result<Ensure, PackageStoreError> {
        state
            .finished
            .insert(name.to_string(), Finished::Failed(detail.clone()));
        Err(self.unavailable(name, detail))
    }

    fn remember(&self, name: &str, state: &mut RebuildState) -> Result<(), PackageStoreError> {
        if state.artifacts.contains_key(name) {
            return Ok(());
        }
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
        for root in self.roots() {
            if let Some(artifact) = self.load_from(root, name, &mut budget)? {
                if crate::rebuild::needs_rebuild(&artifact.metadata) {
                    return Err(self.unavailable(
                        name,
                        "installed wasm does not match the compiler; refusing to run it",
                    ));
                }
                state.artifacts.insert(name.to_string(), artifact);
                return Ok(());
            }
        }
        Err(self.missing(name, self.roots().map(Path::to_path_buf).collect()))
    }

    fn unavailable(&self, name: &str, detail: impl Into<String>) -> PackageStoreError {
        PackageStoreError::Unavailable {
            name: name.to_string(),
            compiler: crate::rebuild::current_compiler().to_string(),
            detail: detail.into(),
        }
    }
}

/// The `@scope/leaf` directory for `name` under `root`, validating the name.
pub(crate) fn package_dir_under(root: &Path, name: &str) -> Result<PathBuf, PackageStoreError> {
    let (scope, package) =
        split_scoped_name(name).ok_or_else(|| PackageStoreError::InvalidPackageName {
            name: name.to_string(),
        })?;
    Ok(root.join(scope).join(package))
}

/// Only an absent package permits searching the next root. Keep metadata
/// errors visible so an inaccessible owned copy cannot select fallback code.
fn package_directory_exists(name: &str, dir: &Path) -> Result<bool, PackageStoreError> {
    let source = match fs::metadata(dir) {
        Ok(metadata) if metadata.is_dir() => return Ok(true),
        Ok(_) => io::Error::new(
            io::ErrorKind::NotADirectory,
            "package path is not a directory",
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => error,
    };
    Err(PackageStoreError::Artifact {
        name: name.to_string(),
        package_dir: dir.to_path_buf(),
        source: ArtifactError::Io {
            path: dir.to_path_buf(),
            source,
        },
    })
}

/// Package names and directories laid out as `@scope/leaf` under one root.
/// Anything unreadable — the root itself (typically absent), a scope, or an
/// entry — is skipped rather than reported.
fn packages_under(root: &Path) -> Vec<(String, PathBuf)> {
    let mut packages = Vec::new();
    let Ok(scopes) = fs::read_dir(root) else {
        return packages;
    };
    for scope in scopes.flatten() {
        let scope_name = scope.file_name().to_string_lossy().into_owned();
        if !scope_name.starts_with('@') || !scope.path().is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(scope.path()) else {
            continue;
        };
        for package in entries.flatten() {
            if package.path().is_dir() {
                let name = format!("{scope_name}/{}", package.file_name().to_string_lossy());
                packages.push((name, package.path()));
            }
        }
    }
    packages
}

impl Default for PackageStore {
    fn default() -> Self {
        Self::new(default_package_store_dir())
    }
}

enum ClosureMark {
    Visiting,
    Done { version: String },
}

fn check_edge_version(
    name: &str,
    required_by: Option<(&str, &str)>,
    found: &str,
) -> Result<(), PackageStoreError> {
    match required_by {
        Some((dependent, required)) if required != found => {
            Err(PackageStoreError::DependencyVersionMismatch {
                name: name.to_string(),
                required_by: dependent.to_string(),
                required: required.to_string(),
                found: found.to_string(),
            })
        }
        _ => Ok(()),
    }
}

#[derive(Debug)]
pub enum PackageStoreError {
    InvalidPackageName {
        name: String,
    },
    MissingPackage {
        name: String,
        /// Every root the lookup searched, in search order.
        searched_roots: Vec<PathBuf>,
        available: Vec<String>,
    },
    Artifact {
        name: String,
        package_dir: PathBuf,
        source: ArtifactError,
    },
    NameMismatch {
        requested: String,
        package_dir: PathBuf,
        metadata_name: String,
        declaration_name: String,
    },
    MissingDependency {
        name: String,
        required_by: String,
        searched_roots: Vec<PathBuf>,
        available: Vec<String>,
    },
    DependencyCycle {
        cycle: Vec<String>,
    },
    DependencyDepthExceeded {
        name: String,
        limit: usize,
    },
    DependencyVersionMismatch {
        name: String,
        required_by: String,
        required: String,
        found: String,
    },
    /// The installed wasm is not from this compiler, and it could not be rebuilt.
    /// The stale module is not returned.
    Unavailable {
        name: String,
        compiler: String,
        detail: String,
    },
}

/// What [`PackageStore::reconcile_installed`] did. Failures do not hide packages
/// that rebuilt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageReconcile {
    pub rebuilt: Vec<String>,
    pub failures: Vec<PackageUnavailable>,
}

/// One installed package that cannot be loaded under the current compiler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageUnavailable {
    pub name: String,
    pub compiler: String,
    pub detail: String,
}

enum Ensure {
    Current,
    Rebuilt,
}

enum Finished {
    Current,
    Rebuilt,
    Failed(String),
}

#[derive(Default)]
struct RebuildState {
    finished: BTreeMap<String, Finished>,
    artifacts: BTreeMap<String, Artifact>,
    visiting: Vec<String>,
    rebuilt: Vec<String>,
}

struct ClosureWalk {
    marks: BTreeMap<String, ClosureMark>,
    stack: Vec<String>,
    order: Vec<Artifact>,
    budget: ArtifactReadBudget,
    rebuild: RebuildState,
}

impl fmt::Display for PackageStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageStoreError::InvalidPackageName { name } => {
                write!(f, "invalid package name `{name}`; expected `@scope/name`")
            }
            PackageStoreError::MissingPackage {
                name,
                searched_roots,
                available,
            } => {
                write!(
                    f,
                    "package `{name}` was not found in {}",
                    join_roots(searched_roots)
                )?;
                write_available(f, available)?;
                write!(
                    f,
                    "; install it with `submilli install <github-url>`, or run \
                     `submilli build publish-local` in the project that provides `{name}`"
                )
            }
            PackageStoreError::Artifact {
                name,
                package_dir,
                source,
            } => write!(
                f,
                "failed to load package `{name}` from {}: {source}",
                package_dir.display()
            ),
            PackageStoreError::NameMismatch {
                requested,
                package_dir,
                metadata_name,
                declaration_name,
            } => write!(
                f,
                "package `{requested}` in {} has metadata name `{metadata_name}` and declaration name `{declaration_name}`",
                package_dir.display()
            ),
            PackageStoreError::MissingDependency {
                name,
                required_by,
                searched_roots,
                available,
            } => {
                write!(
                    f,
                    "package `{name}` (required by `{required_by}`) was not found in {}; run `submilli build` in the project that provides `{name}`",
                    join_roots(searched_roots)
                )?;
                write_available(f, available)
            }
            PackageStoreError::DependencyDepthExceeded { name, limit } => write!(
                f,
                "package dependency depth exceeds {limit} at `{name}`; reduce the dependency chain"
            ),
            PackageStoreError::DependencyCycle { cycle } => write!(
                f,
                "circular package dependency in the store: {}; rebuild the packages involved",
                cycle.join(" -> ")
            ),
            PackageStoreError::DependencyVersionMismatch {
                name,
                required_by,
                required,
                found,
            } => write!(
                f,
                "package `{required_by}` requires `{name}` {required}, but the store has {found}; rebuild `{name}` at {required} or rebuild `{required_by}` against {found}"
            ),
            PackageStoreError::Unavailable {
                name,
                compiler,
                detail,
            } => write!(
                f,
                "package `{name}` is unavailable under compiler {compiler}: {detail}"
            ),
        }
    }
}

/// `a`, `a or b`, `a, b, or c` — the roots a failed lookup searched.
fn join_roots(roots: &[PathBuf]) -> String {
    let shown: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    match shown.as_slice() {
        [] => "no package store".to_string(),
        [one] => one.clone(),
        [first, second] => format!("{first} or {second}"),
        [init @ .., last] => format!("{}, or {last}", init.join(", ")),
    }
}

fn write_available(f: &mut fmt::Formatter<'_>, available: &[String]) -> fmt::Result {
    if available.is_empty() {
        write!(f, "; no packages are available")
    } else {
        write!(f, "; available packages: {}", available.join(", "))
    }
}

impl PackageStoreError {
    /// Whether the package directory exists but lacks one of the artifact's
    /// files: what an interrupted install leaves behind. A fresh install may
    /// overwrite such a directory as if the package were absent.
    pub fn is_unsupported_schema(&self) -> bool {
        matches!(
            self,
            PackageStoreError::Artifact {
                source: ArtifactError::UnsupportedSchema { .. },
                ..
            }
        )
    }

    pub fn is_incomplete_artifact(&self) -> bool {
        matches!(
            self,
            PackageStoreError::Artifact {
                source: ArtifactError::Io { source, .. },
                ..
            } if source.kind() == io::ErrorKind::NotFound
        )
    }
}

impl std::error::Error for PackageStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PackageStoreError::Artifact { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn default_data_root() -> PathBuf {
    fn non_empty(var: &str) -> Option<PathBuf> {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }
    if let Some(dir) = non_empty("SUBMILLI_HOME") {
        return dir;
    }
    if let Some(home) = non_empty("HOME") {
        return home.join(".submilli");
    }
    std::env::temp_dir().join("submilli")
}

pub fn default_package_store_dir() -> PathBuf {
    default_data_root().join("packages")
}

/// Configuration recorded on an installed package, without executing its wasm.
/// Used when a compiler mismatch cannot be rebuilt.
pub fn saved_overrides(
    store: &PackageStore,
    name: &str,
) -> std::collections::BTreeMap<String, String> {
    store
        .locate(name)
        .ok()
        .flatten()
        .and_then(|located| crate::artifact::read_package_identity(&located.dir).ok())
        .map(|identity| identity.metadata.config.overrides)
        .unwrap_or_default()
}

fn is_missing_file(error: &ArtifactError) -> bool {
    matches!(
        error,
        ArtifactError::Io { source, .. } if source.kind() == io::ErrorKind::NotFound
    )
}

fn validate_artifact_name(
    requested: &str,
    package_dir: &Path,
    artifact: Artifact,
) -> Result<Artifact, PackageStoreError> {
    if artifact.metadata.package_name == requested
        && artifact.package_declaration.package_name == requested
    {
        return Ok(artifact);
    }
    Err(PackageStoreError::NameMismatch {
        requested: requested.to_string(),
        package_dir: package_dir.to_path_buf(),
        metadata_name: artifact.metadata.package_name,
        declaration_name: artifact.package_declaration.package_name,
    })
}

/// Split a validated `@org/leaf` name into its `@org` scope (the on-disk
/// directory) and `leaf`. Routes through the canonical org/leaf validator so the
/// store, blueprint, and build manifest agree on what a package name may be.
pub(crate) fn split_scoped_name(name: &str) -> Option<(&str, &str)> {
    let (org, leaf) = submilli_policy::validate_scoped_name(name).ok()?;
    // Keep the `@` prefix on the scope so the layout stays `@org/leaf` on disk.
    Some((&name[..org.len() + 1], leaf))
}

#[cfg(test)]
mod tests {
    use interpreter::PackageDeclaration;
    use tempfile::tempdir;

    use crate::{ArtifactMetadata, write_package_artifact};

    use super::*;

    #[test]
    fn dependency_depth_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..MAX_DEPENDENCY_DEPTH {
            let name = format!("@depth/p{index}");
            let next = format!("@depth/p{}", index + 1);
            let dependencies = if index + 1 < MAX_DEPENDENCY_DEPTH {
                vec![(next.as_str(), "0.0.0-test")]
            } else {
                Vec::new()
            };
            write_package_with_deps(dir.path(), &name, &name, "0.0.0-test", &dependencies);
        }
        let store = PackageStore::new(dir.path());
        assert_eq!(
            store.load_closure(["@depth/p0"]).unwrap().len(),
            MAX_DEPENDENCY_DEPTH
        );
        write_package_with_deps(
            dir.path(),
            "@depth/root",
            "@depth/root",
            "0.0.0-test",
            &[("@depth/p0", "0.0.0-test")],
        );
        assert!(matches!(
            store.load_closure(["@depth/root"]),
            Err(PackageStoreError::DependencyDepthExceeded {
                limit: MAX_DEPENDENCY_DEPTH,
                ..
            })
        ));
    }

    #[test]
    fn scoped_package_maps_under_store_root() {
        let tmp = tempdir().expect("tempdir");
        let store = PackageStore::new(tmp.path());

        let dir = store.package_dir("@pied-piper/util").expect("package dir");

        assert_eq!(dir, tmp.path().join("@pied-piper").join("util"));
    }

    #[test]
    fn missing_package_lists_available_packages() {
        let tmp = tempdir().expect("tempdir");
        write_package(tmp.path(), "@pied-piper/other", "@pied-piper/other");
        let store = PackageStore::new(tmp.path());

        let err = store.load("@pied-piper/util").expect_err("missing package");

        let text = err.to_string();
        assert!(text.contains("@pied-piper/util"), "got: {text}");
        assert!(text.contains(tmp.path().to_str().unwrap()), "got: {text}");
        assert!(text.contains("@pied-piper/other"), "got: {text}");
    }

    #[test]
    fn artifact_name_mismatch_is_rejected() {
        let tmp = tempdir().expect("tempdir");
        write_package(tmp.path(), "@pied-piper/util", "@pied-piper/wrong");
        let store = PackageStore::new(tmp.path());

        let err = store.load("@pied-piper/util").expect_err("name mismatch");

        assert!(matches!(err, PackageStoreError::NameMismatch { .. }));
    }

    fn write_package(root: &Path, dir_name: &str, package_name: &str) {
        write_package_with_deps(root, dir_name, package_name, "0.0.0-test", &[]);
    }

    fn write_package_with_deps(
        root: &Path,
        dir_name: &str,
        package_name: &str,
        version: &str,
        dependencies: &[(&str, &str)],
    ) {
        let mut declaration = PackageDeclaration::with_package(package_name);
        declaration.refresh_shapes();
        let dir = root.join(dir_name.split_once('/').unwrap().0).join(
            dir_name
                .split_once('/')
                .map(|(_, package)| package)
                .unwrap(),
        );
        let dependencies = dependencies
            .iter()
            .map(|(name, version)| crate::ArtifactDependency::new(*name, *version))
            .collect();
        let capabilities = crate::derive_capability_schema(&declaration, &[], &[]);
        let type_info = interpreter::TypeInfoTable {
            package_name: package_name.to_string(),
            types: Vec::new(),
        };
        write_package_artifact(
            dir,
            b"\0asm\x01\0\0\0",
            &type_info,
            &capabilities,
            &declaration,
            &ArtifactMetadata::new(package_name, version, dependencies),
        )
        .expect("write package artifact");
    }

    /// An owned root and one fallback root, each a fresh temp dir.
    fn layered() -> (tempfile::TempDir, tempfile::TempDir, PackageStore) {
        let owned = tempdir().expect("tempdir");
        let fallback = tempdir().expect("tempdir");
        let store = PackageStore::new(owned.path()).with_fallback(fallback.path());
        (owned, fallback, store)
    }

    #[test]
    fn fallback_root_is_searched_after_the_owned_root() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/util", "@pied-piper/util");

        let artifact = store.load("@pied-piper/util").expect("loads from fallback");
        let located = store
            .locate("@pied-piper/util")
            .expect("locate")
            .expect("found");

        assert_eq!(artifact.metadata.package_name, "@pied-piper/util");
        assert_eq!(located.root, fallback.path());
        assert_eq!(
            located.dir,
            fallback.path().join("@pied-piper").join("util")
        );
        assert!(!store.owns(&located.root));
        assert_eq!(
            store.package_dir("@pied-piper/util").unwrap(),
            owned.path().join("@pied-piper").join("util"),
            "writes still target the owned root"
        );
    }

    #[test]
    fn owned_copy_shadows_the_fallback_copy() {
        let (owned, fallback, store) = layered();
        write_package_with_deps(
            owned.path(),
            "@pied-piper/util",
            "@pied-piper/util",
            "2.0.0",
            &[],
        );
        write_package_with_deps(
            fallback.path(),
            "@pied-piper/util",
            "@pied-piper/util",
            "1.0.0",
            &[],
        );

        let artifact = store.load("@pied-piper/util").expect("loads");
        let located = store.locate("@pied-piper/util").unwrap().unwrap();

        assert_eq!(artifact.metadata.package_version, "2.0.0");
        assert_eq!(located.root, owned.path());
        assert!(store.owns(&located.root));
    }

    #[test]
    fn load_owned_ignores_the_fallback() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/util", "@pied-piper/util");

        let err = store.load_owned("@pied-piper/util").expect_err("not owned");

        let PackageStoreError::MissingPackage { searched_roots, .. } = &err else {
            panic!("expected MissingPackage, got {err:?}");
        };
        assert_eq!(searched_roots, &vec![owned.path().to_path_buf()]);
    }

    #[test]
    fn an_owned_only_miss_does_not_advertise_fallback_packages() {
        let (_owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/other", "@pied-piper/other");

        let err = store.load_owned("@pied-piper/util").expect_err("not owned");

        let PackageStoreError::MissingPackage { available, .. } = &err else {
            panic!("expected MissingPackage, got {err:?}");
        };
        assert!(available.is_empty(), "got: {available:?}");
    }

    #[test]
    fn a_broken_owned_copy_does_not_fall_through() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/util", "@pied-piper/util");
        let broken = owned.path().join("@pied-piper").join("util");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("metadata.json"), b"{ not json").unwrap();

        let err = store
            .load("@pied-piper/util")
            .expect_err("owned copy is broken");

        assert!(
            !matches!(err, PackageStoreError::MissingPackage { .. }),
            "got: {err}"
        );
    }

    #[test]
    fn an_empty_owned_package_dir_does_not_fall_through() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/util", "@pied-piper/util");
        // An interrupted install: the directory exists, the files do not.
        fs::create_dir_all(owned.path().join("@pied-piper").join("util")).unwrap();

        let err = store
            .load("@pied-piper/util")
            .expect_err("owned copy is incomplete");

        assert!(
            matches!(err, PackageStoreError::Artifact { .. }),
            "got: {err}"
        );
        let located = store.locate("@pied-piper/util").unwrap().unwrap();
        assert!(store.owns(&located.root), "locate agrees with load");
    }

    #[test]
    fn a_non_directory_owned_package_does_not_fall_through() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/util", "@pied-piper/util");
        let package = owned.path().join("@pied-piper/util");
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, b"not a directory").unwrap();

        assert_package_probe_error(
            store.locate("@pied-piper/util").unwrap_err(),
            &package,
            io::ErrorKind::NotADirectory,
        );
        assert_package_probe_error(
            store.load("@pied-piper/util").unwrap_err(),
            &package,
            io::ErrorKind::NotADirectory,
        );
        assert_package_probe_error(
            store.load_owned("@pied-piper/util").unwrap_err(),
            &package,
            io::ErrorKind::NotADirectory,
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_inaccessible_owned_scope_does_not_fall_through() {
        use std::os::unix::fs::PermissionsExt;

        let (owned, fallback, store) = layered();
        write_package(owned.path(), "@pied-piper/util", "@pied-piper/util");
        write_package(fallback.path(), "@pied-piper/util", "@pied-piper/util");
        let scope = owned.path().join("@pied-piper");
        let package = scope.join("util");
        let permissions = fs::metadata(&scope).unwrap().permissions();
        fs::set_permissions(&scope, fs::Permissions::from_mode(0o000)).unwrap();
        let denied = fs::metadata(&package)
            .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
        let located = store.locate("@pied-piper/util");
        let loaded = store.load("@pied-piper/util");
        let loaded_owned = store.load_owned("@pied-piper/util");
        // Restore before asserting so failed tests can clean up their directories.
        fs::set_permissions(&scope, permissions).unwrap();
        if !denied {
            return; // Root and some filesystems bypass mode-based access checks.
        }
        assert_package_probe_error(
            located.unwrap_err(),
            &package,
            io::ErrorKind::PermissionDenied,
        );
        assert_package_probe_error(
            loaded.unwrap_err(),
            &package,
            io::ErrorKind::PermissionDenied,
        );
        assert_package_probe_error(
            loaded_owned.unwrap_err(),
            &package,
            io::ErrorKind::PermissionDenied,
        );
    }

    fn assert_package_probe_error(error: PackageStoreError, package: &Path, kind: io::ErrorKind) {
        assert!(!error.is_incomplete_artifact(), "{error}");
        let PackageStoreError::Artifact {
            package_dir,
            source: ArtifactError::Io { path, source },
            ..
        } = error
        else {
            panic!("expected package I/O error, got {error}");
        };
        assert_eq!(package_dir, package);
        assert_eq!(path, package);
        assert_eq!(source.kind(), kind);
    }

    #[test]
    fn a_fallback_equal_to_the_owned_root_is_not_searched_twice() {
        let owned = tempdir().expect("tempdir");
        let store = PackageStore::new(owned.path()).with_fallback(owned.path());

        assert_eq!(store.roots().count(), 1);
    }

    #[test]
    fn a_scope_that_is_not_a_directory_hides_only_itself() {
        let (owned, fallback, store) = layered();
        write_package(owned.path(), "@pied-piper/util", "@pied-piper/util");
        // A file where a scope directory is expected.
        fs::write(fallback.path().join("@broken"), b"not a directory").unwrap();
        write_package(fallback.path(), "@zed/extra", "@zed/extra");

        let available = store.available_packages();

        assert_eq!(available, vec!["@pied-piper/util", "@zed/extra"]);
    }

    #[test]
    fn missing_package_names_every_searched_root() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@pied-piper/other", "@pied-piper/other");

        let err = store.load("@pied-piper/util").expect_err("missing");

        let text = err.to_string();
        assert!(text.contains(owned.path().to_str().unwrap()), "got: {text}");
        assert!(
            text.contains(fallback.path().to_str().unwrap()),
            "got: {text}"
        );
        assert!(
            text.contains("available packages: @pied-piper/other"),
            "got: {text}"
        );
    }

    #[test]
    fn available_packages_unions_roots_without_duplicates() {
        let (owned, fallback, store) = layered();
        write_package(owned.path(), "@pied-piper/util", "@pied-piper/util");
        write_package(owned.path(), "@pied-piper/shared", "@pied-piper/shared");
        write_package(fallback.path(), "@pied-piper/shared", "@pied-piper/shared");
        write_package(fallback.path(), "@zed/extra", "@zed/extra");

        let available = store.available_packages();
        let located = store.locate_all();

        assert_eq!(
            available,
            vec!["@pied-piper/shared", "@pied-piper/util", "@zed/extra"]
        );
        let origins: Vec<(&str, bool)> = located
            .iter()
            .map(|l| (l.name.as_str(), store.owns(&l.root)))
            .collect();
        assert_eq!(
            origins,
            vec![
                ("@pied-piper/shared", true),
                ("@pied-piper/util", true),
                ("@zed/extra", false)
            ]
        );
    }

    #[test]
    fn load_closure_resolves_a_dependency_from_the_fallback() {
        let (owned, fallback, store) = layered();
        write_package_with_deps(
            owned.path(),
            "@pied-piper/app",
            "@pied-piper/app",
            "1.0.0",
            &[("@pied-piper/util", "1.0.0")],
        );
        write_package_with_deps(
            fallback.path(),
            "@pied-piper/util",
            "@pied-piper/util",
            "1.0.0",
            &[],
        );

        let closure = store
            .load_closure(["@pied-piper/app"])
            .expect("closure loads");

        let names: Vec<&str> = closure
            .iter()
            .map(|a| a.metadata.package_name.as_str())
            .collect();
        assert_eq!(names, vec!["@pied-piper/util", "@pied-piper/app"]);
    }

    #[test]
    fn load_closure_orders_dependencies_before_dependents() {
        let tmp = tempdir().expect("tempdir");
        // Alphabetical order would instantiate @a/app first; the metadata
        // edge must override it.
        write_package_with_deps(
            tmp.path(),
            "@a/app",
            "@a/app",
            "1.0.0",
            &[("@z/util", "1.0.0")],
        );
        write_package_with_deps(tmp.path(), "@z/util", "@z/util", "1.0.0", &[]);
        let store = PackageStore::new(tmp.path());

        let closure = store.load_closure(["@a/app"]).expect("closure loads");

        let names: Vec<&str> = closure
            .iter()
            .map(|a| a.metadata.package_name.as_str())
            .collect();
        assert_eq!(names, vec!["@z/util", "@a/app"]);
    }

    #[test]
    fn load_closure_loads_shared_dependencies_once() {
        let tmp = tempdir().expect("tempdir");
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/base",
            "@pied-piper/base",
            "1.0.0",
            &[],
        );
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/a",
            "@pied-piper/a",
            "1.0.0",
            &[("@pied-piper/base", "1.0.0")],
        );
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/b",
            "@pied-piper/b",
            "1.0.0",
            &[("@pied-piper/base", "1.0.0")],
        );
        let store = PackageStore::new(tmp.path());

        let closure = store
            .load_closure(["@pied-piper/a", "@pied-piper/b"])
            .expect("closure loads");

        let names: Vec<&str> = closure
            .iter()
            .map(|a| a.metadata.package_name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["@pied-piper/base", "@pied-piper/a", "@pied-piper/b"]
        );
    }

    #[test]
    fn load_closure_bounds_bytes_across_individually_valid_packages() {
        let tmp = tempdir().expect("tempdir");
        write_package(tmp.path(), "@pied-piper/a", "@pied-piper/a");
        write_package(tmp.path(), "@pied-piper/b", "@pied-piper/b");
        let a_dir = tmp.path().join("@pied-piper/a");
        let b_dir = tmp.path().join("@pied-piper/b");
        let (a_total, a_max) = artifact_file_sizes(&a_dir);
        let (b_total, b_max) = artifact_file_sizes(&b_dir);
        let limits = ArtifactReadLimits {
            max_file_bytes: a_max.max(b_max),
            max_total_bytes: a_total.max(b_total),
            max_packages: 2,
        };
        let store = PackageStore::new(tmp.path());

        let error = store
            .load_closure_with_limits(["@pied-piper/a", "@pied-piper/b"], limits)
            .expect_err("combined closure exceeds the single-package budget");

        assert!(matches!(
            error,
            PackageStoreError::Artifact {
                source: ArtifactError::LoadBudgetExceeded { .. },
                ..
            }
        ));
        assert_eq!(
            store
                .load_closure(["@pied-piper/a", "@pied-piper/b"])
                .expect("ordinary budget recovers")
                .len(),
            2
        );
    }

    #[test]
    fn load_closure_missing_dependency_names_the_dependent() {
        let tmp = tempdir().expect("tempdir");
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/app",
            "@pied-piper/app",
            "1.0.0",
            &[("@pied-piper/util", "1.0.0")],
        );
        let store = PackageStore::new(tmp.path());

        let err = store
            .load_closure(["@pied-piper/app"])
            .expect_err("missing dep");

        assert!(matches!(err, PackageStoreError::MissingDependency { .. }));
        let text = err.to_string();
        assert!(text.contains("@pied-piper/util"), "got: {text}");
        assert!(
            text.contains("required by `@pied-piper/app`"),
            "got: {text}"
        );
        assert!(text.contains("submilli build"), "got: {text}");
    }

    #[test]
    fn load_closure_detects_metadata_cycles() {
        let tmp = tempdir().expect("tempdir");
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/a",
            "@pied-piper/a",
            "1.0.0",
            &[("@pied-piper/b", "1.0.0")],
        );
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/b",
            "@pied-piper/b",
            "1.0.0",
            &[("@pied-piper/a", "1.0.0")],
        );
        let store = PackageStore::new(tmp.path());

        let err = store.load_closure(["@pied-piper/a"]).expect_err("cycle");

        assert!(matches!(err, PackageStoreError::DependencyCycle { .. }));
        assert!(
            err.to_string().contains("circular package dependency"),
            "got: {err}"
        );
    }

    #[test]
    fn load_closure_rejects_dependency_version_mismatch() {
        let tmp = tempdir().expect("tempdir");
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/app",
            "@pied-piper/app",
            "1.0.0",
            &[("@pied-piper/util", "2.0.0")],
        );
        write_package_with_deps(
            tmp.path(),
            "@pied-piper/util",
            "@pied-piper/util",
            "1.0.0",
            &[],
        );
        let store = PackageStore::new(tmp.path());

        let err = store
            .load_closure(["@pied-piper/app"])
            .expect_err("mismatch");

        assert!(matches!(
            err,
            PackageStoreError::DependencyVersionMismatch { .. }
        ));
        let text = err.to_string();
        assert!(
            text.contains("2.0.0") && text.contains("1.0.0"),
            "got: {text}"
        );
    }

    fn artifact_file_sizes(dir: &Path) -> (usize, usize) {
        let mut total = 0;
        let mut largest = 0;
        let mut pending = vec![dir.to_path_buf()];
        while let Some(path) = pending.pop() {
            for entry in fs::read_dir(path).expect("read artifact directory") {
                let entry = entry.expect("read artifact entry");
                let metadata = entry.metadata().expect("read artifact metadata");
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else {
                    let bytes = usize::try_from(metadata.len()).expect("fixture fits usize");
                    total += bytes;
                    largest = largest.max(bytes);
                }
            }
        }
        (total, largest)
    }
}
