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
    BuildDiagnostic, DriverError, PackageName, PackageSource, PackageStore, PackageStoreError,
    PackageVersion, build_packages, install_packages, load_manifest,
};

/// What an install wrote and skipped.
#[derive(Debug)]
pub struct InstallReport {
    pub installed: Vec<InstalledPackage>,
    pub up_to_date: Vec<PackageName>,
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
    /// No `submilli.toml` at the repo root.
    NoManifest { repo_dir: PathBuf },
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
    let mismatched: Vec<PackageName> = built
        .iter()
        .filter(|package| scope_of(package.name.as_str()) != github.org)
        .map(|package| package.name.clone())
        .collect();
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

    let mut conflicts = Vec::new();
    let mut up_to_date = Vec::new();
    let mut to_install = Vec::new();
    for package in built {
        match store.load(package.name.as_str()) {
            Err(PackageStoreError::MissingPackage { .. }) => to_install.push(package),
            Err(err) => return Err(InstallError::Store(err)),
            Ok(artifact) => match artifact.metadata.source {
                Some(PackageSource::Github(existing)) if existing.sha == github.sha => {
                    up_to_date.push(package.name.clone());
                }
                existing if upgrade => {
                    let _ = existing;
                    to_install.push(package);
                }
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

    let dirs = install_packages(store, &to_install).map_err(InstallError::Driver)?;
    let installed = to_install
        .into_iter()
        .zip(dirs)
        .map(|(package, dir)| InstalledPackage {
            name: package.name,
            version: package.version,
            dir,
        })
        .collect();

    Ok(InstallReport {
        installed,
        up_to_date,
    })
}

/// Install each fetched dependency in a resolved GitHub closure `plan` into the
/// store, in dependency order (deps before dependents). Network-free — the
/// fetching already happened in
/// [`resolve_github_closure`](crate::resolve::resolve_github_closure). Each
/// node's own re-install / `--upgrade` policy is applied by [`install_from_dir`].
pub fn install_plan(
    store: &PackageStore,
    plan: &[PlannedInstall],
    upgrade: bool,
) -> Result<(), InstallError> {
    for node in plan {
        install_from_dir(
            store,
            node.repo_dir(),
            Some(&node.name),
            &node.source,
            upgrade,
        )?;
    }
    Ok(())
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
    &sha[..sha.len().min(12)]
}
