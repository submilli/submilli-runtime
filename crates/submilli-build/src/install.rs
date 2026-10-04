//! Install already-fetched package repos into the local store.
//!
//! [`install_from_dir`] is the network-free core: given an extracted repo
//! directory and the source provenance to stamp, it loads the manifest, compiles
//! the packages, enforces the org/leaf scoping rule, and writes the artifacts
//! into the store — honouring the re-install / `--upgrade` policy.
//!
//! [`install_plan`] is its counterpart for a whole resolved GitHub closure: it
//! installs each fetched dependency (from
//! [`resolve_github_closure`](crate::resolve::resolve_github_closure)) in
//! dependency order, again network-free. The fetch that produces those repos
//! lives in `submilli-shared`.

use std::path::{Path, PathBuf};

use crate::resolve::PlannedInstall;
use crate::{
    BuildDiagnostic, BuiltPackage, DriverError, PackageName, PackageSource, PackageStore,
    PackageStoreError, PackageVersion, build_packages, install_packages, load_manifest,
};

/// What an install wrote and skipped.
#[derive(Debug)]
pub struct InstallReport {
    pub installed: Vec<InstalledPackage>,
    pub up_to_date: Vec<PackageName>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct InstalledPackage {
    pub name: PackageName,
    pub version: PackageVersion,
    pub dir: PathBuf,
}

/// A package already in the store that blocks a re-install without `--upgrade`.
#[derive(Debug)]
pub struct InstallConflict {
    pub name: PackageName,
    /// Human description of where the installed copy came from.
    pub current: String,
}

#[derive(Debug)]
pub enum InstallError {
    WarningsDenied {
        warnings: Vec<String>,
    },
    Preparation(std::io::Error),
    /// No `submilli.toml` at the repo root.
    NoManifest {
        repo_dir: PathBuf,
    },
    /// The manifest failed to parse. Carries the text so the caller can render
    /// source-anchored diagnostics.
    Manifest {
        manifest_path: PathBuf,
        manifest_text: String,
        diagnostics: Vec<BuildDiagnostic>,
    },
    /// Compilation / dependency resolution failed.
    Driver(DriverError),
    /// One or more packages aren't scoped to the source org.
    ScopeMismatch {
        org: String,
        repo: String,
        packages: Vec<PackageName>,
    },
    /// Packages already installed at a different commit; needs `--upgrade`.
    Conflict {
        incoming: String,
        conflicts: Vec<InstallConflict>,
    },
    /// A store read failed while checking what's already installed.
    Store(PackageStoreError),
}

/// Compile every selected package in `repo_dir`, stamp `source` onto each, and
/// install into `store`. `only` scopes to one package (plus its sibling-dep
/// closure); `None` installs all. Already-installed packages at the same commit
/// are skipped; at a different commit they require `upgrade`.
pub fn install_from_dir(
    store: &PackageStore,
    repo_dir: &Path,
    only: Option<&PackageName>,
    source: &PackageSource,
    upgrade: bool,
) -> Result<InstallReport, InstallError> {
    let mut preparation = InstallPreparation::new(store)?;
    let report = preparation.prepare_repo(repo_dir, only, source, upgrade)?;
    preparation.publish(false)?;
    Ok(report)
}

/// Prepare a dependency closure without changing the destination store.
/// Temporary artifacts supply declarations to subsequent compilations.
pub struct InstallPreparation {
    destination: PackageStore,
    staging: PackageStore,
    _directory: tempfile::TempDir,
    packages: Vec<BuiltPackage>,
    pub warnings: Vec<String>,
}

impl InstallPreparation {
    pub fn new(destination: &PackageStore) -> Result<Self, InstallError> {
        let directory = tempfile::tempdir().map_err(InstallError::Preparation)?;
        let mut staging = PackageStore::new(directory.path());
        for root in destination.roots() {
            staging = staging.with_fallback(root);
        }
        Ok(Self {
            destination: destination.clone(),
            staging,
            _directory: directory,
            packages: Vec::new(),
            warnings: Vec::new(),
        })
    }

    pub fn store(&self) -> &PackageStore {
        &self.staging
    }

    pub fn prepare_plan(
        &mut self,
        plan: &[PlannedInstall],
        upgrade: bool,
    ) -> Result<(), InstallError> {
        for node in plan {
            self.prepare_repo(node.repo_dir(), Some(&node.name), &node.source, upgrade)?;
        }
        Ok(())
    }

    pub fn prepare_repo(
        &mut self,
        repo_dir: &Path,
        only: Option<&PackageName>,
        source: &PackageSource,
        upgrade: bool,
    ) -> Result<InstallReport, InstallError> {
        let mut built = compile_repo(&self.staging, repo_dir, only, source)?;
        let warnings: Vec<String> = built
            .iter()
            .flat_map(|package| package.warnings.clone())
            .collect();
        self.warnings.extend(warnings.iter().cloned());
        let (to_install, up_to_date) =
            select_install(&self.destination, &self.staging, &built, source, upgrade)?;
        // Even same-commit packages supply the freshly compiled declarations.
        install_packages(&self.staging, &built).map_err(InstallError::Driver)?;
        let mut installed = Vec::new();
        for package in &built {
            if to_install.contains(&package.name) {
                installed.push(InstalledPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    dir: self
                        .destination
                        .package_dir(package.name.as_str())
                        .map_err(InstallError::Store)?,
                });
            }
        }
        built.retain(|package| to_install.contains(&package.name));
        for package in built {
            if let Some(existing) = self
                .packages
                .iter_mut()
                .find(|existing| existing.name == package.name)
            {
                *existing = package;
            } else {
                self.packages.push(package);
            }
        }
        Ok(InstallReport {
            installed,
            up_to_date,
            warnings,
        })
    }

    pub fn publish(self, deny_warnings: bool) -> Result<(), InstallError> {
        if deny_warnings && !self.warnings.is_empty() {
            return Err(InstallError::WarningsDenied {
                warnings: self.warnings,
            });
        }
        install_packages(&self.destination, &self.packages).map_err(InstallError::Driver)?;
        Ok(())
    }
}

fn compile_repo(
    store: &PackageStore,
    repo_dir: &Path,
    only: Option<&PackageName>,
    source: &PackageSource,
) -> Result<Vec<BuiltPackage>, InstallError> {
    let manifest_path = repo_dir.join("submilli.toml");
    if !manifest_path.is_file() {
        return Err(InstallError::NoManifest {
            repo_dir: repo_dir.to_path_buf(),
        });
    }
    let manifest = load_manifest(&manifest_path).map_err(|diagnostics| InstallError::Manifest {
        manifest_path: manifest_path.clone(),
        manifest_text: std::fs::read_to_string(&manifest_path).unwrap_or_default(),
        diagnostics,
    })?;
    let mut built =
        build_packages(&manifest, repo_dir, store, only).map_err(InstallError::Driver)?;
    let PackageSource::Github(github) = source;
    let mismatched = built
        .iter()
        .filter(|package| scope_of(package.name.as_str()) != github.org)
        .map(|package| package.name.clone())
        .collect::<Vec<_>>();
    if !mismatched.is_empty() {
        return Err(InstallError::ScopeMismatch {
            org: github.org.clone(),
            repo: github.repo.clone(),
            packages: mismatched,
        });
    }
    for package in &mut built {
        package.source = Some(source.clone());
    }
    Ok(built)
}

fn select_install(
    store: &PackageStore,
    staging: &PackageStore,
    built: &[BuiltPackage],
    source: &PackageSource,
    upgrade: bool,
) -> Result<(Vec<PackageName>, Vec<PackageName>), InstallError> {
    let PackageSource::Github(github) = source;
    let mut conflicts = Vec::new();
    let mut up_to_date = Vec::new();
    let mut to_install = Vec::new();
    for package in built {
        let existing = match staging.load_owned(package.name.as_str()) {
            Err(PackageStoreError::MissingPackage { .. }) => {
                store.load_owned(package.name.as_str())
            }
            result => result,
        };
        match existing {
            Err(PackageStoreError::MissingPackage { .. }) => to_install.push(package.name.clone()),
            Err(err) if err.is_incomplete_artifact() => to_install.push(package.name.clone()),
            Err(err) => return Err(InstallError::Store(err)),
            Ok(artifact) => match artifact.metadata.source {
                Some(PackageSource::Github(existing)) if existing.sha == github.sha => {
                    up_to_date.push(package.name.clone());
                }
                _ if upgrade => to_install.push(package.name.clone()),
                existing => conflicts.push(InstallConflict {
                    name: package.name.clone(),
                    current: describe_source(existing.as_ref()),
                }),
            },
        }
    }
    if !conflicts.is_empty() {
        return Err(InstallError::Conflict {
            incoming: short_sha(&github.sha).to_string(),
            conflicts,
        });
    }
    Ok((to_install, up_to_date))
}

/// Compile and publish a resolved dependency closure.
pub fn install_plan(
    store: &PackageStore,
    plan: &[PlannedInstall],
    upgrade: bool,
) -> Result<(), InstallError> {
    let mut preparation = InstallPreparation::new(store)?;
    preparation.prepare_plan(plan, upgrade)?;
    preparation.publish(false)
}

fn scope_of(name: &str) -> &str {
    submilli_blueprint::validate_scoped_name(name)
        .map(|(org, _)| org)
        .unwrap_or_default()
}

fn describe_source(source: Option<&PackageSource>) -> String {
    match source {
        Some(PackageSource::Github(gh)) => {
            format!("github.com/{}/{}@{}", gh.org, gh.repo, short_sha(&gh.sha))
        }
        None => "a local build".to_string(),
    }
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}
