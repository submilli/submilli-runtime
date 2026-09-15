use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Artifact, ArtifactError, read_package_artifact};

#[derive(Clone, Debug)]
pub struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn package_dir(&self, name: &str) -> Result<PathBuf, PackageStoreError> {
        let (scope, package) =
            split_scoped_name(name).ok_or_else(|| PackageStoreError::InvalidPackageName {
                name: name.to_string(),
            })?;
        Ok(self.root.join(scope).join(package))
    }

    pub fn load(&self, name: &str) -> Result<Artifact, PackageStoreError> {
        let dir = self.package_dir(name)?;
        let artifact = read_package_artifact(&dir).map_err(|source| {
            if source.is_missing_file() {
                PackageStoreError::MissingPackage {
                    name: name.to_string(),
                    store_root: self.root.clone(),
                    package_dir: dir.clone(),
                    available: self.available_packages().unwrap_or_default(),
                }
            } else {
                PackageStoreError::Artifact {
                    name: name.to_string(),
                    package_dir: dir.clone(),
                    source,
                }
            }
        })?;
        validate_artifact_name(name, &dir, artifact)
    }

    pub fn load_many<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<Artifact>, PackageStoreError> {
        names.into_iter().map(|name| self.load(name)).collect()
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
        let mut marks = BTreeMap::new();
        let mut stack = Vec::new();
        let mut order = Vec::new();
        for name in names {
            self.visit_closure(name, None, &mut marks, &mut stack, &mut order)?;
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
    ) -> Result<(), PackageStoreError> {
        match marks.get(name) {
            Some(ClosureMark::Done { version }) => {
                return check_edge_version(name, required_by, version);
            }
            Some(ClosureMark::Visiting) => {
                let start = stack.iter().position(|n| n == name).unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                cycle.push(name.to_string());
                return Err(PackageStoreError::DependencyCycle { cycle });
            }
            None => {}
        }
        let artifact = self.load(name).map_err(|err| match (&err, required_by) {
            (PackageStoreError::MissingPackage { .. }, Some((dependent, _))) => {
                PackageStoreError::MissingDependency {
                    name: name.to_string(),
                    required_by: dependent.to_string(),
                    store_root: self.root.clone(),
                    available: self.available_packages().unwrap_or_default(),
                }
            }
            _ => err,
        })?;
        check_edge_version(name, required_by, &artifact.metadata.package_version)?;
        marks.insert(name.to_string(), ClosureMark::Visiting);
        stack.push(name.to_string());
        for dep in &artifact.metadata.dependencies {
            self.visit_closure(&dep.name, Some((name, &dep.version)), marks, stack, order)?;
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

    pub fn available_packages(&self) -> io::Result<Vec<String>> {
        let mut packages = Vec::new();
        let Ok(scopes) = fs::read_dir(&self.root) else {
            return Ok(packages);
        };
        for scope in scopes {
            let scope = scope?;
            if !scope.file_type()?.is_dir() {
                continue;
            }
            let scope_name = scope.file_name().to_string_lossy().into_owned();
            if !scope_name.starts_with('@') {
                continue;
            }
            for package in fs::read_dir(scope.path())? {
                let package = package?;
                if package.file_type()?.is_dir() {
                    packages.push(format!(
                        "{scope_name}/{}",
                        package.file_name().to_string_lossy()
                    ));
                }
            }
        }
        packages.sort();
        Ok(packages)
    }
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
        store_root: PathBuf,
        package_dir: PathBuf,
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
        store_root: PathBuf,
        available: Vec<String>,
    },
    DependencyCycle {
        cycle: Vec<String>,
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
                store_root,
                package_dir,
                available,
            } => {
                write!(
                    f,
                    "package `{name}` was not found in {} (looked in {})",
                    store_root.display(),
                    package_dir.display()
                )?;
                if available.is_empty() {
                    write!(f, "; no packages are available")
                } else {
                    write!(f, "; available packages: {}", available.join(", "))
                }
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
                store_root,
                available,
            } => {
                write!(
                    f,
                    "package `{name}` (required by `{required_by}`) was not found in {}; run `submilli build` in the project that provides `{name}`",
                    store_root.display()
                )?;
                if available.is_empty() {
                    write!(f, "; no packages are available")
                } else {
                    write!(f, "; available packages: {}", available.join(", "))
                }
            }
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

trait ArtifactErrorExt {
    fn is_missing_file(&self) -> bool;
}

impl ArtifactErrorExt for ArtifactError {
    fn is_missing_file(&self) -> bool {
        matches!(
            self,
            ArtifactError::Io { source, .. } if source.kind() == io::ErrorKind::NotFound
        )
    }
}

#[cfg(test)]
mod tests {
    use interpreter::PackageDeclaration;
    use tempfile::tempdir;

    use crate::{ArtifactMetadata, write_package_artifact};

    use super::*;

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
        let capabilities = crate::derive_capability_schema(&declaration, &[]);
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
}
