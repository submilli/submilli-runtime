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
        self.load_with_budget(name, &mut budget)
    }

    pub(crate) fn load_with_budget(
        &self,
        name: &str,
        budget: &mut ArtifactReadBudget,
    ) -> Result<Artifact, PackageStoreError> {
        for root in self.roots() {
            if let Some(artifact) = self.load_from(root, name, budget)? {
                return Ok(artifact);
            }
        }
        Err(self.missing(name, self.roots().map(Path::to_path_buf).collect()))
    }

    /// Load `name` from the owned root only, ignoring fallbacks. Install-time
    /// decisions (conflicts, upgrades, lockfile satisfaction) use this so the
    /// owned store stays self-contained.
    pub fn load_owned(&self, name: &str) -> Result<Artifact, PackageStoreError> {
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
        let mut marks = BTreeMap::new();
        let mut stack = Vec::new();
        let mut order = Vec::new();
        let mut budget = ArtifactReadBudget::new(limits);
        for name in names {
            self.visit_closure(name, None, &mut marks, &mut stack, &mut order, &mut budget)?;
        }
        Ok(order)
    }

    fn visit_closure(
        &self,
        name: &str,
        required_by: Option<(&str, &str)>,
        marks: &mut BTreeMap<String, ClosureMark>,
        stack: &mut Vec<String>,
        order: &mut Vec<Artifact>,
        budget: &mut ArtifactReadBudget,
    ) -> Result<(), PackageStoreError> {
        match marks.get(name) {
            Some(ClosureMark::Done { version }) => {
                return check_edge_version(name, required_by, version);
            }
            Some(ClosureMark::Visiting) => {
                let start = stack.iter().position(|n| n == name).unwrap_or(0);
                let mut cycle: Vec<_> = stack.iter().skip(start).cloned().collect();
                cycle.push(name.to_string());
                return Err(PackageStoreError::DependencyCycle { cycle });
            }
            None => {}
        }
        if stack.len() >= MAX_DEPENDENCY_DEPTH {
            return Err(PackageStoreError::DependencyDepthExceeded {
                name: name.to_string(),
                limit: MAX_DEPENDENCY_DEPTH,
            });
        }
        let artifact =
            self.load_with_budget(name, budget)
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
        marks.insert(name.to_string(), ClosureMark::Visiting);
        stack.push(name.to_string());
        for dep in &artifact.metadata.dependencies {
            self.visit_closure(
                &dep.name,
                Some((name, &dep.version)),
                marks,
                stack,
                order,
                budget,
            )?;
        }
        stack.pop();
        marks.insert(
            name.to_string(),
            ClosureMark::Done {
                version: artifact.metadata.package_version.clone(),
            },
        );
        order.push(artifact);
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
}

/// The `@scope/leaf` directory for `name` under `root`, validating the name.
fn package_dir_under(root: &Path, name: &str) -> Result<PathBuf, PackageStoreError> {
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
    let (org, leaf) = submilli_blueprint::validate_scoped_name(name).ok()?;
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

        let dir = store.package_dir("@acme/util").expect("package dir");

        assert_eq!(dir, tmp.path().join("@acme").join("util"));
    }

    #[test]
    fn missing_package_lists_available_packages() {
        let tmp = tempdir().expect("tempdir");
        write_package(tmp.path(), "@acme/other", "@acme/other");
        let store = PackageStore::new(tmp.path());

        let err = store.load("@acme/util").expect_err("missing package");

        let text = err.to_string();
        assert!(text.contains("@acme/util"), "got: {text}");
        assert!(text.contains(tmp.path().to_str().unwrap()), "got: {text}");
        assert!(text.contains("@acme/other"), "got: {text}");
    }

    #[test]
    fn artifact_name_mismatch_is_rejected() {
        let tmp = tempdir().expect("tempdir");
        write_package(tmp.path(), "@acme/util", "@acme/wrong");
        let store = PackageStore::new(tmp.path());

        let err = store.load("@acme/util").expect_err("name mismatch");

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
        write_package(fallback.path(), "@acme/util", "@acme/util");

        let artifact = store.load("@acme/util").expect("loads from fallback");
        let located = store.locate("@acme/util").expect("locate").expect("found");

        assert_eq!(artifact.metadata.package_name, "@acme/util");
        assert_eq!(located.root, fallback.path());
        assert_eq!(located.dir, fallback.path().join("@acme").join("util"));
        assert!(!store.owns(&located.root));
        assert_eq!(
            store.package_dir("@acme/util").unwrap(),
            owned.path().join("@acme").join("util"),
            "writes still target the owned root"
        );
    }

    #[test]
    fn owned_copy_shadows_the_fallback_copy() {
        let (owned, fallback, store) = layered();
        write_package_with_deps(owned.path(), "@acme/util", "@acme/util", "2.0.0", &[]);
        write_package_with_deps(fallback.path(), "@acme/util", "@acme/util", "1.0.0", &[]);

        let artifact = store.load("@acme/util").expect("loads");
        let located = store.locate("@acme/util").unwrap().unwrap();

        assert_eq!(artifact.metadata.package_version, "2.0.0");
        assert_eq!(located.root, owned.path());
        assert!(store.owns(&located.root));
    }

    #[test]
    fn load_owned_ignores_the_fallback() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@acme/util", "@acme/util");

        let err = store.load_owned("@acme/util").expect_err("not owned");

        let PackageStoreError::MissingPackage { searched_roots, .. } = &err else {
            panic!("expected MissingPackage, got {err:?}");
        };
        assert_eq!(searched_roots, &vec![owned.path().to_path_buf()]);
    }

    #[test]
    fn an_owned_only_miss_does_not_advertise_fallback_packages() {
        let (_owned, fallback, store) = layered();
        write_package(fallback.path(), "@acme/other", "@acme/other");

        let err = store.load_owned("@acme/util").expect_err("not owned");

        let PackageStoreError::MissingPackage { available, .. } = &err else {
            panic!("expected MissingPackage, got {err:?}");
        };
        assert!(available.is_empty(), "got: {available:?}");
    }

    #[test]
    fn a_broken_owned_copy_does_not_fall_through() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@acme/util", "@acme/util");
        let broken = owned.path().join("@acme").join("util");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("metadata.json"), b"{ not json").unwrap();

        let err = store.load("@acme/util").expect_err("owned copy is broken");

        assert!(
            !matches!(err, PackageStoreError::MissingPackage { .. }),
            "got: {err}"
        );
    }

    #[test]
    fn an_empty_owned_package_dir_does_not_fall_through() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@acme/util", "@acme/util");
        // An interrupted install: the directory exists, the files do not.
        fs::create_dir_all(owned.path().join("@acme").join("util")).unwrap();

        let err = store
            .load("@acme/util")
            .expect_err("owned copy is incomplete");

        assert!(
            matches!(err, PackageStoreError::Artifact { .. }),
            "got: {err}"
        );
        let located = store.locate("@acme/util").unwrap().unwrap();
        assert!(store.owns(&located.root), "locate agrees with load");
    }

    #[test]
    fn a_non_directory_owned_package_does_not_fall_through() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@acme/util", "@acme/util");
        let package = owned.path().join("@acme/util");
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, b"not a directory").unwrap();

        assert_package_probe_error(
            store.locate("@acme/util").unwrap_err(),
            &package,
            io::ErrorKind::NotADirectory,
        );
        assert_package_probe_error(
            store.load("@acme/util").unwrap_err(),
            &package,
            io::ErrorKind::NotADirectory,
        );
        assert_package_probe_error(
            store.load_owned("@acme/util").unwrap_err(),
            &package,
            io::ErrorKind::NotADirectory,
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_inaccessible_owned_scope_does_not_fall_through() {
        use std::os::unix::fs::PermissionsExt;

        let (owned, fallback, store) = layered();
        write_package(owned.path(), "@acme/util", "@acme/util");
        write_package(fallback.path(), "@acme/util", "@acme/util");
        let scope = owned.path().join("@acme");
        let package = scope.join("util");
        let permissions = fs::metadata(&scope).unwrap().permissions();
        fs::set_permissions(&scope, fs::Permissions::from_mode(0o000)).unwrap();
        let denied = fs::metadata(&package)
            .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
        let located = store.locate("@acme/util");
        let loaded = store.load("@acme/util");
        let loaded_owned = store.load_owned("@acme/util");
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
        write_package(owned.path(), "@acme/util", "@acme/util");
        // A file where a scope directory is expected.
        fs::write(fallback.path().join("@broken"), b"not a directory").unwrap();
        write_package(fallback.path(), "@zed/extra", "@zed/extra");

        let available = store.available_packages();

        assert_eq!(available, vec!["@acme/util", "@zed/extra"]);
    }

    #[test]
    fn missing_package_names_every_searched_root() {
        let (owned, fallback, store) = layered();
        write_package(fallback.path(), "@acme/other", "@acme/other");

        let err = store.load("@acme/util").expect_err("missing");

        let text = err.to_string();
        assert!(text.contains(owned.path().to_str().unwrap()), "got: {text}");
        assert!(
            text.contains(fallback.path().to_str().unwrap()),
            "got: {text}"
        );
        assert!(
            text.contains("available packages: @acme/other"),
            "got: {text}"
        );
    }

    #[test]
    fn available_packages_unions_roots_without_duplicates() {
        let (owned, fallback, store) = layered();
        write_package(owned.path(), "@acme/util", "@acme/util");
        write_package(owned.path(), "@acme/shared", "@acme/shared");
        write_package(fallback.path(), "@acme/shared", "@acme/shared");
        write_package(fallback.path(), "@zed/extra", "@zed/extra");

        let available = store.available_packages();
        let located = store.locate_all();

        assert_eq!(available, vec!["@acme/shared", "@acme/util", "@zed/extra"]);
        let origins: Vec<(&str, bool)> = located
            .iter()
            .map(|l| (l.name.as_str(), store.owns(&l.root)))
            .collect();
        assert_eq!(
            origins,
            vec![
                ("@acme/shared", true),
                ("@acme/util", true),
                ("@zed/extra", false)
            ]
        );
    }

    #[test]
    fn load_closure_resolves_a_dependency_from_the_fallback() {
        let (owned, fallback, store) = layered();
        write_package_with_deps(
            owned.path(),
            "@acme/app",
            "@acme/app",
            "1.0.0",
            &[("@acme/util", "1.0.0")],
        );
        write_package_with_deps(fallback.path(), "@acme/util", "@acme/util", "1.0.0", &[]);

        let closure = store.load_closure(["@acme/app"]).expect("closure loads");

        let names: Vec<&str> = closure
            .iter()
            .map(|a| a.metadata.package_name.as_str())
            .collect();
        assert_eq!(names, vec!["@acme/util", "@acme/app"]);
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
        write_package_with_deps(tmp.path(), "@acme/base", "@acme/base", "1.0.0", &[]);
        write_package_with_deps(
            tmp.path(),
            "@acme/a",
            "@acme/a",
            "1.0.0",
            &[("@acme/base", "1.0.0")],
        );
        write_package_with_deps(
            tmp.path(),
            "@acme/b",
            "@acme/b",
            "1.0.0",
            &[("@acme/base", "1.0.0")],
        );
        let store = PackageStore::new(tmp.path());

        let closure = store
            .load_closure(["@acme/a", "@acme/b"])
            .expect("closure loads");

        let names: Vec<&str> = closure
            .iter()
            .map(|a| a.metadata.package_name.as_str())
            .collect();
        assert_eq!(names, vec!["@acme/base", "@acme/a", "@acme/b"]);
    }

    #[test]
    fn load_closure_bounds_bytes_across_individually_valid_packages() {
        let tmp = tempdir().expect("tempdir");
        write_package(tmp.path(), "@acme/a", "@acme/a");
        write_package(tmp.path(), "@acme/b", "@acme/b");
        let a_dir = tmp.path().join("@acme/a");
        let b_dir = tmp.path().join("@acme/b");
        let (a_total, a_max) = artifact_file_sizes(&a_dir);
        let (b_total, b_max) = artifact_file_sizes(&b_dir);
        let limits = ArtifactReadLimits {
            max_file_bytes: a_max.max(b_max),
            max_total_bytes: a_total.max(b_total),
            max_packages: 2,
        };
        let store = PackageStore::new(tmp.path());

        let error = store
            .load_closure_with_limits(["@acme/a", "@acme/b"], limits)
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
                .load_closure(["@acme/a", "@acme/b"])
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
            "@acme/app",
            "@acme/app",
            "1.0.0",
            &[("@acme/util", "1.0.0")],
        );
        let store = PackageStore::new(tmp.path());

        let err = store.load_closure(["@acme/app"]).expect_err("missing dep");

        assert!(matches!(err, PackageStoreError::MissingDependency { .. }));
        let text = err.to_string();
        assert!(text.contains("@acme/util"), "got: {text}");
        assert!(text.contains("required by `@acme/app`"), "got: {text}");
        assert!(text.contains("submilli build"), "got: {text}");
    }

    #[test]
    fn load_closure_detects_metadata_cycles() {
        let tmp = tempdir().expect("tempdir");
        write_package_with_deps(
            tmp.path(),
            "@acme/a",
            "@acme/a",
            "1.0.0",
            &[("@acme/b", "1.0.0")],
        );
        write_package_with_deps(
            tmp.path(),
            "@acme/b",
            "@acme/b",
            "1.0.0",
            &[("@acme/a", "1.0.0")],
        );
        let store = PackageStore::new(tmp.path());

        let err = store.load_closure(["@acme/a"]).expect_err("cycle");

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
            "@acme/app",
            "@acme/app",
            "1.0.0",
            &[("@acme/util", "2.0.0")],
        );
        write_package_with_deps(tmp.path(), "@acme/util", "@acme/util", "1.0.0", &[]);
        let store = PackageStore::new(tmp.path());

        let err = store.load_closure(["@acme/app"]).expect_err("mismatch");

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
